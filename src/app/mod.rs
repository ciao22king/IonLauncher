//! Application state and screen routing.

use crate::art::ArtLoader;
use crate::config::{Config, SortMode};
use crate::model::{Game, LaunchTarget, ScanReport};
use crate::providers;
use crate::theme::Palette;
use egui::{Context, RichText, Vec2};
use std::sync::mpsc::{channel, Receiver};

pub mod details;
pub mod library;
pub mod search;
pub mod settings;
pub mod sidebar;
pub mod toolbar;

/// Pages reachable from the left menu.
///
/// These are views of the same library, not separate per-store sections: the
/// menu switches what you are doing, while the collection stays unified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Library,
    Search,
    Settings,
}

/// Result of a background library scan.
pub struct ScanOutcome {
    pub report: ScanReport,
}

/// A transient message shown at the bottom of the window.
pub struct Toast {
    pub text: String,
    pub is_error: bool,
    /// Timestamp at which the toast should disappear.
    pub expires_at: f64,
}

pub struct IonApp {
    pub config: Config,
    pub screen: Screen,
    pub games: Vec<Game>,
    pub search: String,
    /// Show only starred games. Replaces the old per-store "rooms".
    pub favorites_only: bool,
    /// Set while a background scan is running.
    pub scanning: bool,
    pub scan_rx: Option<Receiver<ScanOutcome>>,
    pub last_report: Option<ScanReport>,
    pub art: ArtLoader,
    pub toast: Option<Toast>,
    /// Currently open details panel, by game id.
    pub details_for: Option<String>,
    /// Cached filtered/sorted view, rebuilt only when inputs change.
    pub view_cache: Option<ViewCache>,
    /// Set when the config needs writing to disk.
    pub config_dirty: bool,
    /// Proton builds found on this machine, refreshed on demand.
    pub proton_installed: Vec<crate::proton::InstalledProton>,
    /// Downloadable community builds, fetched in the background.
    pub proton_releases: Option<Vec<crate::proton::Release>>,
    pub proton_releases_error: Option<String>,
    pub proton_fetching_releases: bool,
    /// In-flight download, if any, plus the tag being installed.
    pub proton_download: Option<(String, std::sync::Arc<crate::proton::DownloadProgress>)>,
    /// Receive channel for the background release listing.
    pub proton_release_rx: Option<Receiver<Result<Vec<crate::proton::Release>, String>>>,
}

/// Cached result of filtering and sorting, so the grid does not re-sort
/// every frame. This is the main reason scrolling stays smooth with a large
/// library on a slow machine.
pub struct ViewCache {
    pub key: ViewKey,
    pub ids: Vec<String>,
}

/// Everything that can change the visible ordering or membership.
#[derive(PartialEq, Clone)]
pub struct ViewKey {
    pub search: String,
    pub sort: SortMode,
    pub favorites_only: bool,
    pub show_missing: bool,
    pub games_len: usize,
    pub favorites_len: usize,
}

impl Default for IonApp {
    fn default() -> Self {
        Self::new(Config::load())
    }
}

impl IonApp {
    pub fn new(config: Config) -> Self {
        let mut app = Self {
            art: ArtLoader::new(config.fetch_artwork),
            config,
            screen: Screen::Library,
            games: Vec::new(),
            search: String::new(),
            favorites_only: false,
            scanning: false,
            scan_rx: None,
            last_report: None,
            toast: None,
            details_for: None,
            view_cache: None,
            config_dirty: false,
            // Filled on first use of the Proton settings section, so a machine
            // without Steam never pays for the directory walk.
            proton_installed: Vec::new(),
            proton_releases: None,
            proton_releases_error: None,
            proton_fetching_releases: false,
            proton_download: None,
            proton_release_rx: None,
        };
        app.rescan();
        app.refresh_proton();
        app
    }

    /// Re-reads the Proton builds installed on this machine.
    pub fn refresh_proton(&mut self) {
        self.proton_installed = crate::proton::installed();
    }

