//! Background artwork loader.
//!
//! Artwork is the only thing in IonLauncher that touches the network, so it
//! is kept completely off the UI thread and off the startup path:
//!
//! * decoding and downloads happen on worker threads
//! * decoded covers are cached on disk, so the second launch needs no network
//! * covers are downscaled before upload, which keeps VRAM use small enough
//!   for integrated GPUs and old cards
//!
//! A game carries an ordered list of [`ArtSource`]s. Local files are tried
//! first because a store that already cached a cover gives us a better image
//! without any network traffic; URL and API sources follow.

use crate::model::ArtSource;
use egui::ColorImage;
use std::collections::HashMap;
use std::io::Read as _;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

/// Longest edge of a decoded cover. Portrait covers are displayed at most
/// ~300 pt wide, so this is generous even on a HiDPI display.
const MAX_EDGE: u32 = 512;

/// Downloads are capped so one slow CDN cannot stall a worker forever.
const FETCH_TIMEOUT: Duration = Duration::from_secs(12);

/// Largest image we will read, in bytes.
const MAX_BYTES: u64 = 20 * 1024 * 1024;

pub struct ArtResult {
    pub game_id: String,
    pub image: Option<ColorImage>,
}

/// Coordinates artwork fetching for the visible portion of the library.
pub struct ArtLoader {
    tx: Sender<ArtResult>,
    rx: Receiver<ArtResult>,
    /// Keys currently being fetched, so scrolling does not re-queue them.
    inflight: std::sync::Arc<Mutex<HashMap<String, bool>>>,
    cache_dir: Option<PathBuf>,
    fetch_enabled: bool,
    /// Guards against an unbounded disk cache on machines used for years.
    cache_limit_bytes: u64,
}

impl ArtLoader {
    pub fn new(fetch_enabled: bool) -> Self {
        let (tx, rx) = channel();
        let inflight = std::sync::Arc::new(Mutex::new(HashMap::new()));
        let cache_dir = art_cache_dir();
        if let Some(dir) = &cache_dir {
            std::fs::create_dir_all(dir).ok();
        }
        let loader = Self {
            tx,
            rx,
            inflight,
            cache_dir,
            fetch_enabled,
            cache_limit_bytes: 256 * 1024 * 1024,
        };
        // Housekeeping happens once at startup rather than during painting.
        loader.trim_disk_cache();
        loader
    }

    pub fn fetch_enabled(&self) -> bool {
        self.fetch_enabled
    }

    pub fn set_fetch_enabled(&mut self, enabled: bool) {
        self.fetch_enabled = enabled;
    }

    /// Queues artwork for `game_id` if it is not already being loaded.
    ///
    /// Returns immediately; results arrive through [`ArtLoader::poll`].
    pub fn request(&self, game_id: &str, sources: &[ArtSource]) {
        if sources.is_empty() {
            return;
        }
        {
            let mut inflight = match self.inflight.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if inflight.contains_key(game_id) {
                return;
            }
            inflight.insert(game_id.to_string(), true);
        }

        let sources = sources.to_vec();
        let tx = self.tx.clone();
        let inflight = self.inflight.clone();
        let cache_dir = self.cache_dir.clone();
        let fetch_enabled = self.fetch_enabled;
        let game_id = game_id.to_string();

        // One short-lived thread per request. Artwork is requested in small
        // batches as cards become visible, so this stays bounded in practice.
        std::thread::spawn(move || {
            let image = load_art(&game_id, &sources, cache_dir.as_deref(), fetch_enabled);
            if let Ok(mut guard) = inflight.lock() {
                guard.remove(&game_id);
            }
            // A closed receiver just means the app is shutting down.
            let _ = tx.send(ArtResult { game_id, image });
        });
    }

    /// Collects finished artwork without blocking.
    pub fn poll(&self) -> Vec<ArtResult> {
        let mut out = Vec::new();
        while let Ok(result) = self.rx.try_recv() {
            out.push(result);
        }
        out
    }

