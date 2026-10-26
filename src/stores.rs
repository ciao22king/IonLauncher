//! Store pages and clientless installs.
//!
//! IonLauncher reads what is already on disk, so a title that is owned but
//! not installed never appears in the library. Two things bridge that gap:
//! opening the storefront so the title can be bought, and handing an install
//! to a clientless downloader when one is present.
//!
//! Downloading a store's titles without its client means speaking that
//! store's own protocol, which is exactly what the community downloaders
//! already do. IonLauncher drives one of those rather than adding a fourth
//! implementation, and says plainly what to install when none is found.
//!
//! The downloaders are single-file binaries that the projects publish
//! themselves, so IonLauncher can fetch one when it is missing. Signing in is
//! still done on the store's own page in the browser: the downloader receives
//! only the resulting code. IonLauncher never sees a password or a token.

use crate::model::{Game, Store};
use crate::util::DownloadProgress;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// The storefront page for a game, for opening in the user's browser.
pub fn store_page(game: &Game) -> String {
    match game.store {
        // The appid is the whole path; the trailing slash avoids a redirect.
        Store::Steam => format!("https://store.steampowered.com/app/{}/", game.native_id),
        // GOG redirects the numeric product id to the canonical page, so the
        // slug is not needed.
        Store::Gog => format!("https://www.gog.com/game/{}", game.native_id),
        // Epic's page slug is an opaque per-title value that cannot be built
        // from the app name, so a store search is opened instead of a URL
        // that would 404.
        Store::Epic => format!(
            "https://store.epicgames.com/en-US/search?q={}",
            crate::json_util::percent_encode(&game.title)
        ),
    }
}

/// Opens a game's storefront page in the default browser.
pub fn open_store_page(game: &Game) -> Result<(), String> {
    crate::util::open_url(&store_page(game))
}

/// A clientless downloader that can install a store's titles.
pub struct Installer {
    pub store: Store,
    /// Executable looked up on `PATH`.
    pub program: &'static str,
    /// Where to get it.
    pub homepage: &'static str,
    /// The one-time sign-in needed before it can download anything.
    pub sign_in: &'static str,
    /// GitHub repository that publishes the standalone binary.
    release_repo: &'static str,
    /// The store page the sign-in happens on, opened in the browser.
    auth_page: &'static str,
}

/// The downloader for a store.
///
/// Steam is the odd one out: `steamcmd` is published by Valve as a tarball
/// rather than as a GitHub release, and it authenticates with a username and
/// password typed into its own prompt, so it is not downloaded or signed in
/// through the same flow as the other two.
pub fn installer(store: Store) -> Installer {
    match store {
        Store::Steam => Installer {
            store,
            program: "steamcmd",
            homepage: "https://developer.valvesoftware.com/wiki/SteamCMD",
            sign_in: "steamcmd +login <your-account>",
            release_repo: "",
            auth_page: "",
        },
        Store::Epic => Installer {
            store,
            program: "legendary",
            homepage: "https://github.com/legendary-gl/legendary",
            sign_in: "legendary auth",
            release_repo: "legendary-gl/legendary",
            auth_page: "https://legendary.gl/epiclogin",
        },
        Store::Gog => Installer {
            store,
            program: "gogdl",
            homepage: "https://github.com/Heroic-Games-Launcher/heroic-gogdl",
            sign_in: "gogdl auth",
            release_repo: "Heroic-Games-Launcher/heroic-gogdl",
            // The client id is the public GOG Galaxy one, the same value the
            // downloader itself embeds. Nothing secret is sent: the user signs
            // in on GOG's own page and the resulting code goes to the downloader.
            auth_page: "https://auth.gog.com/auth?client_id=46899977096215655\
                        &redirect_uri=https%3A%2F%2Fembed.gog.com%2Fon_login_success\
                        %3Forigin%3Dclient&response_type=code&layout=galaxy",
        },
    }
}

impl Installer {
    /// Path to the downloader if it is on `PATH`.
    pub fn find(&self) -> Option<PathBuf> {
        std::env::var_os("PATH").and_then(|path| which_in(self.program, &path))
    }

    /// Path to the copy IonLauncher downloaded, if there is one.
    pub fn downloaded(&self) -> Option<PathBuf> {
        let dir = downloader_dir()?;
        let exe = dir.join(exe_name(self.program));
        exe.is_file().then_some(exe)
    }

