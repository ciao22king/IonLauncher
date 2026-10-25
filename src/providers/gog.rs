//! GOG library detection.
//!
//! GOG has no manifest file per game. The authoritative source depends on
//! the platform:
//!
//! * **Windows** — GOG Galaxy registers every install under
//!   `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games\<id>` (and the 32-bit view),
//!   with `path`, `gameName` and `exe` values.
//! * **GOG Galaxy 2.x (any OS)** — `galaxy-2.0.db`, a SQLite database whose
//!   `LibraryReleases` table lists owned titles and `InstalledBaseProducts`
//!   lists what is installed. Read through our own minimal SQLite reader.
//! * **Heroic / gogdl (Linux, macOS)** — `gog_store/installed.json` plus
//!   `gog_store/library.json`.
//!
//! All available sources are merged and deduplicated.

use crate::json_util as js;
use crate::model::{Game, LaunchTarget, Store};
use crate::sqlite;
use crate::util;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The `goggalaxy://` protocol is what Galaxy itself uses to start a title.
pub fn launch_url(game_id: &str) -> String {
    format!("goggalaxy://openGameView/{game_id}")
}

/// Locates a GOG Galaxy data directory.
pub fn galaxy_data_dir(override_root: Option<&Path>) -> Option<PathBuf> {
    if let Some(root) = override_root {
        if root.is_dir() {
            return Some(root.to_path_buf());
        }
    }

    let mut candidates = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Some(program_data) = std::env::var_os("ProgramData") {
            candidates.push(PathBuf::from(program_data).join("GOG.com").join("Galaxy"));
        }
        candidates.push(PathBuf::from(r"C:\ProgramData\GOG.com\Galaxy"));
    }

    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from("/Users/Shared/GOG.com/Galaxy"));
        if let Some(home) = dirs::home_dir() {
            candidates.push(
                home.join("Library")
                    .join("Application Support")
                    .join("GOG.com")
                    .join("Galaxy"),
            );
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(home) = dirs::home_dir() {
            candidates.push(home.join("GOG.com").join("Galaxy"));
            candidates.push(
                home.join(".var")
                    .join("app")
                    .join("com.gog.galaxy")
                    .join("GOG.com")
                    .join("Galaxy"),
            );
        }
    }

    candidates.into_iter().find(|p| p.is_dir())
}

/// Heroic's GOG store directory, which holds `installed.json`.
fn heroic_gog_dirs() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    vec![
        home.join(".config").join("heroic").join("gog_store"),
        home.join(".var")
            .join("app")
            .join("com.heroicgameslauncher.hgl")
            .join("config")
            .join("gog_store"),
    ]
}

/// Reads installed titles from the Galaxy SQLite database.
pub fn scan_galaxy_db(db_path: &Path) -> Vec<Game> {
    let Ok(db) = sqlite::Database::open(db_path) else {
        return Vec::new();
    };
    let mut games = Vec::new();

    // `InstalledBaseProducts` links a product id to its install path and
    // is the closest equivalent to "installed games".
    if let Ok(rows) = db.rows("InstalledBaseProducts", 5000) {
        for row in rows {
            let Some(product_id) = text_of(&row, &["productId", "product_id"]) else {
                continue;
            };
            let install_path = text_of(&row, &["installPath", "install_path"])
                .map(PathBuf::from)
                .unwrap_or_default();
            let title = lookup_title(&db, &product_id)
                .unwrap_or_else(|| product_id.clone());
            let mut game = Game::new(
                format!("gog:{product_id}"),
                product_id.clone(),
                Store::Gog,
                title,
            );
            game.install_dir = install_path;
            game.launch = LaunchTarget::Url(launch_url(&product_id));
            game.art_urls = gog_art_urls(&product_id);
            games.push(game);
        }
    }

    // Some Galaxy versions only populate the releases table, with the
    // install path recorded per release.
    if games.is_empty() {
        if let Ok(rows) = db.rows("LibraryReleases", 5000) {
            for row in rows {
                let Some(product_id) = text_of(&row, &["gameId", "game_id"]) else {
                    continue;
                };
                let title = text_of(&row, &["title", "releaseKey", "name"])
                    .unwrap_or_else(|| product_id.clone());
                let mut game = Game::new(
                    format!("gog:{product_id}"),
                    product_id.clone(),
                    Store::Gog,
                    title,
                );
                if let Some(path) = text_of(&row, &["installPath", "path"]) {
                    game.install_dir = PathBuf::from(path);
                }
                game.launch = LaunchTarget::Url(launch_url(&product_id));
                game.art_urls = gog_art_urls(&product_id);
                games.push(game);
            }
        }
    }

    games
}

