//! Small filesystem, time and process helpers.
//!
//! Everything here is defensive by design: launcher discovery walks
//! directories that may vanish mid-scan, and store config files are often
//! mid-write while we read them. None of these helpers panic.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Reads a file as UTF-8, refusing anything larger than `max_bytes`.
///
/// The size guard matters: `localconfig.vdf` can grow to tens of megabytes
/// on heavily used accounts, and we only ever need a slice of it.
pub fn read_text_capped(path: &Path, max_bytes: u64) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > max_bytes {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reads a file with a generous default cap.
pub fn read_text(path: &Path) -> Option<String> {
    read_text_capped(path, 64 * 1024 * 1024)
}

/// Lists immediate subdirectories, ignoring unreadable entries.
pub fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect()
}

/// Lists immediate child files with the given extension (case-insensitive).
pub fn files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        })
        .collect();
    out.sort();
    out
}

/// Total size of a directory tree, in bytes. Bounded to keep scans fast.
pub fn dir_size(dir: &Path, max_entries: usize) -> Option<u64> {
    let mut total = 0u64;
    let mut seen = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > max_entries {
                return Some(total);
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    Some(total)
}

pub fn to_epoch(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

pub fn now_epoch() -> u64 {
    to_epoch(SystemTime::now()).unwrap_or(0)
}

/// Parses a timestamp written in several common store formats.
pub fn parse_timestamp(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // Unix seconds / milliseconds.
    if let Ok(n) = raw.parse::<i64>() {
        if n <= 0 {
            return None;
        }
        return Some(if n > 10_000_000_000 { n as u64 / 1000 } else { n as u64 });
    }
    // ISO-8601: 2024-05-01T12:30:00Z or with an offset.
    parse_iso8601(raw)
}

/// Minimal ISO-8601 parser (date + optional time), no external crate.
fn parse_iso8601(raw: &str) -> Option<u64> {
    let bytes = raw.as_bytes();
    if bytes.len() < 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i64 = raw.get(0..4)?.parse().ok()?;
    let month: i64 = raw.get(5..7)?.parse().ok()?;
    let day: i64 = raw.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (mut hour, mut minute, mut sec) = (0i64, 0i64, 0i64);
    if bytes.len() >= 16 && bytes[10] == b'T' {
        hour = raw.get(11..13)?.parse().ok()?;
        minute = raw.get(14..16)?.parse().ok()?;
        if bytes.len() >= 19 {
            sec = raw.get(17..19)?.parse().ok()?;
        }
    }
    // Days since the Unix epoch, via the civil-from-days algorithm.
    let days = days_from_civil(year, month, day);
    let mut ts = days * 86_400 + hour * 3600 + minute * 60 + sec;
    // Normalise an explicit UTC offset, if present.
    if let Some(pos) = raw.rfind(['+', '-']).filter(|p| *p > 10) {
        if let Some(offset) = parse_offset(&raw[pos..]) {
            ts -= offset;
        }
    }
    if ts < 0 {
        return None;
    }
    Some(ts as u64)
}

fn parse_offset(text: &str) -> Option<i64> {
    let sign = match text.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let body = text.get(1..)?;
    let (h, m) = if let Some((h, m)) = body.split_once(':') {
        (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?)
    } else if body.len() == 4 {
        (body[0..2].parse().ok()?, body[2..4].parse().ok()?)
    } else if body.len() == 2 {
        (body.parse().ok()?, 0)
    } else {
        return None;
    };
    Some(sign * (h * 3600 + m * 60))
}

/// Howard Hinnant's `days_from_civil`, which avoids pulling in a date crate.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Human-readable relative age, e.g. `today`, `3 d ago`, `2 mo ago`.
pub fn relative_age(epoch: u64) -> String {
    let now = now_epoch();
    if epoch == 0 || epoch > now {
        return String::new();
    }
    let secs = now - epoch;
    let days = secs / 86_400;
    match days {
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        2..=30 => format!("{days} d ago"),
        31..=364 => format!("{} mo ago", days / 30),
        _ => {
            let years = days / 365;
            if years == 1 {
                "1 y ago".to_string()
            } else {
                format!("{years} y ago")
            }
        }
    }
}

/// Opens a URL with the OS handler. Used for `steam://`, `com.epicgames.launcher://`
/// and `goggalaxy://` protocol links, which keep store features (DRM, cloud
/// saves, achievements, overlay) working the way the store expects.
pub fn open_url(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("empty launch URL".into());
    }
    #[cfg(target_os = "windows")]
    {
        // `start` is a cmd builtin. The empty "" is the window-title argument,
        // which is required so a quoted URL is not mistaken for a title.
        spawn(
            Command::new("cmd").args(["/C", "start", "", url]),
            "cmd /C start",
        )
    }
    #[cfg(target_os = "macos")]
    {
        spawn(Command::new("open").arg(url), "open")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Prefer the desktop handler; fall back to gio/steam when xdg-open
        // is missing on minimal systems.
        match spawn(Command::new("xdg-open").arg(url), "xdg-open") {
            Ok(()) => Ok(()),
            Err(first) => {
                if let Ok(()) = spawn(Command::new("gio").args(["open", url]), "gio open") {
                    return Ok(());
                }
                Err(first)
            }
        }
    }
}

/// Launches an executable detached from IonLauncher, so closing the launcher
/// does not kill the game.
pub fn spawn_exe(exe: &Path, args: &[String], workdir: Option<&Path>) -> Result<(), String> {
    if !exe.exists() {
        return Err(format!("executable not found: {}", exe.display()));
    }
    let mut cmd = Command::new(exe);
    cmd.args(args);
    if let Some(dir) = workdir {
        if dir.is_dir() {
            cmd.current_dir(dir);
        }
    }
    spawn(&mut cmd, &exe.display().to_string())
}

fn spawn(cmd: &mut Command, what: &str) -> Result<(), String> {
    // Detach stdio so the child cannot block on a closed pipe.
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to run {what}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso8601_parsing() {
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_timestamp("2024-05-01T12:30:00Z"), Some(1_714_566_600));
        assert_eq!(parse_timestamp("2024-05-01"), Some(1_714_521_600));
    }

    #[test]
    fn iso8601_honours_utc_offsets() {
        let utc = parse_timestamp("2024-05-01T12:00:00Z").unwrap();
        let plus_two = parse_timestamp("2024-05-01T12:00:00+02:00").unwrap();
        assert_eq!(utc - plus_two, 7200);
    }

    #[test]
    fn numeric_timestamps_and_units() {
        assert_eq!(parse_timestamp("1714566600"), Some(1_714_566_600));
        // Milliseconds are normalised to seconds.
        assert_eq!(parse_timestamp("1714566600000"), Some(1_714_566_600));
        assert_eq!(parse_timestamp("0"), None);
        assert_eq!(parse_timestamp("nonsense"), None);
        assert_eq!(parse_timestamp(""), None);
    }

    #[test]
    fn rejects_impossible_dates() {
        assert_eq!(parse_timestamp("2024-13-01"), None);
        assert_eq!(parse_timestamp("2024-00-10"), None);
    }

    #[test]
    fn relative_ages_read_naturally() {
        let now = now_epoch();
        assert_eq!(relative_age(now), "today");
        assert_eq!(relative_age(now - 86_400 * 3), "3 d ago");
        assert_eq!(relative_age(now - 86_400 * 60), "2 mo ago");
        assert_eq!(relative_age(now - 86_400 * 400), "1 y ago");
        assert_eq!(relative_age(0), "");
    }

    #[test]
    fn read_text_capped_enforces_limit() {
        let dir = std::env::temp_dir().join(format!("ion-util-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, "hello world").unwrap();
        assert_eq!(read_text_capped(&file, 100).unwrap(), "hello world");
        assert!(read_text_capped(&file, 5).is_none());
        assert!(read_text_capped(&dir.join("missing"), 100).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_url_rejects_empty_input() {
        assert!(open_url("").is_err());
    }

    #[test]
    fn spawn_exe_reports_missing_binary() {
        let err = spawn_exe(Path::new("/definitely/not/here"), &[], None).unwrap_err();
        assert!(err.contains("not found"), "unexpected error: {err}");
    }
}
