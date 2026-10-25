//! Left navigation rail: screens, store filters and view controls.

use crate::app::{IonApp, Screen};
use crate::config::ViewMode;
use crate::model::Store;
use crate::theme::{Palette, RADIUS_CARD};
use crate::widgets;
use egui::{CornerRadius, RichText, Sense, Vec2};

pub fn show(app: &mut IonApp, ui: &mut egui::Ui) {
    let favorites = app.config.favorites.len();
    for screen in [Screen::Library, Screen::Favorites, Screen::Settings] {
        let label = match screen {
            Screen::Favorites if favorites > 0 => format!("★  Favourites   {favorites}"),
            _ => match screen {
                Screen::Library => "▤  Library".to_string(),
                Screen::Favorites => "★  Favourites".to_string(),
                Screen::Settings => "⚙  Settings".to_string(),
            },
        };
        if nav_item(ui, &label, app.screen == screen) {
            app.screen = screen;
            app.view_cache = None;
        }
    }

    ui.add_space(18.0);
    ui.label(
        RichText::new("STORES")
            .size(10.5)
            .color(Palette::TEXT_FAINT),
    );
    ui.add_space(6.0);

    for store in Store::ALL {
        let enabled = app.config.enabled(store);
        let count = app.games.iter().filter(|g| g.store == store).count();
        let selected = enabled && app.store_filter.contains(&store);

        let label = if enabled {
            format!("{}   {count}", store.short())
        } else {
            format!("{}   off", store.short())
        };
        let response = store_row(ui, &label, store, selected, enabled);
        if response.clicked() {
            // Clicking toggles the filter; the store stays enabled so a
            // second click brings it back without a trip to Settings.
            if app.store_filter.contains(&store) {
                app.store_filter.retain(|s| *s != store);
            } else {
                app.store_filter.push(store);
            }
            app.view_cache = None;
        }
        response.on_hover_text(if enabled {
            "Show or hide this store in the grid"
        } else {
            "This store is disabled in Settings"
        });
    }

    ui.add_space(18.0);
    ui.label(
        RichText::new("VIEW")
            .size(10.5)
            .color(Palette::TEXT_FAINT),
    );
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        for mode in [ViewMode::Grid, ViewMode::List] {
            let selected = app.config.view == mode;
            if widgets::chip(ui, mode.label(), selected, None) {
                app.config.view = mode;
                app.config_dirty = true;
            }
        }
    });

    ui.add_space(12.0);
    ui.label(RichText::new("SORT BY").size(10.5).color(Palette::TEXT_FAINT));
    ui.add_space(4.0);
    egui::ComboBox::from_id_salt("sort-combo")
        .selected_text(RichText::new(app.config.sort.label()).size(13.0))
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for mode in crate::config::SortMode::ALL {
                if ui
                    .selectable_label(app.config.sort == mode, mode.label())
                    .clicked()
                {
                    app.config.sort = mode;
                    app.config_dirty = true;
                    app.view_cache = None;
                }
            }
        });

    // Footer: total count and scan timing, kept out of the way.
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.add_space(4.0);
        let visible = app.visible_ids().len();
        let total = app.games.len();
        let text = if visible == total {
            format!("{total} games")
        } else {
            format!("{visible} of {total} games")
        };
        ui.label(RichText::new(text).size(11.0).color(Palette::TEXT_FAINT));

        if let Some(report) = &app.last_report {
            let elapsed = report.elapsed_ms;
            ui.label(
                RichText::new(format!("scanned in {elapsed} ms"))
                    .size(10.5)
                    .color(Palette::TEXT_FAINT),
            );
        }
        if app.config.demo_mode {
            ui.label(
                RichText::new("demo library")
                    .size(10.5)
                    .color(Palette::ACCENT),
            );
        }
    });
}

/// A navigation entry, highlighted when active.
fn nav_item(ui: &mut egui::Ui, label: &str, selected: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if selected {
            painter.rect_filled(
                rect,
                CornerRadius::same(RADIUS_CARD),
                Palette::ACCENT_DIM,
            );
        } else if response.hovered() {
            painter.rect_filled(
                rect,
                CornerRadius::same(RADIUS_CARD),
                Palette::SURFACE_HOVER,
            );
        }
        let color = if selected {
            Palette::TEXT
        } else {
            Palette::TEXT_MUTED
        };
        painter.text(
            egui::pos2(rect.min.x + 12.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.5),
            color,
        );
    }
    response.clicked()
}

/// A store filter row with its brand-coloured dot.
fn store_row(
    ui: &mut egui::Ui,
    label: &str,
    store: Store,
    selected: bool,
    enabled: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() && enabled {
            painter.rect_filled(
                rect,
                CornerRadius::same(RADIUS_CARD),
                Palette::SURFACE_HOVER,
            );
        }
        let dot_center = egui::pos2(rect.min.x + 14.0, rect.center().y);
        let accent = if enabled {
            store.accent()
        } else {
            Palette::TEXT_FAINT
        };
        if selected {
            painter.circle_filled(dot_center, 5.0, accent);
        } else {
            painter.circle_stroke(dot_center, 4.5, egui::Stroke::new(1.5, accent));
        }
        let color = match (enabled, selected) {
            (false, _) => Palette::TEXT_FAINT,
            (true, true) => Palette::TEXT,
            (true, false) => Palette::TEXT_MUTED,
        };
        painter.text(
            egui::pos2(rect.min.x + 30.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            color,
        );
    }
    response
}
