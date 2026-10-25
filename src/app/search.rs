//! The search page.
//!
//! A dedicated destination rather than a box in the corner: it gets a large
//! field, the recent searches, and the full grid of results.

use crate::app::{library, IonApp};
use crate::config::ViewMode;
use crate::theme::Palette;
use crate::widgets;
use egui::{Key, RichText, Vec2};

pub fn show(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Search").size(22.0).strong().color(Palette::TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let count = app.visible_ids().len();
            let text = if app.search.trim().is_empty() {
                format!("{} games installed", app.games.len())
            } else if count == 1 {
                "1 match".to_string()
            } else {
                format!("{count} matches")
            };
            ui.label(RichText::new(text).size(12.0).color(Palette::TEXT_FAINT));
        });
    });
    ui.add_space(10.0);

    search_field(app, ui);
    ui.add_space(10.0);

    if app.search.trim().is_empty() {
        recent_searches(app, ui);
        return;
    }

    let view = app.config.view;
    let ids: Vec<String> = app.visible_ids().to_vec();
    if ids.is_empty() {
        no_matches(app, ui);
        return;
    }
    match view {
        ViewMode::Grid => library::grid(app, ui, &ids, "search-grid"),
        ViewMode::List => library::list(app, ui, &ids, "search-list"),
    }
}

fn search_field(app: &mut IonApp, ui: &mut egui::Ui) {
    let response = ui.add_sized(
        Vec2::new(ui.available_width().min(720.0), 34.0),
        egui::TextEdit::singleline(&mut app.search)
            .hint_text("Search your games…")
            .desired_width(f32::INFINITY),
    );
    // Record a term once the user has finished typing it, not on every key.
    if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        let term = app.search.clone();
        app.config.remember_search(&term);
        app.config_dirty = true;
    }

    if !app.search.is_empty() {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if widgets::secondary_button(ui, "✕  Clear").clicked() {
                app.search.clear();
                app.view_cache = None;
            }
            ui.label(
                RichText::new("Showing results from all stores")
                    .size(11.5)
                    .color(Palette::TEXT_FAINT),
            );
        });
    }
}

fn recent_searches(app: &mut IonApp, ui: &mut egui::Ui) {
    if app.config.recent_searches.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Type to search all three stores at once")
                    .size(14.0)
                    .color(Palette::TEXT_MUTED),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Matching is on the game title. Press Enter to keep a term for later.",
                )
                .size(12.0)
                .color(Palette::TEXT_FAINT),
            );
        });
        return;
    }

    ui.label(RichText::new("RECENT").size(10.5).color(Palette::TEXT_FAINT));
    ui.add_space(6.0);

    let terms = app.config.recent_searches.clone();
    let mut chosen: Option<String> = None;
    ui.horizontal_wrapped(|ui| {
        for term in &terms {
            if widgets::chip(ui, term, false, None) {
                chosen = Some(term.clone());
            }
        }
    });

    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if widgets::secondary_button(ui, "Clear history").clicked() {
            app.config.recent_searches.clear();
            app.config_dirty = true;
        }
    });

    if let Some(term) = chosen {
        app.search = term;
        app.view_cache = None;
    }
}

fn no_matches(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.add_space(50.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(format!("No game matches “{}”", app.search.trim()))
                .size(16.0)
                .color(Palette::TEXT),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new("Only installed games are searchable.")
                .size(12.0)
                .color(Palette::TEXT_MUTED),
        );
        ui.add_space(12.0);
        if widgets::secondary_button(ui, "Clear search").clicked() {
            app.search.clear();
            app.view_cache = None;
        }
    });
}
