use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use eframe::egui::{self, Color32, FontId, RichText, Rounding, Stroke, TextureHandle, Vec2};

use crate::ui::state::TileUiActions;

/// Renderiza um bloco de vídeo individual no palco (com controles de Tela Cheia, Mover Posição e Outro Monitor!)
#[allow(clippy::too_many_arguments)]
pub fn render_stream_tile(
    ui: &mut egui::Ui,
    tile_rect: egui::Rect,
    tile_id: u32,
    title: &str,
    subtitle: &str,
    texture: Option<&TextureHandle>,
    volume_atomic: Option<&Arc<AtomicU32>>,
    muted_atomic: Option<&Arc<AtomicBool>>,
    is_focused: bool,
    is_fullscreen: bool,
    total_tiles: usize,
    card_bg: Color32,
    accent: Color32,
    actions: &mut TileUiActions,
) {
    let mut child_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(tile_rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );

    let rounding = if is_fullscreen {
        Rounding::ZERO
    } else {
        Rounding::same(8.0)
    };
    child_ui.painter().rect(
        tile_rect,
        rounding,
        Color32::from_rgb(10, 11, 14),
        if is_fullscreen {
            Stroke::NONE
        } else {
            Stroke::new(1.2_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 26))
        },
    );

    let header_h = 36.0;
    let video_area_h = (tile_rect.height() - header_h).max(20.0);
    let video_rect = egui::Rect::from_min_size(
        tile_rect.min,
        Vec2::new(tile_rect.width(), video_area_h),
    );

    if let Some(tex) = texture {
        let tex_size = tex.size_vec2();
        if tex_size.x > 0.0 && tex_size.y > 0.0 {
            let pad = if is_fullscreen { 0.0 } else { 4.0 };
            let scale = ((video_rect.width() - pad) / tex_size.x)
                .min((video_rect.height() - pad) / tex_size.y);
            let draw_size = Vec2::new(tex_size.x * scale, tex_size.y * scale);
            let img_rect = egui::Rect::from_center_size(video_rect.center(), draw_size);
            child_ui.painter().image(
                tex.id(),
                img_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    } else {
        child_ui.painter().text(
            video_rect.center(),
            egui::Align2::CENTER_CENTER,
            "⏳ Carregando vídeo...",
            FontId::proportional(13.5),
            Color32::from_rgb(160, 166, 174),
        );
    }

    let video_resp = child_ui.interact(
        video_rect,
        egui::Id::new(("tile_video_click", tile_id)),
        egui::Sense::click(),
    );
    if video_resp.double_clicked() {
        if is_fullscreen {
            actions.exit_fullscreen = true;
        } else {
            actions.enter_fullscreen = Some(tile_id);
        }
    }

    let bar_rect = egui::Rect::from_min_size(
        egui::pos2(tile_rect.min.x, tile_rect.max.y - header_h),
        Vec2::new(tile_rect.width(), header_h),
    );
    child_ui.painter().rect_filled(
        bar_rect,
        if is_fullscreen {
            Rounding::ZERO
        } else {
            Rounding::same(6.0)
        },
        card_bg,
    );

    let mut bar_ui = child_ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar_rect.shrink2(Vec2::new(8.0, 4.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );

    bar_ui.colored_label(
        Color32::from_rgb(242, 63, 67),
        RichText::new("● LIVE").size(10.5).strong(),
    );
    bar_ui.label(
        RichText::new(title)
            .size(12.0)
            .strong()
            .color(Color32::WHITE),
    );
    bar_ui.label(
        RichText::new(subtitle)
            .size(10.5)
            .color(Color32::from_rgb(165, 170, 180)),
    );

    bar_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // 1. Botão de Tela Cheia Real (Ocupa 100% do monitor!)
        let fs_lbl = if is_fullscreen {
            "🗗 Sair da Tela Cheia (ESC)"
        } else {
            "⛶ Tela Cheia"
        };
        let fs_btn = egui::Button::new(
            RichText::new(fs_lbl)
                .size(11.0)
                .strong()
                .color(Color32::WHITE),
        )
        .fill(if is_fullscreen {
            Color32::from_rgb(218, 55, 60)
        } else {
            accent
        });
        if ui
            .add(fs_btn)
            .on_hover_text("Deixar esta transmissão em Tela Cheia no monitor (Duplo-clique ou ESC para sair)")
            .clicked()
        {
            if is_fullscreen {
                actions.exit_fullscreen = true;
            } else {
                actions.enter_fullscreen = Some(tile_id);
            }
        }

        // 2. Botão para destacar a transmissão em uma janela independente (arrastar para outro monitor!)
        let pop_btn = egui::Button::new(
            RichText::new("🖥️ Outro Monitor")
                .size(11.0)
                .color(Color32::WHITE),
        )
        .fill(Color32::from_rgb(45, 48, 56));
        if ui
            .add(pop_btn)
            .on_hover_text("Abrir esta transmissão em uma janela separada para arrastar para outro monitor")
            .clicked()
        {
            actions.popout_to_monitor = Some(tile_id);
        }

        // 3. Botões para Mover Posição / Expandir quando há 2+ telas no palco
        if total_tiles > 1 && !is_fullscreen {
            let focus_lbl = if is_focused {
                "🗗 Ver Juntas"
            } else {
                "🔍 Focar"
            };
            let f_btn = egui::Button::new(
                RichText::new(focus_lbl).size(11.0).color(Color32::WHITE),
            )
            .fill(if is_focused {
                accent
            } else {
                Color32::from_rgb(45, 48, 56)
            });
            if ui
                .add(f_btn)
                .on_hover_text("Expandir esta tela no palco ou voltar a ver todas juntas")
                .clicked()
            {
                if is_focused {
                    actions.unfocus = true;
                } else {
                    actions.focus_tile = Some(tile_id);
                }
            }

            if !is_focused {
                let move_btn = egui::Button::new(
                    RichText::new("⇄ Mover")
                        .size(11.0)
                        .color(Color32::WHITE),
                )
                .fill(Color32::from_rgb(45, 48, 56));
                if ui
                    .add(move_btn)
                    .on_hover_text("Trocar a posição desta transmissão de lugar no palco")
                    .clicked()
                {
                    actions.move_tile = Some(tile_id);
                }
            }
        }

        // 4. Controle de Volume e Mute individual da transmissão
        if let (Some(vol_arc), Some(mute_arc)) = (volume_atomic, muted_atomic) {
            let mut vol = vol_arc.load(Ordering::Relaxed);
            if ui
                .add(
                    egui::Slider::new(&mut vol, 0..=150)
                        .suffix("%")
                        .show_value(true),
                )
                .changed()
            {
                vol_arc.store(vol, Ordering::Relaxed);
            }

            let mut muted = mute_arc.load(Ordering::Relaxed);
            if ui
                .small_button(if muted { "🔇" } else { "🔊" })
                .on_hover_text("Mutar/Desmutar som desta transmissão")
                .clicked()
            {
                muted = !muted;
                mute_arc.store(muted, Ordering::Relaxed);
            }
        }
    });
}