    /// The binary to run: the downloaded copy first, then `PATH`.
    ///
    /// A downloaded copy wins so that a version IonLauncher fetched is the one
    /// used, even if an older one sits earlier on `PATH`.
    pub fn executable(&self) -> Option<PathBuf> {
        self.downloaded().or_else(|| self.find())
    }

    /// Whether IonLauncher can fetch this downloader itself.
    pub fn downloadable(&self) -> bool {
        !self.release_repo.is_empty()
    }

    /// The URL of the standalone binary for this platform.
    ///
    /// `releases/latest/download/<asset>` redirects to the newest release, so
    /// no API call is needed and no version has to be tracked here.
    pub fn download_url(&self) -> Option<String> {
        if !self.downloadable() {
            return None;
        }
        let asset = release_asset(self.release_repo, self.program)?;
        Some(format!(
            "https://github.com/{}/releases/latest/download/{asset}",
            self.release_repo
        ))
    }

    /// Where an install writes its output, so a failure can be read back.
    pub fn log_path(&self) -> Option<PathBuf> {
        // Beside the config file, so there is one place to look for state.
        let dir = crate::config::Config::path()?.parent()?.to_path_buf();
        Some(dir.join("installs").join(format!("{}.log", self.program)))
    }

    /// The store page the user signs in on, opened in the browser.
    pub fn auth_url(&self) -> Option<&'static str> {
        (!self.auth_page.is_empty()).then_some(self.auth_page)
    }

    /// Runs the downloader, appending both streams to its log.
    fn run_logged(
        &self,
        exe: &Path,
        args: &[String],
        env: &[(&str, &str)],
    ) -> Result<PathBuf, String> {
        let log = self.log_path().ok_or("no writable config directory")?;
        if let Some(parent) = log.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        let out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map_err(|e| format!("could not open the log: {e}"))?;
        let err = out
            .try_clone()
            .map_err(|e| format!("could not open the log: {e}"))?;

        let mut command = std::process::Command::new(exe);
        command
            .args(args)
            .envs(env.iter().map(|(k, v)| (*k, *v)))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::from(out))
            .stderr(std::process::Stdio::from(err));
        command
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", self.program))?;
        Ok(log)
    }

    /// Starts an install in the background, appending to the store's log.
    ///
    /// The downloaders are long-running and, on a first run, interactive, so
    /// this never blocks the UI. The caller reports the log path, which is
    /// how a failed install is diagnosed.
    pub fn install(&self, game: &Game) -> Result<PathBuf, String> {
        let exe = self
            .executable()
            .ok_or_else(|| format!("{} is not installed", self.program))?;
        self.run_logged(&exe, &self.install_args(game), &[])
    }

    /// Finishes a sign-in with the code the store page handed back.
    ///
    /// The password never passes through IonLauncher: the user signs in on the
    /// store's own page, and only the resulting code is forwarded. Both
    /// downloaders report a bad code by exiting 0 with an error in the log, so
    /// the outcome is read from the log rather than the exit status.
    pub fn sign_in_with_code(&self, code: &str) -> Result<PathBuf, String> {
        let code = code.trim();
        if code.is_empty() {
            return Err("paste the code from the sign-in page first".into());
        }
        let exe = self
            .executable()
            .ok_or_else(|| format!("{} is not installed", self.program))?;

        let log = self.log_path().ok_or("no writable config directory")?;
        // Start from a clean log so the outcome read back below belongs to
        // this attempt and not to an earlier one.
        if let Some(parent) = log.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        std::fs::File::create(&log).map_err(|e| format!("could not open the log: {e}"))?;

        let args = self.sign_in_args(code);
        let env = self.sign_in_env();
        self.run_logged(&exe, &args, &env)?;

        // The downloaders finish quickly and write their verdict to the log,
        // so it is waited for rather than left to be checked on the next scan.
        // The token file is checked too, since a downloader may store it
        // without printing the line this code looks for.
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(250));
            if self.signed_in() == Some(true) {
                return Ok(log);
            }
            if let Some(text) = crate::util::read_text(&log) {
                if sign_in_succeeded(self.store, &text) {
                    return Ok(log);
                }
                if sign_in_failed(self.store, &text) {
                    return Err(format!("sign-in was rejected — see {}", log.display()));
                }
            }
        }
        Err(format!(
            "sign-in did not complete in time — see {}",
            log.display()
        ))
    }

    /// Arguments for a non-interactive install.
    fn install_args(&self, game: &Game) -> Vec<String> {
        match self.store {
            // steamcmd needs a login; it reuses the cached one, so no
            // password is passed on the command line.
            Store::Steam => vec![
                "+login".into(),
                "+app_update".into(),
                game.native_id.clone(),
                "validate".into(),
                "+quit".into(),
            ],
            Store::Epic => vec!["install".into(), game.native_id.clone(), "-y".into()],
            // gogdl refuses to run without a destination, unlike legendary
            // which defaults to ~/Games, so the same default is supplied here.
            Store::Gog => vec![
                "download".into(),
                game.native_id.clone(),
                "--platform".into(),
                "windows".into(),
                "--path".into(),
                games_dir().display().to_string(),
            ],
        }
    }

    /// Arguments that hand the authorization code to the downloader.
    fn sign_in_args(&self, code: &str) -> Vec<String> {
        match self.store {
            Store::Epic => vec!["auth".into(), "--code".into(), code.to_string()],
            // gogdl takes the code through its own `auth` subcommand and needs
            // to be told where to keep the token, since it has no default.
            Store::Gog => vec![
                "--auth-config-path".into(),
                self.auth_file()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                "auth".into(),
                "--code".into(),
                code.to_string(),
            ],
            Store::Steam => Vec::new(),
        }
    }

    /// Environment a sign-in needs.
    ///
    /// Nothing is passed today: `legendary` keeps its token in its own
    /// `~/.config/legendary`, and `gogdl` is told where to write through
    /// `--auth-config-path`. The hook stays because a downloader that only
    /// reads its config location from the environment would need it here.
    fn sign_in_env(&self) -> Vec<(&'static str, &'static str)> {
        Vec::new()
    }

    /// The file a downloader keeps its sign-in in, when it has a fixed one.
    pub fn auth_file(&self) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        match self.store {
            Store::Epic => Some(home.join(".config").join("legendary").join("user.json")),
            Store::Gog => Some(downloader_dir()?.join("gogdl").join("auth.json")),
            Store::Steam => None,
        }
    }

    /// Whether the downloader has a stored sign-in.
    ///
    /// `None` means the downloader does not keep the credential in a file
    /// that can be checked, so IonLauncher says nothing rather than guessing.
    pub fn signed_in(&self) -> Option<bool> {
        Some(self.auth_file()?.is_file())
    }

    /// Downloads the standalone binary for this platform.
    ///
    /// The file is written next to the config and made executable, so the
    /// downloader is available even on a system without Python or a package
    /// manager. Progress is observable through `progress`.
    pub fn download(self, progress: Arc<DownloadProgress>) {
        std::thread::spawn(move || match run_downloader_download(&self, &progress) {
            Ok(path) => progress.finish(path.display().to_string()),
            Err(err) => progress.set_error(err),
        });
    }

    /// The command that installs `game`, as it would be typed in a terminal.
    pub fn install_command(&self, game: &Game) -> String {
        match self.store {
            Store::Steam => format!(
                "steamcmd +login <your-account> +app_update {} validate +quit",
                game.native_id
            ),
            Store::Epic => format!("legendary install {}", game.native_id),
            Store::Gog => format!(
                "gogdl download {} --platform windows --path {}",
                game.native_id,
                games_dir().display()
            ),
        }
    }
}

