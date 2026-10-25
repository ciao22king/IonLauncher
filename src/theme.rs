//! Visual theme.
//!
//! A single dark palette applied to egui's style, plus the small set of
//! colours the widgets draw with directly. Keeping it in one place means the
//! UI code never hardcodes a colour and the whole look can be retuned here.

use egui::{Color32, CornerRadius, Stroke, Visuals};

pub struct Palette;

impl Palette {
    // Surfaces, from furthest back to closest.
    pub const BG: Color32 = Color32::from_rgb(0x0E, 0x11, 0x17);
    pub const SURFACE: Color32 = Color32::from_rgb(0x16, 0x1A, 0x23);
    pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x1D, 0x22, 0x2D);
    pub const CARD: Color32 = Color32::from_rgb(0x1A, 0x1F, 0x29);
    pub const CARD_HOVER: Color32 = Color32::from_rgb(0x22, 0x29, 0x36);
    pub const BORDER: Color32 = Color32::from_rgb(0x27, 0x2E, 0x3B);

    // Text.
    pub const TEXT: Color32 = Color32::from_rgb(0xE8, 0xEC, 0xF2);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x93, 0x9E, 0xB0);
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x66, 0x70, 0x82);

    // Accents.
    pub const ACCENT: Color32 = Color32::from_rgb(0x4C, 0x8D, 0xFF);
    pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x2A, 0x40, 0x6B);
    pub const DANGER: Color32 = Color32::from_rgb(0xE5, 0x53, 0x5B);
    pub const OK: Color32 = Color32::from_rgb(0x3D, 0xD6, 0x8C);
}

/// Standard corner radii, so shapes stay consistent across the app.
pub const RADIUS_CARD: u8 = 10;
pub const RADIUS_CONTROL: u8 = 8;
pub const RADIUS_CHIP: u8 = 12;

/// Applies the IonLauncher look to an egui context.
pub fn apply(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();

    visuals.panel_fill = Palette::BG;
    visuals.window_fill = Palette::SURFACE;
    visuals.extreme_bg_color = Palette::BG;
    visuals.faint_bg_color = Palette::SURFACE_HOVER;
    visuals.window_stroke = Stroke::new(1.0, Palette::BORDER);
    visuals.window_corner_radius = CornerRadius::same(RADIUS_CARD);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS_CONTROL);

    visuals.override_text_color = Some(Palette::TEXT);
    visuals.selection.bg_fill = Palette::ACCENT_DIM;
    visuals.selection.stroke = Stroke::new(1.0, Palette::ACCENT);
    visuals.hyperlink_color = Palette::ACCENT;

    // Widgets: flat by default, lifted on hover, accented when active.
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = Palette::SURFACE;
    widgets.noninteractive.weak_bg_fill = Palette::SURFACE;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, Palette::BORDER);
    widgets.noninteractive.corner_radius = CornerRadius::same(RADIUS_CONTROL);

    widgets.inactive.bg_fill = Palette::SURFACE_HOVER;
    widgets.inactive.weak_bg_fill = Palette::SURFACE_HOVER;
    widgets.inactive.bg_stroke = Stroke::new(1.0, Palette::BORDER);
    widgets.inactive.corner_radius = CornerRadius::same(RADIUS_CONTROL);

    widgets.hovered.bg_fill = Palette::CARD_HOVER;
    widgets.hovered.weak_bg_fill = Palette::CARD_HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0, Palette::ACCENT_DIM);
    widgets.hovered.corner_radius = CornerRadius::same(RADIUS_CONTROL);

    widgets.active.bg_fill = Palette::ACCENT_DIM;
    widgets.active.weak_bg_fill = Palette::ACCENT_DIM;
    widgets.active.bg_stroke = Stroke::new(1.0, Palette::ACCENT);
    widgets.active.corner_radius = CornerRadius::same(RADIUS_CONTROL);

    widgets.open.bg_fill = Palette::SURFACE_HOVER;
    widgets.open.weak_bg_fill = Palette::SURFACE_HOVER;
    widgets.open.bg_stroke = Stroke::new(1.0, Palette::BORDER);

    ctx.set_visuals(visuals);

    // egui 0.37 keeps a separate style per theme, so mutate both to make the
    // look independent of the OS light/dark preference.
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.window_margin = egui::Margin::same(12);
        style.spacing.scroll.bar_width = 8.0;
        style.spacing.scroll.floating = false;
        style.visuals.window_shadow = egui::epaint::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(140),
        };
    });
}