    /// Deletes the on-disk cache, e.g. after changing the artwork setting.
    pub fn clear_disk_cache(&self) {
        if let Some(dir) = &self.cache_dir {
            std::fs::remove_dir_all(dir).ok();
            std::fs::create_dir_all(dir).ok();
        }
    }

    /// Keeps the disk cache under its size limit, oldest files first.
    pub fn trim_disk_cache(&self) {
        let Some(dir) = &self.cache_dir else { return };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = entries
            .flatten()
            .filter_map(|entry| {
                let meta = entry.metadata().ok()?;
                if !meta.is_file() {
                    return None;
                }
                Some((entry.path(), meta.len(), meta.modified().ok()?))
            })
            .collect();
        let total: u64 = files.iter().map(|(_, size, _)| size).sum();
        if total <= self.cache_limit_bytes {
            return;
        }
        // Oldest first, so recently played titles keep their artwork.
        files.sort_by_key(|(_, _, modified)| *modified);
        let mut remaining = total;
        for (path, size, _) in files {
            if remaining <= self.cache_limit_bytes {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                remaining = remaining.saturating_sub(size);
            }
        }
    }
}

/// Cache file name for a game. The id is sanitised so it is a valid filename
/// on every platform.
fn cache_path(dir: &std::path::Path, game_id: &str) -> PathBuf {
    let safe: String = game_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    dir.join(format!("{safe}.png"))
}

/// Loads artwork, preferring the disk cache and falling back to the sources.
///
/// `fetch_enabled` only gates the network-backed sources. A local file is
/// still read when fetching is off: it costs nothing and is often the only
/// cover a machine with a metered connection will ever see.
fn load_art(
    game_id: &str,
    sources: &[ArtSource],
    cache_dir: Option<&std::path::Path>,
    fetch_enabled: bool,
) -> Option<ColorImage> {
    if let Some(dir) = cache_dir {
        let cached = cache_path(dir, game_id);
        if let Ok(bytes) = std::fs::read(&cached) {
            if let Some(image) = decode(&bytes) {
                return Some(image);
            }
            // A truncated cache entry should not wedge the game forever.
            std::fs::remove_file(&cached).ok();
        }
    }

    for source in sources {
        let Some(image) = load_source(source, fetch_enabled) else {
            continue;
        };
        if let Some(dir) = cache_dir {
            // Re-encode the downscaled image, which also normalises whatever
            // exotic format the CDN returned into something cheap to decode.
            if let Some(png) = encode_png(&image) {
                std::fs::write(cache_path(dir, game_id), png).ok();
            }
        }
        return Some(image);
    }
    None
}

/// Resolves one source to a decoded image.
fn load_source(source: &ArtSource, fetch_enabled: bool) -> Option<ColorImage> {
    match source {
        ArtSource::Local(path) => {
            let bytes = std::fs::read(path).ok()?;
            decode(&bytes)
        }
        ArtSource::Url(url) => {
            if !fetch_enabled {
                return None;
            }
            let bytes = fetch(url)?;
            decode(&bytes)
        }
        ArtSource::GogProduct(product_id) => {
            if !fetch_enabled {
                return None;
            }
            // GOG's artwork paths are content hashes, so the API has to be
            // asked which one belongs to this product.
            for url in gog_art_urls(product_id) {
                if let Some(image) = fetch(&url).and_then(|bytes| decode(&bytes)) {
                    return Some(image);
                }
            }
            None
        }
        ArtSource::EpicProduct(app_name) => {
            if !fetch_enabled {
                return None;
            }
            for url in epic_api_art_urls(app_name) {
                if let Some(image) = fetch(&url).and_then(|bytes| decode(&bytes)) {
                    return Some(image);
                }
            }
            None
        }
    }
}

