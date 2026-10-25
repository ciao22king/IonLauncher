//! Panel frames.
//!
//! egui panels default to a flat fill; these give each region the padding,
//! border and background that makes the layout read as distinct surfaces.

use crate::theme::Palette;
use egui::{Frame, Margin, Stroke};

pub fn topbar_frame() -> Frame {
    Frame::new()
        .fill(Palette::SURFACE)
        .inner_margin(Margin::symmetric(14, 9))
        .stroke(Stroke::new(1.0, Palette::BORDER))
}

pub fn sidebar_frame() -> Frame {
    Frame::new()
        .fill(Palette::SURFACE)
        .inner_margin(Margin::symmetric(12, 14))
}

pub fn central_frame() -> Frame {
    Frame::new()
        .fill(Palette::BG)
        .inner_margin(Margin::same(18))
}

pub fn card_frame() -> Frame {
    Frame::new()
        .fill(Palette::CARD)
        .corner_radius(egui::CornerRadius::same(crate::theme::RADIUS_CARD))
        .inner_margin(Margin::same(14))
        .stroke(Stroke::new(1.0, Palette::BORDER))
}
