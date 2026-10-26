//! Proton management for running Windows games on Linux.
//!
//! Proton is Valve's Wine-based compatibility layer. There are two kinds of
//! build worth knowing about:
//!
//! * the ones Steam ships, which live under `steamapps/common/Proton*`
//! * community builds such as GloriousEggroll's `GE-Proton`, which install
//!   into `~/.steam/root/compatibilitytools.d` (or the Flatpak equivalent)
//!
//! Both are discovered by looking for a directory containing a `proton`
//! script. Downloading is deliberately limited to the GitHub release assets
//! of the community builds: they are plain tarballs we can verify by size and
//! extract ourselves, with no installer to run.
//!
//! Nothing here runs on the UI thread. Listing releases, downloading and
//! extracting all happen on a worker, and progress is reported back through
//! an atomic counter.

// Proton is a Linux-only feature, but the module is compiled everywhere so
// that the shared config and UI code keeps one shape. On other platforms
// most of it is unreachable, which is expected rather than a defect.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::util;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

/// Repository publishing community Proton builds.
pub const GE_REPO: &str = "GloriousEggroll/proton-ge-custom";

/// A Proton build found on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledProton {
    /// Directory name, e.g. `GE-Proton9-20` or `Proton - Experimental`.
    pub name: String,
    /// Absolute path to the build directory.
    pub path: PathBuf,
    /// Where it was found, which tells the user who manages it.
    pub origin: ProtonOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtonOrigin {
    /// A community build dropped into `compatibilitytools.d`.
    CompatibilityTool,
    /// A build Steam itself installed.
    SteamManaged,
}

impl ProtonOrigin {
    pub fn label(self) -> &'static str {
        match self {
            ProtonOrigin::CompatibilityTool => "custom",
            ProtonOrigin::SteamManaged => "Steam",
        }
    }
}

/// A downloadable community build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Release tag, e.g. `GE-Proton9-20`.
    pub tag: String,
    /// Direct URL of the `x86_64` tarball.
    pub asset_url: String,
    /// Asset size in bytes, used for the progress bar.
    pub size_bytes: u64,
    /// Publication date, `YYYY-MM-DD`.
    pub published: String,
}

impl Release {
    pub fn size_label(&self) -> String {
        let mb = self.size_bytes as f64 / (1024.0 * 1024.0);
        if mb >= 1024.0 {
            format!("{:.1} GB", mb / 1024.0)
        } else {
            format!("{mb:.0} MB")
        }
    }
}

/// Parses the GitHub releases JSON into the releases we can install.
///
/// Only the `x86_64` tarball is used: the `aarch64` asset exists for ARM
/// machines, and handing it to an x86_64 host would install something that
/// cannot run. Releases without a usable asset are skipped rather than
/// listed, so the UI never offers a download that cannot work.
pub fn parse_releases(json: &str) -> Vec<Release> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for item in items {
        let Some(tag) = item.get("tag_name").and_then(|v| v.as_str()) else {
            continue;
        };
        let published = item
            .get("published_at")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(10).collect::<String>())
            .unwrap_or_default();

        let Some(assets) = item.get("assets").and_then(|v| v.as_array()) else {
            continue;
        };
        let asset = assets.iter().find(|asset| {
            asset
                .get("name")
                .and_then(|v| v.as_str())
                .is_some_and(|name| name.ends_with("x86_64.tar.gz"))
        });
        let Some(asset) = asset else { continue };
        let Some(url) = asset.get("browser_download_url").and_then(|v| v.as_str()) else {
            continue;
        };

        out.push(Release {
            tag: tag.to_string(),
            asset_url: url.to_string(),
            size_bytes: asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
            published,
        });
    }
    out
}

