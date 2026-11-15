use eframe::egui::{self, Color32, Context, FontId, Rounding, Vec2};

use crate::app::Disc2App;
use crate::profile_theme::ServerProfile;
use crate::ui::state::{RoomStatusSnapshot, ServerModalTab};

/// Coluna 1: Barra vertical esquerda com os ícones de Servidores e botões de criar/entrar.
pub fn render_server_rail(
    app: &mut Disc2App,
    ctx: &Context,
    room_snap: &RoomStatusSnapshot,
) {
    let accent = app.custom.accent_color();
    let card_bg = app.custom.card_color();
    let rail_bg = Color32::from_rgb(
        app.custom.bg_sidebar_rgb[0].saturating_sub(11),
        app.custom.bg_sidebar_rgb[1].saturating_sub(11),
        app.custom.bg_sidebar_rgb[2].saturating_sub(12),
    );

    egui::SidePanel::left("discord_server_rail")
        .resizable(false)
        .exact_width(68.0)
        .frame(
            egui::Frame::none()
                .fill(rail_bg)
                .inner_margin(egui::Margin::symmetric(8.0, 10.0)),
        )
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                // Lista de Servidores em que o usuário participa
                let mut select_srv_action: Option<ServerProfile> = None;

                egui::ScrollArea::vertical()
                    .id_salt("servers_rail_scroll")
                    .max_height(ui.available_height() - 115.0)
                    .show(ui, |ui| {
                        for srv in &app.custom.saved_servers {
                            let is_selected = srv
                                .server_id
                                .eq_ignore_ascii_case(&app.custom.selected_server_id);
                            let is_voice_here = room_snap.in_room
                                && room_snap
                                    .connected_srv_id
                                    .eq_ignore_ascii_case(&srv.server_id);

                            let (item_rect, resp) = ui.allocate_exact_size(
                                Vec2::new(52.0, 50.0),
                                egui::Sense::click(),
                            );
                            let icon_center = item_rect.center();
                            let icon_rect =
                                egui::Rect::from_center_size(icon_center, Vec2::splat(44.0));

                            // Barrinha branca lateral igual ao Discord
                            if is_selected {
                                let pill = egui::Rect::from_min_size(
                                    egui::pos2(item_rect.left() - 4.0, icon_center.y - 16.0),
                                    Vec2::new(4.0, 32.0),
                                );
                                ui.painter()
                                    .rect_filled(pill, Rounding::same(2.0), Color32::WHITE);
                            } else if resp.hovered() {
                                let pill = egui::Rect::from_min_size(
                                    egui::pos2(item_rect.left() - 4.0, icon_center.y - 9.0),
                                    Vec2::new(4.0, 18.0),
                                );
                                ui.painter().rect_filled(
                                    pill,
                                    Rounding::same(2.0),
                                    Color32::from_rgb(200, 205, 212),
                                );
                            }

                            let srv_col = Color32::from_rgb(
                                srv.icon_rgb[0],
                                srv.icon_rgb[1],
                                srv.icon_rgb[2],
                            );
                            let rounding = if is_selected || resp.hovered() {
                                Rounding::same(14.0)
                            } else {
                                Rounding::same(22.0)
                            };

                            ui.painter().rect_filled(icon_rect, rounding, srv_col);

                            // Sigla do Servidor (ex: "TV" para "Tropa do Valorant")
                            let initials: String = srv
                                .name
                                .split_whitespace()
                                .filter_map(|w| w.chars().next())
                                .take(2)
                                .collect::<String>()
                                .to_uppercase();
                            let label_txt = if initials.is_empty() {
                                "SZ".to_string()
                            } else {
                                initials
                            };
                            ui.painter().text(
                                icon_center,
                                egui::Align2::CENTER_CENTER,
                                label_txt,
                                FontId::proportional(14.5),
                                Color32::WHITE,
                            );

                            // Bolinha verde indicando que você está conectado numa sala deste servidor
                            if is_voice_here {
                                let badge_pos =
                                    egui::pos2(icon_rect.right() - 4.0, icon_rect.bottom() - 4.0);
                                ui.painter().circle_filled(badge_pos, 6.5, rail_bg);
                                ui.painter().circle_filled(
                                    badge_pos,
                                    4.5,
                                    Color32::from_rgb(35, 165, 90),
                                );
                            }

                            let clicked = resp
                                .on_hover_text(format!(
                                    "{}\nCódigo: {}\n({} salas)",
                                    srv.name,
                                    srv.server_id,
                                    srv.rooms.len()
                                ))
                                .clicked();
                            if clicked {
                                select_srv_action = Some(srv.clone());
                            }

                            ui.add_space(4.0);
                        }
                    });

                if let Some(chosen) = select_srv_action {
                    app.custom.selected_server_id = chosen.server_id.clone();
                    app.custom.save_to_disk();
                    if let Some(hub) = &app.room_hub {
                        hub.register_and_select_server(&chosen);
                    }
                }

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(6.0);

                // Botão Redondo "+" para Criar um Servidor Novo ou Entrar com Convite!
                let (add_rect, add_resp) =
                    ui.allocate_exact_size(Vec2::splat(44.0), egui::Sense::click());
                let add_bg = if add_resp.hovered() {
                    Color32::from_rgb(35, 165, 90)
                } else {
                    card_bg
                };
                let add_fg = if add_resp.hovered() {
                    Color32::WHITE
                } else {
                    Color32::from_rgb(35, 165, 90)
                };
                ui.painter().rect_filled(
                    add_rect,
                    if add_resp.hovered() {
                        Rounding::same(14.0)
                    } else {
                        Rounding::same(22.0)
                    },
                    add_bg,
                );
                ui.painter().text(
                    add_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "➕",
                    FontId::proportional(18.0),
                    add_fg,
                );
                if add_resp
                    .on_hover_text("Criar um Novo Servidor ou Entrar em um Servidor com Convite")
                    .clicked()
                {
                    app.server_modal_tab = ServerModalTab::CreateServer;
                    app.show_server_modal = true;
                }

                ui.add_space(6.0);

                // Botão Redondo "🔑" direto para Entrar em Servidor com Convite
                let (join_rect, join_resp) =
                    ui.allocate_exact_size(Vec2::splat(40.0), egui::Sense::click());
                ui.painter().rect_filled(
                    join_rect,
                    if join_resp.hovered() {
                        Rounding::same(13.0)
                    } else {
                        Rounding::same(20.0)
                    },
                    if join_resp.hovered() { accent } else { card_bg },
                );
                ui.painter().text(
                    join_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "🔑",
                    FontId::proportional(15.0),
                    Color32::WHITE,
                );
                if join_resp
                    .on_hover_text("Entrar em um Servidor usando o Convite de um Amigo")
                    .clicked()
                {
                    app.server_modal_tab = ServerModalTab::JoinServer;
                    app.show_server_modal = true;
                }
            });
        });
}