    /// Starts listing downloadable Proton builds, if not already doing so.
    ///
    /// Only the Linux Proton section offers downloads, so this is unused
    /// elsewhere and gated to keep the Windows build warning-free.
    #[cfg(target_os = "linux")]
    pub fn fetch_proton_releases(&mut self) {
        if self.proton_fetching_releases {
            return;
        }
        self.proton_fetching_releases = true;
        self.proton_releases_error = None;
        let (tx, rx) = channel();
        self.proton_release_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(crate::proton::fetch_releases());
        });
    }

    /// Collects the release listing and any finished download.
    pub fn poll_proton(&mut self) {
        if let Some(rx) = &self.proton_release_rx {
            if let Ok(result) = rx.try_recv() {
                self.proton_fetching_releases = false;
                self.proton_release_rx = None;
                match result {
                    Ok(releases) => self.proton_releases = Some(releases),
                    Err(err) => self.proton_releases_error = Some(err),
                }
            }
        }

        // A finished download needs its new build picked up and reported.
        let finished = self
            .proton_download
            .as_ref()
            .is_some_and(|(_, progress)| progress.is_done());
        if finished {
            let (tag, progress) = self.proton_download.take().expect("checked above");
            if let Some(err) = progress.error() {
                self.notify_error(&format!("Could not install {tag}: {err}"));
            } else {
                self.refresh_proton();
                // Select what was just installed: that is why the user
                // clicked download rather than browsing the list.
                self.config.proton_version = Some(tag.clone());
                self.config.proton_enabled = true;
                self.config_dirty = true;
                self.notify(&format!("{tag} installed"));
            }
        }
    }

    /// The Proton build launches should use, if any.
    ///
    /// A configured build that has since been removed falls back to the
    /// first installed one rather than silently launching without Proton.
    pub fn active_proton(&self) -> Option<&crate::proton::InstalledProton> {
        if !self.config.proton_enabled {
            return None;
        }
        if let Some(wanted) = &self.config.proton_version {
            if let Some(build) = self.proton_installed.iter().find(|b| &b.name == wanted) {
                return Some(build);
            }
        }
        self.proton_installed.first()
    }

    /// Starts a library scan on a worker thread.
    ///
    /// Scanning touches many files and can take a moment on a cold cache,
    /// so it never runs on the UI thread.
    pub fn rescan(&mut self) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        let (tx, rx) = channel();
        self.scan_rx = Some(rx);
        let config = self.config.clone();

        std::thread::spawn(move || {
            let report = if config.demo_mode {
                crate::demo::scan_report()
            } else {
                providers::scan_all(&config)
            };
            let _ = tx.send(ScanOutcome { report });
        });
    }

    /// Collects a finished scan, if any.
    pub fn poll_scan(&mut self) {
        let Some(rx) = &self.scan_rx else { return };
        let Ok(outcome) = rx.try_recv() else { return };
        self.scanning = false;
        self.scan_rx = None;
        self.games = outcome.report.games.clone();
        self.last_report = Some(outcome.report);
        self.view_cache = None;
        // Drop stale texture handles; they belong to the previous library.
        for game in &mut self.games {
            game.art = None;
            game.art_pending = false;
        }
        let count = self.games.len();
        self.notify(&format!("Loaded {count} games"));
    }

    pub fn notify(&mut self, text: &str) {
        self.toast = Some(Toast {
            text: text.to_string(),
            is_error: false,
            expires_at: 0.0,
        });
    }

    pub fn notify_error(&mut self, text: &str) {
        self.toast = Some(Toast {
            text: text.to_string(),
            is_error: true,
            expires_at: 0.0,
        });
    }

    /// Launches a game by id and reports the outcome.
    pub fn launch(&mut self, game_id: &str) {
        let Some(game) = self.games.iter().find(|g| g.id == game_id) else {
            return;
        };
        let title = game.title.clone();
        // Demo entries are fictional, so handing their URLs to the OS would
        // either fail or open the wrong thing.
        if self.config.demo_mode {
            self.notify(&format!("Demo mode: {title} is not installed"));
            return;
        }

        // On Linux a Windows executable normally needs Proton. Doing this
        // before the plain launch path means the user's Proton choice applies
        // without them having to think about which game is which.
        let via_proton = self.proton_launch_plan(game);
        let outcome = match via_proton {
            Some(plan) => plan.run(),
            None => crate::widgets::launch_game(game),
        };

        match outcome {
            Ok(()) => {
                // Mark it as just played so recency sorting reflects reality
                // without waiting for the store to update its own metadata.
                let now = crate::util::now_epoch();
                if let Some(game) = self.games.iter_mut().find(|g| g.id == game_id) {
                    game.last_played = Some(now);
                }
                self.view_cache = None;
                self.notify(&format!("Launching {title}"));
            }
            Err(err) => self.notify_error(&format!("Could not launch {title}: {err}")),
        }
    }

    /// Decides whether a game should start through Proton, and prepares it.
    ///
    /// Returns `None` whenever Proton does not apply: the setting is off, we
    /// are not on Linux, no build is installed, or the title launches through
    /// a store protocol. That last case matters because Steam applies its own
    /// per-game compatibility setting, and overriding it from here would
    /// contradict what the user configured in Steam.
    fn proton_launch_plan(&self, game: &Game) -> Option<crate::proton::ProtonLaunch> {
        if !cfg!(target_os = "linux") {
            return None;
        }
        let build = self.active_proton()?;
        let LaunchTarget::Direct { exe, args } = &game.launch else {
            return None;
        };
        // Only Windows binaries need the compatibility layer.
        if !crate::proton::looks_like_windows_executable(exe) {
            return None;
        }
        let data_dir = crate::proton::data_dir()?;
        let prefix = crate::proton::prefix_for(&data_dir, &game.id);
        Some(crate::proton::ProtonLaunch::new(
            build.clone(),
            prefix,
            exe.clone(),
            args.clone(),
        ))
    }

    pub fn toggle_favorite(&mut self, game_id: &str) {
        let now_favorite = self.config.toggle_favorite(game_id);
        self.config_dirty = true;
        self.view_cache = None;
        let title = self
            .games
            .iter()
            .find(|g| g.id == game_id)
            .map(|g| g.title.clone())
            .unwrap_or_default();
        if now_favorite {
            self.notify(&format!("Added {title} to favourites"));
        } else {
            self.notify(&format!("Removed {title} from favourites"));
        }
    }

    /// Rebuilds the cached filtered view when its inputs change.
    pub fn refresh_view(&mut self) {
        let key = ViewKey {
            search: self.search.trim().to_lowercase(),
            sort: self.config.sort,
            favorites_only: self.favorites_only,
            show_missing: self.config.show_missing,
            games_len: self.games.len(),
            favorites_len: self.config.favorites.len(),
        };
        if self.view_cache.as_ref().is_some_and(|c| c.key == key) {
            return;
        }

        let mut indices: Vec<usize> = self
            .games
            .iter()
            .enumerate()
            .filter(|(_, game)| self.matches(game, &key))
            .map(|(index, _)| index)
            .collect();

        match key.sort {
            SortMode::RecentlyPlayed => indices.sort_by_key(|i| self.games[*i].recency_key()),
            SortMode::Name => indices.sort_by_key(|i| self.games[*i].title.to_lowercase()),
            SortMode::Playtime => indices
                .sort_by_key(|i| std::cmp::Reverse(self.games[*i].playtime_secs.unwrap_or(0))),
            SortMode::Size => {
                indices.sort_by_key(|i| std::cmp::Reverse(self.games[*i].size_bytes.unwrap_or(0)))
            }
        }

        let ids = indices
            .into_iter()
            .map(|i| self.games[i].id.clone())
            .collect();
        self.view_cache = Some(ViewCache { key, ids });
    }

    fn matches(&self, game: &Game, key: &ViewKey) -> bool {
        if key.favorites_only && !self.config.is_favorite(&game.id) {
            return false;
        }
        if !key.show_missing
            && !game.install_dir.as_os_str().is_empty()
            && !game.install_dir.exists()
        {
            return false;
        }
        if key.search.is_empty() {
            return true;
        }
        // Substring match on the title, which is what users expect from a
        // launcher search box.
        game.title.to_lowercase().contains(&key.search)
    }

    /// Ids of the games to draw, in display order.
    pub fn visible_ids(&self) -> &[String] {
        self.view_cache
            .as_ref()
            .map(|c| c.ids.as_slice())
            .unwrap_or(&[])
    }

    pub fn game(&self, id: &str) -> Option<&Game> {
        self.games.iter().find(|g| g.id == id)
    }

    pub fn game_mut(&mut self, id: &str) -> Option<&mut Game> {
        self.games.iter_mut().find(|g| g.id == id)
    }

    /// Persists the config if anything changed.
    pub fn flush_config(&mut self) {
        if self.config_dirty {
            self.config.save();
            self.config_dirty = false;
        }
    }
}

