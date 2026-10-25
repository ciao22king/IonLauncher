//! Settings screen: store toggles, library behaviour and manual paths.

use crate::app::IonApp;
use crate::model::Store;
use crate::providers;
use crate::theme::Palette;
use crate::widgets;
use egui::{RichText, ScrollArea, Vec2};
use std::path::PathBuf;

pub fn show(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.label(RichText::new("Settings").size(22.0).strong().color(Palette::TEXT));
    ui.add_space(4.0);
    ui.label(
        RichText::new("IonLauncher reads each store's own files. Nothing is uploaded anywhere.")
            .size(12.5)
            .color(Palette::TEXT_MUTED),
    );
    ui.add_space(14.0);

    ScrollArea::vertical()
        .id_salt("settings-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            stores_section(app, ui);
            ui.add_space(14.0);
            library_section(app, ui);
            ui.add_space(14.0);
            paths_section(app, ui);
            ui.add_space(14.0);
            about_section(app, ui);
            ui.add_space(20.0);
        });
}

fn stores_section(app: &mut IonApp, ui: &mut egui::Ui) {
    crate::ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_heading(ui, "Stores");
        ui.add_space(8.0);

        for store in Store::ALL {
            let mut enabled = app.config.enabled(store);
            let count = app.games.iter().filter(|g| g.store == store).count();
            let note = app
                .last_report
                .as_ref()
                .and_then(|r| r.note(store))
                .unwrap_or("not scanned yet")
                .to_string();

            ui.horizontal(|ui| {
                ui.painter().circle_filled(
                    ui.cursor().min + Vec2::new(6.0, 9.0),
                    5.0,
                    if enabled {
                        store.accent()
                    } else {
                        Palette::TEXT_FAINT
                    },
                );
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(store.name()).size(14.0).color(Palette::TEXT));
                    let detail = if enabled {
                        format!("{count} games · {note}")
                    } else {
                        "disabled".to_string()
                    };
                    ui.label(
                        RichText::new(detail)
                            .size(11.5)
                            .color(Palette::TEXT_FAINT),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.checkbox(&mut enabled, "").changed() {
                        app.config.set_enabled(store, enabled);
                        app.config_dirty = true;
                        // Keep the filter chips consistent with the toggles.
                        if enabled {
                            if !app.store_filter.contains(&store) {
                                app.store_filter.push(store);
                            }
                        } else {
                            app.store_filter.retain(|s| *s != store);
                        }
                        app.view_cache = None;
                        app.rescan();
                    }
                });
            });
            ui.add_space(6.0);
        }
    });
}

fn library_section(app: &mut IonApp, ui: &mut egui::Ui) {
    crate::ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_heading(ui, "Library");
        ui.add_space(8.0);

        let mut show_missing = app.config.show_missing;
        if ui
            .checkbox(&mut show_missing, "Show games whose install folder is missing")
            .changed()
        {
            app.config.show_missing = show_missing;
            app.config_dirty = true;
            app.view_cache = None;
        }

        ui.add_space(6.0);
        let mut fetch_art = app.config.fetch_artwork;
        if ui
            .checkbox(
                &mut fetch_art,
                "Download cover artwork (needs a network connection)",
            )
            .changed()
        {
            app.config.fetch_artwork = fetch_art;
            app.art.set_fetch_enabled(fetch_art);
            app.config_dirty = true;
            if !fetch_art {
                app.art.clear_disk_cache();
            }
        }
        ui.label(
            RichText::new("Artwork is cached on disk, so covers appear instantly next time.")
                .size(11.0)
                .color(Palette::TEXT_FAINT),
        );

        ui.add_space(10.0);
        let mut card_width = app.config.card_width;
        ui.label(
            RichText::new("Card size")
                .size(12.0)
                .color(Palette::TEXT_MUTED),
        );
        if ui
            .add(egui::Slider::new(&mut card_width, 160.0..=320.0).suffix(" pt"))
            .changed()
        {
            app.config.card_width = card_width;
            app.config_dirty = true;
        }

        ui.add_space(6.0);
        let mut demo = app.config.demo_mode;
        if ui
            .checkbox(&mut demo, "Demo library (sample games, no scanning)")
            .changed()
        {
            app.config.demo_mode = demo;
            app.config_dirty = true;
            app.rescan();
        }

        if !app.config.favorites.is_empty() {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} favourites saved", app.config.favorites.len()))
                        .size(11.5)
                        .color(Palette::TEXT_FAINT),
                );
                if widgets::secondary_button(ui, "Clear").clicked() {
                    app.config.favorites.clear();
                    app.config_dirty = true;
                    app.view_cache = None;
                }
            });
        }
    });
}

