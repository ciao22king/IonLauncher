//! Application state and screen routing.

use crate::art::ArtLoader;
use crate::config::{Config, SortMode};
use crate::model::{Game, ScanReport, Store};
use crate::providers;
use crate::theme::Palette;
use egui::{Context, RichText, Vec2};
use std::sync::mpsc::{channel, Receiver};

pub mod details;
pub mod library;
pub mod settings;
pub mod sidebar;

/// Top-level screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Library,
    Favorites,
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
    pub store_filter: Vec<Store>,
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
    pub stores: Vec<Store>,
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
            store_filter: Store::ALL.to_vec(),
            scanning: false,
            scan_rx: None,
            last_report: None,
            toast: None,
            details_for: None,
            view_cache: None,
            config_dirty: false,
        };
        app.rescan();
        app
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
        match crate::widgets::launch_game(game) {
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
            stores: self.store_filter.clone(),
            sort: self.config.sort,
            favorites_only: self.screen == Screen::Favorites,
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
            SortMode::Playtime => {
                indices.sort_by_key(|i| std::cmp::Reverse(self.games[*i].playtime_secs.unwrap_or(0)))
            }
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
        if !key.stores.contains(&game.store) {
            return false;
        }
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
        self.refresh_view();
        self.paint_toast(&ctx);

        egui::Panel::top("topbar")
            .frame(crate::ui::topbar_frame())
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new("ION").size(19.0).strong().color(Palette::ACCENT));
                    ui.label(
                        RichText::new("LAUNCHER")
                            .size(19.0)
                            .strong()
                            .color(Palette::TEXT),
                    );
                    ui.add_space(14.0);

                    let width = (ui.available_width() - 240.0).clamp(160.0, 460.0);
                    let response = ui.add_sized(
                        Vec2::new(width, 30.0),
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Search your games…")
                            .desired_width(f32::INFINITY),
                    );
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let term = self.search.clone();
                        self.config.remember_search(&term);
                        self.config_dirty = true;
                    }
                    if !self.search.is_empty() && ui.button("✕").on_hover_text("Clear search").clicked()
                    {
                        self.search.clear();
                    }

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
                    });
                });
            });

        egui::Panel::left("sidebar")
            .exact_size(208.0)
            .resizable(false)
            .frame(crate::ui::sidebar_frame())
            .show(ui, |ui| {
                sidebar::show(self, ui);
            });

        // The details sheet takes precedence over the central content, so it
        // is added to the remaining space before the library grid.
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
                Screen::Library | Screen::Favorites => library::show(self, ui),
                Screen::Settings => settings::show(self, ui),
            });

        self.flush_config();
    }
}

impl IonApp {
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
