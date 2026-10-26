//! The game grid / list.
//!
//! Only the cards actually inside the viewport are drawn, and artwork is
//! requested only for those. That keeps frame time flat as the library grows
//! and is what makes the launcher usable on older hardware.

use crate::app::IonApp;
use crate::config::ViewMode;
use crate::theme::{space, text, Palette, RADIUS_LG};
use crate::widgets::{self, elided_job, CardAction};
use egui::{CornerRadius, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, Vec2};

/// Space between cards.
const GAP: f32 = 16.0;

pub fn show(app: &mut IonApp, ui: &mut egui::Ui) {
    header(app, ui);
    ui.add_space(space::MD);

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
        ViewMode::Grid => grid(app, ui, &ids, "library-grid"),
        ViewMode::List => list(app, ui, &ids, "library-list"),
    }
}

/// Title plus the current filter, so the user knows what they are looking at.
fn header(app: &mut IonApp, ui: &mut egui::Ui) {
    let title = if app.favorites_only {
        "Favourites"
    } else {
        "Installed games"
    };
    ui.label(
        RichText::new(title)
            .size(text::XL)
            .strong()
            .color(Palette::TEXT),
    );
    ui.add_space(space::XS);

    ui.horizontal(|ui| {
        // A search set on the Search page still filters here, so say so and
        // give a way out without leaving the page.
        if !app.search.trim().is_empty() {
            ui.label(
                RichText::new(format!("Matching “{}”", app.search.trim()))
                    .size(text::SM)
                    .color(Palette::TEXT_MUTED),
            );
            if ui
                .small_button("Clear")
                .on_hover_text("Clear search")
                .clicked()
            {
                app.search.clear();
                app.view_cache = None;
            }
        } else {
            let visible = app.visible_ids().len();
            let total = app.games.len();
            let label = if app.favorites_only {
                count_label(visible, "favourite", "favourites")
            } else if visible == total {
                format!("{} from all stores", count_label(total, "game", "games"))
            } else {
                format!("{visible} of {total} games")
            };
            ui.label(
                RichText::new(label)
                    .size(text::SM)
                    .color(Palette::TEXT_FAINT),
            );
        }
    });
}

/// `1 game` / `3 games`, so counts never read as "1 games".
pub fn count_label(n: usize, singular: &str, plural: &str) -> String {
    if n == 1 {
        format!("1 {singular}")
    } else {
        format!("{n} {plural}")
    }
}

/// Shared by the library and the search page so both render identically.
pub fn grid(app: &mut IonApp, ui: &mut egui::Ui, ids: &[String], salt: &str) {
    let card_width = app.config.card_width.clamp(150.0, 340.0);
    // Portrait covers are 3:4, with the title overlaid on a scrim at the base,
    // so the card is a touch taller than the cover alone.
    let card_height = card_width * 4.0 / 3.0;
    let card_size = Vec2::new(card_width, card_height);

    ScrollArea::vertical()
        .id_salt(salt)
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
    let (art_sources, art_pending, art_ready) = match app.game(id) {
        Some(game) => (
            game.art_sources.clone(),
            game.art_pending,
            game.art.is_some(),
        ),
        None => return,
    };
    if !art_ready && !art_pending && app.art.fetch_enabled() {
        if let Some(game) = app.game_mut(id) {
            game.art_pending = true;
        }
        app.art.request(id, &art_sources);
    }

    let favorite = app.config.is_favorite(id);
    let game = match app.game(id) {
        Some(game) => game.clone(),
        None => return,
    };

    let (_, action) = widgets::game_card(ui, &game, size, favorite, false);
    apply(app, id, action);
}

pub fn list(app: &mut IonApp, ui: &mut egui::Ui, ids: &[String], salt: &str) {
    ScrollArea::vertical()
        .id_salt(salt)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for id in ids {
                draw_row(app, ui, id);
                ui.add_space(space::SM);
            }
        });
}