fn paths_section(app: &mut IonApp, ui: &mut egui::Ui) {
    crate::ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_heading(ui, "Manual locations");
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Only needed when a store lives somewhere unusual. Leave blank for automatic detection.",
            )
            .size(11.5)
            .color(Palette::TEXT_MUTED),
        );
        ui.add_space(10.0);

        for store in Store::ALL {
            let mut text = app
                .config
                .override_root(store)
                .map(|p| p.display().to_string())
                .unwrap_or_default();

            ui.label(
                RichText::new(store.name())
                    .size(12.0)
                    .color(Palette::TEXT_MUTED),
            );
            ui.horizontal(|ui| {
                let response = ui.add_sized(
                    Vec2::new((ui.available_width() - 120.0).max(120.0), 26.0),
                    egui::TextEdit::singleline(&mut text)
                        .hint_text("automatic")
                        .desired_width(f32::INFINITY),
                );
                if response.changed() {
                    let trimmed = text.trim();
                    let value = if trimmed.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(trimmed))
                    };
                    app.config.set_override_root(store, value);
                    app.config_dirty = true;
                }
                if widgets::secondary_button(ui, "Detect").clicked() {
                    match providers::suggested_root(store) {
                        Some(path) => {
                            app.config.set_override_root(store, Some(path));
                            app.config_dirty = true;
                            app.rescan();
                        }
                        None => app.notify_error(&format!("{} not found on this machine", store.name())),
                    }
                }
            });
            ui.add_space(6.0);
        }

        ui.add_space(4.0);
        if widgets::secondary_button(ui, "Rescan now").clicked() {
            app.rescan();
        }
    });
}

fn about_section(app: &mut IonApp, ui: &mut egui::Ui) {
    crate::ui::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        widgets::section_heading(ui, "About");
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("IonLauncher {}", env!("CARGO_PKG_VERSION")))
                .size(13.0)
                .color(Palette::TEXT),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "A single, fast launcher for Steam, Epic Games and GOG. Written in Rust with an \
                 immediate-mode GPU interface, so it starts instantly and stays responsive on \
                 older machines.",
            )
            .size(11.5)
            .color(Palette::TEXT_MUTED),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new("GPL-2.0-or-later")
                .size(11.0)
                .color(Palette::TEXT_FAINT),
        );
        ui.add_space(6.0);
        if let Some(dir) = crate::art::art_cache_dir() {
            ui.label(
                RichText::new(format!("Artwork cache: {}", dir.display()))
                    .size(10.5)
                    .color(Palette::TEXT_FAINT),
            );
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if widgets::secondary_button(ui, "Clear artwork cache").clicked() {
                app.art.clear_disk_cache();
                app.notify("Artwork cache cleared");
            }
            if widgets::secondary_button(ui, "Open config folder").clicked() {
                match crate::config::Config::path() {
                    Some(path) => {
                        if let Some(parent) = path.parent() {
                            let _ = crate::util::open_url(&parent.display().to_string());
                        }
                    }
                    None => app.notify_error("Config path unavailable"),
                }
            }
        });
    });
}
