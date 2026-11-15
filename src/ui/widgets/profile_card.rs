use eframe::egui::{
    self, Color32, FontId, Mesh, Pos2, Rect, RichText, Rounding, Stroke, TextureHandle, Ui, Vec2,
};

use crate::profile_theme::UserCustomization;

use super::avatar::draw_discord_avatar;

/// Desenha o cartão de perfil estilo Discord com banner, avatar, badges, status e bio.
pub fn draw_discord_profile_card(
    ui: &mut Ui,
    cust: &UserCustomization,
    avatar_tex: Option<&TextureHandle>,
    banner_tex: Option<&TextureHandle>,
    is_live: bool,
) {
    let card_width = ui.available_width().min(320.0);
    egui::Frame::none()
        .fill(Color32::from_rgb(17, 18, 20))
        .rounding(Rounding::same(cust.corner_radius.max(8.0)))
        .stroke(Stroke::new(
            cust.border_stroke_width.max(1.0),
            cust.accent_color().gamma_multiply(0.55),
        ))
        .show(ui, |ui| {
            ui.set_width(card_width);

            let (banner_rect, _) =
                ui.allocate_exact_size(Vec2::new(card_width, 72.0), egui::Sense::hover());
            let painter = ui.painter();

            if let Some(btex) = banner_tex {
                painter.image(
                    btex.id(),
                    banner_rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else {
                let mut mesh = Mesh::default();
                let c1 = cust.banner_color();
                let c2 = cust.banner_color_2();
                mesh.colored_vertex(banner_rect.left_top(), c1);
                mesh.colored_vertex(banner_rect.right_top(), c2);
                mesh.colored_vertex(banner_rect.right_bottom(), c2);
                mesh.colored_vertex(banner_rect.left_bottom(), c1);
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                painter.add(egui::Shape::mesh(mesh));
            }

            if is_live {
                let badge_rect = Rect::from_min_size(
                    Pos2::new(banner_rect.right() - 74.0, banner_rect.top() + 10.0),
                    Vec2::new(64.0, 20.0),
                );
                painter.rect_filled(badge_rect, Rounding::same(4.0), Color32::from_rgb(242, 63, 67));
                painter.text(
                    badge_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "AO VIVO",
                    FontId::proportional(10.5),
                    Color32::WHITE,
                );
            }

            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        draw_discord_avatar(ui, 54.0, cust, avatar_tex);

                        ui.vertical(|ui| {
                            let shown_name = if cust.display_name.trim().is_empty() {
                                "Novo Usuário"
                            } else {
                                cust.display_name.as_str()
                            };
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(shown_name)
                                        .strong()
                                        .size(16.0)
                                        .color(cust.name_color()),
                                );
                                ui.label(
                                    RichText::new(&cust.user_tag)
                                        .size(12.0)
                                        .color(Color32::from_rgb(148, 155, 164)),
                                );
                            });

                            let badges = cust.active_badges();
                            if !badges.is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    for (txt, col) in badges {
                                        egui::Frame::none()
                                            .fill(col.gamma_multiply(0.24))
                                            .stroke(Stroke::new(1.0_f32, col))
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::symmetric(5.0, 1.5))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(txt)
                                                        .size(10.0)
                                                        .strong()
                                                        .color(Color32::WHITE),
                                                );
                                            });
                                    }
                                });
                            }
                        });
                    });

                    if !cust.custom_status.trim().is_empty() {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!("💬 {}", cust.custom_status))
                                .size(12.0)
                                .color(Color32::from_rgb(215, 220, 228)),
                        );
                    }

                    if !cust.bio.trim().is_empty() {
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("SOBRE MIM")
                                .size(10.0)
                                .strong()
                                .color(Color32::from_rgb(148, 155, 164)),
                        );
                        ui.label(
                            RichText::new(&cust.bio)
                                .size(11.5)
                                .color(Color32::from_rgb(195, 200, 208)),
                        );
                    }
                });
        });
}
