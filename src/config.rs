//! Persisted user settings.
//!
//! Stored as JSON next to the platform config dir so the launcher has no
//! external configuration dependency.

use crate::model::Store;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortMode {
    RecentlyPlayed,
    Name,
    Playtime,
    Size,
}

impl SortMode {
    pub const ALL: [SortMode; 4] = [
        SortMode::RecentlyPlayed,
        SortMode::Name,
        SortMode::Playtime,
        SortMode::Size,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortMode::RecentlyPlayed => "Recent",
            SortMode::Name => "Name",
            SortMode::Playtime => "Playtime",
            SortMode::Size => "Size",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    Grid,
    List,
}

impl ViewMode {
    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Grid => "Grid",
            ViewMode::List => "List",
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub steam_enabled: bool,
    pub epic_enabled: bool,
    pub gog_enabled: bool,
    pub sort: SortMode,
    pub view: ViewMode,
    /// Preferred grid card width in points; also drives how many columns fit.
    pub card_width: f32,
    /// Show titles whose install directory is gone.
    pub show_missing: bool,
    /// Download missing artwork from the store CDNs.
    pub fetch_artwork: bool,
    /// Use the built-in sample library instead of scanning the machine.
    #[serde(skip)]
    pub demo_mode: bool,
    /// Manual overrides, used when auto-detection cannot find a store.
    pub steam_root: Option<PathBuf>,
    pub epic_root: Option<PathBuf>,
    pub gog_root: Option<PathBuf>,
    /// Recent searches, most recent first.
    pub recent_searches: Vec<String>,
    /// Game ids the user starred, shown on the Favourites screen.
    pub favorites: Vec<String>,
    /// Run Windows games through Proton instead of the store client.
    ///
    /// Only affects Linux, and only games whose target is a Windows
    /// executable: a `steam://` launch is left to Steam, which has its own
    /// per-title compatibility setting.
    pub proton_enabled: bool,
    /// Name of the Proton build to use, matched against the directory name.
    /// `None` means "pick automatically".
    pub proton_version: Option<String>,
    /// Ask GitHub for a newer release on startup.
    ///
    /// On by default: a launcher that never tells you an update exists leaves
    /// people on old builds, and this is the only channel IonLauncher has.
    /// It is a single request to the public releases API, with nothing sent
    /// but the request itself.
    pub check_updates: bool,
    /// Version the user chose to skip, so the banner stops nagging.
    pub skipped_version: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            steam_enabled: true,
            epic_enabled: true,
            gog_enabled: true,
            sort: SortMode::RecentlyPlayed,
            view: ViewMode::Grid,
            card_width: 232.0,
            show_missing: true,
            fetch_artwork: default_true(),
            demo_mode: false,
            steam_root: None,
            epic_root: None,
            gog_root: None,
            recent_searches: Vec::new(),
            favorites: Vec::new(),
            proton_enabled: false,
            proton_version: None,
            check_updates: default_true(),
            skipped_version: None,
        }
    }
}

impl Config {
    pub fn enabled(&self, store: Store) -> bool {
        match store {
            Store::Steam => self.steam_enabled,
            Store::Epic => self.epic_enabled,
            Store::Gog => self.gog_enabled,
        }
    }

    pub fn set_enabled(&mut self, store: Store, value: bool) {
        match store {
            Store::Steam => self.steam_enabled = value,
            Store::Epic => self.epic_enabled = value,
            Store::Gog => self.gog_enabled = value,
        }
    }

    pub fn override_root(&self, store: Store) -> Option<&Path> {
        match store {
            Store::Steam => self.steam_root.as_deref(),
            Store::Epic => self.epic_root.as_deref(),
            Store::Gog => self.gog_root.as_deref(),
        }
    }

    pub fn set_override_root(&mut self, store: Store, path: Option<PathBuf>) {
        match store {
            Store::Steam => self.steam_root = path,
            Store::Epic => self.epic_root = path,
            Store::Gog => self.gog_root = path,
        }
    }

