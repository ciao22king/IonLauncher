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

use crate::model::{Game, Store};
use std::ffi::OsStr;
use std::path::PathBuf;

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
}

/// The downloader for a store.
pub fn installer(store: Store) -> Installer {
    match store {
        Store::Steam => Installer {
            store,
            program: "steamcmd",
            homepage: "https://developer.valvesoftware.com/wiki/SteamCMD",
            sign_in: "steamcmd +login <your-account>",
        },
        Store::Epic => Installer {
            store,
            program: "legendary",
            homepage: "https://github.com/derrod/legendary",
            sign_in: "legendary auth",
        },
        Store::Gog => Installer {
            store,
            program: "gogdl",
            homepage: "https://github.com/Heroic-Games-Launcher/heroic-gogdl",
            sign_in: "gogdl auth",
        },
    }
}

impl Installer {
    /// Path to the downloader if it is on `PATH`.
    pub fn find(&self) -> Option<PathBuf> {
        std::env::var_os("PATH").and_then(|path| which_in(self.program, &path))
    }

    /// The command that installs `game`, as it would be typed in a terminal.
    pub fn install_command(&self, game: &Game) -> String {
        match self.store {
            Store::Steam => format!(
                "steamcmd +login <your-account> +app_update {} validate +quit",
                game.native_id
            ),
            Store::Epic => format!("legendary install {}", game.native_id),
            Store::Gog => format!("gogdl download {} --platform windows", game.native_id),
        }
    }

    /// Whether the downloader has a stored sign-in.
    ///
    /// `None` means the downloader does not keep the credential in a file
    /// that can be checked, so IonLauncher says nothing rather than guessing.
    pub fn signed_in(&self) -> Option<bool> {
        let home = dirs::home_dir()?;
        let path = auth_file(self.store, &home)?;
        Some(path.is_file())
    }

    /// Where an install writes its output, so a failure can be read back.
    pub fn log_path(&self) -> Option<PathBuf> {
        // Beside the config file, so there is one place to look for state.
        let dir = crate::config::Config::path()?.parent()?.to_path_buf();
        Some(dir.join("installs").join(format!("{}.log", self.program)))
    }

    /// Starts an install in the background, appending to the store's log.
    ///
    /// The downloaders are long-running and, on a first run, interactive, so
    /// this never blocks the UI. The caller reports the log path, which is
    /// how a failed install is diagnosed.
    pub fn install(&self, game: &Game) -> Result<PathBuf, String> {
        let exe = self
            .find()
            .ok_or_else(|| format!("{} is not installed", self.program))?;
        let log = self.log_path().ok_or("no writable config directory")?;
        if let Some(parent) = log.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }

        let args = self.install_args(game);
        let out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map_err(|e| format!("could not open the install log: {e}"))?;
        let err = out
            .try_clone()
            .map_err(|e| format!("could not open the install log: {e}"))?;

        let mut command = std::process::Command::new(&exe);
        command
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::from(out))
            .stderr(std::process::Stdio::from(err));
        command
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", self.program))?;

        Ok(log)
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
            Store::Gog => vec![
                "download".into(),
                game.native_id.clone(),
                "--platform".into(),
                "windows".into(),
            ],
        }
    }
}

/// The file a downloader writes its sign-in to, when it keeps one.
///
/// Only the downloaders that persist a token are listed. Steam's `steamcmd`
/// keeps its session inside its own config tree, which is not a stable
/// location to test, so it reports `None` and the UI stays quiet.
pub fn auth_file(store: Store, home: &std::path::Path) -> Option<PathBuf> {
    match store {
        Store::Epic => Some(home.join(".config").join("legendary").join("user.json")),
        Store::Gog => Some(home.join(".config").join("gogdl").join("config.json")),
        Store::Steam => None,
    }
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
}
