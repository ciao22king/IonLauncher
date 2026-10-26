//! Game details sheet.
//!
//! Rendered inside the right-hand panel opened by the app shell. It is
//! deliberately information-dense: everything IonLauncher knows about a
//! title, plus the two actions that matter.

use crate::app::IonApp;
use crate::theme::{space, text, Palette, RADIUS_LG};
use crate::widgets;
use egui::{CornerRadius, RichText, Vec2};

pub fn show(app: &mut IonApp, ui: &mut egui::Ui) {
    let Some(id) = app.details_for.clone() else {
        return;
    };
    let Some(game) = app.game(&id).cloned() else {
        // The game vanished (a rescan removed it); close the panel.
        app.details_for = None;
        return;
    };

    // Request artwork if the card was never drawn (e.g. opened from a list).
    if game.art.is_none() && !game.art_pending && app.art.fetch_enabled() {
        if let Some(g) = app.game_mut(&id) {
            g.art_pending = true;
        }
        app.art.request(&id, &game.art_sources);
    }

    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Details")
                .size(text::MD)
                .strong()
                .color(Palette::TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("✕").on_hover_text("Close").clicked() {
                close = true;
            }
        });
    });
    ui.add_space(space::MD);

    egui::ScrollArea::vertical()
        .id_salt("details-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Cover.
            let width = ui.available_width();
            let cover_height = (width * 4.0 / 3.0).min(360.0);
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(width, cover_height), egui::Sense::hover());
            match &game.art {
                Some(texture) => {
                    widgets::paint_cover(
                        ui,
                        rect,
                        texture,
                        RADIUS_LG,
                        Palette::SURFACE,
                        widgets::Corners::ALL,
                    );
                }
                None => {
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(RADIUS_LG),
                        Palette::SURFACE_HOVER,
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        game.store.name(),
                        egui::FontId::proportional(text::BASE),
                        game.store.accent(),
                    );
                }
            }

            ui.add_space(space::LG);
            ui.label(
                RichText::new(&game.title)
                    .size(text::LG)
                    .strong()
                    .color(Palette::TEXT),
            );
            ui.add_space(space::SM);
            ui.horizontal(|ui| {
                widgets::status_pill(ui, game.store.name(), game.store.accent());
                if app.config.is_favorite(&id) {
                    widgets::status_pill(ui, "Favourite", Palette::WARNING);
                }
                if !game.install_dir.as_os_str().is_empty() && !game.install_dir.exists() {
                    widgets::status_pill(ui, "Install missing", Palette::DANGER);
                }
            });

            ui.add_space(space::LG);

            // Actions.
            let width = ui.available_width();
            if widgets::primary_button(ui, "Launch", width).clicked() {
                app.launch(&id);
            }
            ui.add_space(space::SM);
            ui.horizontal(|ui| {
                let fav_label = if app.config.is_favorite(&id) {
                    "★  Favourite"
                } else {
                    "☆  Favourite"
                };
                if widgets::secondary_button(ui, fav_label).clicked() {
                    app.toggle_favorite(&id);
                }
                if widgets::secondary_button(ui, "Copy id").clicked() {
                    ui.ctx().copy_text(game.id.clone());
                    app.notify("Game id copied");
                }
            });

            // A title that is owned but not installed is invisible to the
            // scanner, so reaching its storefront from the library is the way
            // back to it — and to buying it, when it is not owned yet.
            ui.add_space(space::SM);
            if widgets::secondary_button(ui, "Open store page").clicked() {
                match crate::stores::open_store_page(&game) {
                    Ok(()) => app.notify("Opening the store page in your browser"),
                    Err(e) => app.notify_error(&format!("Could not open the browser: {e}")),
                }
            }

            // Downloading without the store client is delegated to the
            // community downloader for that store. A downloader IonLauncher
            // fetched itself counts as installed, so `executable()` — not
            // `find()` — decides, matching what `install()` actually runs.
            let installer = crate::stores::installer(game.store);
            let ready = installer.executable().is_some();
            let label = if ready {
                format!("Install with {}", installer.program)
            } else {
                "Install without the client".to_string()
            };
            ui.add_space(space::SM);
            if widgets::secondary_button(ui, &label).clicked() {
                if ready {
                    match installer.install(&game) {
                        Ok(log) => app.notify(&format!(
                            "Installing in the background · log: {}",
                            log.display()
                        )),
                        Err(e) => app.notify_error(&format!("Install failed: {e}")),
                    }
                } else if installer.downloadable() {
                    app.download_downloader(game.store);
                } else {
                    app.notify_error(&format!(
                        "{} is not installed — see {}",
                        installer.program, installer.homepage
                    ));
                }
            }

            // When the downloader is missing, showing the exact command is
            // more useful than a dead end: it can be run once it is installed.
            let installer = crate::stores::installer(game.store);
            if installer.executable().is_none() {
                ui.add_space(space::XS);
                ui.label(
                    RichText::new(installer.install_command(&game))
                        .size(text::XS)
                        .monospace()
                        .color(Palette::TEXT_FAINT),
                );
            }

            ui.add_space(space::XL);
            widgets::section_heading(ui, "Statistics");
            ui.add_space(space::SM);
            egui::Grid::new("stats-grid")
                .num_columns(2)
                .spacing(Vec2::new(space::LG, space::MD))
                .show(ui, |ui| {
                    if let Some(playtime) = game.playtime_label() {
                        widgets::stat(ui, "PLAYTIME", &playtime);
                    }
                    if let Some(size) = game.size_label() {
                        widgets::stat(ui, "SIZE ON DISK", &size);
                    }
                    ui.end_row();
                    if let Some(last) = game.last_played {
                        let age = crate::util::relative_age(last);
                        if !age.is_empty() {
                            widgets::stat(ui, "LAST PLAYED", &age);
                        }
                    }
                    widgets::stat(ui, "STORE", game.store.name());
                    ui.end_row();
                });

            ui.add_space(space::XL);
            widgets::section_heading(ui, "Installation");
            ui.add_space(space::SM);

            let install_text = if game.install_dir.as_os_str().is_empty() {
                "not reported by the store".to_string()
            } else {
                game.install_dir.display().to_string()
            };
            ui.label(
                RichText::new(install_text)
                    .size(text::XS)
                    .monospace()
                    .color(Palette::TEXT_MUTED),
            );
            if !game.install_dir.as_os_str().is_empty() && !game.install_dir.exists() {
                ui.add_space(space::XS);
                ui.label(
                    RichText::new("⚠  This folder no longer exists on disk")
                        .size(text::XS)
                        .color(Palette::DANGER),
                );
            }

            ui.add_space(space::XL);
            widgets::section_heading(ui, "How it launches");
            ui.add_space(space::SM);
            ui.label(
                RichText::new(widgets::launch_target_label(&game.launch))
                    .size(text::XS)
                    .monospace()
                    .color(Palette::TEXT_FAINT),
            );
            ui.add_space(space::XS);
            ui.label(
                RichText::new(match &game.launch {
                    crate::model::LaunchTarget::Url(_) => {
                        "Started through the store client, so cloud saves, achievements and the \
                         overlay keep working."
                    }
                    crate::model::LaunchTarget::Direct { .. } => {
                        "Started directly from its executable, bypassing the store client."
                    }
                })
                .size(text::XS)
                .color(Palette::TEXT_FAINT),
            );

            ui.add_space(space::XL);
        });

    if close || ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        app.details_for = None;
    }
}
