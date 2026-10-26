//! Panel frames.
//!
//! egui panels default to a flat fill; these give each region the padding,
//! border and background that makes the layout read as distinct surfaces.

use crate::theme::{space, Palette, RADIUS_LG};
use egui::{Frame, Margin, Stroke};

pub fn topbar_frame() -> Frame {
    Frame::new()
        .fill(Palette::SURFACE)
        .inner_margin(Margin::symmetric(space::LG as i8, space::MD as i8))
        .stroke(Stroke::new(1.0, Palette::BORDER))
}

pub fn sidebar_frame() -> Frame {
    Frame::new()
        .fill(Palette::SURFACE)
        .inner_margin(Margin::symmetric(space::MD as i8, space::LG as i8))
}

pub fn central_frame() -> Frame {
    Frame::new()
        .fill(Palette::BG)
        .inner_margin(Margin::same(space::XL as i8))
}

pub fn card_frame() -> Frame {
    Frame::new()
        .fill(Palette::CARD)
        .corner_radius(egui::CornerRadius::same(RADIUS_LG))
        .inner_margin(Margin::same(space::LG as i8))
        .stroke(Stroke::new(1.0, Palette::BORDER))
}
