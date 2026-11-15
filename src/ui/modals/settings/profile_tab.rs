use eframe::egui::{self, Color32, Context, RichText};

use crate::app::Disc2App;
use crate::profile_theme::PresenceStatus;
use crate::ui::image_utils::{
    load_banner_texture, load_circular_avatar_texture, pick_image_file_dialog,
};

/// Aba de Perfil, Foto, Banner, Identidade, Badges e Logout do modal de configurações.
pub fn render(app: &mut Disc2App, ctx: &Context, ui: &mut egui::Ui) {
    ui.label(
        RichText::new("FOTO DE PERFIL E IMAGEM DE BANNER")
            .size(11.0)
            .strong()
            .color(Color32::from_rgb(148, 155, 164)),
    );
    ui.add_space(4.0);

    ui.horizontal_wrapped(|ui| {
        if ui.button("📷 Escolher Foto de Perfil...").clicked() {
            if let Some(path) = pick_image_file_dialog() {
                if let Some(tex) = load_circular_avatar_texture(ctx, &path) {
                    app.user_avatar_tex = Some(tex);
                    app.custom.avatar_image_path = path;
                    app.custom.save_to_disk();
                }
            }
        }
        if app.user_avatar_tex.is_some() && ui.small_button("🗑️ Remover Foto").clicked() {
            app.user_avatar_tex = None;
            app.custom.avatar_image_path.clear();
            app.custom.save_to_disk();
        }
    });

    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui.button("🖼️ Escolher Imagem de Banner...").clicked() {
            if let Some(path) = pick_image_file_dialog() {
                if let Some(tex) = load_banner_texture(ctx, &path) {
                    app.user_banner_tex = Some(tex);
                    app.custom.banner_image_path = path;
                    app.custom.save_to_disk();
                }
            }
        }
        if app.user_banner_tex.is_some() && ui.small_button("🗑️ Remover Banner").clicked() {
            app.user_banner_tex = None;
            app.custom.banner_image_path.clear();
            app.custom.save_to_disk();
        }
    });

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(6.0);

    ui.label(
        RichText::new("IDENTIDADE E STATUS")
            .size(11.0)
            .strong()
            .color(Color32::from_rgb(148, 155, 164)),
    );
    ui.horizontal(|ui| {
        ui.label("Nome:");
        if ui
            .add(egui::TextEdit::singleline(&mut app.custom.display_name).desired_width(135.0))
            .changed()
        {
            app.sync_live_config();
        }
        ui.label("Cor:");
        ui.color_edit_button_srgb(&mut app.custom.name_color_rgb);
        ui.label("Tag:");
        ui.add(egui::TextEdit::singleline(&mut app.custom.user_tag).desired_width(60.0));
    });

    ui.add_space(4.0);
    ui.label("Recado / Status Personalizado:");
    ui.add(
        egui::TextEdit::singleline(&mut app.custom.custom_status).desired_width(f32::INFINITY),
    );

    ui.add_space(4.0);
    ui.label("Sobre Mim (Bio):");
    ui.add(
        egui::TextEdit::multiline(&mut app.custom.bio)
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Presença:");
        egui::ComboBox::from_id_salt("presence_combo")
            .selected_text(app.custom.presence.label())
            .show_ui(ui, |ui| {
                for p in [
                    PresenceStatus::Online,
                    PresenceStatus::Idle,
                    PresenceStatus::DoNotDisturb,
                    PresenceStatus::Invisible,
                ] {
                    ui.selectable_value(&mut app.custom.presence, p, p.label());
                }
            });
    });

    ui.add_space(8.0);
    ui.label(
        RichText::new("CORES DO CARD E ANEL DO AVATAR")
            .size(11.0)
            .strong()
            .color(Color32::from_rgb(148, 155, 164)),
    );
    ui.horizontal(|ui| {
        ui.label("Fundo Avatar:");
        ui.color_edit_button_srgb(&mut app.custom.avatar_bg);
        ui.label("Gradiente Banner:");
        ui.color_edit_button_srgb(&mut app.custom.banner_bg);
        ui.color_edit_button_srgb(&mut app.custom.banner_bg_2);
    });

    ui.horizontal(|ui| {
        ui.checkbox(&mut app.custom.avatar_ring_glow, "Anel Neon:");
        ui.color_edit_button_srgb(&mut app.custom.ring_color_rgb);
        ui.add(egui::Slider::new(&mut app.custom.ring_thickness, 0.5..=4.5).suffix(" px"));
    });

    ui.add_space(8.0);
    ui.label(
        RichText::new("INSÍGNIAS (BADGES) E BADGE CUSTOMIZADA")
            .size(11.0)
            .strong()
            .color(Color32::from_rgb(148, 155, 164)),
    );
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut app.custom.badge_crown, "👑 Host");
        ui.checkbox(&mut app.custom.badge_1080p, "⚡ 1080p60");
        ui.checkbox(&mut app.custom.badge_rust, "🦀 Rust");
        ui.checkbox(&mut app.custom.badge_gamer, "🎮 Pro");
    });

    ui.horizontal(|ui| {
        ui.checkbox(&mut app.custom.custom_badge_enabled, "Badge Livre:");
        ui.add(egui::TextEdit::singleline(&mut app.custom.custom_badge_text).desired_width(110.0));
        ui.color_edit_button_srgb(&mut app.custom.custom_badge_rgb);
    });

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(6.0);
    if ui
        .button(
            RichText::new("🚪 Trocar de Conta / Fazer Logout")
                .color(Color32::from_rgb(242, 63, 67)),
        )
        .clicked()
    {
        app.leave_room(ctx);
        app.custom.account_created = false;
        app.custom.save_to_disk();
        app.show_settings_modal = false;
    }
}
