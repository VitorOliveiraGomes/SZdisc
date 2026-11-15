use std::sync::Arc;

use eframe::egui::{self, Color32, Context, FontId, RichText, Vec2};

use crate::app::Disc2App;
use crate::ui::state::{
    RoomStatusSnapshot, SettingsTab, TileUiActions, LOCAL_PREVIEW_TILE_ID,
};
use crate::ui::widgets::{draw_discord_profile_card, render_stream_tile};

/// Palco central de vídeo: exibe o grid de transmissões, foco, tela cheia e estados vazios.
pub fn render_stage(app: &mut Disc2App, ctx: &Context, room_snap: &RoomStatusSnapshot) {
    let accent = app.custom.accent_color();
    let stage_bg = app.custom.stage_color();
    let card_bg = app.custom.card_color();

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(stage_bg))
        .show(ctx, |ui| {
            let avail = ui.available_size();

            let include_local_tile = app.is_sharing
                && !app.eco_hide_preview
                && (app.remote_tiles.is_empty()
                    || app.show_own_preview_in_grid
                    || app.fullscreen_tile_id == Some(LOCAL_PREVIEW_TILE_ID));

            // Mantém a ordem escolhida pelo usuário (`app.tile_order`) para permitir mover as telas de lugar!
            let mut raw_active: Vec<u32> = app
                .remote_tiles
                .keys()
                .copied()
                .filter(|tid| !app.popout_tiles.contains(tid))
                .collect();
            raw_active.sort_unstable();
            if include_local_tile && !app.popout_tiles.contains(&LOCAL_PREVIEW_TILE_ID) {
                raw_active.push(LOCAL_PREVIEW_TILE_ID);
            }

            app.tile_order.retain(|id| raw_active.contains(id));
            for id in &raw_active {
                if !app.tile_order.contains(id) {
                    app.tile_order.push(*id);
                }
            }

            let active_tile_ids = app.tile_order.clone();
            let total_tiles = active_tile_ids.len();

            if total_tiles > 0 {
                let is_fullscreen_mode = app.fullscreen_tile_id.is_some();
                let visible_ids: Vec<u32> = if let Some(fs_id) = app.fullscreen_tile_id {
                    if active_tile_ids.contains(&fs_id) {
                        vec![fs_id]
                    } else {
                        active_tile_ids.clone()
                    }
                } else if let Some(foc) = app.focused_tile_id {
                    if active_tile_ids.contains(&foc) {
                        vec![foc]
                    } else {
                        app.focused_tile_id = None;
                        active_tile_ids.clone()
                    }
                } else {
                    active_tile_ids.clone()
                };

                let stage_rect = if is_fullscreen_mode {
                    ui.available_rect_before_wrap()
                } else {
                    ui.available_rect_before_wrap().shrink(8.0)
                };
                let gap = 8.0;
                let count = visible_ids.len();

                let mut tile_rects: Vec<(u32, egui::Rect)> = Vec::with_capacity(count);
                if count == 1 {
                    tile_rects.push((visible_ids[0], stage_rect));
                } else if count == 2 {
                    let w = (stage_rect.width() - gap) * 0.5;
                    let r0 = egui::Rect::from_min_size(
                        stage_rect.min,
                        Vec2::new(w, stage_rect.height()),
                    );
                    let r1 = egui::Rect::from_min_size(
                        egui::pos2(stage_rect.min.x + w + gap, stage_rect.min.y),
                        Vec2::new(w, stage_rect.height()),
                    );
                    tile_rects.push((visible_ids[0], r0));
                    tile_rects.push((visible_ids[1], r1));
                } else if count == 3 {
                    let h = (stage_rect.height() - gap) * 0.5;
                    let w_top = (stage_rect.width() - gap) * 0.5;
                    let r0 = egui::Rect::from_min_size(stage_rect.min, Vec2::new(w_top, h));
                    let r1 = egui::Rect::from_min_size(
                        egui::pos2(stage_rect.min.x + w_top + gap, stage_rect.min.y),
                        Vec2::new(w_top, h),
                    );
                    let w_bot = (stage_rect.width() * 0.62).max(w_top);
                    let x_bot = stage_rect.min.x + (stage_rect.width() - w_bot) * 0.5;
                    let r2 = egui::Rect::from_min_size(
                        egui::pos2(x_bot, stage_rect.min.y + h + gap),
                        Vec2::new(w_bot, h),
                    );
                    tile_rects.push((visible_ids[0], r0));
                    tile_rects.push((visible_ids[1], r1));
                    tile_rects.push((visible_ids[2], r2));
                } else {
                    let cols = if count <= 4 { 2 } else { 3 };
                    let rows = (count + cols - 1) / cols;
                    let cell_w =
                        (stage_rect.width() - gap * (cols as f32 - 1.0)) / cols as f32;
                    let cell_h =
                        (stage_rect.height() - gap * (rows as f32 - 1.0)) / rows as f32;

                    for (idx, tid) in visible_ids.iter().enumerate() {
                        let c = idx % cols;
                        let r = idx / cols;
                        let min_pos = egui::pos2(
                            stage_rect.min.x + c as f32 * (cell_w + gap),
                            stage_rect.min.y + r as f32 * (cell_h + gap),
                        );
                        tile_rects.push((
                            *tid,
                            egui::Rect::from_min_size(min_pos, Vec2::new(cell_w, cell_h)),
                        ));
                    }
                }

                let mut actions = TileUiActions::default();

                for (tid, rect) in tile_rects {
                    let is_focused = app.focused_tile_id == Some(tid);
                    let is_fs = app.fullscreen_tile_id == Some(tid);
                    if tid == LOCAL_PREVIEW_TILE_ID {
                        let title = format!("{} (Sua Tela)", app.custom.display_name);
                        let sub = format!(
                            "{}×{} • {} FPS • {:.1} Mbps",
                            app.local_video_size.0,
                            app.local_video_size.1,
                            app.local_measured_fps,
                            app.local_measured_mbps
                        );
                        render_stream_tile(
                            ui,
                            rect,
                            tid,
                            &title,
                            &sub,
                            app.local_video_texture.as_ref(),
                            None,
                            None,
                            is_focused,
                            is_fs,
                            total_tiles,
                            card_bg,
                            accent,
                            &mut actions,
                        );
                    } else if let Some(tile) = app.remote_tiles.get(&tid) {
                        let peer_name = app
                            .room_hub
                            .as_ref()
                            .and_then(|h| {
                                h.peers
                                    .lock()
                                    .ok()
                                    .and_then(|m| m.get(&tid).map(|p| p.name.clone()))
                            })
                            .unwrap_or_else(|| format!("Amigo #{}", tid % 1000));

                        let (vol_arc, mute_arc) = app
                            .room_hub
                            .as_ref()
                            .and_then(|h| {
                                h.remote_streams.lock().ok().and_then(|m| {
                                    m.get(&tid).map(|s| {
                                        (Arc::clone(&s.volume_percent), Arc::clone(&s.muted))
                                    })
                                })
                            })
                            .unzip();

                        let sub = format!(
                            "{}×{} • {} FPS • {:.1} Mbps",
                            tile.video_size.0,
                            tile.video_size.1,
                            tile.measured_fps,
                            tile.measured_mbps
                        );

                        render_stream_tile(
                            ui,
                            rect,
                            tid,
                            &peer_name,
                            &sub,
                            Some(&tile.texture),
                            vol_arc.as_ref(),
                            mute_arc.as_ref(),
                            is_focused,
                            is_fs,
                            total_tiles,
                            card_bg,
                            accent,
                            &mut actions,
                        );
                    }
                }

                if actions.exit_fullscreen {
                    app.set_main_fullscreen(ctx, None);
                } else if let Some(fs_tid) = actions.enter_fullscreen {
                    app.set_main_fullscreen(ctx, Some(fs_tid));
                }

                if actions.unfocus {
                    app.focused_tile_id = None;
                } else if let Some(fid) = actions.focus_tile {
                    app.focused_tile_id = Some(fid);
                }

                if let Some(move_tid) = actions.move_tile {
                    if let Some(pos) = app.tile_order.iter().position(|&x| x == move_tid) {
                        let next_pos = (pos + 1) % app.tile_order.len();
                        app.tile_order.swap(pos, next_pos);
                    }
                }

                if let Some(pop_tid) = actions.popout_to_monitor {
                    if app.fullscreen_tile_id == Some(pop_tid) {
                        app.set_main_fullscreen(ctx, None);
                    }
                    app.popout_tiles.insert(pop_tid);
                    app.show_toast(
                        "Janela destacada! Arraste-a para outro monitor ou deixe em tela cheia lá.",
                    );
                }
            } else if !app.popout_tiles.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(avail.y * 0.33);
                    ui.colored_label(
                        accent,
                        RichText::new("🖥️ Transmissões abertas em janelas separadas (Outro Monitor)")
                            .size(17.0)
                            .strong(),
                    );
                    ui.add_space(10.0);
                    if ui
                        .button("📥 Trazer Todas as Telas de Volta para Este Palco")
                        .clicked()
                    {
                        app.popout_tiles.clear();
                        app.popout_fullscreen.clear();
                    }
                });
            } else if app.is_sharing && app.eco_hide_preview {
                ui.vertical_centered(|ui| {
                    ui.add_space(avail.y * 0.33);
                    ui.colored_label(
                        Color32::from_rgb(35, 165, 90),
                        RichText::new("● TRANSMITINDO AO VIVO NA SALA (MODO JOGO ATIVO)")
                            .size(16.5)
                            .strong(),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "A prévia local está oculta para economizar recursos enquanto você joga.",
                        )
                        .size(13.0)
                        .color(Color32::from_rgb(181, 186, 193)),
                    );
                    ui.add_space(10.0);
                    if ui.button("Mostrar Minha Tela Novamente").clicked() {
                        app.eco_hide_preview = false;
                        app.sync_live_config();
                    }
                });
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space((avail.y * 0.12).max(14.0));
                    draw_discord_profile_card(
                        ui,
                        &app.custom,
                        app.user_avatar_tex.as_ref(),
                        app.user_banner_tex.as_ref(),
                        false,
                    );

                    ui.add_space(18.0);
                    ui.label(
                        RichText::new(if room_snap.in_room {
                            format!("Conectado na sala: {}", room_snap.active_room_name)
                        } else if app.custom.saved_servers.is_empty() {
                            "Crie um Servidor ou Entre com Convite na barra esquerda!".to_string()
                        } else {
                            "Selecione um Servidor e clique em uma Sala na esquerda".to_string()
                        })
                        .font(FontId::proportional(19.0))
                        .strong()
                        .color(Color32::WHITE),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "Você pode ter vários Servidores diferentes na coluna esquerda (cada um com suas próprias salas e amigos).\nBasta enviar o convite do servidor 1 vez para seus amigos e ele ficará salvo no app deles!",
                        )
                        .size(12.5)
                        .color(Color32::from_rgb(160, 166, 174)),
                    );
                    ui.add_space(14.0);

                    ui.horizontal(|ui| {
                        let pad = ((ui.available_width() - 560.0) * 0.5).max(0.0);
                        ui.add_space(pad);

                        let srv_btn = egui::Button::new(
                            RichText::new("🌐 Criar / Entrar em Servidor")
                                .size(13.5)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(35, 165, 90))
                        .min_size(Vec2::new(205.0, 38.0));

                        if ui.add(srv_btn).clicked() {
                            app.show_server_modal = true;
                        }

                        let share_hero = egui::Button::new(
                            RichText::new("🖥️ Compartilhar Tela")
                                .size(13.5)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(accent)
                        .min_size(Vec2::new(190.0, 38.0));

                        if ui.add(share_hero).clicked() {
                            app.open_share_picker(ctx);
                        }

                        let cust_btn = egui::Button::new(
                            RichText::new("📷 Conta & Perfil")
                                .size(13.5)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(card_bg)
                        .min_size(Vec2::new(145.0, 38.0));

                        if ui.add(cust_btn).clicked() {
                            app.settings_tab = SettingsTab::Profile;
                            app.show_settings_modal = true;
                        }
                    });
                });
            }
        });
}
