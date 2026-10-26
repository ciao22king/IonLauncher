//! Visual language: design tokens and the egui style built from them.
//!
//! Everything the UI draws is expressed through the tokens here — spacing,
//! radius, type scale, colour, elevation — so the whole look can be retuned
//! from one place and no screen hardcodes a value. The palette is a warm
//! amber accent over layered near-black surfaces: deliberately distinct from
//! the three store brands (Steam blue, Epic grey, GOG violet) so the app's own
//! accent never reads as a store colour.

use egui::{Color32, CornerRadius, Stroke, Visuals};

// ---------------------------------------------------------------------------
// Spacing — a 4px base unit. Every gap and pad in the UI is one of these.
// ---------------------------------------------------------------------------
pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

// ---------------------------------------------------------------------------
// Radius — a deliberate hierarchy, not one radius everywhere.
// ---------------------------------------------------------------------------
/// Small controls: inputs, tags.
pub const RADIUS_SM: u8 = 6;
/// Medium: buttons, menu rows, dropdowns.
pub const RADIUS_MD: u8 = 10;
/// Large: cards, panels.
pub const RADIUS_LG: u8 = 14;
/// Extra large: the toast and other floating sheets.
pub const RADIUS_XL: u8 = 18;

// ---------------------------------------------------------------------------
// Type scale.
// ---------------------------------------------------------------------------
pub mod text {
    /// Meta labels, timestamps.
    pub const XS: f32 = 11.0;
    /// Secondary content, table cells.
    pub const SM: f32 = 13.0;
    /// Body and descriptions.
    pub const BASE: f32 = 15.0;
    /// Section titles.
    pub const MD: f32 = 17.0;
    /// Card headings.
    pub const LG: f32 = 20.0;
    /// Page titles.
    pub const XL: f32 = 24.0;
}

/// The colour system. Names describe role, never appearance, so a retune
/// here changes every screen without touching a screen.
pub struct Palette;

impl Palette {
    // Surfaces, from furthest back to closest.
    pub const BG: Color32 = Color32::from_rgb(0x0B, 0x0B, 0x0D);
    pub const SURFACE: Color32 = Color32::from_rgb(0x13, 0x13, 0x17);
    pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x1C, 0x1C, 0x21);
    pub const CARD: Color32 = Color32::from_rgb(0x17, 0x17, 0x1C);
    pub const CARD_HOVER: Color32 = Color32::from_rgb(0x20, 0x20, 0x27);
    pub const BORDER: Color32 = Color32::from_rgb(0x26, 0x26, 0x2C);
    pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x34, 0x34, 0x3C);

    // Text, by emphasis.
    pub const TEXT: Color32 = Color32::from_rgb(0xF4, 0xF4, 0xF5);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(0xA1, 0xA1, 0xAA);
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x71, 0x71, 0x7A);

    // The one accent. Warm amber, chosen to sit apart from every store brand.
    pub const ACCENT: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);
    /// A dark amber used as a fill behind accent text (selected rows).
    pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x3A, 0x2A, 0x0A);
    /// Text drawn on top of an accent fill: near-black, for contrast.
    pub const ON_ACCENT: Color32 = Color32::from_rgb(0x1A, 0x12, 0x02);

    // Status colours.
    pub const DANGER: Color32 = Color32::from_rgb(0xF8, 0x71, 0x71);
    pub const WARNING: Color32 = Color32::from_rgb(0xFB, 0xBF, 0x24);
    pub const OK: Color32 = Color32::from_rgb(0x4A, 0xDE, 0x80);
    pub const INFO: Color32 = Color32::from_rgb(0x60, 0xA5, 0xFA);

    /// A low-alpha tint of the accent, for subtle selected backgrounds.
    pub fn accent_subtle() -> Color32 {
        Color32::from_rgba_unmultiplied(0xF5, 0x9E, 0x0B, 30)
    }

    /// A low-alpha tint of an arbitrary colour, for badges and fills.
    pub fn tint(color: Color32, alpha: u8) -> Color32 {
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
    }
}

/// Applies the IonLauncher look to an egui context.
pub fn apply(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();

    visuals.panel_fill = Palette::BG;
    visuals.window_fill = Palette::SURFACE;
    visuals.extreme_bg_color = Palette::BG;
    visuals.faint_bg_color = Palette::SURFACE_HOVER;
    visuals.window_stroke = Stroke::new(1.0, Palette::BORDER);
    visuals.window_corner_radius = CornerRadius::same(RADIUS_XL);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS_MD);

    visuals.override_text_color = Some(Palette::TEXT);
    visuals.selection.bg_fill = Palette::ACCENT_DIM;
    visuals.selection.stroke = Stroke::new(1.0, Palette::ACCENT);
    visuals.hyperlink_color = Palette::ACCENT;

    // Widgets: flat by default, lifted on hover, accented when active.
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = Palette::SURFACE;
    widgets.noninteractive.weak_bg_fill = Palette::SURFACE;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, Palette::BORDER);
    widgets.noninteractive.corner_radius = CornerRadius::same(RADIUS_SM);

    widgets.inactive.bg_fill = Palette::SURFACE_HOVER;
    widgets.inactive.weak_bg_fill = Palette::SURFACE_HOVER;
    widgets.inactive.bg_stroke = Stroke::new(1.0, Palette::BORDER);
    widgets.inactive.corner_radius = CornerRadius::same(RADIUS_SM);

    widgets.hovered.bg_fill = Palette::CARD_HOVER;
    widgets.hovered.weak_bg_fill = Palette::CARD_HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0, Palette::BORDER_STRONG);
    widgets.hovered.corner_radius = CornerRadius::same(RADIUS_SM);

    widgets.active.bg_fill = Palette::ACCENT_DIM;
    widgets.active.weak_bg_fill = Palette::ACCENT_DIM;
    widgets.active.bg_stroke = Stroke::new(1.0, Palette::ACCENT);
    widgets.active.corner_radius = CornerRadius::same(RADIUS_SM);

    widgets.open.bg_fill = Palette::SURFACE_HOVER;
    widgets.open.weak_bg_fill = Palette::SURFACE_HOVER;
    widgets.open.bg_stroke = Stroke::new(1.0, Palette::BORDER);

    ctx.set_visuals(visuals);

    // egui 0.37 keeps a separate style per theme, so mutate both to make the
    // look independent of the OS light/dark preference.
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(space::SM, space::SM);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.window_margin = egui::Margin::same(space::MD as i8);
        style.spacing.scroll.bar_width = 8.0;
        style.spacing.scroll.floating = false;
        // A visible focus ring, so keyboard navigation is legible.
        style.visuals.selection.stroke = Stroke::new(1.5, Palette::ACCENT);
        style.visuals.window_shadow = egui::epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(160),
        };
    });
}
