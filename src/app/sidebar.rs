//! The left menu.
//!
//! Pages, not stores: the library is one mixed collection, so the menu
//! switches what you are doing rather than splitting Steam from GOG.

use crate::app::{IonApp, Screen};
use crate::theme::{space, text, Palette, RADIUS_MD};
use egui::{CornerRadius, RichText, Sense, Vec2};

pub fn show(app: &mut IonApp, ui: &mut egui::Ui) {
    let installed = app.games.len();
    let favorites = app.config.favorites.len();
    let on_library = app.screen == Screen::Library;

    // Brand lockup: a bolt mark plus the wordmark, sized to read as the
    // product name rather than another nav item.
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(28.0, 28.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            CornerRadius::same(8),
            crate::theme::Palette::accent_subtle(),
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "⚡",
            egui::FontId::proportional(15.0),
            Palette::ACCENT,
        );
        ui.add_space(space::SM);
        ui.vertical(|ui| {
            ui.label(
                RichText::new("IonLauncher")
                    .size(text::BASE)
                    .strong()
                    .color(Palette::TEXT),
            );
            ui.label(
                RichText::new("One library, three stores")
                    .size(text::XS)
                    .color(Palette::TEXT_FAINT),
            );
        });
    });

    ui.add_space(space::LG);
    ui.add_space(space::SM);

    section(ui, "Library");
    if nav_item(
        ui,
        "▤",
        "Installed",
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

    ui.add_space(space::MD);
    section(ui, "App");
    if nav_item(ui, "⚙", "Settings", None, app.screen == Screen::Settings) {
        app.screen = Screen::Settings;
    }

    // A quiet footer: which stores fed the library and how much each gave.
    // Informational only — there is nothing here to filter by.
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.add_space(space::XS);
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
                ui.add_space(space::LG);
                let label = if enabled {
                    store.short().to_string()
                } else {
                    format!("{} · off", store.short())
                };
                ui.label(
                    RichText::new(label)
                        .size(text::XS)
                        .color(Palette::TEXT_FAINT),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(count.to_string())
                            .size(text::XS)
                            .color(Palette::TEXT_FAINT),
                    );
                });
            });
            ui.add_space(space::XS);
        }
        ui.add_space(space::XS);
    });
}

fn section(ui: &mut egui::Ui, label: &str) {
    ui.add_space(space::XS);
    ui.label(
        RichText::new(label)
            .size(text::XS)
            .color(Palette::TEXT_FAINT),
    );
    ui.add_space(space::XS);
}

/// A full-width menu row. Returns true when clicked.
///
/// The active row gets a solid accent bar on its leading edge plus a tinted
/// background, so selection is legible even for a colour-blind reader.
fn nav_item(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    count: Option<usize>,
    selected: bool,
) -> bool {
    let height = 36.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let fill = if selected {
            Palette::accent_subtle()
        } else if hovered {
            Palette::SURFACE_HOVER
        } else {
            egui::Color32::TRANSPARENT
        };
        if fill != egui::Color32::TRANSPARENT {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(RADIUS_MD), fill);
        }
        if selected {
            // The accent bar: a rounded strip on the leading edge.
            let bar = egui::Rect::from_min_size(
                egui::pos2(rect.min.x + 2.0, rect.center().y - 9.0),
                Vec2::new(3.0, 18.0),
            );
            ui.painter()
                .rect_filled(bar, CornerRadius::same(2), Palette::ACCENT);
        }

        let text_color = if selected {
            Palette::ACCENT
        } else {
            Palette::TEXT_MUTED
        };
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(14.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.label(RichText::new(icon).size(14.0).color(text_color));
        child.add_space(space::SM);
        child.label(RichText::new(label).size(text::SM).color(if selected {
            Palette::TEXT
        } else {
            Palette::TEXT_MUTED
        }));

        if let Some(count) = count {
            child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let color = if selected {
                    Palette::ACCENT
                } else {
                    Palette::TEXT_FAINT
                };
                ui.label(RichText::new(count.to_string()).size(text::XS).color(color));
            });
        }
    }

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}
