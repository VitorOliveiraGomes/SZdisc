use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Context, RichText, Rounding, Stroke, Vec2};

/// Renderiza o toast temporário de feedback no canto inferior direito da janela.
pub fn render_toast(
    ctx: &Context,
    status_toast: &Option<(String, Instant)>,
    card_bg: Color32,
    accent: Color32,
) {
    if let Some((msg, when)) = status_toast {
        if when.elapsed() < Duration::from_secs(4) {
            egui::Area::new(egui::Id::new("toast_area"))
                .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -16.0))
                .show(ctx, |ui| {
                    egui::Frame::none()
                        .fill(card_bg)
                        .stroke(Stroke::new(1.0_f32, accent))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new(msg).size(12.0).color(Color32::WHITE));
                        });
                });
        }
    }
}
