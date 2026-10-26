//! Update checking against the project's GitHub releases.
//!
//! IonLauncher is distributed as files you download and run, not through an
//! app store, so nothing updates it for you. This module closes that gap: it
//! asks GitHub for the newest release, compares it with the version compiled
//! into the binary, and can fetch the installer for this platform.
//!
//! Two deliberate choices:
//!
//! * The check is a plain GET to the public releases API. No account, no
//!   telemetry, nothing sent but the request itself — consistent with the
//!   launcher's promise that it uploads nothing.
//! * Downloading an update never installs it silently. We save the file and
//!   hand it to the OS, so the user sees the same installer they would have
//!   got from the website, and can cancel.

use crate::json_util;
use crate::util;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Releases are read from this repository.
pub const REPO: &str = "ciao22king/IonLauncher";

/// Version currently running, from `Cargo.toml` at compile time.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// A newer release found on GitHub, with the asset for this platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    /// Version without the leading `v`, e.g. `1.0.0`.
    pub version: String,
    /// Release page, opened in the browser when there is nothing to download.
    pub page_url: String,
    /// Download URL of the installer for this platform, if the release has one.
    pub asset_url: Option<String>,
    /// File name of that asset, used to name the download on disk.
    pub asset_name: Option<String>,
    pub size_bytes: u64,
    /// Release notes, shown in the update banner.
    pub notes: String,
}

impl Update {
    pub fn size_label(&self) -> String {
        let mb = self.size_bytes as f64 / (1024.0 * 1024.0);
        if mb >= 1024.0 {
            format!("{:.1} GB", mb / 1024.0)
        } else {
            format!("{mb:.0} MB")
        }
    }
}

/// Parses a version into comparable numbers, ignoring a leading `v`.
///
/// Non-numeric parts are dropped rather than rejected: a tag like
/// `v1.0.0-beta.1` still has to be comparable with `1.0.0`.
fn version_parts(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split(['.', '-', '+'])
        .map(|part| {
            let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse().unwrap_or(0)
        })
        .collect()
}

/// True when `candidate` is a strictly newer version than `current`.
///
/// Missing components count as zero, so `1.0` and `1.0.0` compare equal.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let a = version_parts(candidate);
    let b = version_parts(current);
    let len = a.len().max(b.len());
    for i in 0..len {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// File-name pattern of the installer for the running platform.
///
/// The patterns match what the packaging workflows publish, so a release
/// built by CI is always recognised here.
fn asset_matches(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if cfg!(target_os = "windows") {
        // `IonLauncher-<version>-Setup.exe`
        lower.starts_with("ionlauncher-") && lower.ends_with("-setup.exe")
    } else if cfg!(target_os = "macos") {
        lower.starts_with("ionlauncher-") && lower.ends_with(".dmg")
    } else {
        // Prefer the AppImage: it is the one that runs on any distribution.
        lower.starts_with("ionlauncher-") && lower.ends_with("-x86_64.appimage")
    }
}

/// Fallback for Linux when a release ships only a `.deb`.
fn fallback_matches(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("ionlauncher_") && lower.ends_with("_amd64.deb")
}

/// Parses the GitHub "latest release" payload.
///
/// Returns `None` when the release is not newer than the running version, or
/// when it is a draft or pre-release: offering those would push users onto a
/// build the maintainer has not published as stable yet.
pub fn parse_release(json: &str, current: &str) -> Option<Update> {
    let value: Value = serde_json::from_str(json).ok()?;

    if json_util::get(&value, "draft").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    if json_util::get(&value, "prerelease").and_then(Value::as_bool) == Some(true) {
        return None;
    }

    let tag = json_util::get_str(&value, "tag_name")?;
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    if !is_newer(&version, current) {
        return None;
    }

    let page_url = json_util::get_str(&value, "html_url")
        .unwrap_or_else(|| format!("https://github.com/{REPO}/releases/tag/{tag}"));

    let assets = json_util::get_array(&value, "assets");
    let pick = |predicate: fn(&str) -> bool| -> Option<(String, String, u64)> {
        assets.iter().find_map(|asset| {
            let name = json_util::get_str(asset, "name")?;
            if !predicate(&name) {
                return None;
            }
            let url = json_util::get_str(asset, "browser_download_url")?;
            let size = json_util::get_u64(asset, "size").unwrap_or(0);
            Some((name, url, size))
        })
    };

    let chosen = pick(asset_matches).or_else(|| {
        // Only Linux has a second acceptable format.
        if cfg!(target_os = "linux") {
            pick(fallback_matches)
        } else {
            None
        }
    });

    let (asset_name, asset_url, size_bytes) = match chosen {
        Some((name, url, size)) => (Some(name), Some(url), size),
        None => (None, None, 0),
    };

    Some(Update {
        version,
        page_url,
        asset_url,
        asset_name,
        size_bytes,
        notes: json_util::get_str(&value, "body").unwrap_or_default(),
    })
}

/// Asks GitHub for the newest release.
///
/// Blocks, so call it from a worker thread. `None` means "up to date"; an
/// error means the check itself could not be completed, which the UI shows
/// differently because it is not the user's problem to fix.
pub fn check() -> Result<Option<Update>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("IonLauncher/", env!("CARGO_PKG_VERSION")))
        .build();

    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let response = match agent
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(response) => response,
        // A repository with no releases yet answers 404. That is "up to
        // date", not an error worth alarming anyone about.
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(ureq::Error::Status(code, _)) => {
            return Err(format!("GitHub returned HTTP {code}"));
        }
        Err(err) => return Err(format!("could not reach GitHub: {err}")),
    };

    let body = response
        .into_string()
        .map_err(|e| format!("could not read the response: {e}"))?;

    Ok(parse_release(&body, current_version()))
}

