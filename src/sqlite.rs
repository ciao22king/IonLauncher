//! A minimal read-only SQLite reader.
//!
//! GOG Galaxy keeps its authoritative library in `galaxy-2.0.db` (SQLite).
//! Linking against a C SQLite build would add a compiler dependency and
//! several hundred kilobytes for what amounts to reading one small table,
//! so we walk the B-tree ourselves.
//!
//! Scope: table B-trees, overflow pages, all serial types, UTF-8 and
//! UTF-16 text. Freeblocks and indexed (WITHOUT ROWID / index) trees are
//! not needed for our queries and are skipped.

use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Real(f) => Some(*f as i64),
            Value::Text(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
}

/// One row: column name -> value, so callers never depend on column order.
pub type Row = HashMap<String, Value>;

pub struct Database {
    data: Vec<u8>,
    page_size: usize,
    usable: usize,
    utf16: Option<bool>, // Some(false) = LE, Some(true) = BE
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, String> {
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        // Guard against absurd files; Galaxy DBs are tens of MB at most.
        if meta.len() > 2 * 1024 * 1024 * 1024 {
            return Err("database too large".into());
        }
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        Self::from_bytes(data)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self, String> {
        if data.len() < 100 || &data[..16] != b"SQLite format 3\0" {
            return Err("not a SQLite database".into());
        }
        let raw_page_size = be_u16(&data, 16).ok_or("truncated header")? as usize;
        // The value 1 is the documented stand-in for 65536.
        let page_size = if raw_page_size == 1 {
            65536
        } else {
            raw_page_size
        };
        if page_size < 512 || !page_size.is_power_of_two() {
            return Err(format!("unsupported page size {page_size}"));
        }
        let reserved = data[20] as usize;
        if reserved >= page_size {
            return Err("reserved space exceeds page size".into());
        }
        let encoding = be_u32(&data, 56).unwrap_or(1);
        let utf16 = match encoding {
            1 => None,
            2 => Some(false),
            3 => Some(true),
            other => return Err(format!("unknown text encoding {other}")),
        };
        let usable = page_size - reserved;
        Ok(Self {
            data,
            page_size,
            usable,
            utf16,
        })
    }

    /// Byte range of a page, given its 1-based number.
    fn page(&self, number: u32) -> Option<&[u8]> {
        if number == 0 {
            return None;
        }
        let start = (number as usize - 1).checked_mul(self.page_size)?;
        let end = start.checked_add(self.page_size)?;
        self.data.get(start..end)
    }

    /// The B-tree header offset within a page. Page 1 is offset by the
    /// 100-byte file header.
    fn header_offset(number: u32) -> usize {
        if number == 1 {
            100
        } else {
            0
        }
    }

    /// Reads the schema table and returns `name -> (root page, columns)`.
    fn schema(&self) -> Result<Vec<(String, u32, Vec<String>)>, String> {
        let mut out = Vec::new();
        let root = Self::header_offset(1);
        let page = self.page(1).ok_or("missing page 1")?;
        let cells = self.walk_table(page, root, 1)?;
        for (_, payload) in cells {
            let values = self.decode_record(&payload)?;
            let kind = values.first().and_then(|v| v.as_str()).unwrap_or("");
            let name = values.get(1).and_then(|v| v.as_str()).unwrap_or("");
            // Only ordinary tables have a B-tree root we can walk.
            if kind != "table" || name.starts_with("sqlite_") {
                continue;
            }
            let Some(root_page) = values.get(3).and_then(|v| v.as_i64()) else {
                continue;
            };
            let sql = values.get(4).and_then(|v| v.as_str()).unwrap_or("");
            out.push((name.to_string(), root_page as u32, parse_columns(sql)));
        }
        Ok(out)
    }

    /// Reads every row of `table`, keyed by column name.
    ///
    /// Rows are capped so a surprising database cannot exhaust memory.
    pub fn rows(&self, table: &str, limit: usize) -> Result<Vec<Row>, String> {
        let schema = self.schema()?;
        let Some((_, root, columns)) = schema
            .iter()
            .find(|(name, _, _)| name.eq_ignore_ascii_case(table))
        else {
            return Err(format!("no such table: {table}"));
        };
        let root = *root;
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(number) = stack.pop() {
            let Some(page) = self.page(number) else {
                continue;
            };
            let offset = Self::header_offset(number);
            let cells = self.walk_table(page, offset, number)?;
            for (_, payload) in cells {
                if out.len() >= limit {
                    return Ok(out);
                }
                let values = self.decode_record(&payload)?;
                let mut row = Row::new();
                for (i, column) in columns.iter().enumerate() {
                    if let Some(value) = values.get(i) {
                        row.insert(column.clone(), value.clone());
                    }
                }
                // Interior nodes also carry keys; only rows with a rowid slot
                // produce a full record. Skip anything that decoded empty.
                if !row.is_empty() {
                    out.push(row);
                }
            }
            // Interior pages list child pointers in their cells; collect them.
            for child in self.interior_children(page, offset)? {
                stack.push(child);
            }
        }
        Ok(out)
    }

    /// Walks a table B-tree page and returns `(rowid, payload)` for each cell.
    fn walk_table(
        &self,
        page: &[u8],
        offset: usize,
        page_number: u32,
    ) -> Result<Vec<(u64, Vec<u8>)>, String> {
        let kind = *page.get(offset).ok_or("truncated page header")?;
        let count = be_u16(page, offset + 3).ok_or("truncated page header")? as usize;
        let cell_array = offset + if kind == 0x05 { 12 } else { 8 };
        let mut out = Vec::new();
        for i in 0..count {
            let ptr_at = cell_array + i * 2;
            let cell_offset = be_u16(page, ptr_at).ok_or("bad cell pointer")? as usize;
            if cell_offset == 0 || cell_offset >= page.len() {
                continue;
            }
            // 0x0d = leaf table, 0x05 = interior table.
            if kind == 0x0d {
                let (payload, _) = self.read_cell_payload(page, cell_offset, page_number)?;
                let (_, after_len) = read_varint(page, cell_offset)?;
                let (rowid, _) = read_varint(page, after_len)?;
                out.push((rowid, payload));
            }
        }
        Ok(out)
    }

    /// Child page numbers of an interior table page.
    fn interior_children(&self, page: &[u8], offset: usize) -> Result<Vec<u32>, String> {
        let kind = *page.get(offset).ok_or("truncated page header")?;
        if kind != 0x05 {
            return Ok(Vec::new());
        }
        let count = be_u16(page, offset + 3).ok_or("truncated page header")? as usize;
        let mut children = Vec::new();
        let cell_array = offset + 12;
        for i in 0..count {
            let cell_offset = be_u16(page, cell_array + i * 2).ok_or("bad cell pointer")? as usize;
            if let Some(child) = be_u32(page, cell_offset) {
                children.push(child);
            }
        }
        // The rightmost pointer sits in the header.
        if let Some(rightmost) = be_u32(page, offset + 8) {
            children.push(rightmost);
        }
        Ok(children)
    }

    /// Reads a table-leaf cell payload, following the overflow chain if needed.
    fn read_cell_payload(
        &self,
        page: &[u8],
        cell_offset: usize,
        page_number: u32,
    ) -> Result<(Vec<u8>, usize), String> {
        let (payload_len, mut cursor) = read_varint(page, cell_offset)?;
        let payload_len = payload_len as usize;
        let max_local = self.usable - 35;
        let min_local = (self.usable - 12) * 32 / 255 - 23;
        let (local, overflow) = if payload_len <= max_local {
            (payload_len, None)
        } else {
            let local = min_local + (payload_len - min_local) % (self.usable - 4);
            let local = if local > max_local { min_local } else { local };
            let ptr_at = cursor + local;
            let next = be_u32(page, ptr_at).ok_or("truncated overflow pointer")?;
            (local, Some(next))
        };
        let end = (cursor + local).min(page.len());
        let mut payload = page[cursor.min(page.len())..end].to_vec();
        cursor = end;
        let _ = page_number;
        // Follow the overflow chain: each page is a 4-byte next pointer plus data.
        let mut next = overflow;
        let mut guard = 0;
        while let Some(number) = next {
            guard += 1;
            if guard > 100_000 || payload.len() >= payload_len {
                break;
            }
            let Some(opage) = self.page(number) else {
                break;
            };
            next = be_u32(opage, 0);
            let take = (self.usable - 4).min(payload_len - payload.len());
            let end = (4 + take).min(opage.len());
            if end <= 4 {
                break;
            }
            payload.extend_from_slice(&opage[4..end]);
        }
        Ok((payload, cursor))
    }

    /// Decodes a record payload into values.
    fn decode_record(&self, payload: &[u8]) -> Result<Vec<Value>, String> {
        let (header_len, mut pos) = read_varint(payload, 0)?;
        let header_len = header_len as usize;
        if header_len > payload.len() {
            return Err("record header exceeds payload".into());
        }
        let mut serial_types = Vec::new();
        while pos < header_len {
            let (t, next) = read_varint(payload, pos)?;
            serial_types.push(t);
            pos = next;
        }
        let mut body = header_len;
        let mut values = Vec::with_capacity(serial_types.len());
        for t in serial_types {
            let (value, consumed) = self.decode_value(t, payload, body)?;
            body += consumed;
            values.push(value);
        }
        Ok(values)
    }

    fn decode_value(&self, serial: u64, data: &[u8], at: usize) -> Result<(Value, usize), String> {
        let need = |n: usize| -> Result<&[u8], String> {
            data.get(at..at + n)
                .ok_or_else(|| "truncated record value".to_string())
        };
        Ok(match serial {
            0 => (Value::Null, 0),
            1 => (
                Value::Int(i8::from_be_bytes(need(1)?.try_into().unwrap()) as i64),
                1,
            ),
            2 => (
                Value::Int(i16::from_be_bytes(need(2)?.try_into().unwrap()) as i64),
                2,
            ),
            3 => {
                let b: [u8; 3] = need(3)?.try_into().unwrap();
                let n = i32::from_be_bytes([(b[0] as i8) as u8, b[1], b[2], 0]) >> 8;
                (Value::Int(n as i64), 3)
            }
            4 => (
                Value::Int(i32::from_be_bytes(need(4)?.try_into().unwrap()) as i64),
                4,
            ),
            5 => {
                let b: [u8; 6] = need(6)?.try_into().unwrap();
                let mut v = [if b[0] & 0x80 != 0 { 0xff } else { 0 }; 8];
                v[2..].copy_from_slice(&b);
                (Value::Int(i64::from_be_bytes(v)), 6)
            }
            6 => (
                Value::Int(i64::from_be_bytes(need(8)?.try_into().unwrap())),
                8,
            ),
            7 => (
                Value::Real(f64::from_be_bytes(need(8)?.try_into().unwrap())),
                8,
            ),
            8 => (Value::Int(0), 0),
            9 => (Value::Int(1), 0),
            10 | 11 => (Value::Null, 0),
            n if n >= 12 => {
                let len = ((n - 12) / 2) as usize;
                let raw = need(len)?;
                if n % 2 == 0 {
                    (Value::Blob(raw.to_vec()), len)
                } else {
                    (Value::Text(self.decode_text(raw)?), len)
                }
            }
            other => return Err(format!("bad serial type {other}")),
        })
    }

    fn decode_text(&self, raw: &[u8]) -> Result<String, String> {
        match self.utf16 {
            None => Ok(String::from_utf8_lossy(raw).into_owned()),
            Some(big_endian) => {
                let units: Vec<u16> = raw
                    .chunks_exact(2)
                    .map(|c| {
                        if big_endian {
                            u16::from_be_bytes([c[0], c[1]])
                        } else {
                            u16::from_le_bytes([c[0], c[1]])
                        }
                    })
                    .collect();
                Ok(String::from_utf16_lossy(&units))
            }
        }
    }
}

/// Extracts column names from a `CREATE TABLE` statement.
///
/// Only column definitions produce names; trailing table-level constraints
/// (`UNIQUE(...)`, `FOREIGN KEY(...)`) must be skipped, since their presence
/// would shift every column index in the decoded record.
fn parse_columns(sql: &str) -> Vec<String> {
    let Some(open) = sql.find('(') else {
        return Vec::new();
    };
    let Some(close) = sql.rfind(')') else {
        return Vec::new();
    };
    if close <= open {
        return Vec::new();
    }
    let body = &sql[open + 1..close];
    let mut columns = Vec::new();
    for part in split_top_level(body) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        // A leading quoted identifier may contain spaces, so read it whole
        // rather than taking the first whitespace-delimited token.
        let (first, rest) = take_identifier(part);
        if first.is_empty() {
            continue;
        }
        // Table-level constraints start with a bare keyword, never a quote.
        let bare = !part.starts_with(['"', '`', '[', '\'']);
        let keyword = first.to_ascii_uppercase();
        if bare
            && matches!(
                keyword.as_str(),
                "PRIMARY" | "UNIQUE" | "CHECK" | "FOREIGN" | "CONSTRAINT"
            )
        {
            continue;
        }
        let _ = rest;
        columns.push(first);
    }
    columns
}

