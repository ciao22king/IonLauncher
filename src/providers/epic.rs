//! Epic Games library detection.
//!
//! Epic's client writes one JSON manifest per installed title under
//! `<ProgramData>/Epic/EpicGamesLauncher/Data/Manifests/*.item`. Each
//! manifest contains the launch executable, install location and the
//! `CatalogNamespace`/`CatalogItemId`/`AppName` triple needed to build a
//! launcher URL.
//!
//! Heroic and Legendary (the Linux/macOS clients) are detected too, since
//! they store equivalent metadata in a plain `installed.json`.

use crate::json_util as js;
use crate::model::{ArtSource, Game, LaunchTarget, Store};
use crate::util;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Locates the Epic manifests directory.
pub fn manifests_dir(override_root: Option<&Path>) -> Option<PathBuf> {
    if let Some(root) = override_root {
        // Accept either the manifests dir itself or a parent to search.
        let direct = root.join("Manifests");
        if direct.is_dir() {
            return Some(direct);
        }
        if root.is_dir() {
            return Some(root.to_path_buf());
        }
    }

    let mut candidates = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Some(program_data) = std::env::var_os("ProgramData") {
            candidates.push(
                PathBuf::from(program_data)
                    .join("Epic")
                    .join("EpicGamesLauncher")
                    .join("Data")
                    .join("Manifests"),
            );
        }
        candidates.push(PathBuf::from(
            r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests",
        ));
    }

    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from(
            "/Users/Shared/Epic Games/EpicGamesLauncher/Data/Manifests",
        ));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(home) = dirs::home_dir() {
            // Heroic keeps Epic manifests under its own config tree.
            candidates.push(
                home.join(".config")
                    .join("heroic")
                    .join("legendaryConfig")
                    .join("legendary"),
            );
            candidates.push(
                home.join(".var")
                    .join("app")
                    .join("com.heroicgameslauncher.hgl")
                    .join("config")
                    .join("legendaryConfig")
                    .join("legendary"),
            );
        }
    }

    candidates.into_iter().find(|p| p.is_dir())
}

/// Where Heroic/Legendary records installed Epic titles.
fn heroic_installed_paths() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    vec![
        home.join(".config")
            .join("heroic")
            .join("legendaryConfig")
            .join("legendary")
            .join("installed.json"),
        home.join(".var")
            .join("app")
            .join("com.heroicgameslauncher.hgl")
            .join("config")
            .join("legendaryConfig")
            .join("legendary")
            .join("installed.json"),
    ]
}

/// Builds the `com.epicgames.launcher://` URL for a title.
///
/// Going through the launcher (rather than the raw executable) keeps cloud
/// saves, achievements and ownership checks working.
pub fn launch_url(namespace: &str, catalog_id: &str, app_name: &str) -> String {
    format!(
        "com.epicgames.launcher://apps/{}?action=launch&silent=true",
        js::percent_encode(&format!("{namespace}:{catalog_id}:{app_name}"))
    )
}

/// Converts one Epic manifest JSON object into a `Game`.
pub fn game_from_manifest(value: &Value) -> Option<Game> {
    let app_name = js::get_str(value, "AppName")?;
    let display_name = js::get_str(value, "DisplayName").unwrap_or_else(|| app_name.clone());
    let namespace = js::get_str(value, "CatalogNamespace").unwrap_or_default();
    let catalog_id = js::get_str(value, "CatalogItemId").unwrap_or_default();

    let mut game = Game::new(
        format!("epic:{app_name}"),
        app_name.clone(),
        Store::Epic,
        display_name,
    );

    if let Some(location) = js::get_str(value, "InstallLocation") {
        game.install_dir = PathBuf::from(location);
    }
    game.size_bytes = js::get_u64(value, "InstallSize").filter(|s| *s > 0);
    game.last_played = js::get_str(value, "LastLaunchTime")
        .or_else(|| js::get_str(value, "LastPlayed"))
        .and_then(|t| util::parse_timestamp(&t));

    // Prefer the store protocol when we have the full identifier triple,
    // otherwise fall back to running the executable directly.
    if !namespace.is_empty() && !catalog_id.is_empty() {
        game.launch = LaunchTarget::Url(launch_url(&namespace, &catalog_id, &app_name));
    } else if let Some(exe) = js::get_str(value, "LaunchExecutable") {
        let exe_path = game
            .install_dir
            .join(exe.replace('\\', std::path::MAIN_SEPARATOR_STR));
        game.launch = LaunchTarget::Direct {
            exe: exe_path,
            args: epic_launch_args(value),
        };
    } else {
        return None;
    }

    game.art_sources = epic_art_sources(&app_name, &game.title);
    Some(game)
}

