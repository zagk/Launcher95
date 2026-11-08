use eframe::egui::{
    self, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Painter,
    Rect, Shadow, Stroke, TextStyle,
};
use std::sync::Arc;

pub const WIN95: Color32 = Color32::from_rgb(192, 192, 192);
pub const WIN95_LIGHT: Color32 = Color32::from_rgb(223, 223, 223);
pub const WIN95_SHADOW: Color32 = Color32::from_rgb(128, 128, 128);
pub const WIN95_DARK: Color32 = Color32::from_rgb(64, 64, 64);
pub const NAVY: Color32 = Color32::from_rgb(0, 0, 128);
pub const INK: Color32 = Color32::from_rgb(0, 0, 0);

const UI_FONT_CANDIDATES: [&str; 3] = [
    r"C:\Windows\Fonts\tahoma.ttf",
    r"C:\Windows\Fonts\segoeui.ttf",
    r"C:\Windows\Fonts\arial.ttf",
];

const KOREAN_FONT_CANDIDATES: [&str; 3] = [
    r"C:\Windows\Fonts\malgun.ttf",
    r"C:\Windows\Fonts\gulim.ttc",
    r"C:\Windows\Fonts\batang.ttc",
];

pub fn apply(ctx: &Context) {
    install_fonts(ctx);

    let mut style = (*ctx.style()).clone();
    let mut visuals = egui::Visuals::light();

    visuals.dark_mode = false;
    visuals.panel_fill = WIN95;
    visuals.window_fill = WIN95;
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.faint_bg_color = WIN95_LIGHT;
    visuals.window_stroke = Stroke::new(1.0_f32, WIN95_DARK);
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.menu_corner_radius = CornerRadius::ZERO;
    visuals.window_shadow = Shadow::NONE;
    visuals.popup_shadow = Shadow::NONE;
    visuals.hyperlink_color = NAVY;

    visuals.selection.bg_fill = NAVY;
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);

    visuals.widgets.noninteractive.bg_fill = WIN95;
    visuals.widgets.noninteractive.weak_bg_fill = WIN95;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, WIN95_SHADOW);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, INK);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::ZERO;

    visuals.widgets.inactive.bg_fill = WIN95;
    visuals.widgets.inactive.weak_bg_fill = WIN95;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, WIN95_SHADOW);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, INK);
    visuals.widgets.inactive.corner_radius = CornerRadius::ZERO;
    visuals.widgets.inactive.expansion = 0.0;

    visuals.widgets.hovered.bg_fill = WIN95_LIGHT;
    visuals.widgets.hovered.weak_bg_fill = WIN95_LIGHT;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, WIN95_DARK);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, INK);
    visuals.widgets.hovered.corner_radius = CornerRadius::ZERO;
    visuals.widgets.hovered.expansion = 0.0;

    visuals.widgets.active.bg_fill = WIN95_LIGHT;
    visuals.widgets.active.weak_bg_fill = WIN95_LIGHT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, WIN95_DARK);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, INK);
    visuals.widgets.active.corner_radius = CornerRadius::ZERO;
    visuals.widgets.active.expansion = 0.0;

    visuals.widgets.open.bg_fill = WIN95_LIGHT;
    visuals.widgets.open.weak_bg_fill = WIN95_LIGHT;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, WIN95_DARK);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, INK);
    visuals.widgets.open.corner_radius = CornerRadius::ZERO;
    visuals.widgets.open.expansion = 0.0;

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 3.0);
    style.spacing.window_margin = egui::Margin::same(4);
    style.spacing.menu_margin = egui::Margin::same(3);

    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(15.0, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(12.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(12.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(10.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(11.0, FontFamily::Monospace),
    );

    ctx.set_style(style);
}

fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    let mut has_ui_font = false;
    let mut has_korean_font = false;

    if let Some(bytes) = UI_FONT_CANDIDATES
        .iter()
        .find_map(|path| std::fs::read(path).ok())
    {
        fonts.font_data.insert(
            "win95-ui".to_owned(),
            Arc::new(FontData::from_owned(bytes)),
        );
        has_ui_font = true;
    }
    if let Some(bytes) = KOREAN_FONT_CANDIDATES
        .iter()
        .find_map(|path| std::fs::read(path).ok())
    {
        fonts.font_data.insert(
            "win95-korean".to_owned(),
            Arc::new(FontData::from_owned(bytes)),
        );
        has_korean_font = true;
    }

    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        if has_ui_font {
            family.insert(0, "win95-ui".to_owned());
        }
        if has_korean_font {
            // Put the Korean font at the end so it is used as a fallback only
            // for glyphs the Latin UI font does not contain.
            family.push("win95-korean".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

/// Draws a classic two-pixel bevel inside `rect`.
pub fn bevel(painter: &Painter, rect: Rect, raised: bool) {
    let (outer_tl, outer_br, inner_tl, inner_br) = if raised {
        (Color32::WHITE, WIN95_DARK, WIN95_LIGHT, WIN95_SHADOW)
    } else {
        (WIN95_DARK, Color32::WHITE, WIN95_SHADOW, WIN95_LIGHT)
    };

    painter.line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(1.0_f32, outer_tl),
    );
    painter.line_segment(
        [rect.left_top(), rect.left_bottom()],
        Stroke::new(1.0_f32, outer_tl),
    );
    painter.line_segment(
        [rect.right_top(), rect.right_bottom()],
        Stroke::new(1.0_f32, outer_br),
    );
    painter.line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0_f32, outer_br),
    );

    let inner = rect.shrink(1.0);
    if inner.width() <= 2.0 || inner.height() <= 2.0 {
        return;
    }
    painter.line_segment(
        [inner.left_top(), inner.right_top()],
        Stroke::new(1.0_f32, inner_tl),
    );
    painter.line_segment(
        [inner.left_top(), inner.left_bottom()],
        Stroke::new(1.0_f32, inner_tl),
    );
    painter.line_segment(
        [inner.right_top(), inner.right_bottom()],
        Stroke::new(1.0_f32, inner_br),
    );
    painter.line_segment(
        [inner.left_bottom(), inner.right_bottom()],
        Stroke::new(1.0_f32, inner_br),
    );
}

pub fn sunken_rect(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, CornerRadius::ZERO, Color32::WHITE);
    bevel(painter, rect, false);
}
