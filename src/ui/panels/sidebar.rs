use eframe::egui::{self, Color32, Context, RichText, Rounding, Stroke, Vec2};

use crate::app::Disc2App;
use crate::network::{PeerInfo, ServerMemberPresence};
use crate::ui::state::{RoomStatusSnapshot, ServerModalTab};
use crate::ui::widgets::{draw_discord_avatar, draw_peer_initial_avatar};

/// Coluna 2: Painel de salas e membros do servidor selecionado, além dos rodapés de voz e usuário.
///
/// Ordem obrigatória de renderização no `egui`:
/// 1. `render_user_footer` (`TopBottomPanel::bottom`)
/// 2. `render_voice_connected_panel` (`TopBottomPanel::bottom`)
/// 3. `render_server_rooms_and_members` (área restante)
pub fn render_sidebar(app: &mut Disc2App, ctx: &Context, room_snap: &RoomStatusSnapshot) {
    let sidebar_bg = app.custom.sidebar_color();

    egui::SidePanel::left("sidebar_panel")
        .resizable(false)
        .exact_width(268.0)
        .frame(egui::Frame::none().fill(sidebar_bg))
        .show(ctx, |ui| {
            render_user_footer(app, ui, room_snap);
            render_voice_connected_panel(app, ctx, ui, room_snap);
            render_server_rooms_and_members(app, ctx, ui, room_snap);
        });
}

fn render_user_footer(app: &mut Disc2App, ui: &mut egui::Ui, room_snap: &RoomStatusSnapshot) {
    egui::TopBottomPanel::bottom("discord_user_footer")
        .exact_height(60.0)
        .frame(
            egui::Frame::none()
                .fill(Color32::from_rgb(
                    app.custom.bg_sidebar_rgb[0].saturating_sub(8),
                    app.custom.bg_sidebar_rgb[1].saturating_sub(8),
                    app.custom.bg_sidebar_rgb[2].saturating_sub(9),
                ))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
        )
        .show_inside(ui, |ui| {
            ui.horizontal_centered(|ui| {
                draw_discord_avatar(ui, 36.0, &app.custom, app.user_avatar_tex.as_ref());

                let name_resp = ui
                    .vertical(|ui| {
                        ui.label(
                            RichText::new(&app.custom.display_name)
                                .strong()
                                .size(13.0)
                                .color(app.custom.name_color()),
                        );
                        let sub = if app.is_sharing {
                            format!("🔴 Transmitindo {}", app.res_preset.short_label())
                        } else if room_snap.in_room {
                            format!("🟢 Em: {}", room_snap.active_room_name)
                        } else {
                            "⚪ Fora das salas".to_string()
                        };
                        ui.label(
                            RichText::new(sub)
                                .size(10.5)
                                .color(Color32::from_rgb(160, 166, 174)),
                        );
                    })
                    .response
                    .interact(egui::Sense::click());

                if name_resp.clicked() {
                    app.show_profile_popup = !app.show_profile_popup;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("⚙️")
                        .on_hover_text("Configurações de Conta, Foto e Aparência")
                        .clicked()
                    {
                        app.show_settings_modal = true;
                    }
                });
            });
        });
}

