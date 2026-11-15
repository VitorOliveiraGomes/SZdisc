use std::sync::atomic::Ordering;
use std::sync::Arc;

use eframe::egui::{self, Color32, Context, RichText, Vec2};

use crate::app::Disc2App;
use crate::ui::state::LOCAL_PREVIEW_TILE_ID;

/// Renderiza janelas nativas independentes para transmissões destacadas em Outros Monitores!
pub fn render_popout_windows(app: &mut Disc2App, ctx: &Context) {
    if app.popout_tiles.is_empty() {
        return;
    }

    let popout_ids: Vec<u32> = app.popout_tiles.iter().copied().collect();
    let card_bg = app.custom.card_color();
    let accent = app.custom.accent_color();
    let mut to_dock_back = Vec::new();
    let mut toggle_fs_for = Vec::new();

    for tid in popout_ids {
        let (title, subtitle, tex_opt, vol_arc, mute_arc) = if tid == LOCAL_PREVIEW_TILE_ID {
            if !app.is_sharing {
                to_dock_back.push(tid);
                continue;
            }
            (
                format!("{} (Sua Tela)", app.custom.display_name),
                format!(
                    "{}×{} • {} FPS • {:.1} Mbps",
                    app.local_video_size.0,
                    app.local_video_size.1,
                    app.local_measured_fps,
                    app.local_measured_mbps
                ),
                app.local_video_texture.clone(),
                None,
                None,
            )
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

            let (v, m) = app
                .room_hub
                .as_ref()
                .and_then(|h| {
                    h.remote_streams.lock().ok().and_then(|map| {
                        map.get(&tid)
                            .map(|s| (Arc::clone(&s.volume_percent), Arc::clone(&s.muted)))
                    })
                })
                .unzip();

            (
                peer_name,
                format!(
                    "{}×{} • {} FPS • {:.1} Mbps",
                    tile.video_size.0, tile.video_size.1, tile.measured_fps, tile.measured_mbps
                ),
                Some(tile.texture.clone()),
                v,
                m,
            )
        } else {
            to_dock_back.push(tid);
            continue;
        };

        let is_popout_fs = app.popout_fullscreen.contains(&tid);
        let viewport_id = egui::ViewportId::from_hash_of(("szdisc_popout_monitor", tid));
        let builder = egui::ViewportBuilder::default()
            .with_title(format!("SZdisc — {} (Arraste para Outro Monitor)", title))
            .with_inner_size([960.0, 580.0])
            .with_min_inner_size([420.0, 260.0])
            .with_fullscreen(is_popout_fs);

        let mut dock_clicked = false;
        let mut fs_clicked = false;

        ctx.show_viewport_immediate(viewport_id, builder, |v_ctx, _class| {
            if v_ctx.input(|i| i.viewport().close_requested()) {
                dock_clicked = true;
            }
            if v_ctx.input(|i| i.key_pressed(egui::Key::F11))
                || (is_popout_fs && v_ctx.input(|i| i.key_pressed(egui::Key::Escape)))
            {
                fs_clicked = true;
            }

            egui::TopBottomPanel::bottom(format!("popout_bar_{}", tid))
                .exact_height(40.0)
                .frame(
                    egui::Frame::none()
                        .fill(card_bg)
                        .inner_margin(egui::Margin::symmetric(12.0, 6.0)),
                )
                .show(v_ctx, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.colored_label(
                            Color32::from_rgb(242, 63, 67),
                            RichText::new("● AO VIVO").size(11.5).strong(),
                        );
                        ui.label(
                            RichText::new(&title)
                                .size(13.0)
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.label(
                            RichText::new(&subtitle)
                                .size(11.5)
                                .color(Color32::from_rgb(165, 170, 180)),
                        );

                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let back_btn = egui::Button::new(
                                    RichText::new("📥 Voltar ao Palco Principal")
                                        .size(11.5)
                                        .strong()
                                        .color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(55, 60, 70));
                                if ui.add(back_btn).clicked() {
                                    dock_clicked = true;
                                }

                                let fs_lbl = if is_popout_fs {
                                    "🗗 Sair da Tela Cheia (ESC)"
                                } else {
                                    "⛶ Tela Cheia neste Monitor"
                                };
                                let fs_btn = egui::Button::new(
                                    RichText::new(fs_lbl)
                                        .size(11.5)
                                        .strong()
                                        .color(Color32::WHITE),
                                )
                                .fill(if is_popout_fs {
                                    Color32::from_rgb(218, 55, 60)
                                } else {
                                    accent
                                });
                                if ui.add(fs_btn).clicked() {
                                    fs_clicked = true;
                                }

                                if let (Some(vol), Some(mute)) =
                                    (vol_arc.as_ref(), mute_arc.as_ref())
                                {
                                    let mut v_val = vol.load(Ordering::Relaxed);
                                    if ui
                                        .add(
                                            egui::Slider::new(&mut v_val, 0..=150)
                                                .suffix("%")
                                                .show_value(true),
                                        )
                                        .changed()
                                    {
                                        vol.store(v_val, Ordering::Relaxed);
                                    }
                                    let mut m_val = mute.load(Ordering::Relaxed);
                                    if ui
                                        .small_button(if m_val { "🔇" } else { "🔊" })
                                        .clicked()
                                    {
                                        m_val = !m_val;
                                        mute.store(m_val, Ordering::Relaxed);
                                    }
                                }
                            },
                        );
                    });
                });

            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(Color32::from_rgb(8, 9, 11)))
                .show(v_ctx, |ui| {
                    let rect = ui.available_rect_before_wrap();
                    if let Some(tex) = &tex_opt {
                        let ts = tex.size_vec2();
                        if ts.x > 0.0 && ts.y > 0.0 {
                            let scale = (rect.width() / ts.x).min(rect.height() / ts.y);
                            let draw_size = Vec2::new(ts.x * scale, ts.y * scale);
                            let img_rect =
                                egui::Rect::from_center_size(rect.center(), draw_size);
                            ui.painter().image(
                                tex.id(),
                                img_rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                Color32::WHITE,
                            );
                        }
                    }
                    let resp = ui.interact(
                        rect,
                        egui::Id::new(("popout_dblclick", tid)),
                        egui::Sense::click(),
                    );
                    if resp.double_clicked() {
                        fs_clicked = true;
                    }
                });
        });

        if dock_clicked {
            to_dock_back.push(tid);
        } else if fs_clicked {
            toggle_fs_for.push(tid);
        }
    }

    for tid in to_dock_back {
        app.popout_tiles.remove(&tid);
        app.popout_fullscreen.remove(&tid);
    }
    for tid in toggle_fs_for {
        if app.popout_fullscreen.contains(&tid) {
            app.popout_fullscreen.remove(&tid);
        } else {
            app.popout_fullscreen.insert(tid);
        }
    }
}
