//! Reusable widgets.
//!
//! The card widget is the heart of the launcher, so it is hand-painted
//! rather than assembled from stock widgets: one `allocate_exact_size`, a
//! few shapes and one optional texture, which keeps the per-frame cost low
//! even with hundreds of cards on a software or integrated GPU.

use crate::model::{Game, LaunchTarget, Store};
use crate::theme::{Palette, RADIUS_CARD, RADIUS_CHIP};
use crate::util;
use egui::{
    Align2, Color32, CornerRadius, FontId, Mesh, Rect, Response, RichText, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2,
};

/// What the user did to a game card this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardAction {
    None,
    Launch,
    Details,
    ToggleFavorite,
    StorePage,
}

/// Draws a portrait cover card and reports interaction.
///
/// `favorite` draws a star badge. The whole card is one interactive rect, so
/// there are no nested hit-test regions to keep in sync.
pub fn game_card(
    ui: &mut Ui,
    game: &Game,
    size: Vec2,
    favorite: bool,
    compact: bool,
) -> (Response, CardAction) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let fill = if hovered {
            Palette::CARD_HOVER
        } else {
            Palette::CARD
        };

        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(RADIUS_CARD), fill);

        // Cover art, cropped to fill the top area. Cropping rather than
        // stretching keeps the artwork's own proportions, which matters
        // because the stores ship a mix of portrait and square covers.
        let art_height = (size.y * 0.72).max(0.0);
        let art_rect = Rect::from_min_size(rect.min, Vec2::new(size.x, art_height));
        let rounded_art = Rect::from_min_max(
            rect.min + Vec2::new(1.0, 1.0),
            egui::pos2(rect.max.x - 1.0, art_rect.max.y),
        );
        match &game.art {
            Some(texture) => paint_cover(ui, rounded_art, texture, RADIUS_CARD, fill, Corners::TOP),
            None => paint_art_placeholder(ui, rounded_art, game),
        }

        // A solid band behind the title keeps text legible over bright
        // artwork without darkening the cover itself.
        let text_top = art_rect.max.y;
        let text_rect = Rect::from_min_max(
            egui::pos2(rect.min.x, text_top),
            egui::pos2(rect.max.x, rect.max.y),
        );
        painter.rect_filled(
            text_rect,
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: RADIUS_CARD,
                se: RADIUS_CARD,
            },
            fill,
        );

        paint_card_text(ui, text_rect, game, compact, hovered);
        paint_store_badge(ui, rect, game.store);

        if favorite {
            paint_star_badge(ui, rect);
        }

        // Hover ring, drawn last so it sits on top of the artwork.
        if hovered {
            painter.rect_stroke(
                rect,
                CornerRadius::same(RADIUS_CARD),
                Stroke::new(1.5, Palette::ACCENT),
                StrokeKind::Inside,
            );
        }
    }

    let mut action = CardAction::None;
    if response.clicked() {
        action = CardAction::Details;
    }
    if response.double_clicked() {
        action = CardAction::Launch;
    }
    // Right-click is the quick way to launch without opening the details.
    if response.secondary_clicked() {
        action = CardAction::Launch;
    }
    response.context_menu(|ui| {
        if ui.button("▶  Launch").clicked() {
            action = CardAction::Launch;
            ui.close();
        }
        if ui.button("★  Toggle favourite").clicked() {
            action = CardAction::ToggleFavorite;
            ui.close();
        }
        ui.separator();
        if ui.button("Details").clicked() {
            action = CardAction::Details;
            ui.close();
        }
    });

    (response, action)
}

/// Which corners of a cover get rounded.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Corners {
    pub top: bool,
    pub bottom: bool,
}

impl Corners {
    /// Card covers: the title band below already rounds the bottom.
    pub const TOP: Self = Self {
        top: true,
        bottom: false,
    };
    /// Free-standing covers: details sheet and list thumbnails.
    pub const ALL: Self = Self {
        top: true,
        bottom: true,
    };
}

