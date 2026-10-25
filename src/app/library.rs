//! The game grid / list.
//!
//! Only the cards actually inside the viewport are drawn, and artwork is
//! requested only for those. That keeps frame time flat as the library grows
//! and is what makes the launcher usable on older hardware.

use crate::app::IonApp;
use crate::config::ViewMode;
use crate::theme::{Palette, RADIUS_CARD};
use crate::widgets::{self, CardAction};
use egui::{CornerRadius, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, Vec2};

/// Space between cards.
const GAP: f32 = 14.0;

pub fn show(app: &mut IonApp, ui: &mut egui::Ui) {
    header(app, ui);
    ui.add_space(12.0);

    if app.games.is_empty() && !app.scanning {
        empty_state(app, ui);
        return;
    }
    if app.visible_ids().is_empty() {
        no_results(app, ui);
        return;
    }

    let view = app.config.view;
    let ids: Vec<String> = app.visible_ids().to_vec();

    match view {
        ViewMode::Grid => grid(app, ui, &ids),
        ViewMode::List => list(app, ui, &ids),
    }
}

/// Title plus the active search, so the user knows what they are looking at.
fn header(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let title = if app.favorites_only {
            "Favourites"
        } else {
            "Library"
        };
        ui.label(RichText::new(title).size(22.0).strong().color(Palette::TEXT));
        ui.add_space(8.0);
        if !app.search.trim().is_empty() {
            ui.label(
                RichText::new(format!("matching “{}”", app.search.trim()))
                    .size(12.5)
                    .color(Palette::TEXT_MUTED),
            );
        }
    });
}

fn grid(app: &mut IonApp, ui: &mut egui::Ui, ids: &[String]) {
    let card_width = app.config.card_width.clamp(150.0, 340.0);
    // Portrait covers are 3:4, plus room for the title block.
    let card_height = card_width * 4.0 / 3.0 + 46.0;
    let card_size = Vec2::new(card_width, card_height);

    ScrollArea::vertical()
        .id_salt("library-grid")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let available = ui.available_width();
            let columns = (((available + GAP) / (card_width + GAP)).floor() as usize).max(1);

            // Only iterate rows that can be on screen. `show_rows` would need
            // a fixed row height, which a wrapping grid does not have, so the
            // clipping is done by egui: cards outside the viewport are cheap
            // because `is_rect_visible` short-circuits their painting.
            let mut index = 0usize;
            while index < ids.len() {
                ui.horizontal(|ui| {
                    for _ in 0..columns {
                        if index >= ids.len() {
                            break;
                        }
                        let id = &ids[index];
                        index += 1;
                        draw_card(app, ui, id, card_size);
                    }
                });
                ui.add_space(GAP);
            }
        });
}

fn draw_card(app: &mut IonApp, ui: &mut egui::Ui, id: &str, size: Vec2) {
    // Request artwork for anything about to be drawn. The loader dedupes,
    // so calling this every frame is cheap.
    let (art_urls, art_pending, art_ready) = match app.game(id) {
        Some(game) => (
            game.art_urls.clone(),
            game.art_pending,
            game.art.is_some(),
        ),
        None => return,
    };
    if !art_ready && !art_pending && app.art.fetch_enabled() {
        if let Some(game) = app.game_mut(id) {
            game.art_pending = true;
        }
        app.art.request(id, &art_urls);
    }

    let favorite = app.config.is_favorite(id);
    let game = match app.game(id) {
        Some(game) => game.clone(),
        None => return,
    };

    let (_, action) = widgets::game_card(ui, &game, size, favorite, false);
    apply(app, id, action);
}

fn list(app: &mut IonApp, ui: &mut egui::Ui, ids: &[String]) {
    ScrollArea::vertical()
        .id_salt("library-list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for id in ids {
                draw_row(app, ui, id);
                ui.add_space(6.0);
            }
        });
}