fn render_voice_connected_panel(
    app: &mut Disc2App,
    ctx: &Context,
    ui: &mut egui::Ui,
    room_snap: &RoomStatusSnapshot,
) {
    let accent = app.custom.accent_color();

    egui::TopBottomPanel::bottom("discord_voice_connected_panel")
        .exact_height(if app.is_sharing { 114.0 } else { 80.0 })
        .frame(
            egui::Frame::none()
                .fill(Color32::from_rgb(
                    app.custom.bg_sidebar_rgb[0].saturating_sub(4),
                    app.custom.bg_sidebar_rgb[1].saturating_sub(4),
                    app.custom.bg_sidebar_rgb[2].saturating_sub(5),
                ))
                .stroke(Stroke::new(
                    1.0_f32,
                    Color32::from_rgba_unmultiplied(255, 255, 255, 15),
                ))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
        )
        .show_inside(ui, |ui| {
            if room_snap.in_room {
                let conn_srv_name = app
                    .custom
                    .saved_servers
                    .iter()
                    .find(|s| s.server_id.eq_ignore_ascii_case(&room_snap.connected_srv_id))
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| room_snap.connected_srv_id.clone());

                ui.colored_label(
                    Color32::from_rgb(35, 165, 90),
                    RichText::new(format!(
                        "🟢 {} / {}",
                        room_snap.active_room_name, conn_srv_name
                    ))
                    .strong()
                    .size(11.5),
                );
                ui.add_space(5.0);

                ui.columns(2, |cols| {
                    let share_txt = if app.is_sharing {
                        "🔄 Trocar Tela"
                    } else {
                        "🖥️ Compartilhar"
                    };
                    let share_btn = egui::Button::new(
                        RichText::new(share_txt)
                            .size(11.5)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(if app.is_sharing {
                        accent
                    } else {
                        Color32::from_rgb(43, 45, 49)
                    })
                    .min_size(Vec2::new(cols[0].available_width(), 30.0));

                    if cols[0].add(share_btn).clicked() {
                        app.open_share_picker(ctx);
                    }

                    let leave_btn = egui::Button::new(
                        RichText::new("📞 Sair da Sala")
                            .size(11.5)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(218, 55, 60))
                    .min_size(Vec2::new(cols[1].available_width(), 30.0));

                    if cols[1].add(leave_btn).clicked() {
                        app.leave_room(ctx);
                    }
                });

                if app.is_sharing {
                    ui.add_space(4.0);
                    let stop_stream_btn = egui::Button::new(
                        RichText::new("⏹ Parar Transmissão (Continuar na Sala)")
                            .size(11.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(180, 45, 50))
                    .min_size(Vec2::new(ui.available_width(), 24.0));

                    if ui.add(stop_stream_btn).clicked() {
                        app.stop_sharing(ctx);
                    }
                }
            } else {
                ui.colored_label(
                    Color32::from_rgb(148, 155, 164),
                    RichText::new("⚪ Desconectado de Sala de Voz")
                        .strong()
                        .size(11.5),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Clique em qualquer sala do servidor acima para entrar!")
                        .size(10.5)
                        .color(Color32::from_rgb(140, 146, 155)),
                );
            }
        });
}

fn render_server_rooms_and_members(
    app: &mut Disc2App,
    ctx: &Context,
    ui: &mut egui::Ui,
    room_snap: &RoomStatusSnapshot,
) {
    let accent = app.custom.accent_color();
    let card_bg = app.custom.card_color();

    egui::Frame::none()
        .inner_margin(egui::Margin::same(11.0))
        .show(ui, |ui| {
            let selected_srv_opt = app
                .custom
                .saved_servers
                .iter()
                .find(|s| s.server_id.eq_ignore_ascii_case(&app.custom.selected_server_id))
                .cloned()
                .or_else(|| app.custom.saved_servers.first().cloned());

            if let Some(current_srv) = selected_srv_opt {
                if !app
                    .custom
                    .selected_server_id
                    .eq_ignore_ascii_case(&current_srv.server_id)
                {
                    app.custom.selected_server_id = current_srv.server_id.clone();
                    if let Some(hub) = &app.room_hub {
                        hub.register_and_select_server(&current_srv);
                    }
                }

                // Cabeçalho do Servidor Atual + Botão Convidar Amigos
                egui::Frame::none()
                    .fill(card_bg)
                    .rounding(Rounding::same(7.0))
                    .inner_margin(egui::Margin::symmetric(9.0, 7.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(&current_srv.name)
                                        .strong()
                                        .size(14.5)
                                        .color(Color32::WHITE),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "ID: {} • RAM {:.0}MB",
                                        current_srv.server_id, app.ram_mb
                                    ))
                                    .size(10.0)
                                    .color(Color32::from_rgb(148, 155, 164)),
                                );
                            });

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button("🗑️")
                                        .on_hover_text("Sair / Remover este servidor da sua barra")
                                        .clicked()
                                    {
                                        let del_id = current_srv.server_id.clone();
                                        if let Some(hub) = &app.room_hub {
                                            hub.remove_server(&del_id);
                                        }
                                        app.custom
                                            .saved_servers
                                            .retain(|s| !s.server_id.eq_ignore_ascii_case(&del_id));
                                        app.custom.selected_server_id = app
                                            .custom
                                            .saved_servers
                                            .first()
                                            .map(|s| s.server_id.clone())
                                            .unwrap_or_default();
                                        app.custom.save_to_disk();
                                    }

                                    let inv_btn = egui::Button::new(
                                        RichText::new("📨 Convidar")
                                            .size(11.0)
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(accent);

                                    if ui
                                        .add(inv_btn)
                                        .on_hover_text(
                                            "Copiar o Convite deste Servidor para mandar aos seus amigos!",
                                        )
                                        .clicked()
                                    {
                                        let token = current_srv.invite_token();
                                        ctx.copy_text(token);
                                        app.show_toast(format!(
                                            "Convite de '{}' copiado! Mande para os amigos deste servidor.",
                                            current_srv.name
                                        ));
                                    }
                                },
                            );
                        });
                    });

                ui.add_space(8.0);

                // Criar nova sala dentro deste Servidor específico
                ui.label(
                    RichText::new("SALAS DESTE SERVIDOR")
                        .size(10.0)
                        .strong()
                        .color(Color32::from_rgb(148, 155, 164)),
                );
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut app.new_room_name_input)
                            .hint_text("➕ Nova sala neste servidor...")
                            .desired_width(178.0),
                    );
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let add_btn = egui::Button::new(
                        RichText::new("➕ Criar")
                            .size(11.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(accent);

                    if (ui.add(add_btn).clicked() || enter)
                        && !app.new_room_name_input.trim().is_empty()
                    {
                        let rname = app.new_room_name_input.trim().to_string();
                        app.new_room_name_input.clear();
                        if let Some(hub) = &app.room_hub {
                            let updated =
                                hub.create_and_enter_room(&current_srv.server_id, &rname);
                            if let Some(srv_mut) = app
                                .custom
                                .saved_servers
                                .iter_mut()
                                .find(|s| s.server_id.eq_ignore_ascii_case(&current_srv.server_id))
                            {
                                srv_mut.rooms = updated;
                            }
                            app.custom.save_to_disk();
                        }
                        app.show_toast(format!(
                            "Sala '{}' criada no servidor '{}'!",
                            rname, current_srv.name
                        ));
                    }
                });

                ui.add_space(6.0);

                let mut peers_in_my_room: Vec<PeerInfo> = app
                    .room_hub
                    .as_ref()
                    .and_then(|h| {
                        h.peers
                            .lock()
                            .ok()
                            .map(|m| m.values().cloned().collect())
                    })
                    .unwrap_or_default();
                peers_in_my_room.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

                let server_members_list: Vec<ServerMemberPresence> = app
                    .room_hub
                    .as_ref()
                    .and_then(|h| {
                        h.server_members
                            .lock()
                            .ok()
                            .map(|m| m.values().cloned().collect())
                    })
                    .unwrap_or_default();

                let mut room_to_join: Option<String> = None;
                let mut room_to_delete: Option<String> = None;

                egui::ScrollArea::vertical()
                    .id_salt("server_rooms_scroll")
                    .max_height(355.0)
                    .show(ui, |ui| {
                        for room_name in &current_srv.rooms {
                            let is_my_current_room = room_snap.in_room
                                && room_snap
                                    .connected_srv_id
                                    .eq_ignore_ascii_case(&current_srv.server_id)
                                && room_snap.active_room_name.eq_ignore_ascii_case(room_name);

                            let mut room_members: Vec<(String, bool, String, bool)> = Vec::new();

                            if is_my_current_room {
                                room_members.push((
                                    format!("{} (Você)", app.custom.display_name),
                                    app.is_sharing,
                                    app.res_preset.short_label().to_string(),
                                    true,
                                ));
                                for p in &peers_in_my_room {
                                    room_members.push((
                                        p.name.clone(),
                                        p.is_sharing,
                                        p.res_label.clone(),
                                        false,
                                    ));
                                }
                                for sm in &server_members_list {
                                    if sm
                                        .server_id
                                        .eq_ignore_ascii_case(&current_srv.server_id)
                                        && sm.current_room.eq_ignore_ascii_case(room_name)
                                        && !peers_in_my_room.iter().any(|p| {
                                            p.peer_id == sm.peer_id
                                                || p.name.eq_ignore_ascii_case(&sm.name)
                                        })
                                        && !sm.name.eq_ignore_ascii_case(&app.custom.display_name)
                                    {
                                        room_members.push((
                                            sm.name.clone(),
                                            sm.is_sharing,
                                            sm.res_label.clone(),
                                            false,
                                        ));
                                    }
                                }
                            } else {
                                for sm in &server_members_list {
                                    if sm
                                        .server_id
                                        .eq_ignore_ascii_case(&current_srv.server_id)
                                        && sm.current_room.eq_ignore_ascii_case(room_name)
                                    {
                                        room_members.push((
                                            sm.name.clone(),
                                            sm.is_sharing,
                                            sm.res_label.clone(),
                                            false,
                                        ));
                                    }
                                }
                            }

                            let header_fill = if is_my_current_room {
                                Color32::from_rgba_unmultiplied(88, 101, 242, 42)
                            } else {
                                card_bg
                            };
                            let header_stroke = if is_my_current_room {
                                Stroke::new(1.2_f32, accent)
                            } else {
                                Stroke::new(
                                    1.0_f32,
                                    Color32::from_rgba_unmultiplied(255, 255, 255, 12),
                                )
                            };

                            let channel_resp = egui::Frame::none()
                                .fill(header_fill)
                                .stroke(header_stroke)
                                .rounding(Rounding::same(6.0))
                                .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        let icon = if is_my_current_room {
                                            "🟢 🔊"
                                        } else {
                                            "🔊"
                                        };
                                        ui.label(
                                            RichText::new(format!("{} {}", icon, room_name))
                                                .strong()
                                                .size(12.5)
                                                .color(Color32::WHITE),
                                        );

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if current_srv.rooms.len() > 1 && !is_my_current_room
                                                {
                                                    if ui
                                                        .small_button("✕")
                                                        .on_hover_text(
                                                            "Remover esta sala do servidor",
                                                        )
                                                        .clicked()
                                                    {
                                                        room_to_delete = Some(room_name.clone());
                                                    }
                                                }

                                                if !is_my_current_room {
                                                    if ui.small_button("Entrar").clicked() {
                                                        room_to_join = Some(room_name.clone());
                                                    }
                                                } else {
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "{} na sala",
                                                            room_members.len()
                                                        ))
                                                        .size(9.5)
                                                        .color(Color32::from_rgb(134, 239, 172)),
                                                    );
                                                }
                                            },
                                        );
                                    });
                                })
                                .response
                                .interact(egui::Sense::click());

                            if channel_resp.clicked()
                                && !is_my_current_room
                                && room_to_delete.is_none()
                            {
                                room_to_join = Some(room_name.clone());
                            }

                            for (m_name, m_sharing, m_res, is_me) in room_members {
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(12.0);
                                    egui::Frame::none()
                                        .fill(card_bg.linear_multiply(0.92))
                                        .rounding(Rounding::same(5.0))
                                        .inner_margin(egui::Margin::symmetric(7.0, 4.0))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                if is_me {
                                                    draw_discord_avatar(
                                                        ui,
                                                        22.0,
                                                        &app.custom,
                                                        app.user_avatar_tex.as_ref(),
                                                    );
                                                } else {
                                                    draw_peer_initial_avatar(
                                                        ui, 22.0, &m_name, accent,
                                                    );
                                                }

                                                ui.vertical(|ui| {
                                                    ui.label(
                                                        RichText::new(&m_name)
                                                            .strong()
                                                            .size(11.0)
                                                            .color(if is_me {
                                                                app.custom.name_color()
                                                            } else {
                                                                Color32::WHITE
                                                            }),
                                                    );
                                                    if m_sharing {
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "Transmitindo ({})",
                                                                m_res
                                                            ))
                                                            .size(9.0)
                                                            .color(Color32::from_rgb(
                                                                134, 239, 172,
                                                            )),
                                                        );
                                                    }
                                                });

                                                if m_sharing {
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            egui::Frame::none()
                                                                .fill(Color32::from_rgb(
                                                                    242, 63, 67,
                                                                ))
                                                                .rounding(Rounding::same(4.0))
                                                                .inner_margin(
                                                                    egui::Margin::symmetric(
                                                                        4.0, 1.0,
                                                                    ),
                                                                )
                                                                .show(ui, |ui| {
                                                                    ui.label(
                                                                        RichText::new("LIVE")
                                                                            .size(8.5)
                                                                            .strong()
                                                                            .color(Color32::WHITE),
                                                                    );
                                                                });
                                                        },
                                                    );
                                                }
                                            });
                                        });
                                });
                            }

                            ui.add_space(5.0);
                        }
                    });

                if let Some(del_room) = room_to_delete {
                    if let Some(srv_mut) = app
                        .custom
                        .saved_servers
                        .iter_mut()
                        .find(|s| s.server_id.eq_ignore_ascii_case(&current_srv.server_id))
                    {
                        if srv_mut.rooms.len() > 1 {
                            srv_mut.rooms.retain(|r| !r.eq_ignore_ascii_case(&del_room));
                            if let Some(hub) = &app.room_hub {
                                if let Ok(mut map) = hub.server_rooms_map.lock() {
                                    map.insert(
                                        srv_mut.server_id.to_uppercase(),
                                        srv_mut.rooms.clone(),
                                    );
                                }
                            }
                            app.custom.save_to_disk();
                        }
                    }
                } else if let Some(target_room) = room_to_join {
                    if let Some(hub) = &app.room_hub {
                        hub.enter_server_room(&current_srv.server_id, &target_room);
                    }
                    app.show_toast(format!(
                        "Conectado em '{}' ({})",
                        target_room, current_srv.name
                    ));
                }
            } else {
                // Estado inicial quando o usuário ainda não entrou nem criou nenhum servidor!
                ui.vertical_centered(|ui| {
                    ui.add_space(36.0);
                    ui.colored_label(
                        accent,
                        RichText::new("🌐 Nenhum Servidor").size(16.0).strong(),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "Crie o seu próprio Servidor para um grupo de amigos ou entre usando o convite de um amigo!",
                        )
                        .size(12.0)
                        .color(Color32::from_rgb(165, 170, 180)),
                    );
                    ui.add_space(16.0);

                    let create_btn = egui::Button::new(
                        RichText::new("🚀 Criar Novo Servidor")
                            .size(13.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(35, 165, 90))
                    .min_size(Vec2::new(ui.available_width(), 36.0));

                    if ui.add(create_btn).clicked() {
                        app.server_modal_tab = ServerModalTab::CreateServer;
                        app.show_server_modal = true;
                    }

                    ui.add_space(8.0);

                    let join_btn = egui::Button::new(
                        RichText::new("🔑 Entrar com Convite")
                            .size(13.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(accent)
                    .min_size(Vec2::new(ui.available_width(), 36.0));

                    if ui.add(join_btn).clicked() {
                        app.server_modal_tab = ServerModalTab::JoinServer;
                        app.show_server_modal = true;
                    }
                });
            }
        });
}