/// Splits the leading identifier from the rest of a column definition.
///
/// Handles `"quoted name"`, `` `quoted` ``, `[bracketed]` and bare names.
fn take_identifier(part: &str) -> (String, &str) {
    let mut chars = part.char_indices();
    let Some((_, first)) = chars.next() else {
        return (String::new(), "");
    };
    let closer = match first {
        '"' => Some('"'),
        '`' => Some('`'),
        '[' => Some(']'),
        '\'' => Some('\''),
        _ => None,
    };
    match closer {
        Some(closer) => {
            let start = first.len_utf8();
            match part[start..].find(closer) {
                Some(offset) => {
                    let name = &part[start..start + offset];
                    let rest_start = start + offset + closer.len_utf8();
                    (name.to_string(), &part[rest_start..])
                }
                // Unterminated quote: take the remainder as the name.
                None => (part[start..].to_string(), ""),
            }
        }
        None => {
            // A bare name ends at whitespace or at the start of a constraint
            // body, e.g. `UNIQUE(releaseKey)` is the keyword `UNIQUE`.
            let end = part
                .find(|c: char| c.is_whitespace() || c == '(')
                .unwrap_or(part.len());
            (part[..end].to_string(), &part[end..])
        }
    }
}

/// Splits on commas that are not inside parentheses or quotes.
fn split_top_level(body: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for ch in body.chars() {
        match quote {
            Some(q) => {
                current.push(ch);
                if ch == q {
                    quote = None;
                }
                continue;
            }
            None => match ch {
                '\'' | '"' | '`' => {
                    quote = Some(ch);
                    current.push(ch);
                }
                '(' => {
                    depth += 1;
                    current.push(ch);
                }
                ')' => {
                    depth -= 1;
                    current.push(ch);
                }
                ',' if depth == 0 => {
                    parts.push(std::mem::take(&mut current));
                }
                _ => current.push(ch),
            },
        }
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

fn be_u16(data: &[u8], at: usize) -> Option<u16> {
    let s = data.get(at..at + 2)?;
    Some(u16::from_be_bytes([s[0], s[1]]))
}

fn be_u32(data: &[u8], at: usize) -> Option<u32> {
    let s = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// SQLite varints are big-endian, 7 bits per byte, up to 9 bytes.
fn read_varint(data: &[u8], at: usize) -> Result<(u64, usize), String> {
    let mut result: u64 = 0;
    for i in 0..9 {
        let byte = *data
            .get(at + i)
            .ok_or_else(|| "truncated varint".to_string())?;
        if i == 8 {
            result = (result << 8) | byte as u64;
            return Ok((result, at + 9));
        }
        result = (result << 7) | (byte & 0x7f) as u64;
        if byte & 0x80 == 0 {
            return Ok((result, at + i + 1));
        }
    }
    Err("varint too long".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_decoding() {
        assert_eq!(read_varint(&[0x00], 0).unwrap(), (0, 1));
        assert_eq!(read_varint(&[0x7f], 0).unwrap(), (127, 1));
        assert_eq!(read_varint(&[0x81, 0x00], 0).unwrap(), (128, 2));
        assert_eq!(read_varint(&[0x81, 0x7f], 0).unwrap(), (255, 2));
        assert!(read_varint(&[0x80], 0).is_err());
    }

    #[test]
    fn column_parsing_skips_constraints() {
        let sql = "CREATE TABLE LibraryReleases (releaseKey TEXT PRIMARY KEY, \
                   gameId TEXT, title TEXT, nested TEXT DEFAULT (json('{}')), \
                   UNIQUE(releaseKey))";
        assert_eq!(
            parse_columns(sql),
            vec!["releaseKey", "gameId", "title", "nested"]
        );
    }

    #[test]
    fn column_parsing_handles_quoted_names() {
        let sql = r#"CREATE TABLE t ("weird name" TEXT, normal INTEGER)"#;
        assert_eq!(parse_columns(sql), vec!["weird name", "normal"]);
    }

    #[test]
    fn split_respects_nesting_and_quotes() {
        assert_eq!(
            split_top_level("a, f(x,y), 'c,d'"),
            vec!["a", " f(x,y)", " 'c,d'"]
        );
    }

    #[test]
    fn rejects_non_sqlite_input() {
        assert!(Database::from_bytes(vec![0u8; 200]).is_err());
        assert!(Database::from_bytes(b"SQLite format 3\0".to_vec()).is_err());
    }
}