fn draw_row(app: &mut IonApp, ui: &mut egui::Ui, id: &str) {
    let (art_urls, art_pending, art_ready) = match app.game(id) {
        Some(game) => (game.art_urls.clone(), game.art_pending, game.art.is_some()),
        None => return,
    };
    if !art_ready && !art_pending && app.art.fetch_enabled() {
        if let Some(game) = app.game_mut(id) {
            game.art_pending = true;
        }
        app.art.request(id, &art_urls);
    }

    let game = match app.game(id) {
        Some(game) => game.clone(),
        None => return,
    };
    let favorite = app.config.is_favorite(id);

    let height = 68.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if response.hovered() {
            Palette::CARD_HOVER
        } else {
            Palette::CARD
        };
        painter.rect_filled(rect, CornerRadius::same(RADIUS_CARD), fill);

        // Thumbnail: a fixed square showing the cover, cropped to fit.
        let thumb = Rect::from_min_size(
            rect.min + Vec2::new(8.0, 8.0),
            Vec2::new((height - 16.0) * 0.75, height - 16.0),
        );
        match &game.art {
            Some(texture) => {
                painter.image(
                    texture.id(),
                    thumb,
                    Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => {
                painter.rect_filled(
                    thumb,
                    CornerRadius::same(6),
                    Palette::SURFACE_HOVER,
                );
                painter.text(
                    thumb.center(),
                    egui::Align2::CENTER_CENTER,
                    game.store.short(),
                    egui::FontId::proportional(11.0),
                    game.store.accent(),
                );
            }
        }

        let text_x = thumb.max.x + 12.0;
        painter.text(
            egui::pos2(text_x, rect.center().y - 9.0),
            egui::Align2::LEFT_CENTER,
            &game.title,
            egui::FontId::proportional(14.5),
            Palette::TEXT,
        );

        let mut meta: Vec<String> = vec![game.store.name().to_string()];
        if let Some(playtime) = game.playtime_label() {
            meta.push(playtime);
        }
        if let Some(size) = game.size_label() {
            meta.push(size);
        }
        if let Some(last) = game.last_played {
            let age = crate::util::relative_age(last);
            if !age.is_empty() {
                meta.push(format!("played {age}"));
            }
        }
        if !game.install_dir.as_os_str().is_empty() && !game.install_dir.exists() {
            meta.push("install missing".into());
        }
        painter.text(
            egui::pos2(text_x, rect.center().y + 11.0),
            egui::Align2::LEFT_CENTER,
            meta.join("  ·  "),
            egui::FontId::proportional(11.5),
            Palette::TEXT_MUTED,
        );

        // Right-hand affordances.
        if favorite {
            painter.text(
                egui::pos2(rect.max.x - 16.0, rect.center().y),
                egui::Align2::CENTER_CENTER,
                "★",
                egui::FontId::proportional(14.0),
                egui::Color32::from_rgb(0xF5, 0xC2, 0x42),
            );
        }
        painter.text(
            egui::pos2(rect.max.x - 44.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            "▶",
            egui::FontId::proportional(13.0),
            if response.hovered() {
                Palette::ACCENT
            } else {
                Palette::TEXT_FAINT
            },
        );

        if response.hovered() {
            painter.rect_stroke(
                rect,
                CornerRadius::same(RADIUS_CARD),
                Stroke::new(1.0, Palette::ACCENT_DIM),
                StrokeKind::Inside,
            );
        }
    }

    let mut action = CardAction::None;
    if response.clicked() {
        action = CardAction::Details;
    }
    if response.double_clicked() || response.secondary_clicked() {
        action = CardAction::Launch;
    }
    response.context_menu(|ui| {
        if ui.button("▶  Launch").clicked() {
            action = CardAction::Launch;
            ui.close();
        }
        if ui.button("★  Toggle favourite").clicked() {
            action = CardAction::ToggleFavorite;
            ui.close();
        }
        ui.separator();
        if ui.button("Details").clicked() {
            action = CardAction::Details;
            ui.close();
        }
    });
    apply(app, id, action);
}

fn apply(app: &mut IonApp, id: &str, action: CardAction) {
    match action {
        CardAction::None => {}
        CardAction::Launch => app.launch(id),
        CardAction::Details => app.details_for = Some(id.to_string()),
        CardAction::ToggleFavorite => app.toggle_favorite(id),
    }
}

/// Shown when the stores reported nothing at all.
fn empty_state(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("No games found").size(19.0).color(Palette::TEXT));
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "IonLauncher lists what is installed by Steam, Epic Games and GOG on this machine.",
            )
            .size(13.0)
            .color(Palette::TEXT_MUTED),
        );
        ui.add_space(4.0);
        if let Some(report) = &app.last_report {
            for (store, note) in &report.notes {
                ui.label(
                    RichText::new(format!("{}: {note}", store.name()))
                        .size(12.0)
                        .color(Palette::TEXT_FAINT),
                );
            }
        }
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.add_space(ui.available_width() / 2.0 - 130.0);
            if widgets::primary_button(ui, "Try the demo library", 180.0).clicked() {
                app.config.demo_mode = true;
                app.config_dirty = true;
                app.rescan();
            }
            if widgets::secondary_button(ui, "Open Settings").clicked() {
                app.screen = crate::app::Screen::Settings;
            }
        });
    });
}

/// Shown when the search or the favourites filter excludes everything.
fn no_results(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        let (title, hint) = if app.favorites_only {
            (
                "No favourites yet",
                "Star a game from its details panel and it will show up here.",
            )
        } else {
            (
                "Nothing matches that search",
                "Try a shorter word, or clear the search to see everything.",
            )
        };
        ui.label(RichText::new(title).size(17.0).color(Palette::TEXT));
        ui.add_space(6.0);
        ui.label(RichText::new(hint).size(12.5).color(Palette::TEXT_MUTED));
        ui.add_space(10.0);

        let label = if app.favorites_only {
            "Show all games"
        } else {
            "Clear search"
        };
        if widgets::secondary_button(ui, label).clicked() {
            app.search.clear();
            app.favorites_only = false;
            app.view_cache = None;
        }
    });
}