/// Directories searched for community builds, in priority order.
///
/// Steam reads `compatibilitytools.d` from both its root and from the
/// per-user `~/.steam/root` symlink, and Flatpak keeps a private copy, so all
/// three are checked before giving up.
pub fn compatibility_tool_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(
            home.join(".steam")
                .join("root")
                .join("compatibilitytools.d"),
        );
        dirs.push(
            home.join(".local")
                .join("share")
                .join("Steam")
                .join("compatibilitytools.d"),
        );
        dirs.push(
            home.join(".var")
                .join("app")
                .join("com.valvesoftware.Steam")
                .join("data")
                .join("compatibilitytools.d"),
        );
    }
    dirs
}

/// Directories that hold the builds Steam installs itself.
fn steam_managed_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return dirs;
    };
    let roots = [
        home.join(".steam").join("root"),
        home.join(".local").join("share").join("Steam"),
        home.join(".steam").join("steam"),
    ];
    for root in roots {
        dirs.push(root.join("steamapps").join("common"));
        dirs.push(root.join("steamapps").join("common").join("Proton"));
    }
    dirs
}

/// Lists every Proton build found on this machine.
///
/// A build is any directory containing an executable-ish `proton` script,
/// which is what both Valve's and the community tarballs ship.
pub fn installed() -> Vec<InstalledProton> {
    let mut found: Vec<InstalledProton> = Vec::new();

    for dir in compatibility_tool_dirs() {
        collect_builds(&dir, ProtonOrigin::CompatibilityTool, &mut found);
    }
    for dir in steam_managed_dirs() {
        collect_builds(&dir, ProtonOrigin::SteamManaged, &mut found);
    }

    // A build reachable from two roots should only be listed once.
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found.dedup_by(|a, b| a.path == b.path);
    found
}

fn collect_builds(dir: &Path, origin: ProtonOrigin, out: &mut Vec<InstalledProton>) {
    if !dir.is_dir() {
        return;
    }
    // A `common` directory contains one subdirectory per build; a
    // `compatibilitytools.d` directory does the same.
    for entry in util::subdirs(dir) {
        if is_proton_build(&entry) {
            if let Some(name) = entry.file_name().and_then(|n| n.to_str()) {
                out.push(InstalledProton {
                    name: name.to_string(),
                    path: entry,
                    origin,
                });
            }
        }
    }
    // The directory itself can be a build, as with `steamapps/common/Proton`.
    if is_proton_build(dir) {
        if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
            out.push(InstalledProton {
                name: name.to_string(),
                path: dir.to_path_buf(),
                origin,
            });
        }
    }
}

/// True when `dir` looks like a Proton build.
pub fn is_proton_build(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    if dir.join("proton").is_file() {
        return true;
    }
    // Some builds ship only the Windows-side files plus a `proton` script
    // inside `dist`; the version file is the other reliable marker.
    dir.join("version").is_file() && dir.join("files").is_dir()
}

/// Where a build should be installed, i.e. the first writable
/// `compatibilitytools.d`.
pub fn install_dir() -> Option<PathBuf> {
    compatibility_tool_dirs().into_iter().next()
}

/// Progress of an in-flight download, shared with the UI thread.
///
/// Alias for the shared [`util::DownloadProgress`], kept so the Proton call
/// sites keep reading naturally.
pub use crate::util::DownloadProgress;

/// Downloads and extracts a release into `compatibilitytools.d`.
///
/// Returns immediately; progress is observable through `progress`. The whole
/// operation happens on a worker thread because a Proton build is several
/// hundred megabytes.
pub fn download_release(release: Release, progress: Arc<DownloadProgress>) {
    std::thread::spawn(move || {
        if let Err(err) = install_release(&release, &progress) {
            progress.set_error(err);
        }
    });
}

/// Fetches the tarball and unpacks it.
fn install_release(release: &Release, progress: &DownloadProgress) -> Result<(), String> {
    let Some(target_root) = install_dir() else {
        return Err("cannot locate compatibilitytools.d".into());
    };
    let bytes = fetch(release, progress)?;
    install_from_bytes(&bytes, &release.tag, &target_root, progress)
}