impl eframe::App for IonApp {
    // egui 0.37 hands the app a root `Ui` rather than a `Context`, and panels
    // are built with the unified `Panel`/`CentralPanel` types.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_scan();
        self.poll_artwork(&ctx);
        self.poll_proton();
        self.refresh_view();
        self.paint_toast(&ctx);
        self.handle_shortcuts(&ctx);
        self.keep_animating(&ctx);

        egui::Panel::top("topbar")
            .frame(crate::ui::topbar_frame())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("ION")
                            .size(19.0)
                            .strong()
                            .color(Palette::ACCENT),
                    );
                    ui.label(
                        RichText::new("LAUNCHER")
                            .size(19.0)
                            .strong()
                            .color(Palette::TEXT),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if self.scanning {
                            "⟳  Scanning…"
                        } else {
                            "⟳  Refresh"
                        };
                        let button = egui::Button::new(RichText::new(label).size(13.0))
                            .fill(Palette::SURFACE_HOVER);
                        if ui
                            .add_enabled(!self.scanning, button)
                            .on_hover_text("Rescan the stores for installed games")
                            .clicked()
                        {
                            self.rescan();
                        }

                        if let Some(report) = &self.last_report {
                            let text = if self.scanning {
                                "scanning…".to_string()
                            } else {
                                format!("{} games · {} ms", self.games.len(), report.elapsed_ms)
                            };
                            ui.label(RichText::new(text).size(11.5).color(Palette::TEXT_FAINT));
                        }
                    });
                });
            });

        // The left menu is the primary navigation. It switches pages; it does
        // not split the library by store.
        egui::Panel::left("menu")
            .exact_size(196.0)
            .resizable(false)
            .frame(crate::ui::sidebar_frame())
            .show(ui, |ui| {
                sidebar::show(self, ui);
            });

        // The details sheet takes precedence over the central content, so it
        // is added to the remaining space before the page body.
        let details_open = self.details_for.is_some();
        if details_open {
            egui::Panel::right("details-panel")
                .exact_size(340.0)
                .resizable(false)
                .frame(crate::ui::sidebar_frame())
                .show(ui, |ui| {
                    details::show(self, ui);
                });
        }

        egui::CentralPanel::default()
            .frame(crate::ui::central_frame())
            .show(ui, |ui| match self.screen {
                Screen::Library => {
                    toolbar::show(self, ui);
                    library::show(self, ui);
                }
                Screen::Search => search::show(self, ui),
                Screen::Settings => settings::show(self, ui),
            });

        self.flush_config();
    }
}

