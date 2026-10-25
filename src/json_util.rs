//! Helpers for reading third-party JSON with unknown or inconsistent casing.
//!
//! Epic manifests and Heroic/Legendary files have changed key names and
//! capitalisation between versions (`AppName` vs `app_name`,
//! `InstallLocation` vs `install_path`), so every lookup here is
//! case-insensitive and tolerant of missing keys.

use serde_json::Value;

/// Case-insensitive object lookup.
///
/// Underscores are ignored as well as case, because the same field appears as
/// `InstallSize` in Epic manifests and `install_size` in Heroic/Legendary
/// files; both must resolve from one call site.
pub fn get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    let obj = value.as_object()?;
    if let Some(v) = obj.get(key) {
        return Some(v);
    }
    let wanted = normalize_key(key);
    obj.iter()
        .find(|(k, _)| normalize_key(k) == wanted)
        .map(|(_, v)| v)
}

/// Lowercases and drops underscores and spaces, for tolerant key comparison.
fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|c| *c != '_' && *c != ' ' && *c != '-')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Case-insensitive string lookup, treating `null` as absent.
pub fn get_str(value: &Value, key: &str) -> Option<String> {
    match get(value, key)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub fn get_u64(value: &Value, key: &str) -> Option<u64> {
    match get(value, key)? {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)),
        // Some stores write sizes as strings.
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn get_array<'a>(value: &'a Value, key: &str) -> Vec<&'a Value> {
    match get(value, key) {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(other) => vec![other],
        None => Vec::new(),
    }
}

/// Iterates the entries of a JSON object, or of an array of objects.
///
/// This covers both shapes used on disk: `{"<id>": {...}}` and `[{...}, {...}]`.
pub fn entries(value: &Value) -> Vec<&Value> {
    match value {
        Value::Object(map) => map.values().collect(),
        Value::Array(items) => items.iter().collect(),
        _ => Vec::new(),
    }
}

/// Percent-encodes everything outside the unreserved set.
///
/// Epic launch URLs embed a `namespace:catalogId:appName` triple, and the
/// separators must be escaped or the handler mis-parses the path.
pub fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lookups_ignore_casing() {
        let v = json!({ "AppName": "Fortnite", "install_size": "42" });
        assert_eq!(get_str(&v, "appname").unwrap(), "Fortnite");
        assert_eq!(get_str(&v, "APPNAME").unwrap(), "Fortnite");
        assert_eq!(get_u64(&v, "InstallSize").unwrap(), 42);
    }

    #[test]
    fn empty_strings_are_treated_as_absent() {
        let v = json!({ "DisplayName": "" });
        assert!(get_str(&v, "DisplayName").is_none());
        assert!(get_str(&v, "Missing").is_none());
    }

    #[test]
    fn numbers_stringify_for_label_fields() {
        let v = json!({ "gameID": 1207658691 });
        assert_eq!(get_str(&v, "gameid").unwrap(), "1207658691");
        assert_eq!(get_u64(&v, "gameid").unwrap(), 1207658691);
    }

    #[test]
    fn entries_handle_map_and_array_shapes() {
        let map = json!({ "a": {"t": "A"}, "b": {"t": "B"} });
        assert_eq!(entries(&map).len(), 2);
        let arr = json!([{"t": "A"}, {"t": "B"}]);
        assert_eq!(entries(&arr).len(), 2);
        assert!(entries(&json!("scalar")).is_empty());
    }

    #[test]
    fn percent_encoding_escapes_separators() {
        assert_eq!(percent_encode("abc-1_2.3~"), "abc-1_2.3~");
        assert_eq!(percent_encode("a:b"), "a%3Ab");
        assert_eq!(percent_encode("a b/c"), "a%20b%2Fc");
        // The Epic triple must round-trip through the escape intact.
        assert_eq!(
            percent_encode("ns:cat:app"),
            "ns%3Acat%3Aapp"
        );
    }
}
