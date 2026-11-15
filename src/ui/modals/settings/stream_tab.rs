use eframe::egui::{self, Color32, RichText};

use crate::app::Disc2App;

/// Aba de Áudio e Opções de Captura / Modo Eco dentro do modal de configurações.
pub fn render(app: &mut Disc2App, ui: &mut egui::Ui) {
    ui.label(
        RichText::new("ÁUDIO E CAPTURA DE TELA")
            .size(11.0)
            .strong()
            .color(Color32::from_rgb(148, 155, 164)),
    );
    ui.add_space(6.0);

    if ui
        .checkbox(
            &mut app.share_audio_enabled,
            "🔊 Transmitir áudio isolado do jogo/app (WASAPI Process Loopback)",
        )
        .changed()
    {
        app.sync_live_config();
    }

    ui.add_space(6.0);
    if ui
        .checkbox(
            &mut app.custom.show_cursor_in_stream,
            "🖱️ Mostrar cursor do mouse na transmissão",
        )
        .changed()
    {
        app.sync_live_config();
    }

    ui.add_space(6.0);
    if ui
        .checkbox(
            &mut app.auto_game_eco,
            "⚡ Modo Jogo Inteligente (Pausar prévia local automaticamente quando você estiver focado no jogo)",
        )
        .changed()
    {
        app.sync_live_config();
    }

    ui.add_space(6.0);
    if ui
        .checkbox(
            &mut app.eco_hide_preview,
            "🎮 Ocultar prévia local sempre (mesmo com o SZdisc aberto na tela)",
        )
        .changed()
    {
        app.sync_live_config();
    }
}