/// Where a downloaded installer is written.
///
/// The user's own download directory is deliberate: the file is exactly the
/// one they would have downloaded themselves, and they can see it.
pub fn download_dir() -> Option<PathBuf> {
    let dir = dirs::download_dir().or_else(dirs::home_dir)?;
    let target = dir.join("IonLauncher-updates");
    std::fs::create_dir_all(&target).ok()?;
    Some(target)
}

/// Downloads the update in the background, reporting progress as it goes.
///
/// The installer is not run: the UI offers that as a separate, explicit step.
pub fn download(update: Update, progress: Arc<util::DownloadProgress>) {
    std::thread::spawn(move || {
        if let Err(err) = run_download(&update, &progress) {
            progress.set_error(err);
        }
    });
}

fn run_download(update: &Update, progress: &util::DownloadProgress) -> Result<(), String> {
    let Some(url) = update.asset_url.clone() else {
        return Err("this release has no installer for your platform".into());
    };
    let Some(dir) = download_dir() else {
        return Err("could not find a download directory".into());
    };

    let name = update
        .asset_name
        .clone()
        .unwrap_or_else(|| format!("IonLauncher-{}.installer", update.version));
    let target = dir.join(&name);

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(1800))
        .user_agent(concat!("IonLauncher/", env!("CARGO_PKG_VERSION")))
        .build();

    let response = agent
        .get(&url)
        .call()
        .map_err(|e| format!("download failed: {e}"))?;

    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(update.size_bytes);
    progress
        .total
        .store(total, std::sync::atomic::Ordering::Relaxed);

    // Streamed to a temporary file and renamed at the end, so an interrupted
    // download cannot leave a truncated file that looks installable.
    let staging = target.with_extension("part");
    let mut file = std::fs::File::create(&staging)
        .map_err(|e| format!("cannot write to {}: {e}", staging.display()))?;

    let mut reader = response.into_reader();
    let mut buffer = [0u8; 256 * 1024];
    loop {
        let read = std::io::Read::read(&mut reader, &mut buffer)
            .map_err(|e| format!("download interrupted: {e}"))?;
        if read == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buffer[..read])
            .map_err(|e| format!("cannot write to disk: {e}"))?;
        progress
            .written
            .fetch_add(read as u64, std::sync::atomic::Ordering::Relaxed);
    }
    drop(file);

    if progress.written.load(std::sync::atomic::Ordering::Relaxed) == 0 {
        std::fs::remove_file(&staging).ok();
        return Err("the download was empty".into());
    }

    std::fs::rename(&staging, &target).map_err(|e| format!("cannot finish the download: {e}"))?;

    progress.finish(target.display().to_string());
    Ok(())
}