/// Asks GOG's public API for a product's artwork URLs.
///
/// The portrait box art is preferred, then the wide background as a fallback
/// for products that have no box art. A response that cannot be parsed yields
/// no URLs rather than an error, because artwork is always optional.
pub fn gog_art_urls(product_id: &str) -> Vec<String> {
    let numeric: String = product_id.chars().filter(|c| c.is_ascii_digit()).collect();
    if numeric.is_empty() {
        return Vec::new();
    }
    let url = format!("https://api.gog.com/v2/games/{numeric}");
    let Some(body) = fetch(&url) else {
        return Vec::new();
    };
    let Ok(text) = std::str::from_utf8(&body) else {
        return Vec::new();
    };
    parse_gog_art_urls(text)
}

/// Asks Epic's storefront content API for a title's artwork URLs.
///
/// Epic's CDN paths embed opaque hashes, so unlike Steam they cannot be built
/// from the app name. The content API is public and returns the real hero and
/// portrait art. The app name has to be turned into a storefront slug, which
/// is lowercase and hyphen-separated; several candidate forms are tried
/// because Epic's slugs do not follow one rule.
pub fn epic_api_art_urls(app_name: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for slug in epic_slug_candidates(app_name) {
        let api =
            format!("https://store-content.ak.epicgames.com/api/en-US/content/products/{slug}");
        let Some(body) = fetch(&api) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&body) else {
            continue;
        };
        urls.extend(parse_epic_art_urls(text));
        if !urls.is_empty() {
            break;
        }
    }
    urls
}

/// Slug forms worth trying for an Epic app name.
///
/// `AlanWake2` becomes `alanwake2`, `alan-wake-2` and `alan-wake2`; Epic uses
/// different conventions per title, so all plausible spellings are offered.
pub fn epic_slug_candidates(app_name: &str) -> Vec<String> {
    let lower: String = app_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    let mut out = vec![lower.clone()];

    // Insert a hyphen before each internal capital letter, then also before
    // trailing digits, covering both `alan-wake2` and `alan-wake-2`.
    let mut spaced = String::new();
    let mut prev_lower_or_digit = false;
    for ch in app_name.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && prev_lower_or_digit {
                spaced.push('-');
            }
            spaced.push(ch.to_ascii_lowercase());
            prev_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        }
    }
    if !spaced.is_empty() && spaced != lower {
        out.push(spaced.clone());
    }
    // `foo2` -> `foo-2`, for titles that separate the sequel number.
    let with_digit_gap: String = {
        let mut s = String::new();
        let mut prev_digit = false;
        for ch in spaced.chars() {
            if ch.is_ascii_digit() && !prev_digit && !s.is_empty() && !s.ends_with('-') {
                s.push('-');
            }
            prev_digit = ch.is_ascii_digit();
            s.push(ch);
        }
        s
    };
    if !with_digit_gap.is_empty() && !out.contains(&with_digit_gap) {
        out.push(with_digit_gap);
    }
    out.dedup();
    out
}

/// Pulls the hero/portrait image links out of an Epic content API response.
///
/// Split from the request so the parsing can be tested without the network.
pub fn parse_epic_art_urls(body: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let mut urls = Vec::new();
    // The portrait is the cover the grid wants; the wide background is the
    // fallback for titles that have no portrait art.
    for key in ["portraitBackgroundImageUrl", "backgroundImageUrl"] {
        let Some(pages) = value.get("pages").and_then(|p| p.as_array()) else {
            continue;
        };
        for page in pages {
            if let Some(href) = page
                .get("data")
                .and_then(|d| d.get("hero"))
                .and_then(|h| h.get(key))
                .and_then(|v| v.as_str())
            {
                if !urls.iter().any(|u| u == href) {
                    urls.push(href.to_string());
                }
            }
        }
    }
    urls
}

