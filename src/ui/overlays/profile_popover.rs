use eframe::egui::{self, Context};

use crate::app::Disc2App;
use crate::ui::widgets::draw_discord_profile_card;

/// Renderiza o popover flutuante com o cartão de perfil completo ao clicar no usuário no rodapé.
pub fn render_profile_popover(app: &Disc2App, ctx: &Context) {
    if !app.show_profile_popup {
        return;
    }

    let avail_y = (ctx.screen_rect().height()
        - if app.fullscreen_tile_id.is_none() {
            46.0
        } else {
            0.0
        })
    .max(240.0);

    egui::Area::new(egui::Id::new("discord_profile_popover"))
        .fixed_pos(egui::Pos2::new(78.0, avail_y - 220.0))
        .show(ctx, |ui| {
            draw_discord_profile_card(
                ui,
                &app.custom,
                app.user_avatar_tex.as_ref(),
                app.user_banner_tex.as_ref(),
                app.is_sharing,
            );
        });
}