/// Epic passes these on the command line when it starts a title directly.
fn epic_launch_args(value: &Value) -> Vec<String> {
    let mut args = Vec::new();
    for (key, prefix) in [
        ("AppName", "-AUTH_TYPE=exchangecode -epicapp="),
        ("CatalogNamespace", "-epicnamespace="),
    ] {
        if let Some(v) = js::get_str(value, key) {
            args.push(format!("{prefix}{v}"));
        }
    }
    args
}

/// Epic artwork sources for an app name.
///
/// Epic's public CDN paths are keyed by a lowercased app name, but the real
/// storefront URLs embed opaque hashes, so the guessed paths are a best
/// effort. The launchers do cache the real covers locally, and those are
/// tried first because they always work offline.
pub fn epic_art_sources(app_name: &str, title: &str) -> Vec<ArtSource> {
    let mut sources = Vec::new();

    for dir in epic_cache_dirs(app_name) {
        for name in [
            "portrait.jpg",
            "cover.jpg",
            "DieselGameBoxTall.jpg",
            "DieselGameBox.jpg",
            "wide.jpg",
        ] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                sources.push(ArtSource::local(candidate));
            }
        }
    }

    // Epic's storefront content API resolves the real, hashed CDN URLs. The
    // app name is an internal codename for many titles, so the human title is
    // offered as a second lookup when the two differ.
    sources.push(ArtSource::epic_product(app_name));
    if !title.is_empty() && title != app_name {
        sources.push(ArtSource::epic_product(title));
    }

    // Guessed CDN paths are kept last: they only work for a handful of
    // older titles, but they cost nothing when the API already answered.
    for key in [app_name.to_ascii_lowercase(), title.to_ascii_lowercase()] {
        if key.is_empty() {
            continue;
        }
        for name in [
            format!("{key}-portrait-600x800.jpg"),
            format!("{key}-wide-1920x1080.jpg"),
        ] {
            sources.push(ArtSource::url(format!(
                "https://cdn2.unrealengine.com/{name}"
            )));
        }
    }
    sources
}

/// Directories where Epic clients cache a title's artwork.
fn epic_cache_dirs(app_name: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    // Heroic stores per-game art under its config and data roots.
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        bases.push(home.join(".config").join("heroic"));
        bases.push(
            home.join("Library")
                .join("Application Support")
                .join("heroic"),
        );
        bases.push(
            home.join(".var")
                .join("app")
                .join("com.heroicgameslauncher.hgl")
                .join("config")
                .join("heroic"),
        );
    }
    if let Some(data) = dirs::data_dir() {
        bases.push(data.join("heroic"));
    }
    // The Epic Games Launcher's own web cache on Windows.
    if let Some(local) = dirs::data_local_dir() {
        bases.push(
            local
                .join("EpicGamesLauncher")
                .join("Saved")
                .join("webcache"),
        );
    }

    for base in bases {
        dirs.push(base.join("images").join(app_name));
        dirs.push(base.join("images").join(app_name.to_ascii_lowercase()));
        dirs.push(base.join("epic_store").join("library").join(app_name));
        dirs.push(base.join("icons").join(app_name));
    }
    dirs
}

