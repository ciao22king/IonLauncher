//! Core data types shared by every store provider and by the UI.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// The three stores IonLauncher aggregates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Store {
    Steam,
    Epic,
    Gog,
}

impl Store {
    pub const ALL: [Store; 3] = [Store::Steam, Store::Epic, Store::Gog];

    pub fn name(self) -> &'static str {
        match self {
            Store::Steam => "Steam",
            Store::Epic => "Epic Games",
            Store::Gog => "GOG",
        }
    }

    /// Short label used on cards and filter chips.
    pub fn short(self) -> &'static str {
        match self {
            Store::Steam => "Steam",
            Store::Epic => "Epic",
            Store::Gog => "GOG",
        }
    }

    /// Brand accent colour, used for the store dot on each card.
    pub fn accent(self) -> egui::Color32 {
        match self {
            Store::Steam => egui::Color32::from_rgb(0x66, 0xC0, 0xF4),
            Store::Epic => egui::Color32::from_rgb(0xB6, 0xB6, 0xC8),
            Store::Gog => egui::Color32::from_rgb(0xA1, 0x7B, 0xE8),
        }
    }

    /// Registry / config key used by the provider.
    pub fn key(self) -> &'static str {
        match self {
            Store::Steam => "steam",
            Store::Epic => "epic",
            Store::Gog => "gog",
        }
    }

}

impl fmt::Display for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How a game should be started.
///
/// Almost everything goes through a store protocol handler, which keeps
/// DRM, cloud saves and achievements working. `Direct` exists as a fallback
/// for DRM-free installs that were detected without any store client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LaunchTarget {
    /// e.g. `steam://runsappid/440`
    Url(String),
    /// Absolute path to an executable, optionally with arguments.
    Direct { exe: PathBuf, args: Vec<String> },
}

/// A single game, normalised across all stores.
#[derive(Clone, Serialize, Deserialize)]
pub struct Game {
    /// Stable id, unique across stores: `<store>:<native id>`.
    pub id: String,
    /// Native identifier (Steam appid, Epic catalog item id, GOG game id).
    pub native_id: String,
    pub store: Store,
    pub title: String,
    /// Path used to group games and to pick artwork. May not exist.
    pub install_dir: PathBuf,
    pub launch: LaunchTarget,
    /// Steam-only. Not all titles report it.
    pub app_id: Option<u32>,
    /// On-disk size in bytes, when the store records it.
    pub size_bytes: Option<u64>,
    /// Seconds played, when the store records it.
    pub playtime_secs: Option<u64>,
    /// Epoch seconds of the last play session.
    pub last_played: Option<u64>,
    /// Artwork URLs tried in order, most preferred first.
    pub art_urls: Vec<String>,
    /// Decoded artwork, filled in by the background loader.
    #[serde(skip)]
    pub art: Option<egui::TextureHandle>,
    /// True while a network artwork fetch is in flight.
    #[serde(skip)]
    pub art_pending: bool,
}

impl std::fmt::Debug for Game {
    /// Hand-written because `TextureHandle` is not `Debug`; the GPU handle is
    /// not interesting to log anyway.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("id", &self.id)
            .field("store", &self.store)
            .field("title", &self.title)
            .field("install_dir", &self.install_dir)
            .field("launch", &self.launch)
            .field("size_bytes", &self.size_bytes)
            .field("playtime_secs", &self.playtime_secs)
            .field("last_played", &self.last_played)
            .field("art", &self.art.is_some())
            .finish()
    }
}

impl Game {
    pub fn new(id: String, native_id: String, store: Store, title: String) -> Self {
        Self {
            id,
            native_id,
            store,
            title,
            install_dir: PathBuf::new(),
            launch: LaunchTarget::Url(String::new()),
            app_id: None,
            size_bytes: None,
            playtime_secs: None,
            last_played: None,
            art_urls: Vec::new(),
            art: None,
            art_pending: false,
        }
    }

    /// Human-readable playtime, e.g. `12.5 h` or `45 min`.
    pub fn playtime_label(&self) -> Option<String> {
        let secs = self.playtime_secs?;
        if secs < 60 {
            return None;
        }
        let mins = secs / 60;
        if mins < 60 {
            return Some(format!("{mins} min"));
        }
        let hours = mins as f64 / 60.0;
        if hours < 10.0 {
            Some(format!("{hours:.1} h"))
        } else {
            Some(format!("{:.0} h", hours))
        }
    }

    pub fn size_label(&self) -> Option<String> {
        let bytes = self.size_bytes?;
        if bytes == 0 {
            return None;
        }
        const GB: f64 = 1024.0 * 1024.0 * 1024.0;
        const MB: f64 = 1024.0 * 1024.0;
        let b = bytes as f64;
        if b >= GB {
            Some(format!("{:.1} GB", b / GB))
        } else {
            Some(format!("{:.0} MB", b / MB))
        }
    }

    /// Sort key: most recently played first, never-played titles last.
    pub fn recency_key(&self) -> (u8, std::cmp::Reverse<u64>) {
        match self.last_played {
            Some(t) => (0, std::cmp::Reverse(t)),
            None => (1, std::cmp::Reverse(0)),
        }
    }
}

/// Summary of what a scan discovered, shown on the Library screen.
#[derive(Debug, Clone, Default)]
pub struct ScanReport {
    pub games: Vec<Game>,
    /// Human-readable note per store, e.g. "not installed" or "42 games".
    pub notes: Vec<(Store, String)>,
    pub elapsed_ms: u128,
}

impl ScanReport {
    pub fn note(&self, store: Store) -> Option<&str> {
        self.notes
            .iter()
            .find(|(s, _)| *s == store)
            .map(|(_, n)| n.as_str())
    }
}
