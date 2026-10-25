//! Controls for the Installed games page.
//!
//! The left menu owns navigation and the Favourites view, so this strip only
//! carries what changes how the library is displayed: view mode and sort order.

use crate::app::IonApp;
use crate::config::{SortMode, ViewMode};
use crate::theme::Palette;
use crate::widgets;
use egui::RichText;

pub fn show(app: &mut IonApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        for mode in [ViewMode::Grid, ViewMode::List] {
            let selected = app.config.view == mode;
            if widgets::chip(ui, mode.label(), selected, None) {
                app.config.view = mode;
                app.config_dirty = true;
            }
        }

        ui.add_space(10.0);
        ui.label(RichText::new("Sort").size(12.0).color(Palette::TEXT_FAINT));
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
    });
    ui.add_space(10.0);
}