/// Reads all Epic manifests found on disk.
pub fn scan(override_root: Option<&Path>) -> Vec<Game> {
    let mut games = Vec::new();

    if let Some(dir) = manifests_dir(override_root) {
        for manifest in util::files_with_ext(&dir, "item") {
            let Some(text) = util::read_text(&manifest) else {
                continue;
            };
            // A single corrupt manifest must not abort the whole scan.
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            // A `.item` file normally holds one manifest object, but some
            // third-party tools write an array instead, so accept both.
            let entries: Vec<&Value> = match &value {
                Value::Array(items) => items.iter().collect(),
                Value::Object(_) => vec![&value],
                _ => Vec::new(),
            };
            for entry in entries {
                if let Some(game) = game_from_manifest(entry) {
                    games.push(game);
                }
            }
        }
    }

    games.extend(scan_heroic());
    games.sort_by_key(|g| g.title.to_lowercase());
    games.dedup_by(|a, b| a.id == b.id);
    games
}

/// Reads Heroic/Legendary's `installed.json`, which is a map keyed by app name.
fn scan_heroic() -> Vec<Game> {
    let mut games = Vec::new();
    for path in heroic_installed_paths() {
        let Some(text) = util::read_text(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        for entry in js::entries(&value) {
            if let Some(game) = heroic_game(entry) {
                games.push(game);
            }
        }
    }
    games
}

fn heroic_game(entry: &Value) -> Option<Game> {
    let app_name = js::get_str(entry, "app_name")
        .or_else(|| js::get_str(entry, "appName"))
        .or_else(|| js::get_str(entry, "app_title"))?;
    let title = js::get_str(entry, "title")
        .or_else(|| js::get_str(entry, "app_title"))
        .unwrap_or_else(|| app_name.clone());

    let mut game = Game::new(
        format!("epic:{app_name}"),
        app_name.clone(),
        Store::Epic,
        title,
    );
    if let Some(dir) = js::get_str(entry, "install_path") {
        game.install_dir = PathBuf::from(dir);
    }
    game.size_bytes = js::get_u64(entry, "install_size").filter(|s| *s > 0);
    game.last_played = js::get_str(entry, "last_played")
        .or_else(|| js::get_str(entry, "last_launched"))
        .and_then(|t| util::parse_timestamp(&t));

    let namespace = js::get_str(entry, "namespace").unwrap_or_default();
    let catalog_id = js::get_str(entry, "catalog_item_id").unwrap_or_default();
    if !namespace.is_empty() && !catalog_id.is_empty() {
        game.launch = LaunchTarget::Url(launch_url(&namespace, &catalog_id, &app_name));
    } else {
        let exe = js::get_str(entry, "executable").unwrap_or_default();
        if exe.is_empty() {
            return None;
        }
        let exe_path = if Path::new(&exe).is_absolute() {
            PathBuf::from(&exe)
        } else {
            game.install_dir.join(exe)
        };
        game.launch = LaunchTarget::Direct {
            exe: exe_path,
            args: Vec::new(),
        };
    }
    game.art_sources = epic_art_sources(&app_name, &game.title);
    Some(game)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ion-epic-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn builds_a_launcher_url_with_escaped_separators() {
        let url = launch_url("fn", "cat", "Fortnite");
        assert!(url.starts_with("com.epicgames.launcher://apps/"));
        assert!(url.contains("fn%3Acat%3AFortnite"));
        assert!(url.ends_with("?action=launch&silent=true"));
    }

    #[test]
    fn parses_a_manifest() {
        let value = json!({
            "AppName": "Fortnite",
            "DisplayName": "Fortnite",
            "CatalogNamespace": "fn",
            "CatalogItemId": "cat",
            "InstallLocation": "/games/Fortnite",
            "InstallSize": 40000000000u64,
            "LastLaunchTime": "2024-05-01T12:30:00Z"
        });
        let game = game_from_manifest(&value).unwrap();
        assert_eq!(game.id, "epic:Fortnite");
        assert_eq!(game.store, Store::Epic);
        assert_eq!(game.title, "Fortnite");
        assert_eq!(game.install_dir, PathBuf::from("/games/Fortnite"));
        assert_eq!(game.size_bytes, Some(40_000_000_000));
        assert_eq!(game.last_played, Some(1_714_566_600));
        match &game.launch {
            LaunchTarget::Url(u) => assert!(u.contains("fn%3Acat%3AFortnite")),
            other => panic!("unexpected launch target: {other:?}"),
        }
    }

    #[test]
    fn falls_back_to_the_executable_without_a_catalog_id() {
        let value = json!({
            "AppName": "SomeGame",
            "DisplayName": "Some Game",
            "InstallLocation": "/games/SomeGame",
            "LaunchExecutable": "bin/game.exe"
        });
        let game = game_from_manifest(&value).unwrap();
        match &game.launch {
            LaunchTarget::Direct { exe, .. } => {
                assert!(exe.ends_with("bin/game.exe"), "got {}", exe.display())
            }
            other => panic!("unexpected launch target: {other:?}"),
        }
    }

    #[test]
    fn windows_style_executable_separators_are_normalised() {
        let value = json!({
            "AppName": "SomeGame",
            "DisplayName": "Some Game",
            "InstallLocation": "/games/SomeGame",
            "LaunchExecutable": "bin\\win64\\game.exe"
        });
        let game = game_from_manifest(&value).unwrap();
        match &game.launch {
            LaunchTarget::Direct { exe, .. } => {
                assert_eq!(exe, Path::new("/games/SomeGame/bin/win64/game.exe"))
            }
            other => panic!("unexpected launch target: {other:?}"),
        }
    }

    #[test]
    fn a_manifest_without_an_app_name_is_rejected() {
        assert!(game_from_manifest(&json!({"DisplayName": "X"})).is_none());
    }

    #[test]
    fn a_manifest_without_any_launch_method_is_rejected() {
        assert!(game_from_manifest(&json!({"AppName": "X"})).is_none());
    }

    #[test]
    fn scan_reads_item_files_and_skips_corrupt_ones() {
        let dir = temp_dir("scan");
        std::fs::write(
            dir.join("a.item"),
            json!({
                "AppName": "Alpha",
                "DisplayName": "Alpha",
                "CatalogNamespace": "ns",
                "CatalogItemId": "id"
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("broken.item"), "{ not json").unwrap();
        // Files with other extensions are ignored entirely.
        std::fs::write(dir.join("notes.txt"), "hello").unwrap();

        let games = scan(Some(&dir));
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "Alpha");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_handles_an_array_of_manifests() {
        let dir = temp_dir("array");
        // Some third-party tools write a single JSON array instead of one
        // file per title.
        std::fs::write(
            dir.join("all.item"),
            json!([{
                "AppName": "Beta",
                "DisplayName": "Beta",
                "CatalogNamespace": "ns",
                "CatalogItemId": "id"
            }])
            .to_string(),
        )
        .unwrap();
        let games = scan(Some(&dir));
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "Beta");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn heroic_entries_are_converted() {
        let entry = json!({
            "app_name": "HeroicGame",
            "title": "Heroic Game",
            "install_path": "/games/Heroic",
            "install_size": 5000,
            "namespace": "ns",
            "catalog_item_id": "cid",
            "last_played": "2024-05-01T12:30:00Z"
        });
        let game = heroic_game(&entry).unwrap();
        assert_eq!(game.id, "epic:HeroicGame");
        assert_eq!(game.title, "Heroic Game");
        assert_eq!(game.size_bytes, Some(5000));
        assert!(matches!(game.launch, LaunchTarget::Url(_)));
    }

    #[test]
    fn heroic_entries_without_launch_info_are_skipped() {
        assert!(heroic_game(&json!({"app_name": "X"})).is_none());
    }

    #[test]
    fn art_sources_try_the_api_then_guessed_cdn_paths() {
        let sources = epic_art_sources("Fortnite", "Fortnite");
        // The API resolves real hashed URLs, so it is tried before the
        // guessed paths that only work for a few older titles.
        assert!(
            matches!(&sources[0], ArtSource::EpicProduct(name) if name == "Fortnite"),
            "expected an API lookup first, got {:?}",
            sources[0]
        );
        let urls: Vec<&String> = sources
            .iter()
            .filter_map(|s| match s {
                ArtSource::Url(url) => Some(url),
                _ => None,
            })
            .collect();
        assert!(urls.iter().any(|u| u.contains("fortnite-portrait")));
    }
}
