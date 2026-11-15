use eframe::egui::{self, Color32, RichText};

use crate::app::Disc2App;
use crate::profile_theme::ThemePreset;

/// Aba de Aparência e Temas. Retorna `true` caso o usuário tenha alterado alguma propriedade visual.
pub fn render(app: &mut Disc2App, ui: &mut egui::Ui) -> bool {
    let mut theme_changed = false;

    ui.label(
        RichText::new("TEMAS PREDEFINIDOS")
            .size(11.0)
            .strong()
            .color(Color32::from_rgb(148, 155, 164)),
    );
    ui.add_space(4.0);

    for preset in [
        ThemePreset::DiscordDark,
        ThemePreset::MidnightOled,
        ThemePreset::CyberpunkNeon,
        ThemePreset::EmeraldGamer,
        ThemePreset::CrimsonRuby,
        ThemePreset::ArcticOcean,
    ] {
        let is_sel = app.custom.theme_preset == preset;
        if ui.selectable_label(is_sel, preset.label()).clicked() {
            app.custom.apply_preset_colors(preset);
            theme_changed = true;
        }
    }

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(6.0);

    ui.label(
        RichText::new("PERSONALIZAÇÃO LIVRE DE CORES")
            .size(11.0)
            .strong()
            .color(Color32::from_rgb(148, 155, 164)),
    );
    ui.horizontal(|ui| {
        ui.label("Destaque:");
        if ui
            .color_edit_button_srgb(&mut app.custom.accent_rgb)
            .changed()
        {
            app.custom.theme_preset = ThemePreset::Custom;
            theme_changed = true;
        }
        ui.label("Texto Principal:");
        if ui.color_edit_button_srgb(&mut app.custom.text_rgb).changed() {
            app.custom.theme_preset = ThemePreset::Custom;
            theme_changed = true;
        }
    });

    ui.horizontal(|ui| {
        ui.label("Barra Lateral:");
        if ui
            .color_edit_button_srgb(&mut app.custom.bg_sidebar_rgb)
            .changed()
        {
            app.custom.theme_preset = ThemePreset::Custom;
            theme_changed = true;
        }
        ui.label("Palco Central:");
        if ui
            .color_edit_button_srgb(&mut app.custom.bg_stage_rgb)
            .changed()
        {
            app.custom.theme_preset = ThemePreset::Custom;
            theme_changed = true;
        }
    });

    ui.horizontal(|ui| {
        ui.label("Painéis / Cards:");
        if ui
            .color_edit_button_srgb(&mut app.custom.bg_card_rgb)
            .changed()
        {
            app.custom.theme_preset = ThemePreset::Custom;
            theme_changed = true;
        }
    });

    ui.add_space(8.0);
    ui.label(
        RichText::new("GEOMETRIA E ESCALA DO APP")
            .size(11.0)
            .strong()
            .color(Color32::from_rgb(148, 155, 164)),
    );
    ui.horizontal(|ui| {
        ui.label("Cantos Arredondados:");
        if ui
            .add(egui::Slider::new(&mut app.custom.corner_radius, 0.0..=16.0).suffix(" px"))
            .changed()
        {
            theme_changed = true;
        }
    });

    ui.horizontal(|ui| {
        ui.label("Espessura da Borda:");
        if ui
            .add(egui::Slider::new(&mut app.custom.border_stroke_width, 0.5..=3.0).suffix(" px"))
            .changed()
        {
            theme_changed = true;
        }
    });

    ui.horizontal(|ui| {
        ui.label("Zoom da Interface:");
        if ui
            .add(egui::Slider::new(&mut app.custom.ui_scale, 0.90..=1.15))
            .changed()
        {
            theme_changed = true;
        }
    });

    theme_changed
}
