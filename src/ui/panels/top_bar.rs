use std::sync::Arc;

use eframe::egui::{self, Color32, Context, RichText, Rounding, Stroke};

use crate::app::Disc2App;
use crate::ui::state::ResolutionPreset;
use crate::updater::{
    check_for_updates_async, download_and_apply_update_async, UpdateStatus, CURRENT_VERSION,
};

/// Barra superior do palco principal com qualidade, FPS, bitrate/JPEG, Modo Turbo, pop-outs e auto-updater.
pub fn render_top_bar(app: &mut Disc2App, ctx: &Context) {
    let card_bg = app.custom.card_color();

    egui::TopBottomPanel::top("stage_top_bar")
        .exact_height(46.0)
        .frame(
            egui::Frame::none()
                .fill(card_bg)
                .inner_margin(egui::Margin::symmetric(12.0, 7.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                if app.is_sharing || !app.remote_tiles.is_empty() {
                    egui::Frame::none()
                        .fill(Color32::from_rgb(242, 63, 67))
                        .rounding(Rounding::same(4.0))
                        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("AO VIVO")
                                    .size(11.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                        });
                }

                ui.label(RichText::new("Sua Qualidade:").size(11.5));
                let prev_res = app.res_preset;
                egui::ComboBox::from_id_salt("combo_res")
                    .selected_text(app.res_preset.short_label())
                    .width(70.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut app.res_preset,
                            ResolutionPreset::P1080,
                            ResolutionPreset::P1080.label(),
                        );
                        ui.selectable_value(
                            &mut app.res_preset,
                            ResolutionPreset::P900,
                            ResolutionPreset::P900.label(),
                        );
                        ui.selectable_value(
                            &mut app.res_preset,
                            ResolutionPreset::P720,
                            ResolutionPreset::P720.label(),
                        );
                        ui.selectable_value(
                            &mut app.res_preset,
                            ResolutionPreset::P480,
                            ResolutionPreset::P480.label(),
                        );
                        ui.selectable_value(
                            &mut app.res_preset,
                            ResolutionPreset::P360,
                            ResolutionPreset::P360.label(),
                        );
                    });
                if app.res_preset != prev_res {
                    app.sync_live_config();
                }

                let prev_fps = app.fps_preset;
                egui::ComboBox::from_id_salt("combo_fps")
                    .selected_text(format!("{} FPS", app.fps_preset))
                    .width(68.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut app.fps_preset, 60, "60 FPS");
                        ui.selectable_value(&mut app.fps_preset, 30, "30 FPS");
                        ui.selectable_value(&mut app.fps_preset, 15, "15 FPS");
                    });
                if app.fps_preset != prev_fps {
                    app.sync_live_config();
                }

                let prev_q = app.jpeg_quality;
                let q_label = match app.jpeg_quality {
                    90 => "Ultra",
                    75 => "Alta",
                    65 => "Turbo",
                    _ => "Leve",
                };
                egui::ComboBox::from_id_salt("combo_quality")
                    .selected_text(q_label)
                    .width(64.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut app.jpeg_quality, 90, "Ultra (90%)");
                        ui.selectable_value(&mut app.jpeg_quality, 75, "Alta (75%)");
                        ui.selectable_value(&mut app.jpeg_quality, 65, "Turbo 60fps (65%)");
                        ui.selectable_value(&mut app.jpeg_quality, 48, "Leve (48%)");
                    });
                if app.jpeg_quality != prev_q {
                    app.sync_live_config();
                }

                let is_turbo_preset = app.res_preset == ResolutionPreset::P720
                    && app.fps_preset == 60
                    && app.jpeg_quality == 65
                    && app.auto_game_eco;
                let turbo_btn = egui::Button::new(
                    RichText::new("⚡ Modo Turbo Jogos")
                        .size(11.0)
                        .strong()
                        .color(Color32::WHITE),
                )
                .fill(if is_turbo_preset {
                    Color32::from_rgb(35, 165, 90)
                } else {
                    Color32::from_rgb(52, 56, 66)
                });
                if ui
                    .add(turbo_btn)
                    .on_hover_text(
                        "Ativa 720p HD a 60 FPS + Bitrate Adaptativo + Pausa automática da prévia local enquanto você joga (Zero peso no PC!)",
                    )
                    .clicked()
                {
                    app.res_preset = ResolutionPreset::P720;
                    app.fps_preset = 60;
                    app.jpeg_quality = 65;
                    app.auto_game_eco = true;
                    app.sync_live_config();
                    app.show_toast(
                        "⚡ Modo Turbo Jogos ativado: 720p 60 FPS com Zero Peso enquanto você joga!",
                    );
                }

                if app.is_sharing {
                    let prev_aud = app.share_audio_enabled;
                    ui.checkbox(&mut app.share_audio_enabled, "🔊 Meu Áudio");
                    if app.share_audio_enabled != prev_aud {
                        app.sync_live_config();
                    }

                    if !app.remote_tiles.is_empty() {
                        ui.checkbox(
                            &mut app.show_own_preview_in_grid,
                            "👁 Ver Minha Tela Junto",
                        );
                    } else {
                        let prev_eco = app.eco_hide_preview;
                        ui.checkbox(&mut app.eco_hide_preview, "Modo Jogo");
                        if app.eco_hide_preview != prev_eco {
                            app.sync_live_config();
                        }
                    }
                }

                if app.focused_tile_id.is_some() {
                    if ui.button("🗗 Ver Telas Juntas").clicked() {
                        app.focused_tile_id = None;
                    }
                }

                if !app.popout_tiles.is_empty() {
                    if ui
                        .button(format!("📥 Juntar Telas ({})", app.popout_tiles.len()))
                        .on_hover_text(
                            "Trazer todas as telas destacadas em outros monitores de volta para o palco principal",
                        )
                        .clicked()
                    {
                        app.popout_tiles.clear();
                        app.popout_fullscreen.clear();
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let upd_state = app
                        .update_status
                        .lock()
                        .map(|g| g.clone())
                        .unwrap_or(UpdateStatus::Idle);

                    match upd_state {
                        UpdateStatus::Downloading => {
                            ui.colored_label(
                                Color32::from_rgb(250, 204, 21),
                                RichText::new("⏳ Atualizando...").size(12.0).strong(),
                            );
                        }
                        UpdateStatus::UpdateAvailable(ver) => {
                            let btn = egui::Button::new(
                                RichText::new(format!("⬇ Atualizar (v{})", ver))
                                    .size(12.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(35, 165, 90));
                            if ui
                                .add(btn)
                                .on_hover_text(
                                    "Baixa a nova versão direto do GitHub e reinicia sozinho",
                                )
                                .clicked()
                            {
                                download_and_apply_update_async(Arc::clone(&app.update_status));
                            }
                        }
                        UpdateStatus::UpToDate => {
                            if ui
                                .button(format!("✅ v{} (Atualizado)", CURRENT_VERSION))
                                .on_hover_text(
                                    "Você já está na versão mais recente! Clique para verificar novamente",
                                )
                                .clicked()
                            {
                                check_for_updates_async(Arc::clone(&app.update_status));
                            }
                        }
                        _ => {
                            if ui
                                .button(format!("🔍 Verificar (v{})", CURRENT_VERSION))
                                .on_hover_text(
                                    "Verifica no GitHub se há uma versão mais nova disponível",
                                )
                                .clicked()
                            {
                                check_for_updates_async(Arc::clone(&app.update_status));
                            }
                        }
                    }

                    egui::Frame::none()
                        .fill(Color32::from_rgba_unmultiplied(35, 165, 90, 42))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(35, 165, 90)))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("⚡ v{} • DXGI GPU", CURRENT_VERSION))
                                    .size(11.0)
                                    .strong()
                                    .color(Color32::from_rgb(134, 239, 172)),
                            );
                        });

                    if ui.button("🎨 Conta & Tema").clicked() {
                        app.show_settings_modal = true;
                    }
                });
            });
        });
}
