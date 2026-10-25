//! Built-in sample library.
//!
//! IonLauncher can only show what is actually installed, which makes the UI
//! impossible to evaluate on a machine with no games (or in a container).
//! Demo mode fills the library with realistic entries across all three
//! stores so the interface can be tried, screenshotted and tested.

use crate::model::{Game, LaunchTarget, Store};
use crate::providers::{epic, gog, steam};

struct Sample {
    store: Store,
    native_id: &'static str,
    title: &'static str,
    size_bytes: u64,
    playtime_secs: u64,
    /// Days since last played; `None` means never played.
    last_played_days: Option<u64>,
}

/// A spread of real, well-known titles per store, with plausible numbers.
const SAMPLES: &[Sample] = &[
    Sample { store: Store::Steam, native_id: "570", title: "Dota 2", size_bytes: 41_000_000_000, playtime_secs: 184_320, last_played_days: Some(0) },
    Sample { store: Store::Steam, native_id: "440", title: "Team Fortress 2", size_bytes: 28_700_000_000, playtime_secs: 92_700, last_played_days: Some(3) },
    Sample { store: Store::Steam, native_id: "1245620", title: "ELDEN RING", size_bytes: 60_200_000_000, playtime_secs: 43_200, last_played_days: Some(9) },
    Sample { store: Store::Steam, native_id: "1091500", title: "Cyberpunk 2077", size_bytes: 70_100_000_000, playtime_secs: 27_000, last_played_days: Some(21) },
    Sample { store: Store::Steam, native_id: "292030", title: "The Witcher 3: Wild Hunt", size_bytes: 51_400_000_000, playtime_secs: 61_200, last_played_days: Some(64) },
    Sample { store: Store::Steam, native_id: "413150", title: "Stardew Valley", size_bytes: 1_200_000_000, playtime_secs: 18_900, last_played_days: Some(140) },
    Sample { store: Store::Steam, native_id: "1145360", title: "Hades", size_bytes: 15_300_000_000, playtime_secs: 7_200, last_played_days: Some(210) },
    Sample { store: Store::Steam, native_id: "620", title: "Portal 2", size_bytes: 13_000_000_000, playtime_secs: 2_100, last_played_days: Some(400) },
    Sample { store: Store::Epic, native_id: "Fortnite", title: "Fortnite", size_bytes: 39_500_000_000, playtime_secs: 12_600, last_played_days: Some(1) },
    Sample { store: Store::Epic, native_id: "Control", title: "Control Ultimate Edition", size_bytes: 42_800_000_000, playtime_secs: 9_000, last_played_days: Some(33) },
    Sample { store: Store::Epic, native_id: "GTAV", title: "Grand Theft Auto V", size_bytes: 105_000_000_000, playtime_secs: 33_000, last_played_days: Some(77) },
    Sample { store: Store::Epic, native_id: "RocketLeague", title: "Rocket League", size_bytes: 22_400_000_000, playtime_secs: 55_800, last_played_days: Some(5) },
    Sample { store: Store::Epic, native_id: "AlanWake2", title: "Alan Wake 2", size_bytes: 86_000_000_000, playtime_secs: 5_400, last_played_days: Some(120) },
    Sample { store: Store::Epic, native_id: "DeathStranding", title: "Death Stranding", size_bytes: 68_000_000_000, playtime_secs: 0, last_played_days: None },
    Sample { store: Store::Gog, native_id: "1207658924", title: "The Witcher 2: Assassins of Kings", size_bytes: 23_100_000_000, playtime_secs: 15_600, last_played_days: Some(45) },
    Sample { store: Store::Gog, native_id: "1207658930", title: "Baldur's Gate 3", size_bytes: 122_000_000_000, playtime_secs: 98_400, last_played_days: Some(2) },
    Sample { store: Store::Gog, native_id: "1207658679", title: "Heroes of Might and Magic III", size_bytes: 900_000_000, playtime_secs: 6_300, last_played_days: Some(300) },
    Sample { store: Store::Gog, native_id: "1423049311", title: "Disco Elysium", size_bytes: 8_400_000_000, playtime_secs: 11_100, last_played_days: Some(18) },
    Sample { store: Store::Gog, native_id: "1207659020", title: "Planescape: Torment", size_bytes: 1_600_000_000, playtime_secs: 0, last_played_days: None },
    Sample { store: Store::Gog, native_id: "1787841355", title: "Hollow Knight", size_bytes: 9_000_000_000, playtime_secs: 14_400, last_played_days: Some(88) },
];