/// Finds a human title for a product id in `Products`.
fn lookup_title(db: &sqlite::Database, product_id: &str) -> Option<String> {
    let rows = db.rows("Products", 5000).ok()?;
    for row in rows {
        let id = text_of(&row, &["productId", "product_id"])?;
        if id != product_id {
            continue;
        }
        return text_of(&row, &["name", "title"]);
    }
    None
}

/// Case-insensitive column lookup.
fn text_of(row: &sqlite::Row, names: &[&str]) -> Option<String> {
    for name in names {
        if let Some(value) = row.get(*name) {
            if let Some(s) = value.as_str() {
                if !s.trim().is_empty() {
                    return Some(s.trim().to_string());
                }
            }
        }
        // Fall back to a case-insensitive scan.
        for (key, value) in row {
            if key.eq_ignore_ascii_case(name) {
                if let Some(s) = value.as_str() {
                    if !s.trim().is_empty() {
                        return Some(s.trim().to_string());
                    }
                }
            }
        }
    }
    None
}

/// GOG's public image CDN. The `_product_card` image is the portrait cover.
pub fn gog_art_urls(product_id: &str) -> Vec<String> {
    let numeric: String = product_id.chars().filter(|c| c.is_ascii_digit()).collect();
    if numeric.is_empty() {
        return Vec::new();
    }
    vec![
        format!("https://images.gog.com/{numeric}_product_card.jpg"),
        format!("https://images.gog.com/{numeric}_glx_vertical_cover.webp"),
    ]
}

/// Public alias used by the demo library and the UI.
pub fn art_urls_for(product_id: &str) -> Vec<String> {
    gog_art_urls(product_id)
}

/// Reads Heroic/gogdl's `installed.json`.
pub fn scan_heroic() -> Vec<Game> {
    let mut games = Vec::new();
    for dir in heroic_gog_dirs() {
        let Some(text) = util::read_text(&dir.join("installed.json")) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        // Shape: {"installed": [{"appName": "...", "install_path": "...",
        //          "title": "..."}]}
        let list = if js::get(&value, "installed").is_some() {
            js::get_array(&value, "installed")
        } else {
            js::entries(&value)
        };
        for entry in list {
            if let Some(game) = heroic_gog_game(entry) {
                games.push(game);
            }
        }
    }
    games
}

fn heroic_gog_game(entry: &Value) -> Option<Game> {
    let app_name = js::get_str(entry, "appName")
        .or_else(|| js::get_str(entry, "app_name"))
        .or_else(|| js::get_str(entry, "game_id"))?;
    let title = js::get_str(entry, "title")
        .or_else(|| js::get_str(entry, "name"))
        .unwrap_or_else(|| app_name.clone());

    let mut game = Game::new(
        format!("gog:{app_name}"),
        app_name.clone(),
        Store::Gog,
        title,
    );
    if let Some(dir) = js::get_str(entry, "install_path") {
        game.install_dir = PathBuf::from(dir);
    }
    game.size_bytes = js::get_u64(entry, "install_size")
        .or_else(|| js::get_u64(entry, "size"))
        .filter(|s| *s > 0);
    game.launch = LaunchTarget::Url(launch_url(&app_name));
    game.art_urls = gog_art_urls(&app_name);
    Some(game)
}

/// Reads GOG installs from the Windows registry.
#[cfg(target_os = "windows")]
pub fn scan_registry() -> Vec<Game> {
    let mut games = Vec::new();
    for (root, path) in [
        (
            crate::providers::registry::Hive::LocalMachine,
            r"SOFTWARE\WOW6432Node\GOG.com\Games",
        ),
        (
            crate::providers::registry::Hive::LocalMachine,
            r"SOFTWARE\GOG.com\Games",
        ),
    ] {
        for subkey in crate::providers::registry::subkeys(root, path) {
            let full = format!(r"{path}\{subkey}");
            let Some(name) = crate::providers::registry::read_string_hive(root, &full, "gameName")
            else {
                continue;
            };
            let Some(install_dir) = crate::providers::registry::read_string_hive(root, &full, "path")
            else {
                continue;
            };
            let mut game = Game::new(
                format!("gog:{subkey}"),
                subkey.clone(),
                Store::Gog,
                name,
            );
            game.install_dir = PathBuf::from(&install_dir);
            // Prefer the registered executable; fall back to the protocol.
            match crate::providers::registry::read_string_hive(root, &full, "exe") {
                Some(exe) => {
                    let exe_path = PathBuf::from(install_dir.clone()).join(
                        exe.replace('\\', std::path::MAIN_SEPARATOR_STR),
                    );
                    if exe_path.exists() {
                        game.launch = LaunchTarget::Direct {
                            exe: exe_path,
                            args: Vec::new(),
                        };
                    } else {
                        game.launch = LaunchTarget::Url(launch_url(&subkey));
                    }
                }
                None => game.launch = LaunchTarget::Url(launch_url(&subkey)),
            }
            game.art_urls = gog_art_urls(&subkey);
            games.push(game);
        }
    }
    games
}

