//! The single control strip below the search bar.
//!
//! Everything the sidebar used to do — switching screens, toggling stores,
//! choosing a view and a sort order — lives here in one row, so the library
//! is presented as one collection rather than separate per-store rooms.

use crate::app::{IonApp, Screen};
use crate::config::{SortMode, ViewMode};
use crate::theme::Palette;
use crate::widgets;
use egui::{RichText, Vec2};

pub fn show(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        // Library / Settings. These are the only two destinations now.
        for (screen, label) in [(Screen::Library, "Library"), (Screen::Settings, "Settings")] {
            let selected = app.screen == screen;
            if widgets::chip(ui, label, selected, None) {
                app.screen = screen;
                app.view_cache = None;
            }
        }

        separator(ui);
        favorites_toggle(app, ui);
        separator(ui);
        view_and_sort(app, ui);

        // Counts live on the right so they never push the controls around.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let visible = app.visible_ids().len();
            let total = app.games.len();
            let text = if visible == total {
                format!("{total} games")
            } else {
                format!("{visible} of {total}")
            };
            ui.label(RichText::new(text).size(11.5).color(Palette::TEXT_FAINT));

            if let Some(report) = &app.last_report {
                ui.label(
                    RichText::new(format!("· scanned in {} ms", report.elapsed_ms))
                        .size(11.0)
                        .color(Palette::TEXT_FAINT),
                );
            }
        });
    });
}

fn separator(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 20.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, Palette::BORDER);
    ui.add_space(6.0);
}

fn favorites_toggle(app: &mut IonApp, ui: &mut egui::Ui) {
    let count = app.config.favorites.len();
    let label = if count > 0 {
        format!("★ Favourites {count}")
    } else {
        "★ Favourites".to_string()
    };
    if widgets::chip(ui, &label, app.favorites_only, None) {
        app.favorites_only = !app.favorites_only;
        app.view_cache = None;
    }
}

fn view_and_sort(app: &mut IonApp, ui: &mut egui::Ui) {
    for mode in [ViewMode::Grid, ViewMode::List] {
        let selected = app.config.view == mode;
        if widgets::chip(ui, mode.label(), selected, None) {
            app.config.view = mode;
            app.config_dirty = true;
        }
    }

    ui.add_space(6.0);
    egui::ComboBox::from_id_salt("sort-combo")
        .selected_text(RichText::new(app.config.sort.label()).size(12.5))
        .width(132.0)
        .show_ui(ui, |ui| {
            for mode in SortMode::ALL {
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
}