/// Where IonLauncher keeps the downloaders it fetched.
///
/// Under the data dir rather than the config dir: these are executables, not
/// settings, and they can be deleted and fetched again at any time.
pub fn downloader_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("IonLauncher").join("downloaders"))
}

/// The base directory games are installed into.
///
/// `legendary` defaults to `~/Games` on its own; `gogdl` has no default and
/// requires a destination, so the same folder is used for both and the games
/// from the two stores end up side by side.
pub fn games_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Games")
}

/// The file name a downloader is saved as on this platform.
fn exe_name(program: &str) -> String {
    if cfg!(windows) {
        format!("{program}.exe")
    } else {
        program.to_string()
    }
}

/// The release asset that carries the binary for this platform.
///
/// Both projects publish one asset per OS and architecture, but they spell
/// the 64-bit x86 and the macOS names differently, so the name is assembled
/// from the target rather than matched loosely.
fn release_asset(repo: &str, program: &str) -> Option<String> {
    let gog = repo.contains("gogdl");
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        if gog {
            "macos"
        } else {
            "macOS"
        }
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if gog {
        "x86_64"
    } else {
        "x64"
    };
    let ext = if cfg!(windows) { ".exe" } else { "" };
    Some(format!("{program}_{os}_{arch}{ext}"))
}

