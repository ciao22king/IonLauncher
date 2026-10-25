//! Background artwork loader.
//!
//! Artwork is the only thing in IonLauncher that touches the network, so it
//! is kept completely off the UI thread and off the startup path:
//!
//! * decoding and downloads happen on worker threads
//! * decoded covers are cached on disk, so the second launch needs no network
//! * covers are downscaled before upload, which keeps VRAM use small enough
//!   for integrated GPUs and old cards

use egui::ColorImage;
use std::io::Read as _;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

/// Longest edge of a decoded cover. Portrait covers are displayed at most
/// ~300 pt wide, so this is generous even on a HiDPI display.
const MAX_EDGE: u32 = 512;

/// Downloads are capped so one slow CDN cannot stall a worker forever.
const FETCH_TIMEOUT: Duration = Duration::from_secs(12);

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
    pub fn request(&self, game_id: &str, urls: &[String]) {
        if urls.is_empty() {
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

        let urls = urls.to_vec();
        let tx = self.tx.clone();
        let inflight = self.inflight.clone();
        let cache_dir = self.cache_dir.clone();
        let fetch_enabled = self.fetch_enabled;
        let game_id = game_id.to_string();

        // One short-lived thread per request. Artwork is requested in small
        // batches as cards become visible, so this stays bounded in practice.
        std::thread::spawn(move || {
            let image = load_art(&game_id, &urls, cache_dir.as_deref(), fetch_enabled);
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

/// Loads artwork, preferring the disk cache and falling back to the network.
fn load_art(
    game_id: &str,
    urls: &[String],
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

    if !fetch_enabled {
        return None;
    }

    for url in urls {
        let Some(bytes) = fetch(url) else { continue };
        let Some(image) = decode(&bytes) else { continue };
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
    if length.is_some_and(|len| len > 20 * 1024 * 1024) {
        return None;
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(20 * 1024 * 1024)
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
    fn loader_skips_requests_without_urls() {
        let loader = ArtLoader::new(true);
        loader.request("steam:1", &[]);
        assert!(loader.poll().is_empty());
    }

    #[test]
    fn loader_reports_nothing_when_fetching_is_disabled() {
        let loader = ArtLoader::new(false);
        assert!(!loader.fetch_enabled());
        // With no cache and fetching off, the request resolves to no artwork.
        loader.request("steam:2", &["https://example.invalid/a.jpg".into()]);
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
    fn cache_dir_is_under_the_platform_cache_root() {
        let dir = art_cache_dir().expect("cache dir");
        assert!(dir.ends_with("art"));
        assert!(dir.to_string_lossy().contains("IonLauncher"));
    }
}