fn draw_row(app: &mut IonApp, ui: &mut egui::Ui, id: &str) {
    let (art_sources, art_pending, art_ready) = match app.game(id) {
        Some(game) => (
            game.art_sources.clone(),
            game.art_pending,
            game.art.is_some(),
        ),
        None => return,
    };
    if !art_ready && !art_pending && app.art.fetch_enabled() {
        if let Some(game) = app.game_mut(id) {
            game.art_pending = true;
        }
        app.art.request(id, &art_sources);
    }

    let game = match app.game(id) {
        Some(game) => game.clone(),
        None => return,
    };
    let favorite = app.config.is_favorite(id);

    let height = 72.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let hovered = response.hovered();
        let fill = if hovered {
            Palette::CARD_HOVER
        } else {
            Palette::CARD
        };
        painter.rect_filled(rect, CornerRadius::same(RADIUS_LG), fill);

        // Thumbnail: a fixed portrait crop of the cover.
        let thumb = Rect::from_min_size(
            rect.min + Vec2::new(space::MD, space::MD),
            Vec2::new((height - space::MD * 2.0) * 0.72, height - space::MD * 2.0),
        );
        match &game.art {
            Some(texture) => widgets::paint_cover(
                ui,
                thumb,
                texture,
                crate::theme::RADIUS_SM,
                fill,
                widgets::Corners::ALL,
            ),
            None => {
                painter.rect_filled(
                    thumb,
                    CornerRadius::same(crate::theme::RADIUS_SM),
                    Palette::SURFACE_HOVER,
                );
                painter.text(
                    thumb.center(),
                    egui::Align2::CENTER_CENTER,
                    game.store.short(),
                    egui::FontId::proportional(text::XS),
                    game.store.accent(),
                );
            }
        }

        let text_x = thumb.max.x + space::MD;
        let text_width = (rect.max.x - 52.0 - text_x).max(20.0);
        let title_galley = painter.layout_job(elided_job(
            &game.title,
            egui::FontId::proportional(text::BASE),
            Palette::TEXT,
            text_width,
            1,
        ));
        painter.galley(
            egui::pos2(text_x, rect.center().y - 10.0 - title_galley.size().y / 2.0),
            title_galley,
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
        let meta_galley = painter.layout_job(elided_job(
            &meta.join("  ·  "),
            egui::FontId::proportional(text::XS),
            Palette::TEXT_MUTED,
            text_width,
            1,
        ));
        painter.galley(
            egui::pos2(text_x, rect.center().y + 10.0 - meta_galley.size().y / 2.0),
            meta_galley,
            Palette::TEXT_MUTED,
        );

        // Right-hand affordances: a favourite star, then a launch button that
        // lights up on hover.
        if favorite {
            painter.text(
                egui::pos2(rect.max.x - 18.0, rect.center().y),
                egui::Align2::CENTER_CENTER,
                "★",
                egui::FontId::proportional(14.0),
                Palette::WARNING,
            );
        }
        let play_center = egui::pos2(rect.max.x - 48.0, rect.center().y);
        let play_color = if hovered {
            Palette::ACCENT
        } else {
            Palette::TEXT_FAINT
        };
        painter.circle_stroke(play_center, 14.0, Stroke::new(1.5, play_color));
        let s = 5.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(play_center.x - s * 0.5 + 1.0, play_center.y - s),
                egui::pos2(play_center.x - s * 0.5 + 1.0, play_center.y + s),
                egui::pos2(play_center.x + s * 0.8 + 1.0, play_center.y),
            ],
            play_color,
            Stroke::NONE,
        ));

        if hovered {
            painter.rect_stroke(
                rect,
                CornerRadius::same(RADIUS_LG),
                Stroke::new(1.0, Palette::BORDER_STRONG),
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
        if ui.button("Launch").clicked() {
            action = CardAction::Launch;
            ui.close();
        }
        if ui.button("Toggle favourite").clicked() {
            action = CardAction::ToggleFavorite;
            ui.close();
        }
        ui.separator();
        if ui.button("Open store page").clicked() {
            action = CardAction::StorePage;
            ui.close();
        }
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
        CardAction::StorePage => app.open_store_page(id),
    }
}

/// Shown when the stores reported nothing at all.
fn empty_state(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.add_space(space::XXL * 2.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(48.0, 48.0), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 24.0, Palette::SURFACE_HOVER);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "▤",
            egui::FontId::proportional(20.0),
            Palette::TEXT_FAINT,
        );
        ui.add_space(space::MD);
        ui.label(
            RichText::new("No games found")
                .size(text::MD)
                .strong()
                .color(Palette::TEXT),
        );
        ui.add_space(space::XS);
        ui.label(
            RichText::new(
                "IonLauncher lists what is installed by Steam, Epic Games and GOG on this machine.",
            )
            .size(text::SM)
            .color(Palette::TEXT_MUTED),
        );
        if let Some(report) = &app.last_report {
            ui.add_space(space::XS);
            for (store, note) in &report.notes {
                ui.label(
                    RichText::new(format!("{}: {note}", store.name()))
                        .size(text::XS)
                        .color(Palette::TEXT_FAINT),
                );
            }
        }
        ui.add_space(space::LG);
        ui.horizontal(|ui| {
            ui.add_space(ui.available_width() / 2.0 - 150.0);
            if widgets::primary_button(ui, "Try the demo library", 180.0).clicked() {
                app.config.demo_mode = true;
                app.config_dirty = true;
                app.rescan();
            }
            ui.add_space(space::SM);
            if widgets::secondary_button(ui, "Open Settings").clicked() {
                app.screen = crate::app::Screen::Settings;
            }
        });
    });
}

/// Shown when the search or the favourites filter excludes everything.
fn no_results(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.add_space(space::XXL * 2.0);
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
        ui.label(
            RichText::new(title)
                .size(text::MD)
                .strong()
                .color(Palette::TEXT),
        );
        ui.add_space(space::XS);
        ui.label(
            RichText::new(hint)
                .size(text::SM)
                .color(Palette::TEXT_MUTED),
        );
        ui.add_space(space::LG);

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

#[cfg(test)]
mod tests {
    use super::count_label;

    #[test]
    fn counts_use_singular_for_one() {
        assert_eq!(count_label(1, "game", "games"), "1 game");
        assert_eq!(count_label(0, "game", "games"), "0 games");
        assert_eq!(count_label(7, "game", "games"), "7 games");
        assert_eq!(count_label(1, "favourite", "favourites"), "1 favourite");
    }
}
