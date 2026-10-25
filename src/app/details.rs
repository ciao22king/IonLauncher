//! Game details sheet.
//!
//! Rendered inside the right-hand panel opened by the app shell. It is
//! deliberately information-dense: everything IonLauncher knows about a
//! title, plus the two actions that matter.

use crate::app::IonApp;
use crate::theme::{Palette, RADIUS_CARD};
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
                .size(15.0)
                .strong()
                .color(Palette::TEXT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("✕").on_hover_text("Close").clicked() {
                close = true;
            }
        });
    });
    ui.add_space(10.0);

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
                    let size = texture.size_vec2();
                    let aspect = if size.y > 0.0 { size.x / size.y } else { 0.75 };
                    widgets::paint_fit(ui, rect, texture.id(), aspect);
                }
                None => {
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(RADIUS_CARD),
                        Palette::SURFACE_HOVER,
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        game.store.name(),
                        egui::FontId::proportional(14.0),
                        game.store.accent(),
                    );
                }
            }

            ui.add_space(14.0);
            ui.label(
                RichText::new(&game.title)
                    .size(19.0)
                    .strong()
                    .color(Palette::TEXT),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(Vec2::new(10.0, 10.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(dot.center(), 4.0, game.store.accent());
                ui.label(
                    RichText::new(game.store.name())
                        .size(12.5)
                        .color(Palette::TEXT_MUTED),
                );
            });

            ui.add_space(14.0);

            // Actions.
            let width = ui.available_width();
            if widgets::primary_button(ui, "▶   Launch", width).clicked() {
                app.launch(&id);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let fav_label = if app.config.is_favorite(&id) {
                    "★  Favourite"
                } else {
                    "☆  Favourite"
                };
                if widgets::secondary_button(ui, fav_label).clicked() {
                    app.toggle_favorite(&id);
                }
                if widgets::secondary_button(ui, "⧉  Copy id").clicked() {
                    ui.ctx().copy_text(game.id.clone());
                    app.notify("Game id copied");
                }
            });

            ui.add_space(16.0);
            widgets::section_heading(ui, "Statistics");
            ui.add_space(6.0);
            egui::Grid::new("stats-grid")
                .num_columns(2)
                .spacing(Vec2::new(18.0, 10.0))
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

            ui.add_space(16.0);
            widgets::section_heading(ui, "Installation");
            ui.add_space(6.0);

            let install_text = if game.install_dir.as_os_str().is_empty() {
                "not reported by the store".to_string()
            } else {
                game.install_dir.display().to_string()
            };
            ui.label(
                RichText::new(install_text)
                    .size(11.5)
                    .color(Palette::TEXT_MUTED),
            );
            if !game.install_dir.as_os_str().is_empty() && !game.install_dir.exists() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("⚠  This folder no longer exists on disk")
                        .size(11.5)
                        .color(Palette::DANGER),
                );
            }

            ui.add_space(16.0);
            widgets::section_heading(ui, "How it launches");
            ui.add_space(6.0);
            ui.label(
                RichText::new(widgets::launch_target_label(&game.launch))
                    .size(11.0)
                    .color(Palette::TEXT_FAINT),
            );
            ui.add_space(4.0);
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
                .size(11.0)
                .color(Palette::TEXT_FAINT),
            );

            ui.add_space(20.0);
        });

    if close || ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        app.details_for = None;
    }
}