/// Draws a texture into `rect` as a cover: scaled up to fill the whole rect
/// and centre-cropped, so the image keeps its aspect ratio and never distorts.
///
/// `Painter::image` cannot round the corners of a texture, so the corners
/// selected by `corners` are masked with `corner_fill`, the colour of the
/// surface the cover is sitting on.
pub fn paint_cover(
    ui: &Ui,
    rect: Rect,
    texture: &egui::TextureHandle,
    radius: u8,
    corner_fill: Color32,
    corners: Corners,
) {
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    let size = texture.size_vec2();
    if size.x < 1.0 || size.y < 1.0 {
        return;
    }

    // UV window that crops the texture to the target aspect ratio, centred.
    // Without this the cover would be stretched to the card's proportions.
    let target = rect.width() / rect.height();
    let source = size.x / size.y;
    let uv = if source > target {
        let keep = target / source;
        let inset = (1.0 - keep) * 0.5;
        Rect::from_min_max(egui::pos2(inset, 0.0), egui::pos2(1.0 - inset, 1.0))
    } else {
        let keep = source / target;
        let inset = (1.0 - keep) * 0.5;
        Rect::from_min_max(egui::pos2(0.0, inset), egui::pos2(1.0, 1.0 - inset))
    };

    let mut mesh = Mesh::with_texture(texture.id());
    mesh.add_rect_with_uv(rect, uv, Color32::WHITE);
    ui.painter().add(Shape::mesh(mesh));

    if radius > 0 {
        paint_corner_notches(ui, rect, radius as f32, corner_fill, corners);
    }
}

/// Fills the four square corner notches of `rect` so a rectangular image reads
/// as having rounded corners.
///
/// Each notch is the region between the square corner and the quarter-circle
/// of `radius`, drawn as a fan from the corner point to points on the arc.
fn paint_corner_notches(ui: &Ui, rect: Rect, r: f32, fill: Color32, corners: Corners) {
    let painter = ui.painter();
    let notch = |apex: egui::Pos2, center: egui::Pos2, from: f32, to: f32| {
        let steps = 6;
        let mut mesh = Mesh::default();
        mesh.colored_vertex(apex, fill);
        for i in 0..=steps {
            let t = from + (to - from) * (i as f32 / steps as f32);
            let (s, c) = t.sin_cos();
            mesh.colored_vertex(center + Vec2::new(c * r, s * r), fill);
            if i > 0 {
                mesh.add_triangle(0, i as u32, i as u32 + 1);
            }
        }
        painter.add(Shape::mesh(mesh));
    };

    use std::f32::consts::{FRAC_PI_2, PI};
    if corners.top {
        // Top-left: arc centre inset by r, sweeping from the left edge upward.
        notch(
            rect.min,
            egui::pos2(rect.min.x + r, rect.min.y + r),
            PI,
            PI + FRAC_PI_2,
        );
        // Top-right.
        notch(
            egui::pos2(rect.max.x, rect.min.y),
            egui::pos2(rect.max.x - r, rect.min.y + r),
            PI + FRAC_PI_2,
            2.0 * PI,
        );
    }
    if corners.bottom {
        notch(
            egui::pos2(rect.min.x, rect.max.y),
            egui::pos2(rect.min.x + r, rect.max.y - r),
            FRAC_PI_2,
            PI,
        );
        notch(
            rect.max,
            egui::pos2(rect.max.x - r, rect.max.y - r),
            0.0,
            FRAC_PI_2,
        );
    }
}

/// Placeholder shown before artwork arrives: the store accent plus the
/// title's initials, so the grid still reads well while loading.
fn paint_art_placeholder(ui: &Ui, rect: Rect, game: &Game) {
    let painter = ui.painter();
    let accent = game.store.accent();
    painter.rect_filled(
        rect,
        CornerRadius {
            nw: RADIUS_CARD,
            ne: RADIUS_CARD,
            sw: 0,
            se: 0,
        },
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 26),
    );

    let initials = initials_of(&game.title);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        initials,
        FontId::proportional((rect.height() * 0.30).clamp(20.0, 56.0)),
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 150),
    );
}

/// First letters of the first two words, e.g. "The Witcher 3" -> "TW".
fn initials_of(title: &str) -> String {
    let mut out = String::new();
    for word in title.split_whitespace() {
        if let Some(first) = word.chars().next() {
            if first.is_alphanumeric() {
                out.extend(first.to_uppercase());
            }
        }
        if out.chars().count() >= 2 {
            break;
        }
    }
    if out.is_empty() {
        "?".to_string()
    } else {
        out
    }
}

