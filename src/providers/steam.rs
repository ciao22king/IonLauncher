//! Steam library detection.
//!
//! Steam has no single index of installed games, so this walks the same
//! files the client itself uses:
//!
//! * `<root>/steamapps/libraryfolders.vdf` — every library location
//! * `<library>/steamapps/appmanifest_<appid>.acf` — one per installed game
//! * `<root>/userdata/<user>/config/localconfig.vdf` — playtime + last played
//!
//! Only installed titles are reported. `appmanifest_*.acf` exists exactly
//! when a game is installed, which is the distinction users care about.

use crate::model::{ArtSource, Game, LaunchTarget, Store};
use crate::util;
use crate::vdf;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Result of locating Steam, before any game parsing.
pub struct SteamInstall {
    pub root: PathBuf,
    pub libraries: Vec<PathBuf>,
}

/// Finds the Steam root directory on this machine.
pub fn locate(override_root: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = override_root {
        if path.is_dir() {
            return Some(path.to_path_buf());
        }
    }

    #[cfg(target_os = "windows")]
    {
        // The registry is authoritative and covers custom install drives.
        if let Some(path) = crate::providers::registry::read_string(
            r"HKEY_CURRENT_USER\Software\Valve\Steam",
            "SteamPath",
        ) {
            let p = PathBuf::from(path.replace('/', "\\"));
            if p.is_dir() {
                return Some(p);
            }
        }
        if let Some(path) = crate::providers::registry::read_string(
            r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ) {
            let p = PathBuf::from(path);
            if p.is_dir() {
                return Some(p);
            }
        }
    }

    let home = dirs::home_dir()?;
    #[allow(unused_mut)]
    let mut candidates = vec![
        home.join(".steam").join("steam"),
        home.join(".steam").join("root"),
        home.join(".local").join("share").join("Steam"),
        home.join(".var")
            .join("app")
            .join("com.valvesoftware.Steam")
            .join("data")
            .join("steam"),
        // Flatpak keeps its own copy under ~/.var.
        home.join(".var")
            .join("app")
            .join("com.valvesoftware.Steam")
            .join(".steam")
            .join("steam"),
    ];

    #[cfg(target_os = "macos")]
    candidates.push(
        home.join("Library")
            .join("Application Support")
            .join("Steam"),
    );

    #[cfg(target_os = "windows")]
    {
        if let Some(program_files) = std::env::var_os("ProgramFiles(x86)") {
            candidates.push(PathBuf::from(program_files).join("Steam"));
        }
        candidates.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    }

    candidates.into_iter().find(|p| is_steam_root(p))
}

/// A Steam root always contains `steamapps` or the client binary.
fn is_steam_root(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    path.join("steamapps").is_dir()
        || path.join("steam.exe").is_file()
        || path.join("ubuntu12_32").is_dir()
        || path.join("package").join("steam.sh").is_file()
}

/// Resolves Steam, including every configured library folder.
pub fn detect(override_root: Option<&Path>) -> Option<SteamInstall> {
    let root = locate(override_root)?;
    let mut libraries = vec![root.clone()];
    for extra in read_library_folders(&root) {
        if !libraries.iter().any(|l| same_dir(l, &extra)) {
            libraries.push(extra);
        }
    }
    Some(SteamInstall { root, libraries })
}

