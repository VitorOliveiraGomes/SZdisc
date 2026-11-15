use eframe::egui::{self, Color32, Context, RichText, Rounding, Stroke, Vec2};

use crate::app::Disc2App;
use crate::profile_theme::{generate_server_id, ServerProfile};
use crate::ui::state::ServerModalTab;

/// Modal de criar um novo servidor ou entrar via código/convite (estilo Discord).
pub fn render_server_modal(app: &mut Disc2App, ctx: &Context) {
    if !app.show_server_modal {
        return;
    }

    let accent = app.custom.accent_color();
    let card_bg = app.custom.card_color();
    let sidebar_bg = app.custom.sidebar_color();

    egui::Window::new("🌐 Servidores do SZdisc — Criar ou Entrar em um Servidor")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .fixed_size(Vec2::new(520.0, 410.0))
        .frame(
            egui::Frame::none()
                .fill(card_bg)
                .rounding(Rounding::same(12.0))
                .stroke(Stroke::new(1.5_f32, accent))
                .inner_margin(egui::Margin::same(20.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let tabs = [
                    (ServerModalTab::CreateServer, "🚀 Criar Meu Servidor"),
                    (ServerModalTab::JoinServer, "🔑 Entrar com Convite"),
                ];
                for (tab, label) in tabs {
                    let active = app.server_modal_tab == tab;
                    let btn = egui::Button::new(
                        RichText::new(label)
                            .size(13.0)
                            .strong()
                            .color(if active {
                                Color32::WHITE
                            } else {
                                Color32::from_rgb(181, 186, 193)
                            }),
                    )
                    .fill(if active { accent } else { sidebar_bg })
                    .min_size(Vec2::new(190.0, 34.0));

                    if ui.add(btn).clicked() {
                        app.server_modal_tab = tab;
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✕").clicked() {
                        app.show_server_modal = false;
                    }
                });
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(10.0);

            match app.server_modal_tab {
                ServerModalTab::CreateServer => {
                    ui.label(
                        RichText::new("Crie um Servidor privado para o seu grupo de amigos!")
                            .size(15.0)
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.label(
                        RichText::new(
                            "Cada servidor tem suas próprias salas isoladas. Só quem receber o convite deste servidor verá ele e as salas dele!",
                        )
                        .size(12.0)
                        .color(Color32::from_rgb(165, 170, 180)),
                    );

                    ui.add_space(14.0);

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("NOME DO SERVIDOR *")
                                    .size(11.0)
                                    .strong()
                                    .color(Color32::from_rgb(181, 186, 193)),
                            );
                            ui.add_space(4.0);
                            ui.add(
                                egui::TextEdit::singleline(&mut app.create_srv_name_input)
                                    .hint_text("Ex: Tropa do Valorant, Servidor do Vitor...")
                                    .desired_width(360.0)
                                    .margin(Vec2::new(10.0, 8.0)),
                            );
                        });

                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("COR DO ÍCONE")
                                    .size(11.0)
                                    .strong()
                                    .color(Color32::from_rgb(181, 186, 193)),
                            );
                            ui.add_space(6.0);
                            ui.color_edit_button_srgb(&mut app.create_srv_color_rgb);
                        });
                    });

                    ui.add_space(12.0);

                    ui.label(
                        RichText::new("SALAS INICIAIS DO SERVIDOR (SEPARADAS POR VÍRGULA)")
                            .size(11.0)
                            .strong()
                            .color(Color32::from_rgb(181, 186, 193)),
                    );
                    ui.add_space(4.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut app.create_srv_rooms_input)
                            .hint_text("Ex: Sala Geral, Valorant, Cinema")
                            .desired_width(f32::INFINITY)
                            .margin(Vec2::new(10.0, 8.0)),
                    );

                    ui.add_space(20.0);

                    let valid = !app.create_srv_name_input.trim().is_empty();
                    let create_btn = egui::Button::new(
                        RichText::new("✨ Criar Servidor e Copiar Convite")
                            .size(14.5)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(if valid {
                        Color32::from_rgb(35, 165, 90)
                    } else {
                        sidebar_bg
                    })
                    .min_size(Vec2::new(ui.available_width(), 40.0));

                    if ui.add_enabled(valid, create_btn).clicked() && valid {
                        let srv_name = app.create_srv_name_input.trim().to_string();
                        let srv_id = generate_server_id(&srv_name);
                        let mut rooms: Vec<String> = app
                            .create_srv_rooms_input
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        if rooms.is_empty() {
                            rooms.push("Sala Geral".to_string());
                        }

                        let new_srv = ServerProfile {
                            server_id: srv_id.clone(),
                            name: srv_name.clone(),
                            icon_rgb: app.create_srv_color_rgb,
                            rooms,
                        };
                        let invite = new_srv.invite_token();
                        ctx.copy_text(invite);

                        if let Some(hub) = &app.room_hub {
                            hub.register_and_select_server(&new_srv);
                        }
                        app.custom.selected_server_id = srv_id;
                        app.custom.saved_servers.push(new_srv);
                        app.custom.save_to_disk();

                        app.create_srv_name_input.clear();
                        app.show_server_modal = false;
                        app.show_toast(format!(
                            "Servidor '{}' criado! Convite copiado para enviar aos amigos.",
                            srv_name
                        ));
                    }
                }

                ServerModalTab::JoinServer => {
                    ui.label(
                        RichText::new("Entrar no Servidor de um Amigo")
                            .size(15.0)
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.label(
                        RichText::new(
                            "Cole abaixo o Convite ou Código do Servidor que seu amigo te mandou.\nVocê só precisa fazer isso 1 única vez: o servidor ficará salvo na sua barra esquerda para sempre!",
                        )
                        .size(12.0)
                        .color(Color32::from_rgb(165, 170, 180)),
                    );

                    ui.add_space(18.0);

                    ui.label(
                        RichText::new("CONVITE OU CÓDIGO DO SERVIDOR *")
                            .size(11.0)
                            .strong()
                            .color(Color32::from_rgb(181, 186, 193)),
                    );
                    ui.add_space(4.0);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut app.join_srv_invite_input)
                            .hint_text("Ex: SZ-48291/Tropa do Valorant/Geral,Jogos  ou  SZ-48291")
                            .desired_width(f32::INFINITY)
                            .margin(Vec2::new(10.0, 8.0)),
                    );

                    ui.add_space(22.0);

                    let parsed_srv = ServerProfile::from_invite_input(&app.join_srv_invite_input);
                    let valid = parsed_srv.is_some();
                    let enter = resp.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && valid;

                    let join_btn = egui::Button::new(
                        RichText::new("🔗 Entrar e Salvar Servidor")
                            .size(14.5)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(if valid {
                        Color32::from_rgb(35, 165, 90)
                    } else {
                        sidebar_bg
                    })
                    .min_size(Vec2::new(ui.available_width(), 40.0));

                    if (ui.add_enabled(valid, join_btn).clicked() || enter) && valid {
                        if let Some(new_srv) = parsed_srv {
                            let srv_id = new_srv.server_id.clone();
                            let srv_name = new_srv.name.clone();

                            if let Some(existing) = app
                                .custom
                                .saved_servers
                                .iter_mut()
                                .find(|s| s.server_id.eq_ignore_ascii_case(&srv_id))
                            {
                                for r in &new_srv.rooms {
                                    if !existing.rooms.iter().any(|x| x.eq_ignore_ascii_case(r)) {
                                        existing.rooms.push(r.clone());
                                    }
                                }
                                if !new_srv.name.starts_with("Servidor SZ-") {
                                    existing.name = new_srv.name.clone();
                                }
                                if let Some(hub) = &app.room_hub {
                                    hub.register_and_select_server(existing);
                                }
                            } else {
                                if let Some(hub) = &app.room_hub {
                                    hub.register_and_select_server(&new_srv);
                                }
                                app.custom.saved_servers.push(new_srv);
                            }

                            app.custom.selected_server_id = srv_id;
                            app.custom.save_to_disk();
                            app.join_srv_invite_input.clear();
                            app.show_server_modal = false;
                            app.show_toast(format!(
                                "Você entrou no servidor '{}'! Clique em qualquer sala dele para entrar.",
                                srv_name
                            ));
                        }
                    }
                }
            }
        });
}