fn paint_card_text(ui: &Ui, rect: Rect, game: &Game, compact: bool, hovered: bool) {
    let painter = ui.painter();
    let pad = 9.0;
    let inner = Rect::from_min_max(
        egui::pos2(rect.min.x + pad, rect.min.y + 5.0),
        egui::pos2(rect.max.x - pad, rect.max.y - 5.0),
    );
    if inner.width() <= 4.0 || inner.height() <= 4.0 {
        return;
    }

    let title_size = if compact { 12.5 } else { 13.5 };
    // Two rows maximum, elided with "…" when the title is longer. Without the
    // row limit a long title is simply clipped mid-glyph by the band.
    let galley = painter.layout_job(elided_job(
        &game.title,
        FontId::proportional(title_size),
        Palette::TEXT,
        inner.width(),
        2,
    ));
    let title_height = galley.size().y.min(inner.height() * 0.72);
    painter.galley(inner.min, galley, Palette::TEXT);

    // Metadata line: store, then playtime or size when known.
    let mut meta: Vec<String> = vec![game.store.short().to_string()];
    if let Some(playtime) = game.playtime_label() {
        meta.push(playtime);
    } else if let Some(size) = game.size_label() {
        meta.push(size);
    }
    if !game.install_dir.as_os_str().is_empty() && !game.install_dir.exists() {
        meta.push("missing".into());
    }

    let meta_rect = Rect::from_min_max(
        egui::pos2(inner.min.x, inner.min.y + title_height + 2.0),
        inner.max,
    );
    if meta_rect.height() >= 10.0 {
        let color = if hovered {
            Palette::TEXT_MUTED
        } else {
            Palette::TEXT_FAINT
        };
        let galley = painter.layout_job(elided_job(
            &meta.join("  ·  "),
            FontId::proportional(11.0),
            color,
            meta_rect.width(),
            1,
        ));
        painter.galley(meta_rect.min, galley, color);
    }
}

/// Lays out `text` wrapped to `width` and elided after `max_rows`, so long
/// strings end in "…" instead of being cut mid-word by the paint rect.
pub fn elided_job(
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
    max_rows: usize,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping {
            max_width: width,
            max_rows,
            break_anywhere: max_rows == 1,
            overflow_character: Some('…'),
        },
        ..Default::default()
    };
    job.append(
        text,
        0.0,
        egui::text::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    job
}

/// A small dot in the store's brand colour, top-left of the card.
fn paint_store_badge(ui: &Ui, rect: Rect, store: Store) {
    let painter = ui.painter();
    let center = egui::pos2(rect.min.x + 14.0, rect.min.y + 14.0);
    painter.circle_filled(center, 6.0, Color32::from_black_alpha(150));
    painter.circle_filled(center, 4.0, store.accent());
}

/// Star badge shown on favourite cards, top-right.
fn paint_star_badge(ui: &Ui, rect: Rect) {
    let painter = ui.painter();
    let center = egui::pos2(rect.max.x - 15.0, rect.min.y + 15.0);
    painter.circle_filled(center, 8.0, Color32::from_black_alpha(150));
    painter.text(
        center,
        Align2::CENTER_CENTER,
        "★",
        FontId::proportional(11.0),
        Color32::from_rgb(0xF5, 0xC2, 0x42),
    );
}

/// A pill-shaped filter chip. Returns true when clicked.
pub fn chip(ui: &mut Ui, label: &str, selected: bool, accent: Option<Color32>) -> bool {
    let text = RichText::new(label).size(12.5);
    let text = if selected {
        text.color(Color32::WHITE)
    } else {
        text.color(Palette::TEXT_MUTED)
    };
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        FontId::proportional(12.5),
        if selected {
            Color32::WHITE
        } else {
            Palette::TEXT_MUTED
        },
    );
    let padding = Vec2::new(11.0, 6.0);
    let size = galley.size() + padding * 2.0;
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let (fill, stroke_color) = if selected {
            (
                accent.unwrap_or(Palette::ACCENT_DIM),
                accent.unwrap_or(Palette::ACCENT),
            )
        } else if response.hovered() {
            (Palette::CARD_HOVER, Palette::BORDER)
        } else {
            (Palette::SURFACE_HOVER, Palette::BORDER)
        };
        painter.rect_filled(rect, CornerRadius::same(RADIUS_CHIP), fill);
        painter.rect_stroke(
            rect,
            CornerRadius::same(RADIUS_CHIP),
            Stroke::new(1.0, stroke_color),
            StrokeKind::Inside,
        );
        painter.galley(
            rect.center() - galley.size() / 2.0,
            galley,
            if selected {
                Color32::WHITE
            } else {
                Palette::TEXT_MUTED
            },
        );
    }
    let _ = text;
    response.clicked()
}