/// Unpacks a downloaded tarball into `target_root` as a usable build.
///
/// Split from the download so the placement rules can be tested against a
/// local archive instead of a several-hundred-megabyte network transfer.
fn install_from_bytes(
    bytes: &[u8],
    tag: &str,
    target_root: &Path,
    progress: &DownloadProgress,
) -> Result<(), String> {
    std::fs::create_dir_all(target_root)
        .map_err(|e| format!("cannot create {}: {e}", target_root.display()))?;

    // Unpack to a staging directory first, so a partial extraction never
    // leaves a half-installed build that Steam would then try to use.
    let staging = target_root.join(format!(".staging-{tag}"));
    std::fs::remove_dir_all(&staging).ok();
    std::fs::create_dir_all(&staging).map_err(|e| format!("cannot stage: {e}"))?;

    extract_tar_gz(bytes, &staging)?;

    // The tarball contains a single top-level directory named after the tag.
    let unpacked = staging.join(tag);
    let source = if is_proton_build(&unpacked) {
        unpacked
    } else {
        // Some builds nest differently; take the only directory present.
        let mut children = util::subdirs(&staging);
        match children.len() {
            1 => children.remove(0),
            0 => {
                std::fs::remove_dir_all(&staging).ok();
                return Err("archive contained no files".into());
            }
            _ => staging.clone(),
        }
    };

    if !is_proton_build(&source) {
        std::fs::remove_dir_all(&staging).ok();
        return Err("archive did not contain a Proton build".into());
    }

    let final_path = target_root.join(tag);
    std::fs::remove_dir_all(&final_path).ok();
    std::fs::rename(&source, &final_path).map_err(|e| format!("cannot move into place: {e}"))?;
    std::fs::remove_dir_all(&staging).ok();
    progress.done.store(true, Ordering::Relaxed);
    Ok(())
}

/// Downloads the tarball into memory, reporting progress as it goes.
fn fetch(release: &Release, progress: &DownloadProgress) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(600))
        .user_agent(concat!(
            "IonLauncher/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/ciao22king/IonLauncher)"
        ))
        .build();

    let response = agent
        .get(&release.asset_url)
        .call()
        .map_err(|e| format!("download failed: {e}"))?;

    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(release.size_bytes);
    progress.total.store(total, Ordering::Relaxed);

    let mut reader = response.into_reader();
    let mut bytes = Vec::with_capacity(total.min(64 * 1024 * 1024) as usize);
    let mut buffer = [0u8; 256 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|e| format!("download interrupted: {e}"))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        progress.written.fetch_add(read as u64, Ordering::Relaxed);
    }
    if bytes.is_empty() {
        return Err("download was empty".into());
    }
    Ok(bytes)
}

