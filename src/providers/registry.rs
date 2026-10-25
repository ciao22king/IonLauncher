//! Minimal Windows registry access.
//!
//! Only the read operations the providers need: single string values and
//! subkey enumeration. Using the `winreg` crate directly here keeps the
//! `unsafe` surface in one small, auditable place instead of spreading
//! through the providers.

#![cfg(target_os = "windows")]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    LocalMachine,
    CurrentUser,
}

impl Hive {
    fn to_winreg(self) -> winreg::HKEY {
        match self {
            Hive::LocalMachine => winreg::enums::HKEY_LOCAL_MACHINE,
            Hive::CurrentUser => winreg::enums::HKEY_CURRENT_USER,
        }
    }
}

/// Parses a full hive-prefixed key path such as
/// `HKEY_CURRENT_USER\Software\Valve\Steam`.
fn split_path(full: &str) -> Option<(Hive, String)> {
    let upper = full.to_ascii_uppercase();
    for (prefix, hive) in [
        ("HKEY_LOCAL_MACHINE\\", Hive::LocalMachine),
        ("HKLM\\", Hive::LocalMachine),
        ("HKEY_CURRENT_USER\\", Hive::CurrentUser),
        ("HKCU\\", Hive::CurrentUser),
    ] {
        if upper.starts_with(prefix) {
            return Some((hive, full[prefix.len()..].to_string()));
        }
    }
    None
}

/// Reads a string value from a hive-prefixed key path.
pub fn read_string(full_key: &str, value_name: &str) -> Option<String> {
    let (hive, path) = split_path(full_key)?;
    read_string_hive(hive, &path, value_name)
}

/// Reads a string value from an explicit hive + subkey.
pub fn read_string_hive(hive: Hive, subkey: &str, value_name: &str) -> Option<String> {
    use winreg::RegKey;
    let root = RegKey::predef(hive.to_winreg());
    let key = root.open_subkey(subkey).ok()?;
    key.get_value::<String, _>(value_name).ok()
}

/// Lists the immediate subkeys of an explicit hive + subkey.
pub fn subkeys(hive: Hive, subkey: &str) -> Vec<String> {
    use winreg::RegKey;
    let root = RegKey::predef(hive.to_winreg());
    let Ok(key) = root.open_subkey(subkey) else {
        return Vec::new();
    };
    key.enum_keys().flatten().collect()
}