/// Whether a downloader's log reports a completed sign-in.
///
/// `legendary` prints `Successfully logged in as "..."`; `gogdl` prints the
/// token JSON. Both also print unrelated warnings at startup, so only these
/// exact markers are trusted.
fn sign_in_succeeded(store: Store, log: &str) -> bool {
    match store {
        Store::Epic => log.contains("Successfully logged in"),
        Store::Gog => log.contains("access_token"),
        Store::Steam => false,
    }
}

/// Whether a downloader's log reports a rejected sign-in.
///
/// Both downloaders exit 0 even when they refuse the code, so the message in
/// the log is the only signal. The markers are the downloaders' own failure
/// strings: matching a bare `error` would also catch the harmless
/// `ValueError` warning `legendary` prints before it starts.
fn sign_in_failed(store: Store, log: &str) -> bool {
    match store {
        Store::Epic => log.contains("Login attempt failed") || log.contains("cannot login"),
        Store::Gog => log.contains("\"error\": true") || log.contains("\"error\":true"),
        Store::Steam => false,
    }
}

/// Downloads a downloader's binary and makes it executable.
fn run_downloader_download(
    installer: &Installer,
    progress: &DownloadProgress,
) -> Result<PathBuf, String> {
    let url = installer
        .download_url()
        .ok_or("this downloader is not distributed as a standalone binary")?;
    let dir = downloader_dir().ok_or("could not find a data directory")?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let target = dir.join(exe_name(installer.program));

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(600))
        .user_agent(concat!(
            "IonLauncher/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/ciao22king/IonLauncher)"
        ))
        .build();
    let response = agent
        .get(&url)
        .call()
        .map_err(|e| format!("download failed: {e}"))?;

    progress.total.store(
        response
            .header("Content-Length")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0),
        std::sync::atomic::Ordering::Relaxed,
    );

    // Staged and renamed, so an interrupted download cannot leave a truncated
    // file that would later be run as the downloader.
    let staging = target.with_extension("part");
    let mut file = std::fs::File::create(&staging)
        .map_err(|e| format!("cannot write to {}: {e}", staging.display()))?;
    let mut reader = response.into_reader();
    let mut buffer = [0u8; 64 * 1024];
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

    make_executable(&staging)?;
    std::fs::rename(&staging, &target).map_err(|e| format!("cannot finish the download: {e}"))?;
    Ok(target)
}

