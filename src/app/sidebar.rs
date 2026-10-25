//! The left menu.
//!
//! Pages, not stores: the library is one mixed collection, so the menu
//! switches what you are doing rather than splitting Steam from GOG.

use crate::app::{IonApp, Screen};
use crate::theme::{Palette, RADIUS_CONTROL};
use egui::{CornerRadius, RichText, Sense, Vec2};

pub fn show(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.add_space(2.0);

    let installed = app.games.len();
    let favorites = app.config.favorites.len();

    section(ui, "LIBRARY");
    ui.add_space(2.0);
    let on_library = app.screen == Screen::Library;
    if nav_item(
        ui,
        "▤",
        "Installed games",
        Some(installed),
        on_library && !app.favorites_only,
    ) {
        app.screen = Screen::Library;
        app.favorites_only = false;
        app.view_cache = None;
    }
    if nav_item(ui, "⌕", "Search", None, app.screen == Screen::Search) {
        app.screen = Screen::Search;
    }
    if nav_item(
        ui,
        "★",
        "Favourites",
        Some(favorites),
        on_library && app.favorites_only,
    ) {
        app.screen = Screen::Library;
        app.favorites_only = true;
        app.view_cache = None;
    }

    ui.add_space(14.0);
    section(ui, "APP");
    ui.add_space(2.0);
    if nav_item(ui, "⚙", "Settings", None, app.screen == Screen::Settings) {
        app.screen = Screen::Settings;
    }

    // A quiet footer: which stores fed the library and how much each gave.
    // Informational only — there is nothing here to filter by.
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.add_space(4.0);
        for store in crate::model::Store::ALL.iter().rev() {
            let count = app.games.iter().filter(|g| g.store == *store).count();
            let enabled = app.config.enabled(*store);
            ui.horizontal(|ui| {
                ui.painter().circle_filled(
                    ui.cursor().min + Vec2::new(5.0, 8.0),
                    4.0,
                    if enabled {
                        store.accent()
                    } else {
                        Palette::TEXT_FAINT
                    },
                );
                ui.add_space(14.0);
                let text = if enabled {
                    format!("{}  {count}", store.short())
                } else {
                    format!("{}  off", store.short())
                };
                ui.label(RichText::new(text).size(11.5).color(Palette::TEXT_FAINT));
            });
            ui.add_space(3.0);
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new("One library, three stores")
                .size(10.5)
                .color(Palette::TEXT_FAINT),
        );
    });
}

fn section(ui: &mut egui::Ui, label: &str) {
    ui.label(RichText::new(label).size(10.5).color(Palette::TEXT_FAINT));
}

/// A full-width menu row. Returns true when clicked.
fn nav_item(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    count: Option<usize>,
    selected: bool,
) -> bool {
    let height = 34.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let fill = if selected {
            Palette::ACCENT_DIM
        } else if hovered {
            Palette::SURFACE_HOVER
        } else {
            egui::Color32::TRANSPARENT
        };
        if fill != egui::Color32::TRANSPARENT {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(RADIUS_CONTROL), fill);
        }

        let text_color = if selected {
            Palette::TEXT
        } else {
            Palette::TEXT_MUTED
        };
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(10.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.label(RichText::new(icon).size(13.0).color(text_color));
        child.add_space(8.0);
        child.label(RichText::new(label).size(13.0).color(text_color));

        if let Some(count) = count {
            child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(count.to_string())
                        .size(11.5)
                        .color(Palette::TEXT_FAINT),
                );
            });
        }
    }

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}