/// Reads `libraryfolders.vdf` and returns the library paths it lists.
///
/// The file has had three layouts over the years; all three are handled:
/// numbered keys with a `path` child (current), numbered keys whose value
/// *is* the path (older), and a `LibraryFolders` wrapper.
pub fn read_library_folders(root: &Path) -> Vec<PathBuf> {
    let candidates = [
        root.join("steamapps").join("libraryfolders.vdf"),
        root.join("config").join("libraryfolders.vdf"),
        root.join("steamapps").join("libraryfolders.vdf.txt"),
    ];
    for path in candidates {
        let Some(text) = util::read_text(&path) else {
            continue;
        };
        let Ok(parsed) = vdf::parse(&text) else {
            continue;
        };
        let parsed = parsed
            .get("libraryfolders")
            .or_else(|| parsed.get("LibraryFolders"))
            .cloned()
            .unwrap_or(parsed);
        let mut out = Vec::new();
        for value in parsed.values() {
            // Current layout: the value is a map with a `path` key.
            let candidate = value
                .get_str("path")
                .or_else(|| value.get_str("Path"))
                .map(PathBuf::from)
                // Legacy layout: the value is the path itself.
                .or_else(|| value.as_str().map(PathBuf::from));
            if let Some(path) = candidate {
                let path = normalize(&path);
                if path.is_dir() {
                    out.push(path);
                }
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    Vec::new()
}

/// Steam writes Windows paths with backslashes even when read on Linux
/// (shared libraries on a dual-boot drive), so convert when needed.
fn normalize(path: &Path) -> PathBuf {
    #[cfg(not(target_os = "windows"))]
    {
        let text = path.to_string_lossy();
        if text.contains('\\') {
            return PathBuf::from(text.replace('\\', "/"));
        }
    }
    path.to_path_buf()
}

fn same_dir(a: &Path, b: &Path) -> bool {
    // Steam paths often differ only by a trailing separator or a symlink.
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Playtime and last-played data, keyed by appid.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayStats {
    pub playtime_secs: u64,
    pub last_played: u64,
}

/// Reads playtime for every user profile found under `userdata`.
///
/// When several profiles exist the largest playtime wins, which matches
/// what the user most likely means by "my playtime".
pub fn read_playtime(root: &Path) -> HashMap<u32, PlayStats> {
    let mut stats: HashMap<u32, PlayStats> = HashMap::new();
    let userdata = root.join("userdata");
    if !userdata.is_dir() {
        return stats;
    }
    for profile in util::subdirs(&userdata) {
        // `0` is the anonymous profile Steam creates for guests.
        if profile.file_name().and_then(|n| n.to_str()) == Some("0") {
            continue;
        }
        let config = profile.join("config").join("localconfig.vdf");
        // These files can reach tens of megabytes; 32 MiB is far above any
        // realistic profile while still bounding memory use.
        let Some(text) = util::read_text_capped(&config, 32 * 1024 * 1024) else {
            continue;
        };
        let Ok(parsed) = vdf::parse(&text) else {
            continue;
        };
        // `Software` -> `Valve` -> `Steam` -> `apps` -> `<appid>` -> `Playtime`
        let Some(apps) = parsed
            .get("UserLocalConfigStore")
            .and_then(|v| v.get("Software"))
            .and_then(|v| v.get("Valve"))
            .and_then(|v| v.get("Steam"))
            .and_then(|v| v.get("apps"))
        else {
            continue;
        };
        for (key, entry) in apps.as_map().into_iter().flatten() {
            let Ok(appid) = key.parse::<u32>() else {
                continue;
            };
            // Steam reports minutes here, despite the bare `Playtime` name.
            let minutes = entry
                .get_u64("Playtime")
                .or_else(|| entry.get_u64("playtime"))
                .unwrap_or(0);
            let last = entry
                .get_u64("LastPlayed")
                .or_else(|| entry.get_u64("lastplayed"))
                .unwrap_or(0);
            let candidate = PlayStats {
                playtime_secs: minutes * 60,
                last_played: last,
            };
            let existing = stats.entry(appid).or_default();
            if candidate.playtime_secs > existing.playtime_secs {
                existing.playtime_secs = candidate.playtime_secs;
            }
            if candidate.last_played > existing.last_played {
                existing.last_played = candidate.last_played;
            }
        }
    }
    stats
}

/// Parses one `appmanifest_<appid>.acf` file.
pub fn parse_app_manifest(path: &Path) -> Option<Game> {
    let text = util::read_text(path)?;
    let parsed = vdf::parse(&text).ok()?;
    let state = parsed.get("AppState").unwrap_or(&parsed);

    let appid = state.get_u32("appid")?;
    let name = state.get_str("name").unwrap_or("").trim().to_string();
    if name.is_empty() {
        return None;
    }
    let install_dir = state.get_str("installdir").unwrap_or("").to_string();

    let mut game = Game::new(
        format!("steam:{appid}"),
        appid.to_string(),
        Store::Steam,
        name.clone(),
    );
    game.app_id = Some(appid);
    game.size_bytes = state.get_u64("SizeOnDisk").filter(|s| *s > 0);
    game.launch = LaunchTarget::Url(format!("steam://runsappid/{appid}"));
    // The install directory sits beside the manifest: `steamapps/common/<dir>`.
    if !install_dir.is_empty() {
        if let Some(steamapps) = path.parent() {
            game.install_dir = steamapps.join("common").join(&install_dir);
        }
    }
    game.art_sources = steam_art_sources(appid, None);
    Some(game)
}

/// Steam's public capsule CDN, plus the local library cache.
///
/// The cache is tried first: Steam writes `library_600x900_2x.jpg` there when
/// a game is installed, which is both higher resolution than the public 1x
/// copy and available with no network at all.
///
/// `library_600x900` is the portrait cover used on grid cards; `header` is the
/// wide banner used on list rows.
pub fn steam_art_sources(appid: u32, steam_root: Option<&Path>) -> Vec<ArtSource> {
    let mut sources = Vec::new();

    // Steam keeps one cache directory per library root. The artwork is stored
    // under `<appcache>/librarycache/<appid>/`, and the 2x file is the one
    // worth using when present.
    if let Some(root) = steam_root {
        for dir in [
            root.join("appcache").join("librarycache"),
            root.join("config").join("librarycache"),
        ] {
            let app_dir = dir.join(appid.to_string());
            for name in [
                "library_600x900_2x.jpg",
                "library_600x900.jpg",
                "header_2x.jpg",
                "header.jpg",
            ] {
                let candidate = app_dir.join(name);
                if candidate.is_file() {
                    sources.push(ArtSource::local(candidate));
                }
            }
        }
    }

    // The public CDN. The 2x portrait cover is preferred over the 1x one so
    // the grid stays sharp on a HiDPI display.
    for name in [
        "library_600x900_2x.jpg",
        "library_600x900.jpg",
        "header.jpg",
        "capsule_616x353.jpg",
    ] {
        sources.push(ArtSource::url(format!(
            "https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/{name}"
        )));
    }
    sources
}

/// Scans every library folder and returns all installed Steam games.
pub fn scan(install: &SteamInstall) -> Vec<Game> {
    let stats = read_playtime(&install.root);
    let mut games: Vec<Game> = Vec::new();
    for library in &install.libraries {
        let steamapps = library.join("steamapps");
        for manifest in util::files_with_ext(&steamapps, "acf") {
            let Some(file_name) = manifest.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // `appmanifest_*.acf` is the sole reliable marker of an install.
            if !file_name.starts_with("appmanifest_") {
                continue;
            }
            if let Some(mut game) = parse_app_manifest(&manifest) {
                if let Some(appid) = game.app_id {
                    // The root is known here, so the on-disk library cache can
                    // be added ahead of the CDN URLs.
                    game.art_sources = steam_art_sources(appid, Some(&install.root));
                    if let Some(stat) = stats.get(&appid) {
                        game.playtime_secs = Some(stat.playtime_secs).filter(|s| *s > 0);
                        game.last_played = Some(stat.last_played).filter(|s| *s > 0);
                    }
                }
                // Manifests without SizeOnDisk are common on older clients;
                // fall back to measuring the install folder.
                if game.size_bytes.is_none() && game.install_dir.is_dir() {
                    game.size_bytes = util::dir_size(&game.install_dir, 20_000);
                }
                games.push(game);
            }
        }
    }
    games.sort_by_key(|g| g.title.to_lowercase());
    games.dedup_by(|a, b| a.id == b.id);
    games
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ion-steam-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_app_manifest_into_a_game() {
        let dir = temp_dir("manifest");
        let steamapps = dir.join("steamapps");
        std::fs::create_dir_all(steamapps.join("common").join("Team Fortress 2")).unwrap();
        let manifest = steamapps.join("appmanifest_440.acf");
        std::fs::write(
            &manifest,
            r#"
"AppState"
{
    "appid"      "440"
    "name"       "Team Fortress 2"
    "installdir" "Team Fortress 2"
    "SizeOnDisk" "28765432100"
    "StateFlags" "4"
}
"#,
        )
        .unwrap();

        let game = parse_app_manifest(&manifest).unwrap();
        assert_eq!(game.id, "steam:440");
        assert_eq!(game.native_id, "440");
        assert_eq!(game.app_id, Some(440));
        assert_eq!(game.store, Store::Steam);
        assert_eq!(game.title, "Team Fortress 2");
        assert_eq!(game.size_bytes, Some(28_765_432_100));
        assert_eq!(
            game.install_dir,
            steamapps.join("common").join("Team Fortress 2")
        );
        match &game.launch {
            LaunchTarget::Url(u) => assert_eq!(u, "steam://runsappid/440"),
            other => panic!("unexpected launch target: {other:?}"),
        }
        assert!(game
            .art_sources
            .iter()
            .any(|s| matches!(s, ArtSource::Url(url) if url.contains("/440/"))));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn manifest_without_a_name_is_skipped() {
        let dir = temp_dir("noname");
        let manifest = dir.join("appmanifest_1.acf");
        std::fs::write(&manifest, "\"AppState\" { \"appid\" \"1\" \"name\" \"\" }").unwrap();
        assert!(parse_app_manifest(&manifest).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_library_folders_current_layout() {
        let dir = temp_dir("libs");
        let steamapps = dir.join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        let second = dir.join("second-library");
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(
            steamapps.join("libraryfolders.vdf"),
            format!(
                r#"
"libraryfolders"
{{
    "0"
    {{
        "path" "{}"
    }}
    "1"
    {{
        "path" "{}"
    }}
    "2"
    {{
        "path" "/does/not/exist"
    }}
}}
"#,
                dir.display(),
                second.display()
            ),
        )
        .unwrap();

        let libs = read_library_folders(&dir);
        // The nonexistent path is dropped.
        assert_eq!(libs.len(), 2);
        assert!(libs.contains(&dir));
        assert!(libs.contains(&second));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_library_folders_legacy_layout() {
        let dir = temp_dir("legacy");
        let steamapps = dir.join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        let legacy = dir.join("old-lib");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(
            steamapps.join("libraryfolders.vdf"),
            format!("\"LibraryFolders\" {{ \"1\" \"{}\" }}", legacy.display()),
        )
        .unwrap();

        let libs = read_library_folders(&dir);
        assert_eq!(libs, vec![legacy]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_playtime_from_localconfig() {
        let dir = temp_dir("playtime");
        let profile = dir.join("userdata").join("12345").join("config");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(
            profile.join("localconfig.vdf"),
            r#"
"UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "apps"
                {
                    "440"
                    {
                        "Playtime"   "1234"
                        "LastPlayed" "1714566600"
                    }
                    "620"
                    {
                        "Playtime" "10"
                    }
                }
            }
        }
    }
}
"#,
        )
        .unwrap();

        let stats = read_playtime(&dir);
        // `Playtime` is in minutes; 1234 min -> 74040 s.
        assert_eq!(stats.get(&440).unwrap().playtime_secs, 1234 * 60);
        assert_eq!(stats.get(&440).unwrap().last_played, 1_714_566_600);
        assert_eq!(stats.get(&620).unwrap().playtime_secs, 600);
        assert_eq!(stats.get(&620).unwrap().last_played, 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn skips_the_anonymous_profile() {
        let dir = temp_dir("anon");
        let profile = dir.join("userdata").join("0").join("config");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(
            profile.join("localconfig.vdf"),
            r#""UserLocalConfigStore" { "Software" { "Valve" { "Steam" { "apps" { "440" { "Playtime" "99" } } } } } }"#,
        )
        .unwrap();
        assert!(read_playtime(&dir).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn playtime_keeps_the_largest_across_profiles() {
        let dir = temp_dir("multi");
        for (user, minutes) in [("111", 10u32), ("222", 500u32)] {
            let profile = dir.join("userdata").join(user).join("config");
            std::fs::create_dir_all(&profile).unwrap();
            std::fs::write(
                profile.join("localconfig.vdf"),
                format!(
                    r#""UserLocalConfigStore" {{ "Software" {{ "Valve" {{ "Steam" {{ "apps" {{ "440" {{ "Playtime" "{minutes}" }} }} }} }} }} }}"#
                ),
            )
            .unwrap();
        }
        let stats = read_playtime(&dir);
        assert_eq!(stats.get(&440).unwrap().playtime_secs, 500 * 60);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_collects_games_from_every_library() {
        let dir = temp_dir("scan");
        let mut libraries = Vec::new();
        for (index, appid) in [440u32, 620].iter().enumerate() {
            let library = dir.join(format!("lib{index}"));
            let steamapps = library.join("steamapps");
            std::fs::create_dir_all(&steamapps).unwrap();
            std::fs::write(
                steamapps.join(format!("appmanifest_{appid}.acf")),
                format!(
                    "\"AppState\" {{ \"appid\" \"{appid}\" \"name\" \"Game {appid}\" \"installdir\" \"Game\" \"SizeOnDisk\" \"1000\" }}"
                ),
            )
            .unwrap();
            libraries.push(library);
        }
        let install = SteamInstall {
            root: dir.clone(),
            libraries,
        };
        let games = scan(&install);
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].title, "Game 440");
        assert_eq!(games[1].title, "Game 620");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_ignores_files_that_are_not_app_manifests() {
        let dir = temp_dir("filter");
        let steamapps = dir.join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        std::fs::write(
            steamapps.join("other_thing.acf"),
            "\"AppState\" { \"appid\" \"9\" \"name\" \"X\" }",
        )
        .unwrap();
        let install = SteamInstall {
            root: dir.clone(),
            libraries: vec![dir.clone()],
        };
        assert!(scan(&install).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn art_urls_are_ordered_cover_first() {
        let sources = steam_art_sources(440, None);
        let urls: Vec<&String> = sources
            .iter()
            .filter_map(|s| match s {
                ArtSource::Url(url) => Some(url),
                _ => None,
            })
            .collect();
        // The 2x portrait cover is preferred, then the 1x one, then the banner.
        assert!(urls[0].ends_with("library_600x900_2x.jpg"));
        assert!(urls[1].ends_with("library_600x900.jpg"));
        assert!(urls.iter().any(|u| u.ends_with("header.jpg")));
        assert!(urls.len() >= 3);
    }

    #[test]
    fn a_local_library_cache_is_preferred_over_the_cdn() {
        // Steam's own cache is higher resolution and needs no network, so it
        // must be tried before any URL.
        let root = temp_dir("artcache");
        let cache = root.join("appcache").join("librarycache").join("440");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("library_600x900_2x.jpg"), b"jpeg").unwrap();

        let sources = steam_art_sources(440, Some(&root));
        assert!(
            matches!(&sources[0], ArtSource::Local(p) if p.ends_with("library_600x900_2x.jpg")),
            "local cache should come first, got {:?}",
            sources[0]
        );
        // The CDN entries must still be present as fallbacks.
        assert!(sources.iter().any(|s| matches!(s, ArtSource::Url(_))));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn no_local_cache_means_only_cdn_sources() {
        let sources = steam_art_sources(440, Some(Path::new("/definitely/not/here")));
        assert!(sources.iter().all(|s| matches!(s, ArtSource::Url(_))));
    }
}