/// Marks a file executable on Unix; a no-op elsewhere.
fn make_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)
            .map_err(|e| format!("cannot make {} executable: {e}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Looks up `program` on `PATH`, honouring the platform's executable suffixes.
fn which_in(program: &str, path: &OsStr) -> Option<PathBuf> {
    // Windows resolves bare names through PATHEXT, so the common ones are
    // tried explicitly rather than relying on the shell.
    let suffixes: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    for dir in std::env::split_paths(path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for suffix in suffixes {
            let candidate = dir.join(format!("{program}{suffix}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(store: Store, native_id: &str, title: &str) -> Game {
        Game::new(
            format!("{}:{native_id}", store.key()),
            native_id.to_string(),
            store,
            title.to_string(),
        )
    }

    #[test]
    fn steam_pages_use_the_appid() {
        let g = game(Store::Steam, "440", "Team Fortress 2");
        assert_eq!(store_page(&g), "https://store.steampowered.com/app/440/");
    }

    #[test]
    fn gog_pages_use_the_product_id() {
        let g = game(Store::Gog, "1207658924", "The Witcher");
        assert_eq!(store_page(&g), "https://www.gog.com/game/1207658924");
    }

    #[test]
    fn epic_pages_search_by_title_with_the_title_escaped() {
        let g = game(Store::Epic, "SomeGame", "Alan Wake 2");
        let url = store_page(&g);
        assert!(url.starts_with("https://store.epicgames.com/en-US/search?q="));
        // Spaces must be escaped or the query is truncated at the first one.
        assert!(url.ends_with("Alan%20Wake%202"), "{url}");
        assert!(!url.contains(' '));
    }

    #[test]
    fn each_store_gets_its_own_downloader() {
        assert_eq!(installer(Store::Steam).program, "steamcmd");
        assert_eq!(installer(Store::Epic).program, "legendary");
        assert_eq!(installer(Store::Gog).program, "gogdl");
    }

    #[test]
    fn install_commands_carry_the_native_id() {
        let steam = game(Store::Steam, "440", "Team Fortress 2");
        assert!(installer(Store::Steam)
            .install_command(&steam)
            .contains("app_update 440"));

        let epic = game(Store::Epic, "Fortnite", "Fortnite");
        assert_eq!(
            installer(Store::Epic).install_command(&epic),
            "legendary install Fortnite"
        );

        let gog = game(Store::Gog, "1207658924", "The Witcher");
        assert!(installer(Store::Gog)
            .install_command(&gog)
            .contains("1207658924"));
    }

    #[test]
    fn gogdl_install_args_include_the_required_destination() {
        // gogdl exits with an argument error when `--path` is missing, so a
        // download without it would never start.
        let gog = game(Store::Gog, "1207658924", "The Witcher");
        let args = installer(Store::Gog).install_args(&gog);
        let path_at = args
            .iter()
            .position(|a| a == "--path")
            .expect("gogdl needs --path");
        assert!(!args[path_at + 1].is_empty());
    }

    #[test]
    fn which_in_finds_an_executable_on_the_path() {
        let dir = std::env::temp_dir().join(format!("ionlauncher-which-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("legendary");
        std::fs::write(&exe, b"#!/bin/sh\n").unwrap();

        let found = which_in("legendary", dir.as_os_str());
        assert_eq!(found.as_deref(), Some(exe.as_path()));

        // A program that is not there must not be reported as present.
        assert!(which_in("definitely-not-a-real-program", dir.as_os_str()).is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn which_in_skips_empty_path_entries() {
        // A leading or trailing ':' in PATH yields an empty entry, which
        // must not be treated as the current directory.
        let joined =
            std::env::join_paths([PathBuf::from(""), PathBuf::from("/nonexistent")]).unwrap();
        assert!(which_in("legendary", &joined).is_none());
    }

    #[test]
    fn only_the_github_distributed_downloaders_are_downloadable() {
        assert!(installer(Store::Epic).downloadable());
        assert!(installer(Store::Gog).downloadable());
        // steamcmd comes from Valve's own site, so it has no release to fetch.
        assert!(!installer(Store::Steam).downloadable());
        assert!(installer(Store::Steam).download_url().is_none());
    }

    #[test]
    fn download_urls_point_at_the_projects_latest_release() {
        let epic = installer(Store::Epic).download_url().unwrap();
        assert_eq!(
            epic,
            format!(
                "https://github.com/legendary-gl/legendary/releases/latest/download/{}",
                release_asset("legendary-gl/legendary", "legendary").unwrap()
            )
        );

        let gog = installer(Store::Gog).download_url().unwrap();
        assert_eq!(
            gog,
            format!(
                "https://github.com/Heroic-Games-Launcher/heroic-gogdl/releases/latest/download/{}",
                release_asset("Heroic-Games-Launcher/heroic-gogdl", "gogdl").unwrap()
            )
        );
    }

    #[test]
    fn the_asset_name_matches_the_projects_naming_for_this_platform() {
        let legendary = release_asset("legendary-gl/legendary", "legendary").unwrap();
        let gogdl = release_asset("Heroic-Games-Launcher/heroic-gogdl", "gogdl").unwrap();

        assert!(legendary.starts_with("legendary_"));
        assert!(gogdl.starts_with("gogdl_"));

        // The two projects spell the same platform differently, and picking
        // the wrong one would 404 at download time.
        if cfg!(windows) {
            assert!(legendary.ends_with(".exe") && gogdl.ends_with(".exe"));
        } else if cfg!(target_os = "macos") {
            assert!(legendary.contains("macOS"));
            assert!(gogdl.contains("macos"));
        } else {
            assert!(legendary.contains("linux"));
            assert!(gogdl.contains("linux"));
        }

        // The 64-bit x86 spelling differs between the two projects.
        if !cfg!(target_arch = "aarch64") && cfg!(windows) {
            assert!(legendary.contains("x64"));
            assert!(gogdl.contains("x86_64"));
        }
    }

    #[test]
    fn a_downloader_is_only_reported_ready_when_its_file_exists() {
        // The downloaded copy is looked for under the data dir; when nothing
        // has been fetched, the path must not be claimed to exist.
        if let Some(path) = installer(Store::Epic).downloaded() {
            assert!(path.is_file());
        }
    }

    #[test]
    fn a_rejected_sign_in_is_read_from_the_log() {
        // Both downloaders exit 0 on a bad code, so the log is the signal.
        assert!(sign_in_failed(
            Store::Epic,
            "Login attempt failed, please see log for details."
        ));
        assert!(sign_in_failed(
            Store::Epic,
            "No exchange token/authorization code, cannot login."
        ));
        assert!(sign_in_failed(Store::Gog, "{\"error\": true}"));
        assert!(!sign_in_failed(
            Store::Epic,
            "Successfully logged in as \"someone\""
        ));
        assert!(!sign_in_failed(Store::Gog, "{\"access_token\": \"x\"}"));
    }

    #[test]
    fn a_harmless_startup_warning_is_not_read_as_a_failed_sign_in() {
        // legendary prints this ValueError warning before it does anything;
        // treating it as an error would report a good login as broken.
        let log = "[Core] WARNING: Getting locale failed: ValueError('not enough values \
                   to unpack (expected 2, got 1)'), falling back to using en-US.\n\
                   [cli] INFO: Testing existing login data if present...";
        assert!(!sign_in_failed(Store::Epic, log));
    }

    #[test]
    fn a_completed_sign_in_is_recognised_in_the_log() {
        assert!(sign_in_succeeded(
            Store::Epic,
            "[cli] INFO: Successfully logged in as \"someone\""
        ));
        assert!(sign_in_succeeded(Store::Gog, "{\"access_token\": \"abc\"}"));
        assert!(!sign_in_succeeded(Store::Epic, "Login attempt failed"));
        assert!(!sign_in_succeeded(Store::Gog, "{\"error\": true}"));
    }

    #[test]
    fn an_empty_code_is_refused_before_running_anything() {
        let err = installer(Store::Epic).sign_in_with_code("   ").unwrap_err();
        assert!(err.contains("paste the code"), "unexpected message: {err}");
    }

    #[test]
    fn sign_in_urls_point_at_the_stores_own_pages() {
        // Epic's page is the project's redirector; GOG's carries the public
        // Galaxy client id and lands on GOG's own login.
        assert_eq!(
            installer(Store::Epic).auth_url(),
            Some("https://legendary.gl/epiclogin")
        );
        let gog = installer(Store::Gog).auth_url().unwrap();
        assert!(gog.starts_with("https://auth.gog.com/auth?client_id="));
        assert!(gog.contains("embed.gog.com%2Fon_login_success"));

        // steamcmd prompts for credentials itself, so there is no page.
        assert!(installer(Store::Steam).auth_url().is_none());
    }

    #[test]
    fn gog_sign_in_tells_the_downloader_where_to_keep_the_token() {
        // gogdl has no default config location, so the path must be passed.
        let args = installer(Store::Gog).sign_in_args("CODE");
        let path_flag = args.iter().position(|a| a == "--auth-config-path");
        assert!(
            path_flag.is_some(),
            "auth-config-path is required: {args:?}"
        );
        assert!(!args[path_flag.unwrap() + 1].is_empty());
        assert!(args.contains(&"CODE".to_string()));

        // legendary keeps its token itself and needs no such flag.
        let epic = installer(Store::Epic).sign_in_args("CODE");
        assert_eq!(epic, vec!["auth", "--code", "CODE"]);
    }

    #[test]
    fn each_downloader_has_its_own_log_so_they_do_not_clobber_each_other() {
        let epic = installer(Store::Epic).log_path();
        let gog = installer(Store::Gog).log_path();
        if let (Some(epic), Some(gog)) = (epic, gog) {
            assert_ne!(epic, gog);
            assert!(epic.to_string_lossy().ends_with("legendary.log"));
            assert!(gog.to_string_lossy().ends_with("gogdl.log"));
        }
    }
}