/// Opens a downloaded installer with the OS, so the user completes the same
/// install flow they would have from the website.
pub fn run_installer(path: &str) -> Result<(), String> {
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err("the downloaded file is gone".into());
    }
    util::open_url(&path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison_orders_numerically_not_lexically() {
        // The trap this guards: "1.10.0" < "1.9.0" as strings.
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(!is_newer("1.9.0", "1.10.0"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(is_newer("1.0.1", "1.0.0"));
    }

    #[test]
    fn equal_and_older_versions_are_not_updates() {
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
        // Missing components read as zero.
        assert!(!is_newer("1.0", "1.0.0"));
        assert!(is_newer("1.0.1", "1.0"));
    }

    #[test]
    fn tags_may_carry_a_v_prefix_and_suffix() {
        assert!(is_newer("v1.1.0", "1.0.0"));
        assert!(is_newer("1.1.0-beta.1", "1.0.0"));
        assert!(!is_newer("v1.0.0", "1.0.0"));
    }

    #[test]
    fn a_newer_release_is_parsed_with_its_asset() {
        let json = r#"{
            "tag_name": "v1.0.0",
            "html_url": "https://github.com/ciao22king/IonLauncher/releases/tag/v1.0.0",
            "body": "First stable release.",
            "assets": [
                {"name": "IonLauncher-1.0.0-Setup.exe", "browser_download_url": "https://example.test/setup.exe", "size": 1234},
                {"name": "ionlauncher_1.0.0_amd64.deb", "browser_download_url": "https://example.test/app.deb", "size": 999},
                {"name": "ionlauncher-1.0.0-x86_64.AppImage", "browser_download_url": "https://example.test/app.AppImage", "size": 4321}
            ]
        }"#;
        let update = parse_release(json, "0.1.0").expect("should be an update");
        assert_eq!(update.version, "1.0.0");
        assert_eq!(update.notes, "First stable release.");
        // Exactly one asset must be chosen, and it must be the right kind.
        let name = update.asset_name.clone().unwrap();
        if cfg!(target_os = "windows") {
            assert_eq!(name, "IonLauncher-1.0.0-Setup.exe");
        } else if cfg!(target_os = "macos") {
            assert!(name.ends_with(".dmg"), "unexpected asset {name}");
        } else {
            assert!(name.ends_with(".AppImage"), "unexpected asset {name}");
            assert_eq!(update.size_bytes, 4321);
        }
    }

    #[test]
    fn the_same_version_is_not_an_update() {
        let json = r#"{"tag_name": "v1.0.0", "html_url": "u", "assets": []}"#;
        assert!(parse_release(json, "1.0.0").is_none());
    }

    #[test]
    fn drafts_and_prereleases_are_ignored() {
        // A maintainer staging a release must not push it to users.
        let draft = r#"{"tag_name": "v9.9.9", "draft": true, "assets": []}"#;
        assert!(parse_release(draft, "1.0.0").is_none());
        let pre = r#"{"tag_name": "v9.9.9", "prerelease": true, "assets": []}"#;
        assert!(parse_release(pre, "1.0.0").is_none());
    }

    #[test]
    fn a_release_without_a_matching_asset_still_reports_the_update() {
        // The user can still be told, and sent to the release page, even if
        // this platform has no file attached.
        let json = r#"{
            "tag_name": "v2.0.0",
            "html_url": "https://example.test/tag",
            "assets": [{"name": "source.tar.gz", "browser_download_url": "u", "size": 1}]
        }"#;
        let update = parse_release(json, "1.0.0").expect("should be an update");
        assert_eq!(update.version, "2.0.0");
        assert!(update.asset_url.is_none());
        assert_eq!(update.page_url, "https://example.test/tag");
    }

    #[test]
    fn malformed_payloads_do_not_panic() {
        assert!(parse_release("not json", "1.0.0").is_none());
        assert!(parse_release("{}", "1.0.0").is_none());
        assert!(parse_release(r#"{"tag_name": ""}"#, "1.0.0").is_none());
        // An update with no assets array at all.
        let no_assets = r#"{"tag_name": "v3.0.0"}"#;
        let update = parse_release(no_assets, "1.0.0").expect("should be an update");
        assert!(update.asset_url.is_none());
        assert!(update.page_url.contains("3.0.0"));
    }

    #[test]
    fn the_running_version_is_the_package_version() {
        assert_eq!(current_version(), env!("CARGO_PKG_VERSION"));
        assert!(!is_newer(current_version(), current_version()));
    }
}
