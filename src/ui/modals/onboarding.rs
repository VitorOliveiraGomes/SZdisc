use eframe::egui::{self, Color32, Context, FontId, RichText, Rounding, Stroke, Vec2};

use crate::app::Disc2App;
use crate::ui::image_utils::{load_circular_avatar_texture, pick_image_file_dialog};
use crate::ui::widgets::draw_discord_avatar;

/// Tela inicial de criação de conta / login antes do acesso ao app.
pub fn render_account_onboarding(app: &mut Disc2App, ctx: &Context) {
    let accent = app.custom.accent_color();
    let stage_bg = app.custom.stage_color();
    let card_bg = app.custom.card_color();
    let sidebar_bg = app.custom.sidebar_color();

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(stage_bg))
        .show(ctx, |ui| {
            let avail = ui.available_size();
            ui.vertical_centered(|ui| {
                ui.add_space((avail.y * 0.08).max(16.0));

                egui::Frame::none()
                    .fill(card_bg)
                    .rounding(Rounding::same(14.0))
                    .stroke(Stroke::new(1.5_f32, accent))
                    .inner_margin(egui::Margin::same(28.0))
                    .show(ui, |ui| {
                        ui.set_width(470.0);

                        ui.vertical_centered(|ui| {
                            ui.colored_label(
                                accent,
                                RichText::new("● Bem-vindo ao SZdisc")
                                    .size(22.0)
                                    .strong(),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(
                                    "Crie sua conta para criar ou participar de Servidores com seus amigos",
                                )
                                .size(13.0)
                                .color(Color32::from_rgb(181, 186, 193)),
                            );
                        });

                        ui.add_space(16.0);

                        ui.vertical_centered(|ui| {
                            draw_discord_avatar(
                                ui,
                                72.0,
                                &app.custom,
                                app.user_avatar_tex.as_ref(),
                            );
                            ui.add_space(8.0);
                            if ui.button("📷 Escolher Foto de Perfil (Opcional)").clicked() {
                                if let Some(path) = pick_image_file_dialog() {
                                    if let Some(tex) = load_circular_avatar_texture(ctx, &path) {
                                        app.user_avatar_tex = Some(tex);
                                        app.custom.avatar_image_path = path;
                                    }
                                }
                            }
                        });

                        ui.add_space(14.0);

                        ui.label(
                            RichText::new("SEU NOME DE EXIBIÇÃO (COMO SEUS AMIGOS VÃO TE VER NA SALA) *")
                                .size(11.0)
                                .strong()
                                .color(Color32::from_rgb(181, 186, 193)),
                        );
                        ui.add_space(4.0);
                        let name_edit = egui::TextEdit::singleline(&mut app.custom.display_name)
                            .hint_text("Ex: Pedro, Lucas, Vitor...")
                            .font(FontId::proportional(15.0))
                            .desired_width(f32::INFINITY)
                            .margin(egui::Vec2::new(10.0, 8.0));
                        let name_resp = ui.add(name_edit);

                        ui.add_space(10.0);

                        ui.columns(2, |cols| {
                            cols[0].label(
                                RichText::new("NOME DE USUÁRIO (@LOGIN)")
                                    .size(11.0)
                                    .strong()
                                    .color(Color32::from_rgb(181, 186, 193)),
                            );
                            cols[0].add_space(4.0);
                            cols[0].add(
                                egui::TextEdit::singleline(&mut app.custom.username_login)
                                    .hint_text("Ex: pedro_gamer")
                                    .desired_width(f32::INFINITY),
                            );

                            cols[1].label(
                                RichText::new("SUA TAG & COR DO NOME")
                                    .size(11.0)
                                    .strong()
                                    .color(Color32::from_rgb(181, 186, 193)),
                            );
                            cols[1].add_space(4.0);
                            cols[1].horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut app.custom.user_tag)
                                        .desired_width(85.0),
                                );
                                ui.color_edit_button_srgb(&mut app.custom.name_color_rgb);
                                ui.color_edit_button_srgb(&mut app.custom.avatar_bg);
                            });
                        });

                        ui.add_space(10.0);

                        ui.label(
                            RichText::new("RECADO / STATUS PERSONALIZADO")
                                .size(11.0)
                                .strong()
                                .color(Color32::from_rgb(181, 186, 193)),
                        );
                        ui.add_space(4.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut app.custom.custom_status)
                                .hint_text("Ex: Jogando Valorant • 1080p 60fps")
                                .desired_width(f32::INFINITY),
                        );

                        ui.add_space(18.0);

                        let valid_name = !app.custom.display_name.trim().is_empty();
                        let enter_pressed = name_resp.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            && valid_name;

                        let create_btn = egui::Button::new(
                            RichText::new("🚀 Criar Conta e Entrar no SZdisc")
                                .size(15.0)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(if valid_name {
                            Color32::from_rgb(35, 165, 90)
                        } else {
                            sidebar_bg
                        })
                        .min_size(Vec2::new(ui.available_width(), 42.0));

                        if (ui.add_enabled(valid_name, create_btn).clicked() || enter_pressed)
                            && valid_name
                        {
                            app.custom.display_name = app.custom.display_name.trim().to_string();
                            if app.custom.username_login.trim().is_empty() {
                                app.custom.username_login = app
                                    .custom
                                    .display_name
                                    .to_lowercase()
                                    .replace(' ', "_");
                            }
                            app.custom.account_created = true;
                            app.custom.save_to_disk();
                            let res_lbl = app.current_res_status_label();
                            if let Some(hub) = &app.room_hub {
                                hub.update_my_status(
                                    app.custom.display_name.clone(),
                                    &app.custom.user_tag,
                                    app.is_sharing,
                                    res_lbl,
                                );
                            }
                            app.show_toast(format!(
                                "Bem-vindo(a), {}! Crie um Servidor ou entre com convite.",
                                app.custom.display_name
                            ));
                        }

                        if !valid_name {
                            ui.add_space(6.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("Digite seu nome acima para liberar a entrada.")
                                        .size(11.5)
                                        .color(Color32::from_rgb(250, 204, 21)),
                                );
                            });
                        }
                    });
            });
        });
}