/// Pulls the image links out of a GOG v2 API response.
///
/// Split from the request so the parsing can be tested without the network.
pub fn parse_gog_art_urls(body: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(links) = value.get("_links") else {
        return Vec::new();
    };

    let mut urls = Vec::new();
    // Box art is the portrait cover the grid wants; the wide background and
    // the Galaxy background are progressively worse fallbacks.
    for key in ["boxArtImage", "backgroundImage", "galaxyBackgroundImage"] {
        if let Some(href) = links
            .get(key)
            .and_then(|v| v.get("href"))
            .and_then(|v| v.as_str())
        {
            urls.push(href.to_string());
        }
    }
    // The product image is templated; `product_630` is the store-page size.
    if let Some(href) = links
        .get("_embedded")
        .and_then(|v| v.get("product"))
        .and_then(|v| v.get("_links"))
        .and_then(|v| v.get("image"))
        .and_then(|v| v.get("href"))
        .and_then(|v| v.as_str())
    {
        if href.contains("{formatter}") {
            urls.push(href.replace("{formatter}", "product_630"));
        } else {
            urls.push(href.to_string());
        }
    }
    urls
}

fn fetch(url: &str) -> Option<Vec<u8>> {
    let agent = ureq::AgentBuilder::new()
        .timeout(FETCH_TIMEOUT)
        .user_agent("IonLauncher/0.1 (+https://github.com/ciao22king/IonLauncher)")
        .build();
    let response = agent.get(url).call().ok()?;
    // Guard against a redirect loop or a mislabelled huge file.
    let length = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    if length.is_some_and(|len| len > MAX_BYTES) {
        return None;
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(bytes)
}

/// Decodes and downscales image bytes into an egui image.
fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = downscale(image);
    let rgba = image.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

/// Scales an image down so its longest edge is at most [`MAX_EDGE`].
///
/// Upscaling is never performed: a small cover stays small, which is both
/// faster and sharper than letting the GPU stretch a low-res image.
fn downscale(image: image::DynamicImage) -> image::DynamicImage {
    let (width, height) = (image.width(), image.height());
    let longest = width.max(height);
    if longest <= MAX_EDGE {
        return image;
    }
    let scale = MAX_EDGE as f32 / longest as f32;
    let new_width = ((width as f32 * scale).round() as u32).max(1);
    let new_height = ((height as f32 * scale).round() as u32).max(1);
    image.resize_exact(new_width, new_height, image::imageops::FilterType::Triangle)
}

fn encode_png(image: &ColorImage) -> Option<Vec<u8>> {
    let width = u32::try_from(image.size[0]).ok()?;
    let height = u32::try_from(image.size[1]).ok()?;
    let mut raw = Vec::with_capacity(image.pixels.len() * 4);
    for pixel in &image.pixels {
        raw.extend_from_slice(&pixel.to_array());
    }
    let buffer = image::RgbaImage::from_raw(width, height, raw)?;
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(buffer)
        .write_to(&mut out, image::ImageFormat::Png)
        .ok()?;
    Some(out.into_inner())
}

/// Cache location: `~/.cache/IonLauncher/art` on Linux,
/// `~/Library/Caches/...` on macOS, `%LOCALAPPDATA%\...` on Windows.
pub fn art_cache_dir() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("IonLauncher").join("art"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_paths_are_sanitised_for_every_platform() {
        let dir = PathBuf::from("/tmp/art");
        // The `steam:440` style id must not leak a colon into a filename.
        let path = cache_path(&dir, "steam:440");
        assert_eq!(path.file_name().unwrap(), "steam_440.png");
        let path = cache_path(&dir, "epic:My Game/1");
        assert_eq!(path.file_name().unwrap(), "epic_My_Game_1.png");
    }

    #[test]
    fn downscale_shrinks_large_images_and_keeps_aspect_ratio() {
        let big = image::DynamicImage::ImageRgba8(image::RgbaImage::new(1200, 600));
        let scaled = downscale(big);
        assert_eq!(scaled.width(), MAX_EDGE);
        // 2:1 aspect ratio is preserved.
        assert_eq!(scaled.height(), MAX_EDGE / 2);
    }

    #[test]
    fn downscale_never_upscales_small_images() {
        let small = image::DynamicImage::ImageRgba8(image::RgbaImage::new(64, 64));
        let scaled = downscale(small);
        assert_eq!((scaled.width(), scaled.height()), (64, 64));
    }

    #[test]
    fn decode_round_trips_through_png() {
        let source = ColorImage::new(
            [2, 1],
            vec![
                egui::Color32::from_rgb(255, 0, 0),
                egui::Color32::from_rgb(0, 128, 255),
            ],
        );
        let png = encode_png(&source).expect("encode");
        let decoded = decode(&png).expect("decode");
        assert_eq!(decoded.size, [2, 1]);
        assert_eq!(decoded.pixels[0], source.pixels[0]);
        assert_eq!(decoded.pixels[1], source.pixels[1]);
    }

    #[test]
    fn decode_rejects_garbage_without_panicking() {
        assert!(decode(b"this is not an image").is_none());
        assert!(decode(&[]).is_none());
        assert!(decode(&[0xff, 0xd8, 0xff]).is_none());
    }

    #[test]
    fn loader_skips_requests_without_sources() {
        let loader = ArtLoader::new(true);
        loader.request("steam:1", &[]);
        assert!(loader.poll().is_empty());
    }

    #[test]
    fn local_artwork_is_read_without_any_network() {
        // A store that already cached the cover must not need the network,
        // even when fetching is disabled entirely.
        let dir = temp_dir("local-art");
        let image_path = dir.join("cover.png");
        let source = ColorImage::new([4, 4], vec![egui::Color32::from_rgb(10, 20, 30); 16]);
        std::fs::write(&image_path, encode_png(&source).unwrap()).unwrap();

        let image = load_source(&ArtSource::local(&image_path), false);
        assert!(image.is_some(), "a local file needs no network");
        assert_eq!(image.unwrap().size, [4, 4]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_local_file_falls_through_to_the_next_source() {
        let dir = temp_dir("fallthrough");
        let real = dir.join("real.png");
        let source = ColorImage::new([2, 2], vec![egui::Color32::WHITE; 4]);
        std::fs::write(&real, encode_png(&source).unwrap()).unwrap();

        let sources = vec![
            ArtSource::local(dir.join("absent.png")),
            ArtSource::local(&real),
        ];
        let image = load_art("g", &sources, None, false);
        assert!(image.is_some(), "the second source should have been used");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn network_sources_are_skipped_when_fetching_is_disabled() {
        assert!(load_source(&ArtSource::url("https://example.invalid/a.jpg"), false).is_none());
        assert!(load_source(&ArtSource::gog_product("1207658930"), false).is_none());
        assert!(load_source(&ArtSource::epic_product("Fortnite"), false).is_none());
    }

    #[test]
    fn epic_artwork_links_are_extracted_from_the_api_response() {
        let body = r#"{
            "pages": [
                {"data": {"hero": {
                    "portraitBackgroundImageUrl": "https://cdn2.unrealengine.com/portrait.jpg",
                    "backgroundImageUrl": "https://cdn2.unrealengine.com/wide.jpg"
                }}}
            ]
        }"#;
        let urls = parse_epic_art_urls(body);
        assert_eq!(urls.len(), 2);
        // The portrait is the cover the grid wants, so it comes first.
        assert!(urls[0].ends_with("portrait.jpg"));
        assert!(urls[1].ends_with("wide.jpg"));
    }

    #[test]
    fn epic_parsing_survives_responses_without_hero_art() {
        assert!(parse_epic_art_urls("{}").is_empty());
        assert!(parse_epic_art_urls("not json").is_empty());
        assert!(parse_epic_art_urls(r#"{"pages":[]}"#).is_empty());
        assert!(parse_epic_art_urls(r#"{"pages":[{"data":{}}]}"#).is_empty());
    }

    #[test]
    fn epic_slugs_cover_the_naming_conventions() {
        // Epic mixes camelCase and kebab-case slugs, so several forms are tried.
        let fortnite = epic_slug_candidates("Fortnite");
        assert!(fortnite.contains(&"fortnite".to_string()));

        let rocket = epic_slug_candidates("RocketLeague");
        assert!(rocket.contains(&"rocketleague".to_string()));
        assert!(rocket.contains(&"rocket-league".to_string()));

        let alan = epic_slug_candidates("AlanWake2");
        assert!(alan.contains(&"alanwake2".to_string()));
        assert!(alan.contains(&"alan-wake-2".to_string()));
    }

    #[test]
    fn gog_artwork_links_are_extracted_from_the_api_response() {
        let body = r#"{
            "_links": {
                "boxArtImage": {"href": "https://images.gog-statics.com/box.jpg"},
                "backgroundImage": {"href": "https://images.gog-statics.com/bg.jpg"}
            }
        }"#;
        let urls = parse_gog_art_urls(body);
        assert_eq!(urls.len(), 2);
        // Portrait box art is the cover the grid wants, so it comes first.
        assert_eq!(urls[0], "https://images.gog-statics.com/box.jpg");
        assert_eq!(urls[1], "https://images.gog-statics.com/bg.jpg");
    }

    #[test]
    fn gog_product_image_template_is_filled_in() {
        let body = r#"{
            "_links": {
                "_embedded": {
                    "product": {
                        "_links": {
                            "image": {
                                "href": "https://images.gog-statics.com/hash_{formatter}.png",
                                "templated": true
                            }
                        }
                    }
                }
            }
        }"#;
        let urls = parse_gog_art_urls(body);
        assert_eq!(urls.len(), 1);
        assert!(!urls[0].contains("{formatter}"), "template was not filled");
        assert!(urls[0].ends_with("_product_630.png"));
    }

    #[test]
    fn gog_responses_without_images_are_not_fatal() {
        assert!(parse_gog_art_urls("{}").is_empty());
        assert!(parse_gog_art_urls("not json").is_empty());
        assert!(parse_gog_art_urls(r#"{"_links":{}}"#).is_empty());
        // A product id with no digits can never resolve, so no request is made.
        assert!(gog_art_urls("no-digits-here").is_empty());
    }

    #[test]
    fn loader_reports_nothing_when_fetching_is_disabled() {
        let loader = ArtLoader::new(false);
        assert!(!loader.fetch_enabled());
        // With no cache and fetching off, the request resolves to no artwork.
        loader.request(
            "steam:2",
            &[ArtSource::url("https://example.invalid/a.jpg")],
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let results = loader.poll();
            if !results.is_empty() {
                assert!(results[0].image.is_none());
                break;
            }
            assert!(std::time::Instant::now() < deadline, "loader never replied");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn the_disk_cache_short_circuits_every_source() {
        // Once a cover is cached, a broken source list must not matter.
        let dir = temp_dir("cache-shortcircuit");
        let image = ColorImage::new([3, 3], vec![egui::Color32::from_rgb(1, 2, 3); 9]);
        std::fs::write(cache_path(&dir, "steam:42"), encode_png(&image).unwrap()).unwrap();

        let sources = vec![ArtSource::local(dir.join("nope.png"))];
        let loaded = load_art("steam:42", &sources, Some(&dir), false);
        assert!(loaded.is_some(), "the cached cover should have been used");
        assert_eq!(loaded.unwrap().size, [3, 3]);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ion-art-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cache_dir_is_under_the_platform_cache_root() {
        let dir = art_cache_dir().expect("cache dir");
        assert!(dir.ends_with("art"));
        assert!(dir.to_string_lossy().contains("IonLauncher"));
    }
}
