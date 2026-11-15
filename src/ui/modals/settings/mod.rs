pub mod appearance_tab;
pub mod profile_tab;
pub mod stream_tab;

use eframe::egui::{self, Color32, Context, RichText, Rounding, Stroke, Vec2};

use crate::app::Disc2App;
use crate::ui::state::SettingsTab;
use crate::ui::theme::apply_theme_to_egui;
use crate::ui::widgets::draw_discord_profile_card;

/// Modal de personalização livre do perfil, tema do aplicativo e preferências de transmissão.
pub fn render_settings_modal(app: &mut Disc2App, ctx: &Context) {
    if !app.show_settings_modal {
        return;
    }

    let accent = app.custom.accent_color();
    let card_bg = app.custom.card_color();
    let sidebar_bg = app.custom.sidebar_color();
    let mut theme_changed = false;

    egui::Window::new("⚙️ Personalização Livre do Perfil, Conta e do Aplicativo")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .fixed_size(Vec2::new(820.0, 550.0))
        .frame(
            egui::Frame::none()
                .fill(card_bg)
                .rounding(Rounding::same(app.custom.corner_radius.max(10.0)))
                .stroke(Stroke::new(1.5_f32, accent))
                .inner_margin(egui::Margin::same(16.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let tabs = [
                    (SettingsTab::Profile, "👤 Minha Conta & Foto"),
                    (SettingsTab::Appearance, "🎨 Cores e Estilo do App"),
                    (SettingsTab::Stream, "🔊 Áudio e Transmissão"),
                ];
                for (tab, label) in tabs {
                    let active = app.settings_tab == tab;
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
                    .min_size(Vec2::new(185.0, 32.0));

                    if ui.add(btn).clicked() {
                        app.settings_tab = tab;
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("✓ Salvar e Fechar").clicked() {
                        app.custom.save_to_disk();
                        app.sync_live_config();
                        app.show_settings_modal = false;
                        app.show_toast("Personalizações salvas!");
                    }
                });
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            ui.columns(2, |cols| {
                egui::ScrollArea::vertical()
                    .id_salt("settings_left_scroll")
                    .max_height(440.0)
                    .show(&mut cols[0], |ui| match app.settings_tab {
                        SettingsTab::Profile => profile_tab::render(app, ctx, ui),
                        SettingsTab::Appearance => {
                            if appearance_tab::render(app, ui) {
                                theme_changed = true;
                            }
                        }
                        SettingsTab::Stream => stream_tab::render(app, ui),
                    });

                cols[1].vertical_centered(|ui| {
                    ui.label(
                        RichText::new("PRÉVIA DO SEU PERFIL AO VIVO")
                            .size(11.0)
                            .strong()
                            .color(Color32::from_rgb(148, 155, 164)),
                    );
                    ui.add_space(8.0);
                    draw_discord_profile_card(
                        ui,
                        &app.custom,
                        app.user_avatar_tex.as_ref(),
                        app.user_banner_tex.as_ref(),
                        app.is_sharing,
                    );
                });
            });
        });

    if theme_changed {
        apply_theme_to_egui(&app.custom, ctx);
        app.custom.save_to_disk();
    }
}