    /// Config file location, e.g. `~/.config/IonLauncher/config.json`.
    pub fn path() -> Option<PathBuf> {
        let dir = dirs::config_dir()?.join("IonLauncher");
        Some(dir.join("config.json"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Some(text) = std::fs::read_to_string(path).ok() else {
            return Self::default();
        };
        // A corrupt config should never stop the launcher from starting.
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let Ok(text) = serde_json::to_string_pretty(self) else {
            return;
        };
        // Write to a sibling then rename, so an interrupted write cannot
        // truncate a good config.
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    /// Records a search term, keeping the list short and deduplicated.
    pub fn remember_search(&mut self, term: &str) {
        let term = term.trim();
        if term.len() < 2 {
            return;
        }
        self.recent_searches
            .retain(|s| !s.eq_ignore_ascii_case(term));
        self.recent_searches.insert(0, term.to_string());
        self.recent_searches.truncate(6);
    }

    pub fn is_favorite(&self, game_id: &str) -> bool {
        self.favorites.iter().any(|f| f == game_id)
    }

    /// Adds or removes a favourite, returning the new state.
    pub fn toggle_favorite(&mut self, game_id: &str) -> bool {
        if self.is_favorite(game_id) {
            self.favorites.retain(|f| f != game_id);
            false
        } else {
            self.favorites.push(game_id.to_string());
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_enable_every_store() {
        let cfg = Config::default();
        for store in Store::ALL {
            assert!(cfg.enabled(store));
        }
        assert_eq!(cfg.sort, SortMode::RecentlyPlayed);
        assert_eq!(cfg.view, ViewMode::Grid);
    }

    #[test]
    fn update_checks_are_on_by_default_and_persist() {
        let cfg = Config::default();
        assert!(cfg.check_updates, "updates should be checked by default");
        assert_eq!(cfg.skipped_version, None);

        let cfg = Config {
            check_updates: false,
            skipped_version: Some("1.2.3".into()),
            ..Config::default()
        };
        let text = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert!(!back.check_updates);
        assert_eq!(back.skipped_version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn enabling_and_overriding_roundtrip() {
        let mut cfg = Config::default();
        cfg.set_enabled(Store::Epic, false);
        assert!(!cfg.enabled(Store::Epic));
        cfg.set_override_root(Store::Gog, Some(PathBuf::from("/games/gog")));
        assert_eq!(cfg.override_root(Store::Gog), Some(Path::new("/games/gog")));
        assert_eq!(cfg.override_root(Store::Steam), None);
    }

    #[test]
    fn config_serialises_through_json() {
        let cfg = Config {
            sort: SortMode::Playtime,
            view: ViewMode::List,
            card_width: 300.0,
            ..Config::default()
        };
        let text = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(back.sort, SortMode::Playtime);
        assert_eq!(back.view, ViewMode::List);
        assert_eq!(back.card_width, 300.0);
    }

    #[test]
    fn demo_mode_is_never_persisted() {
        // `--demo` shows a sample library for one session; writing that into
        // the saved config would hide the user's real games on next launch.
        let cfg = Config {
            demo_mode: true,
            ..Config::default()
        };
        let text = serde_json::to_string(&cfg).unwrap();
        assert!(!text.contains("demo_mode"));
        let back: Config = serde_json::from_str(&text).unwrap();
        assert!(!back.demo_mode);
    }

    #[test]
    fn partial_json_falls_back_to_defaults() {
        // Forward/backward compatible: unknown keys ignored, missing keys default.
        let cfg: Config = serde_json::from_str(r#"{"sort":"Name","future_key":1}"#).unwrap();
        assert_eq!(cfg.sort, SortMode::Name);
        assert_eq!(cfg.card_width, Config::default().card_width);
    }

    #[test]
    fn search_history_is_deduped_and_capped() {
        let mut cfg = Config::default();
        for i in 0..10 {
            cfg.remember_search(&format!("game {i}"));
        }
        assert_eq!(cfg.recent_searches.len(), 6);
        assert_eq!(cfg.recent_searches[0], "game 9");
        // Re-searching an existing term moves it to the front without duplicating.
        cfg.remember_search("game 5");
        assert_eq!(cfg.recent_searches[0], "game 5");
        assert_eq!(
            cfg.recent_searches
                .iter()
                .filter(|s| *s == "game 5")
                .count(),
            1
        );
        // Too-short terms are ignored.
        cfg.remember_search("a");
        assert_eq!(cfg.recent_searches[0], "game 5");
    }

    #[test]
    fn proton_settings_roundtrip_and_default_off() {
        // Proton must be opt-in: turning it on for someone who has no build
        // installed would break their Windows games rather than help.
        let cfg = Config::default();
        assert!(!cfg.proton_enabled);
        assert_eq!(cfg.proton_version, None);

        let cfg = Config {
            proton_enabled: true,
            proton_version: Some("GE-Proton9-20".into()),
            ..Config::default()
        };
        let text = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert!(back.proton_enabled);
        assert_eq!(back.proton_version.as_deref(), Some("GE-Proton9-20"));
    }

    #[test]
    fn favorites_toggle_on_and_off() {
        let mut cfg = Config::default();
        assert!(!cfg.is_favorite("steam:440"));
        assert!(cfg.toggle_favorite("steam:440"));
        assert!(cfg.is_favorite("steam:440"));
        assert!(!cfg.toggle_favorite("steam:440"));
        assert!(!cfg.is_favorite("steam:440"));
        assert!(cfg.favorites.is_empty());
    }
}
