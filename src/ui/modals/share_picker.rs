use eframe::egui::{self, Color32, Context, FontId, RichText, Rounding, Stroke, Vec2};

use crate::app::Disc2App;
use crate::capture::SourceCategory;
use crate::ui::state::ResolutionPreset;

/// Modal visual de escolha da tela, janela ou navegador a ser compartilhada na sala.
pub fn render_share_picker_modal(app: &mut Disc2App, ctx: &Context) {
    if !app.show_share_modal {
        return;
    }

    let accent = app.custom.accent_color();
    let card_bg = app.custom.card_color();
    let sidebar_bg = app.custom.sidebar_color();

    egui::Window::new("🖥️ Compartilhar sua Tela na Sala")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .fixed_size(Vec2::new(780.0, 545.0))
        .frame(
            egui::Frame::none()
                .fill(card_bg)
                .rounding(Rounding::same(app.custom.corner_radius.max(10.0)))
                .stroke(Stroke::new(1.5_f32, accent))
                .inner_margin(egui::Margin::same(18.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(if app.is_sharing {
                            "Trocar a Janela / Jogo transmitido (sem sair da sala)"
                        } else {
                            "Escolha o que deseja transmitir na sala"
                        })
                        .size(17.0)
                        .strong()
                        .color(Color32::WHITE),
                    );
                    ui.label(
                        RichText::new(
                            "Selecione um aplicativo/jogo, uma guia do navegador ou a tela inteira. Seus amigos verão na mesma hora!",
                        )
                        .size(12.0)
                        .color(Color32::from_rgb(181, 186, 193)),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✕ Fechar").clicked() {
                        app.show_share_modal = false;
                    }
                    if ui.button("↻ Atualizar Miniaturas").clicked() {
                        app.refresh_sources_and_thumbnails(ctx);
                    }
                });
            });

            ui.add_space(10.0);

            let count_apps = app
                .sources
                .iter()
                .filter(|s| s.category == SourceCategory::Application)
                .count();
            let count_browsers = app
                .sources
                .iter()
                .filter(|s| s.category == SourceCategory::Browser)
                .count();
            let count_screens = app
                .sources
                .iter()
                .filter(|s| s.category == SourceCategory::Screen)
                .count();

            egui::Frame::none()
                .fill(sidebar_bg)
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::same(5.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let tabs = [
                            (
                                SourceCategory::Application,
                                format!("🪟 Aplicativos e Jogos ({})", count_apps),
                            ),
                            (
                                SourceCategory::Browser,
                                format!("🌐 Guias / Navegadores ({})", count_browsers),
                            ),
                            (
                                SourceCategory::Screen,
                                format!("🖥️ Tela Inteira ({})", count_screens),
                            ),
                        ];

                        for (cat, label) in tabs {
                            let active = app.share_tab == cat;
                            let btn = egui::Button::new(
                                RichText::new(label)
                                    .size(12.0)
                                    .strong()
                                    .color(if active {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_rgb(181, 186, 193)
                                    }),
                            )
                            .fill(if active { accent } else { Color32::TRANSPARENT })
                            .min_size(Vec2::new(180.0, 29.0));

                            if ui.add(btn).clicked() {
                                app.select_first_in_category(cat);
                            }
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut app.share_search_query)
                                    .hint_text("🔍 Filtrar app/janela...")
                                    .desired_width(155.0),
                            );
                        });
                    });
                });

            ui.add_space(10.0);

            let q_lower = app.share_search_query.trim().to_lowercase();
            let filtered_indices: Vec<usize> = app
                .sources
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    if !q_lower.is_empty() {
                        s.name.to_lowercase().contains(&q_lower)
                            || s.subtitle.to_lowercase().contains(&q_lower)
                            || s.exe_name.to_lowercase().contains(&q_lower)
                    } else {
                        s.category == app.share_tab
                    }
                })
                .map(|(i, _)| i)
                .collect();

            let mut confirm_go_live = false;

            egui::ScrollArea::vertical()
                .id_salt("modal_sources_grid")
                .max_height(285.0)
                .show(ui, |ui| {
                    if filtered_indices.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(60.0);
                            ui.label(
                                RichText::new(match app.share_tab {
                                    SourceCategory::Browser => {
                                        "Nenhuma janela de navegador detectada. Abra o Google Chrome, Edge, Brave, Opera ou Firefox e clique em '↻ Atualizar Miniaturas'."
                                    }
                                    SourceCategory::Application => {
                                        "Nenhum aplicativo encontrado com esse filtro."
                                    }
                                    SourceCategory::Screen => "Nenhum monitor encontrado.",
                                })
                                .size(13.0)
                                .color(Color32::from_rgb(148, 155, 164)),
                            );
                        });
                    } else {
                        egui::Grid::new("discord_thumb_grid")
                            .num_columns(3)
                            .spacing(Vec2::new(12.0, 12.0))
                            .show(ui, |ui| {
                                for (col_pos, src_idx) in filtered_indices.iter().enumerate() {
                                    let src = &app.sources[*src_idx];
                                    let is_selected = *src_idx == app.selected_source_idx;

                                    let border_col = if is_selected {
                                        accent
                                    } else {
                                        Color32::from_rgba_unmultiplied(255, 255, 255, 20)
                                    };
                                    let card_fill = if is_selected {
                                        sidebar_bg.linear_multiply(1.28)
                                    } else {
                                        sidebar_bg
                                    };

                                    let resp = egui::Frame::none()
                                        .fill(card_fill)
                                        .rounding(Rounding::same(8.0))
                                        .stroke(Stroke::new(
                                            if is_selected { 2.5_f32 } else { 1.0_f32 },
                                            border_col,
                                        ))
                                        .inner_margin(egui::Margin::same(7.0))
                                        .show(ui, |ui| {
                                            ui.set_width(226.0);
                                            ui.vertical(|ui| {
                                                if let Some(tex) =
                                                    app.source_thumbnails.get(&src.id)
                                                {
                                                    ui.image((
                                                        tex.id(),
                                                        Vec2::new(226.0, 124.0),
                                                    ));
                                                } else {
                                                    let (r, _) = ui.allocate_exact_size(
                                                        Vec2::new(226.0, 124.0),
                                                        egui::Sense::hover(),
                                                    );
                                                    ui.painter().rect_filled(
                                                        r,
                                                        Rounding::same(5.0),
                                                        Color32::from_rgb(15, 16, 20),
                                                    );
                                                    ui.painter().text(
                                                        r.center(),
                                                        egui::Align2::CENTER_CENTER,
                                                        &src.exe_name,
                                                        FontId::proportional(13.0),
                                                        Color32::from_rgb(148, 155, 164),
                                                    );
                                                }

                                                ui.add_space(5.0);
                                                let short_title = if src.name.chars().count() > 28 {
                                                    let s: String =
                                                        src.name.chars().take(27).collect();
                                                    format!("{}…", s)
                                                } else {
                                                    src.name.clone()
                                                };
                                                ui.label(
                                                    RichText::new(short_title)
                                                        .strong()
                                                        .size(12.5)
                                                        .color(Color32::WHITE),
                                                );
                                                ui.label(
                                                    RichText::new(&src.subtitle)
                                                        .size(10.5)
                                                        .color(Color32::from_rgb(148, 155, 164)),
                                                );
                                            });
                                        })
                                        .response
                                        .interact(egui::Sense::click());

                                    if resp.clicked() {
                                        app.selected_source_idx = *src_idx;
                                    }
                                    if resp.double_clicked() {
                                        app.selected_source_idx = *src_idx;
                                        confirm_go_live = true;
                                    }

                                    if (col_pos + 1) % 3 == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                    }
                });

            ui.add_space(10.0);

            egui::Frame::none()
                .fill(sidebar_bg)
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Resolução:")
                                .size(12.0)
                                .strong()
                                .color(Color32::from_rgb(181, 186, 193)),
                        );
                        for preset in [
                            ResolutionPreset::P1080,
                            ResolutionPreset::P900,
                            ResolutionPreset::P720,
                            ResolutionPreset::P480,
                            ResolutionPreset::P360,
                        ] {
                            let sel = app.res_preset == preset;
                            if ui.selectable_label(sel, preset.short_label()).clicked() {
                                app.res_preset = preset;
                            }
                        }

                        ui.separator();

                        ui.label(
                            RichText::new("Quadros:")
                                .size(12.0)
                                .strong()
                                .color(Color32::from_rgb(181, 186, 193)),
                        );
                        for fps in [60u8, 30, 15] {
                            let sel = app.fps_preset == fps;
                            if ui.selectable_label(sel, format!("{} FPS", fps)).clicked() {
                                app.fps_preset = fps;
                            }
                        }

                        ui.separator();

                        ui.checkbox(
                            &mut app.share_audio_enabled,
                            RichText::new("🔊 Compartilhar Áudio").size(12.0).strong(),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn_lbl = if app.is_sharing {
                                "🔄 Trocar para esta Tela"
                            } else {
                                "🔴 Ir Ao Vivo"
                            };
                            let go_btn = egui::Button::new(
                                RichText::new(btn_lbl)
                                    .size(13.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(35, 165, 90))
                            .min_size(Vec2::new(150.0, 32.0));

                            if ui.add(go_btn).clicked() {
                                confirm_go_live = true;
                            }
                        });
                    });
                });

            if confirm_go_live {
                app.start_or_switch_sharing(ctx.clone());
            }
        });
}
