use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, TextureHandle, Ui, Vec2};

use crate::profile_theme::UserCustomization;

/// Desenha o avatar circular do usuário local com anel opcional e indicador de presença.
pub fn draw_discord_avatar(
    ui: &mut Ui,
    size: f32,
    cust: &UserCustomization,
    avatar_tex: Option<&TextureHandle>,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let painter = ui.painter();
    let center = rect.center();
    let radius = size * 0.5;

    if let Some(tex) = avatar_tex {
        let img_rect = Rect::from_center_size(center, Vec2::splat(size - 3.0));
        painter.image(
            tex.id(),
            img_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        painter.circle_filled(center, radius - 2.0, cust.avatar_color());
        let initial = cust.display_name.chars().next().unwrap_or('?');
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            initial.to_uppercase().to_string(),
            FontId::proportional(size * 0.44),
            Color32::WHITE,
        );
    }

    if cust.avatar_ring_glow && cust.ring_thickness > 0.1 {
        painter.circle_stroke(
            center,
            radius - 1.0,
            Stroke::new(cust.ring_thickness, cust.ring_color()),
        );
    }

    let dot_r = (size * 0.16).max(4.5);
    let dot_pos = Pos2::new(rect.right() - dot_r * 0.75, rect.bottom() - dot_r * 0.75);
    painter.circle_filled(dot_pos, dot_r + 2.0, Color32::from_rgb(17, 18, 20));
    painter.circle_filled(dot_pos, dot_r, cust.presence.color());
}

/// Desenha um avatar circular simples com a inicial de um participante remoto.
pub fn draw_peer_initial_avatar(ui: &mut Ui, size: f32, name: &str, bg_color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), size * 0.5, bg_color);
    let init: String = name
        .chars()
        .next()
        .unwrap_or('A')
        .to_uppercase()
        .collect();
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        init,
        FontId::proportional(size * 0.5),
        Color32::WHITE,
    );
}