/// No registry on non-Windows platforms, so this source contributes nothing.
#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn scan_registry() -> Vec<Game> {
    Vec::new()
}

/// Merges every available GOG source.
pub fn scan(override_root: Option<&Path>) -> Vec<Game> {
    let mut games: HashMap<String, Game> = HashMap::new();

    let add = |incoming: Vec<Game>, games: &mut HashMap<String, Game>| {
        for game in incoming {
            match games.get_mut(&game.id) {
                Some(existing) => {
                    // Prefer whichever record knows the install path.
                    if existing.install_dir.as_os_str().is_empty()
                        && !game.install_dir.as_os_str().is_empty()
                    {
                        existing.install_dir = game.install_dir;
                    }
                    if existing.size_bytes.is_none() {
                        existing.size_bytes = game.size_bytes;
                    }
                    if existing.art_urls.is_empty() {
                        existing.art_urls = game.art_urls;
                    }
                }
                None => {
                    games.insert(game.id.clone(), game);
                }
            }
        }
    };

    #[cfg(target_os = "windows")]
    add(scan_registry(), &mut games);

    if let Some(dir) = galaxy_data_dir(override_root) {
        // Galaxy keeps the database at the root of its data dir.
        for candidate in [dir.join("galaxy-2.0.db"), dir.join("storage").join("galaxy-2.0.db")] {
            if candidate.is_file() {
                add(scan_galaxy_db(&candidate), &mut games);
                break;
            }
        }
    }

    add(scan_heroic(), &mut games);

    let mut out: Vec<Game> = games.into_values().collect();
    out.sort_by_key(|g| g.title.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn launch_url_uses_the_galaxy_protocol() {
        assert_eq!(
            launch_url("1207658691"),
            "goggalaxy://openGameView/1207658691"
        );
    }

    #[test]
    fn art_urls_extract_the_numeric_id() {
        let urls = gog_art_urls("1207658691");
        assert!(urls[0].contains("1207658691_product_card"));
        // Non-numeric ids cannot be looked up on the CDN.
        assert!(gog_art_urls("gog_game_name").is_empty());
    }

    #[test]
    fn heroic_entries_are_converted() {
        let entry = json!({
            "appName": "1207658691",
            "title": "The Witcher 3",
            "install_path": "/games/witcher3",
            "install_size": 50000000000u64
        });
        let game = heroic_gog_game(&entry).unwrap();
        assert_eq!(game.id, "gog:1207658691");
        assert_eq!(game.store, Store::Gog);
        assert_eq!(game.title, "The Witcher 3");
        assert_eq!(game.size_bytes, Some(50_000_000_000));
        assert!(matches!(game.launch, LaunchTarget::Url(_)));
    }

    #[test]
    fn heroic_entries_without_an_app_name_are_skipped() {
        assert!(heroic_gog_game(&json!({"title": "X"})).is_none());
    }

    #[test]
    fn text_of_is_case_insensitive_and_skips_blanks() {
        let mut row = sqlite::Row::new();
        row.insert("productId".into(), sqlite::Value::Text("123".into()));
        row.insert("blank".into(), sqlite::Value::Text("   ".into()));
        assert_eq!(text_of(&row, &["productid"]).unwrap(), "123");
        assert!(text_of(&row, &["blank"]).is_none());
        assert!(text_of(&row, &["missing"]).is_none());
    }

    #[test]
    fn scanning_a_missing_database_yields_nothing() {
        assert!(scan_galaxy_db(Path::new("/definitely/not/a/db.sqlite")).is_empty());
    }
}
