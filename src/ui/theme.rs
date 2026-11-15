use eframe::egui::{self, Color32, Context, Rounding, Stroke};

use crate::profile_theme::UserCustomization;

/// Instala fontes de emoji e símbolos do sistema no contexto do egui.
pub fn install_system_emoji_fonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguiemj.ttf") {
        fonts.font_data.insert(
            "segoe_ui_emoji".to_owned(),
            egui::FontData::from_owned(bytes),
        );
        if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            fam.push("segoe_ui_emoji".to_owned());
        }
        if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            fam.push("segoe_ui_emoji".to_owned());
        }
    }
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguisym.ttf") {
        fonts.font_data.insert(
            "segoe_ui_symbol".to_owned(),
            egui::FontData::from_owned(bytes),
        );
        if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            fam.push("segoe_ui_symbol".to_owned());
        }
        if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            fam.push("segoe_ui_symbol".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

/// Aplica as cores, arredondamento, borda e escala configurados pelo usuário ao `egui::Context`.
pub fn apply_theme_to_egui(custom: &UserCustomization, ctx: &Context) {
    ctx.set_pixels_per_point(custom.ui_scale.clamp(0.85, 1.25));

    let mut visuals = egui::Visuals::dark();
    let r = Rounding::same(custom.corner_radius);
    let text_col = Color32::from_rgb(custom.text_rgb[0], custom.text_rgb[1], custom.text_rgb[2]);

    visuals.override_text_color = Some(text_col);
    visuals.panel_fill = custom.sidebar_color();
    visuals.window_fill = custom.card_color();
    visuals.window_rounding = Rounding::same((custom.corner_radius + 4.0).min(18.0));
    visuals.menu_rounding = r;

    visuals.widgets.noninteractive.bg_fill = custom.card_color();
    visuals.widgets.noninteractive.rounding = r;

    visuals.widgets.inactive.bg_fill = Color32::from_rgb(
        custom.bg_card_rgb[0].saturating_add(10),
        custom.bg_card_rgb[1].saturating_add(10),
        custom.bg_card_rgb[2].saturating_add(12),
    );
    visuals.widgets.inactive.rounding = r;

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(
        custom.bg_card_rgb[0].saturating_add(22),
        custom.bg_card_rgb[1].saturating_add(22),
        custom.bg_card_rgb[2].saturating_add(26),
    );
    visuals.widgets.hovered.rounding = r;
    visuals.widgets.hovered.bg_stroke =
        Stroke::new(custom.border_stroke_width, custom.accent_color());

    visuals.widgets.active.bg_fill = custom.accent_color();
    visuals.widgets.active.rounding = r;

    visuals.selection.bg_fill = custom.accent_color();
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);

    ctx.set_visuals(visuals);
}