impl IonApp {
    /// Keeps the window repainting while background work is in flight.
    ///
    /// egui only redraws in response to input, so without this a download or
    /// a scan would appear frozen until the user moved the mouse: results
    /// arrive on a channel that nothing is polling.
    fn keep_animating(&self, ctx: &Context) {
        let busy = self.scanning
            || self.proton_fetching_releases
            || self.proton_download.is_some()
            || self.toast.is_some()
            || self.games.iter().any(|g| g.art_pending);
        if busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// Uploads finished artwork into the GPU and attaches it to its game.
    fn poll_artwork(&mut self, ctx: &Context) {
        for result in self.art.poll() {
            let Some(game) = self.game_mut(&result.game_id) else {
                continue;
            };
            game.art_pending = false;
            if let Some(image) = result.image {
                let texture = ctx.load_texture(
                    format!("art-{}", result.game_id),
                    image,
                    egui::TextureOptions::LINEAR,
                );
                game.art = Some(texture);
            }
        }
    }

    /// Ctrl+F jumps to the search page, the way a browser or file manager does.
    fn handle_shortcuts(&mut self, ctx: &Context) {
        let pressed = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F));
        if pressed {
            self.screen = Screen::Search;
        }
    }

    fn paint_toast(&mut self, ctx: &Context) {
        let now = ctx.input(|i| i.time);
        let Some(toast) = &mut self.toast else { return };
        if toast.expires_at == 0.0 {
            toast.expires_at = now + 3.6;
        }
        if now >= toast.expires_at {
            self.toast = None;
            return;
        }
        let color = if toast.is_error {
            Palette::DANGER
        } else {
            Palette::OK
        };
        let text = toast.text.clone();
        egui::Area::new(egui::Id::unique("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, Vec2::new(0.0, -18.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Palette::SURFACE)
                    .stroke(egui::Stroke::new(1.0, color))
                    .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_CONTROL))
                    .inner_margin(egui::Margin::symmetric(14, 9))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 4],
                        blur: 14,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(150),
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("●").size(11.0).color(color));
                            ui.label(RichText::new(text).size(13.0).color(Palette::TEXT));
                        });
                    });
            });
    }
}