/// Builds the demo library.
///
/// Artwork URLs point at the real store CDNs, so with artwork fetching
/// enabled the demo looks exactly like a real library.
pub fn library() -> Vec<Game> {
    let now = crate::util::now_epoch();
    let mut games: Vec<Game> = SAMPLES
        .iter()
        .map(|sample| {
            let mut game = Game::new(
                format!("{}:{}", sample.store.key(), sample.native_id),
                sample.native_id.to_string(),
                sample.store,
                sample.title.to_string(),
            );
            game.size_bytes = Some(sample.size_bytes);
            game.playtime_secs = Some(sample.playtime_secs).filter(|s| *s > 0);
            game.last_played = sample
                .last_played_days
                .map(|days| now.saturating_sub(days * 86_400));
            // Demo installs live under a plausible per-store path. They do
            // not exist on disk, which is exactly the state the "missing
            // install" indicator is meant to describe.
            game.install_dir = std::env::temp_dir()
                .join("IonLauncherDemo")
                .join(sample.store.key())
                .join(sample.title.replace([':', '/'], "_"));
            game.launch = match sample.store {
                Store::Steam => LaunchTarget::Url(format!("steam://runsappid/{}", sample.native_id)),
                Store::Epic => LaunchTarget::Url(epic::launch_url("demo", "demo", sample.native_id)),
                Store::Gog => LaunchTarget::Url(gog::launch_url(sample.native_id)),
            };
            game.art_urls = match sample.store {
                Store::Steam => sample
                    .native_id
                    .parse::<u32>()
                    .map(steam::steam_art_urls)
                    .unwrap_or_default(),
                Store::Epic => epic::epic_art_urls(sample.native_id),
                Store::Gog => gog::art_urls_for(sample.native_id),
            };
            game
        })
        .collect();

    // Demo mode should look like a real library, which is sorted by title.
    games.sort_by_key(|g| g.title.to_lowercase());
    games
}

/// Wraps the demo library in a `ScanReport` so it flows through exactly the
/// same path as a real scan.
pub fn scan_report() -> crate::model::ScanReport {
    let started = std::time::Instant::now();
    let games = library();
    let notes = crate::model::Store::ALL
        .iter()
        .map(|store| {
            let count = games.iter().filter(|g| g.store == *store).count();
            (*store, format!("{count} games (demo)"))
        })
        .collect();
    crate::model::ScanReport {
        games,
        notes,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_library_covers_every_store() {
        let games = library();
        for store in Store::ALL {
            assert!(
                games.iter().any(|g| g.store == store),
                "demo library has no {store} games"
            );
        }
        assert_eq!(games.len(), SAMPLES.len());
    }

    #[test]
    fn demo_games_have_unique_ids() {
        let games = library();
        let mut ids: Vec<&str> = games.iter().map(|g| g.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "demo library has duplicate ids");
    }

    #[test]
    fn demo_games_are_launchable_and_fully_populated() {
        for game in library() {
            assert!(!game.title.is_empty());
            assert!(!game.art_urls.is_empty(), "{} has no artwork", game.title);
            match &game.launch {
                LaunchTarget::Url(url) => assert!(!url.is_empty()),
                LaunchTarget::Direct { .. } => panic!("demo games should use a store URL"),
            }
        }
    }

    #[test]
    fn demo_library_is_sorted_by_title() {
        let games = library();
        let titles: Vec<String> = games.iter().map(|g| g.title.to_lowercase()).collect();
        let mut sorted = titles.clone();
        sorted.sort();
        assert_eq!(titles, sorted);
    }

    #[test]
    fn demo_library_exercises_sort_and_filter_paths() {
        let games = library();
        // Recency sorting needs both played and never-played entries.
        assert!(games.iter().any(|g| g.last_played.is_some()));
        assert!(games.iter().any(|g| g.last_played.is_none()));
        // Playtime formatting needs a range of magnitudes.
        let labels: Vec<String> = games.iter().filter_map(|g| g.playtime_label()).collect();
        assert!(labels.iter().any(|l| l.ends_with("min")));
        assert!(labels.iter().any(|l| l.ends_with(" h")));
    }
}