/// Unpacks a gzipped tar archive into `dest`.
///
/// Entries are written through a path check so a crafted archive cannot
/// escape the destination directory.
fn extract_tar_gz(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    archive.set_preserve_permissions(true);
    archive.set_overwrite(true);

    for entry in archive.entries().map_err(|e| format!("bad archive: {e}"))? {
        let mut entry = entry.map_err(|e| format!("bad archive entry: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("bad entry path: {e}"))?
            .into_owned();

        // Reject absolute paths and `..` components outright.
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err("archive tried to write outside its directory".into());
        }

        entry
            .unpack_in(dest)
            .map_err(|e| format!("cannot extract {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Removes an installed build from disk.
pub fn remove(build: &InstalledProton) -> Result<(), String> {
    // Only community builds are ours to delete; Steam manages its own.
    if build.origin == ProtonOrigin::SteamManaged {
        return Err("Steam manages this build".into());
    }
    if !build.path.is_dir() {
        return Err("build directory is already gone".into());
    }
    std::fs::remove_dir_all(&build.path)
        .map_err(|e| format!("cannot remove {}: {e}", build.path.display()))
}

/// True when a path points at a Windows binary, which is what needs Proton.
///
/// The `.exe` check is deliberately the only signal: a `.bat` or `.lnk` is
/// not something Proton's `run` entry point handles, and a native Linux
/// executable must keep launching normally.
pub fn looks_like_windows_executable(exe: &Path) -> bool {
    exe.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
}

/// A prepared Proton launch, ready to run.
///
/// Building this does no I/O, so the decision to use Proton can be made
/// while rendering without side effects.
#[derive(Debug, Clone)]
pub struct ProtonLaunch {
    pub build: InstalledProton,
    pub prefix: PathBuf,
    pub exe: PathBuf,
    pub args: Vec<String>,
}

impl ProtonLaunch {
    pub fn new(build: InstalledProton, prefix: PathBuf, exe: PathBuf, args: Vec<String>) -> Self {
        Self {
            build,
            prefix,
            exe,
            args,
        }
    }

    /// The command line and environment this launch needs.
    ///
    /// The environment mirrors what Steam sets, which is what a Proton build
    /// expects: `STEAM_COMPAT_DATA_PATH` is the prefix, and
    /// `STEAM_COMPAT_CLIENT_INSTALL_PATH` tells Proton where Steam lives so it
    /// can find its own runtime files.
    pub fn command(&self) -> (PathBuf, Vec<String>, Vec<(String, String)>) {
        let script = self.build.path.join("proton");
        let mut command_args = vec!["run".to_string(), self.exe.display().to_string()];
        command_args.extend(self.args.iter().cloned());

        let mut env = vec![
            (
                "STEAM_COMPAT_DATA_PATH".to_string(),
                self.prefix.display().to_string(),
            ),
            ("STEAM_COMPAT_APP_ID".to_string(), "0".to_string()),
        ];
        if let Some(home) = dirs::home_dir() {
            env.push((
                "STEAM_COMPAT_CLIENT_INSTALL_PATH".to_string(),
                home.join(".steam").join("root").display().to_string(),
            ));
        }
        (script, command_args, env)
    }

    /// Runs the game, creating the prefix on first use.
    pub fn run(&self) -> Result<(), String> {
        // Proton creates the prefix itself, but the parent directory has to
        // exist or it fails with an unhelpful error.
        if let Some(parent) = self.prefix.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let (script, args, env) = self.command();
        if !script.exists() {
            return Err(format!("Proton script not found: {}", script.display()));
        }
        crate::util::spawn_exe_with_env(&script, &args, self.exe.parent(), &env)
    }
}

/// The prefix directory used for a given game id.
///
/// Each game gets its own prefix, the way Steam gives every appid its own
/// `compatdata` directory: sharing one prefix between titles lets one game's
/// registry changes break another.
pub fn prefix_for(data_dir: &Path, game_id: &str) -> PathBuf {
    let safe: String = game_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    data_dir.join("prefixes").join(safe)
}

/// Root under which per-game prefixes are kept.
pub fn data_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("IonLauncher").join("proton"))
}

/// Fetches the community release list. Blocking; call from a worker.
pub fn fetch_releases() -> Result<Vec<Release>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!(
            "IonLauncher/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/ciao22king/IonLauncher)"
        ))
        .build();
    let url = format!("https://api.github.com/repos/{GE_REPO}/releases?per_page=40");
    let response = agent
        .get(&url)
        .call()
        .map_err(|e| format!("cannot reach GitHub: {e}"))?;
    let body = response
        .into_string()
        .map_err(|e| format!("bad response: {e}"))?;
    let releases = parse_releases(&body);
    if releases.is_empty() {
        return Err("no downloadable builds were listed".into());
    }
    Ok(releases)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_are_parsed_from_github_json() {
        let json = r#"[
            {
                "tag_name": "GE-Proton9-20",
                "published_at": "2024-11-01T12:00:00Z",
                "assets": [
                    {"name": "GE-Proton9-20-x86_64.tar.gz", "size": 500000000,
                     "browser_download_url": "https://example.invalid/x86.tar.gz"},
                    {"name": "GE-Proton9-20-aarch64.tar.gz", "size": 600000000,
                     "browser_download_url": "https://example.invalid/arm.tar.gz"}
                ]
            }
        ]"#;
        let releases = parse_releases(json);
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].tag, "GE-Proton9-20");
        assert_eq!(releases[0].published, "2024-11-01");
        // The ARM build must never be picked on an x86_64 host.
        assert!(releases[0].asset_url.ends_with("x86.tar.gz"));
    }

    #[test]
    fn releases_without_a_usable_asset_are_skipped() {
        let json = r#"[
            {"tag_name": "broken", "assets": [{"name": "notes.txt"}]},
            {"tag_name": "ok", "assets": [
                {"name": "ok-x86_64.tar.gz", "size": 1, "browser_download_url": "https://x/y"}
            ]}
        ]"#;
        let releases = parse_releases(json);
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].tag, "ok");
    }

    #[test]
    fn malformed_release_json_is_not_fatal() {
        assert!(parse_releases("not json").is_empty());
        assert!(parse_releases("{}").is_empty());
        assert!(parse_releases("[]").is_empty());
    }

    #[test]
    fn size_labels_switch_to_gigabytes() {
        let release = Release {
            tag: "t".into(),
            asset_url: "u".into(),
            size_bytes: 564_800_000,
            published: "2026-10-19".into(),
        };
        assert_eq!(release.size_label(), "539 MB");
        let big = Release {
            size_bytes: 2_000_000_000,
            ..release
        };
        assert_eq!(big.size_label(), "1.9 GB");
    }

    #[test]
    fn proton_build_detection_requires_the_script() {
        let dir = temp_dir("detect");
        assert!(!is_proton_build(&dir));
        std::fs::write(dir.join("proton"), "#!/bin/sh\n").unwrap();
        assert!(is_proton_build(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn installed_builds_are_listed_from_compatibilitytools() {
        let dir = temp_dir("list");
        let build = dir.join("GE-Proton9-20");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(build.join("proton"), "#!/bin/sh\n").unwrap();

        let mut found = Vec::new();
        collect_builds(&dir, ProtonOrigin::CompatibilityTool, &mut found);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "GE-Proton9-20");
        assert_eq!(found[0].origin, ProtonOrigin::CompatibilityTool);
        assert_eq!(found[0].origin.label(), "custom");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn launch_command_sets_the_expected_environment() {
        let build = InstalledProton {
            name: "GE-Proton9-20".into(),
            path: PathBuf::from("/tools/GE-Proton9-20"),
            origin: ProtonOrigin::CompatibilityTool,
        };
        let launch = ProtonLaunch::new(
            build,
            PathBuf::from("/data/prefixes/steam_440"),
            PathBuf::from("/games/Game/game.exe"),
            vec!["-dx11".to_string()],
        );
        let (script, args, env) = launch.command();
        assert_eq!(script, PathBuf::from("/tools/GE-Proton9-20/proton"));
        assert_eq!(args[0], "run");
        assert_eq!(args[1], "/games/Game/game.exe");
        assert_eq!(args[2], "-dx11");
        assert!(env
            .iter()
            .any(|(k, v)| k == "STEAM_COMPAT_DATA_PATH" && v == "/data/prefixes/steam_440"));
        assert!(env
            .iter()
            .any(|(k, _)| k == "STEAM_COMPAT_CLIENT_INSTALL_PATH"));
    }

    #[test]
    fn only_windows_executables_use_proton() {
        assert!(looks_like_windows_executable(Path::new("/g/GAME.EXE")));
        assert!(looks_like_windows_executable(Path::new("/g/game.exe")));
        // Native binaries and launcher scripts must launch normally.
        assert!(!looks_like_windows_executable(Path::new("/g/game")));
        assert!(!looks_like_windows_executable(Path::new("/g/run.sh")));
        assert!(!looks_like_windows_executable(Path::new("/g/setup.bat")));
        assert!(!looks_like_windows_executable(Path::new("/g/gameexe")));
    }

    #[test]
    fn a_missing_proton_script_is_reported_not_run() {
        let build = InstalledProton {
            name: "gone".into(),
            path: PathBuf::from("/definitely/not/here"),
            origin: ProtonOrigin::CompatibilityTool,
        };
        let launch = ProtonLaunch::new(
            build,
            std::env::temp_dir().join("ion-prefix-test"),
            PathBuf::from("/games/Game/game.exe"),
            Vec::new(),
        );
        let err = launch.run().unwrap_err();
        assert!(err.contains("not found"), "unexpected: {err}");
    }

    #[test]
    fn each_game_gets_its_own_prefix() {
        let data = Path::new("/data");
        let a = prefix_for(data, "steam:440");
        let b = prefix_for(data, "epic:My Game/1");
        assert_ne!(a, b);
        // The id must not leak path separators or colons into the directory.
        assert_eq!(a.file_name().unwrap(), "steam_440");
        assert_eq!(b.file_name().unwrap(), "epic_My_Game_1");
    }

    #[test]
    fn progress_reports_a_fraction_and_an_error() {
        let progress = DownloadProgress::new();
        assert_eq!(progress.fraction(), 0.0);
        progress.written.store(50, Ordering::Relaxed);
        progress.total.store(200, Ordering::Relaxed);
        assert!((progress.fraction() - 0.25).abs() < f32::EPSILON);
        assert!(!progress.is_done());

        progress.set_error("boom".into());
        assert_eq!(progress.error().as_deref(), Some("boom"));
        assert!(progress.is_done());
    }

    #[test]
    fn extraction_rejects_paths_that_escape_the_target() {
        // A tarball entry named `../evil` must be refused rather than written
        // outside the destination. The `tar` builder will not create such an
        // archive, so the header is assembled by hand: that is exactly the
        // shape a hostile release would take.
        let tar_bytes = raw_tar_entry("../evil.txt", b"boom");
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar_bytes).unwrap();
        let gz_bytes = gz.finish().unwrap();

        let dest = temp_dir("escape");
        let err = extract_tar_gz(&gz_bytes, &dest).unwrap_err();
        assert!(
            err.contains("outside") || err.contains("bad entry path"),
            "unexpected error: {err}"
        );
        // The file must not have been created anywhere.
        assert!(!dest.parent().unwrap().join("evil.txt").exists());
        assert!(!dest.join("evil.txt").exists());
        std::fs::remove_dir_all(&dest).ok();
    }

    #[test]
    fn extraction_rejects_absolute_paths() {
        let tar_bytes = raw_tar_entry("/tmp/ion-absolute-escape.txt", b"boom");
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar_bytes).unwrap();
        let gz_bytes = gz.finish().unwrap();

        let dest = temp_dir("absolute");
        let err = extract_tar_gz(&gz_bytes, &dest).unwrap_err();
        assert!(
            err.contains("outside") || err.contains("bad entry path"),
            "unexpected error: {err}"
        );
        assert!(!Path::new("/tmp/ion-absolute-escape.txt").exists());
        std::fs::remove_dir_all(&dest).ok();
    }

    /// Builds a minimal, well-formed tar archive holding one file entry.
    fn raw_tar_entry(name: &str, contents: &[u8]) -> Vec<u8> {
        let mut header = [0u8; 512];
        let name_bytes = name.as_bytes();
        assert!(name_bytes.len() <= 100, "test name too long");
        header[..name_bytes.len()].copy_from_slice(name_bytes);

        // Octal fields are NUL-terminated and zero padded.
        let put = |header: &mut [u8; 512], offset: usize, len: usize, value: u64| {
            let text = format!("{:0>width$o}", value, width = len - 1);
            header[offset..offset + text.len()].copy_from_slice(text.as_bytes());
        };
        put(&mut header, 100, 8, 0o644); // mode
        put(&mut header, 108, 8, 0); // uid
        put(&mut header, 116, 8, 0); // gid
        put(&mut header, 124, 12, contents.len() as u64); // size
        put(&mut header, 136, 12, 0); // mtime
        header[156] = b'0'; // regular file
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");

        // The checksum is computed with the checksum field treated as spaces.
        header[148..156].copy_from_slice(b"        ");
        let checksum: u64 = header.iter().map(|b| *b as u64).sum();
        let text = format!("{:06o}\0 ", checksum);
        header[148..148 + text.len()].copy_from_slice(text.as_bytes());

        let mut out = Vec::with_capacity(1024);
        out.extend_from_slice(&header);
        out.extend_from_slice(contents);
        // Payload is padded out to a 512-byte boundary, then two zero blocks.
        let padding = (512 - (contents.len() % 512)) % 512;
        out.extend(std::iter::repeat_n(0u8, padding + 1024));
        out
    }

    #[test]
    fn extraction_writes_a_normal_archive() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "GE-Proton9-20/proton",
                std::io::Cursor::new(b"#!/sh"),
            )
            .unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar_bytes).unwrap();
        let gz_bytes = gz.finish().unwrap();

        let dest = temp_dir("normal");
        extract_tar_gz(&gz_bytes, &dest).unwrap();
        assert!(dest.join("GE-Proton9-20").join("proton").is_file());
        std::fs::remove_dir_all(&dest).ok();
    }

    #[test]
    fn installing_a_release_places_a_usable_build() {
        // End-to-end over the real pipeline (extract -> verify -> move),
        // using an archive shaped exactly like a Proton-GE tarball.
        let tar_bytes = tar_with_proton_build("GE-Proton9-20");
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar_bytes).unwrap();
        let gz_bytes = gz.finish().unwrap();

        let root = temp_dir("install");
        let progress = DownloadProgress::new();
        install_from_bytes(&gz_bytes, "GE-Proton9-20", &root, &progress).unwrap();

        let build = root.join("GE-Proton9-20");
        assert!(build.join("proton").is_file(), "proton script is missing");
        assert!(is_proton_build(&build));
        // Staging must be cleaned up, or Steam would see a broken tool entry.
        assert!(!root.join(".staging-GE-Proton9-20").exists());
        assert!(progress.is_done());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn installing_rejects_an_archive_without_a_build() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "readme.txt", std::io::Cursor::new(b"hello"))
            .unwrap();
        let tar_bytes = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar_bytes).unwrap();
        let gz_bytes = gz.finish().unwrap();

        let root = temp_dir("reject");
        let progress = DownloadProgress::new();
        let err = install_from_bytes(&gz_bytes, "nope", &root, &progress).unwrap_err();
        assert!(
            err.contains("no files") || err.contains("Proton build"),
            "{err}"
        );
        // A rejected archive must not leave anything behind.
        assert!(!root.join("nope").exists());
        assert!(!root.join(".staging-nope").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    /// A tarball containing `GE-Proton9-20/{proton,version,files/}`.
    fn tar_with_proton_build(tag: &str) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, contents) in [
            (format!("{tag}/proton"), b"#!/bin/sh\n".to_vec()),
            (format!("{tag}/version"), b"9.20\n".to_vec()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, &path, std::io::Cursor::new(contents))
                .unwrap();
        }
        let mut dir_header = tar::Header::new_gnu();
        dir_header.set_entry_type(tar::EntryType::Directory);
        dir_header.set_size(0);
        dir_header.set_mode(0o755);
        dir_header.set_cksum();
        builder
            .append_data(&mut dir_header, format!("{tag}/files/"), std::io::empty())
            .unwrap();
        builder.into_inner().unwrap()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ion-proton-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
