//! Parser for Valve's KeyValues ("VDF") format.
//!
//! Steam stores everything in this format: `libraryfolders.vdf` (where the
//! libraries live), `appmanifest_<appid>.acf` (what is installed),
//! `localconfig.vdf` (playtime) and `loginusers.vdf`. It is a simple
//! nested `"key" "value"` tree, so a hand-written parser keeps the
//! dependency list short and the binary small.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Map(BTreeMap<String, Value>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            Value::Map(_) => None,
        }
    }

    pub fn as_map(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Map(m) => Some(m),
            Value::Str(_) => None,
        }
    }

    /// Case-insensitive child lookup: Steam is inconsistent about casing
    /// (`appid` in one file, `appID` in another).
    pub fn get(&self, key: &str) -> Option<&Value> {
        let map = self.as_map()?;
        if let Some(v) = map.get(key) {
            return Some(v);
        }
        map.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.get_str(key)?.trim().parse().ok()
    }

    pub fn get_u32(&self, key: &str) -> Option<u32> {
        self.get_str(key)?.trim().parse().ok()
    }

    /// Iterate the values of a map, ignoring keys.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.as_map().into_iter().flat_map(|m| m.values())
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            pos: 0,
        }
    }

    fn skip_trivia(&mut self) {
        loop {
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            // `//` line comment.
            if self.bytes[self.pos..].starts_with(b"//") {
                while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            // `/* */` block comment.
            if self.bytes[self.pos..].starts_with(b"/*") {
                self.pos += 2;
                while self.pos < self.bytes.len() && !self.bytes[self.pos..].starts_with(b"*/") {
                    self.pos += 1;
                }
                self.pos = (self.pos + 2).min(self.bytes.len());
                continue;
            }
            break;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_trivia();
        self.bytes.get(self.pos).copied()
    }

    /// Reads either a quoted string or a bare token.
    fn read_token(&mut self) -> Result<String, String> {
        self.skip_trivia();
        if self.pos >= self.bytes.len() {
            return Err("unexpected end of input".into());
        }
        if self.bytes[self.pos] == b'"' {
            self.pos += 1;
            let mut out = String::new();
            while self.pos < self.bytes.len() {
                match self.bytes[self.pos] {
                    b'"' => {
                        self.pos += 1;
                        return Ok(out);
                    }
                    b'\\' => {
                        self.pos += 1;
                        match self.bytes.get(self.pos).copied() {
                            Some(b'n') => out.push('\n'),
                            Some(b't') => out.push('\t'),
                            Some(b'r') => out.push('\r'),
                            // Windows paths arrive as `C:\\Games\\Steam`.
                            Some(b'\\') => out.push('\\'),
                            Some(b'"') => out.push('"'),
                            Some(c) => out.push(c as char),
                            None => return Err("dangling escape".into()),
                        }
                        self.pos += 1;
                    }
                    c => {
                        // Values may contain UTF-8; copy the whole char.
                        let start = self.pos;
                        self.pos += utf8_len(c);
                        out.push_str(
                            std::str::from_utf8(&self.bytes[start..self.pos])
                                .map_err(|e| e.to_string())?,
                        );
                    }
                }
            }
            Err("unterminated string".into())
        } else {
            let start = self.pos;
            while self.pos < self.bytes.len()
                && !self.bytes[self.pos].is_ascii_whitespace()
                && self.bytes[self.pos] != b'"'
                && self.bytes[self.pos] != b'{'
                && self.bytes[self.pos] != b'}'
            {
                self.pos += 1;
            }
            if start == self.pos {
                return Err(format!("unexpected byte at {}", self.pos));
            }
            Ok(String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned())
        }
    }

    /// Parses the body of a map, assuming the opening brace was consumed.
    fn read_map_body(&mut self) -> Result<BTreeMap<String, Value>, String> {
        let mut map = BTreeMap::new();
        loop {
            match self.peek() {
                None => return Err("unterminated block".into()),
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(map);
                }
                _ => {}
            }
            let key = self.read_token()?;
            match self.peek() {
                Some(b'{') => {
                    self.pos += 1;
                    map.insert(key, Value::Map(self.read_map_body()?));
                }
                Some(b'}') => {
                    // Tolerate a trailing key with no value.
                    map.insert(key, Value::Str(String::new()));
                }
                None => return Err("unterminated block".into()),
                _ => {
                    let val = self.read_token()?;
                    map.insert(key, Value::Str(val));
                }
            }
        }
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// Parses a whole VDF document. The returned value is normally a `Map`.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser::new(text);
    match p.peek() {
        None => Ok(Value::Map(BTreeMap::new())),
        // Anonymous root: `{ "a" "b" }`
        Some(b'{') => {
            p.pos += 1;
            Ok(Value::Map(p.read_map_body()?))
        }
        _ => {
            let key = p.read_token()?;
            if p.peek() == Some(b'{') {
                p.pos += 1;
                let body = p.read_map_body()?;
                let mut root = BTreeMap::new();
                root.insert(key, Value::Map(body));
                Ok(Value::Map(root))
            } else {
                Ok(Value::Str(key))
            }
        }
    }
}

/// Convenience: parse and return the named root block.
#[cfg(test)]
pub fn parse_root(text: &str, root: &str) -> Result<Value, String> {
    let v = parse(text)?;
    if let Some(inner) = v.get(root) {
        return Ok(inner.clone());
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_blocks_and_types() {
        let text = r#"
        "libraryfolders"
        {
            "0"
            {
                "path"      "/home/u/.steam/steam"
                "label"     ""
                "contentid" "1234"
                "apps"
                {
                    "440"   "30000"
                    "620"   "500"
                }
            }
        }
        "#;
        let root = parse_root(text, "libraryfolders").unwrap();
        let lib = root.get("0").unwrap();
        assert_eq!(lib.get_str("path").unwrap(), "/home/u/.steam/steam");
        assert_eq!(lib.get_u32("contentid").unwrap(), 1234);
        let apps = lib.get("apps").unwrap();
        assert_eq!(apps.get_u64("440").unwrap(), 30000);
        assert_eq!(apps.values().count(), 2);
    }

    #[test]
    fn handles_escaped_windows_paths() {
        let text = r#""LibraryFolders" { "1" { "path" "D:\\SteamLibrary" } }"#;
        let root = parse(text).unwrap();
        let path = root.get("LibraryFolders").unwrap().get("1").unwrap();
        assert_eq!(path.get_str("path").unwrap(), r"D:\SteamLibrary");
    }

    #[test]
    fn is_case_insensitive_and_skips_comments() {
        let text = r#"
        "AppState"
        {
            // the id
            "AppID"   "440"   /* inline */
            "Name"    "Team Fortress 2"
            "SizeOnDisk" "25000000000"
        }
        "#;
        let root = parse_root(text, "appstate").unwrap();
        assert_eq!(root.get_u32("appid").unwrap(), 440);
        assert_eq!(root.get_str("NAME").unwrap(), "Team Fortress 2");
        assert_eq!(root.get_u64("sizeondisk").unwrap(), 25_000_000_000);
    }

    #[test]
    fn bare_tokens_without_quotes() {
        let v = parse("{ key value }").unwrap();
        assert_eq!(v.get_str("key").unwrap(), "value");
    }

    #[test]
    fn unicode_values_survive() {
        let v = parse(r#""x" { "name" "Pokémon Café" }"#).unwrap();
        assert_eq!(v.get("x").unwrap().get_str("name").unwrap(), "Pokémon Café");
    }

    #[test]
    fn unterminated_block_is_an_error() {
        assert!(parse(r#""a" { "b" "c""#).is_err());
    }
}