/// A labelled stat, e.g. `Playtime / 42.5 h`. Skips itself when empty.
pub fn stat(ui: &mut Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label).size(10.5).color(Palette::TEXT_FAINT));
        ui.label(RichText::new(value).size(13.0).color(Palette::TEXT));
    });
}

/// A large primary action button.
pub fn primary_button(ui: &mut Ui, label: &str, width: f32) -> Response {
    let button = egui::Button::new(RichText::new(label).size(14.0).color(Color32::WHITE))
        .fill(Palette::ACCENT)
        .corner_radius(CornerRadius::same(RADIUS_CARD))
        .min_size(Vec2::new(width, 38.0));
    ui.add(button)
}

/// A secondary, quieter button.
pub fn secondary_button(ui: &mut Ui, label: &str) -> Response {
    let button = egui::Button::new(RichText::new(label).size(13.0).color(Palette::TEXT))
        .fill(Palette::SURFACE_HOVER)
        .stroke(Stroke::new(1.0, Palette::BORDER))
        .corner_radius(CornerRadius::same(RADIUS_CARD));
    ui.add(button)
}

/// A section heading with a muted rule underneath.
pub fn section_heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(15.0).strong().color(Palette::TEXT));
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Palette::BORDER);
    ui.add_space(2.0);
}

/// Renders the correct icon/text for a launch target, used in details.
pub fn launch_target_label(target: &LaunchTarget) -> String {
    match target {
        LaunchTarget::Url(url) => url.clone(),
        LaunchTarget::Direct { exe, args } => {
            if args.is_empty() {
                exe.display().to_string()
            } else {
                format!("{} {}", exe.display(), args.join(" "))
            }
        }
    }
}

/// Opens a game through its store, returning a user-facing error message.
pub fn launch_game(game: &Game) -> Result<(), String> {
    match &game.launch {
        LaunchTarget::Url(url) => util::open_url(url),
        LaunchTarget::Direct { exe, args } => {
            let workdir = if game.install_dir.is_dir() {
                Some(game.install_dir.as_path())
            } else {
                None
            };
            util::spawn_exe(exe, args, workdir)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Store;

    fn game(title: &str) -> Game {
        Game::new("steam:1".into(), "1".into(), Store::Steam, title.into())
    }

    #[test]
    fn initials_use_the_first_two_words() {
        assert_eq!(initials_of("The Witcher 3"), "TW");
        assert_eq!(initials_of("Hades"), "H");
        assert_eq!(initials_of("  "), "?");
        assert_eq!(initials_of("3D Mark"), "3M");
    }

    #[test]
    fn launch_target_labels_are_readable() {
        let url = LaunchTarget::Url("steam://runsappid/440".into());
        assert_eq!(launch_target_label(&url), "steam://runsappid/440");

        let direct = LaunchTarget::Direct {
            exe: "/games/a/run.sh".into(),
            args: vec!["-x".into()],
        };
        assert_eq!(launch_target_label(&direct), "/games/a/run.sh -x");

        let bare = LaunchTarget::Direct {
            exe: "/games/a/run.sh".into(),
            args: vec![],
        };
        assert_eq!(launch_target_label(&bare), "/games/a/run.sh");
    }

    #[test]
    fn launching_a_missing_executable_reports_an_error() {
        let mut g = game("Broken");
        g.launch = LaunchTarget::Direct {
            exe: "/definitely/not/here".into(),
            args: vec![],
        };
        let err = launch_game(&g).unwrap_err();
        assert!(err.contains("not found"), "unexpected: {err}");
    }

    #[test]
    fn launching_an_empty_url_reports_an_error() {
        let mut g = game("Broken");
        g.launch = LaunchTarget::Url(String::new());
        assert!(launch_game(&g).is_err());
    }
}
