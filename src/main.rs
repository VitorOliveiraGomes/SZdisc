#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod capture;
mod network;
mod profile_theme;
mod updater;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use audio::spawn_system_audio_loopback;
use capture::{
    capture_thumbnail_rgba, get_process_ram_mb, is_szdisc_in_foreground, list_capture_sources,
    CaptureSource, FastCapturer, SourceCategory,
};
use eframe::egui::{
    self, Color32, ColorImage, Context, FontId, RichText, Rounding, Stroke, TextureHandle,
    TextureOptions, Vec2,
};
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
use network::{derive_persistent_peer_id, PeerInfo, RoomHub, ServerMemberPresence};
use profile_theme::{
    draw_discord_avatar, draw_discord_profile_card, generate_server_id, load_banner_texture,
    load_circular_avatar_texture, pick_image_file_dialog, PresenceStatus, ServerProfile,
    ThemePreset, UserCustomization,
};
use updater::{
    check_for_updates_async, cleanup_old_exe, download_and_apply_update_async, UpdateStatus,
    CURRENT_VERSION,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResolutionPreset {
    P1080,
    P900,
    P720,
    P480,
    P360,
}

impl ResolutionPreset {
    fn dimensions(self) -> (u32, u32) {
        match self {
            Self::P1080 => (1920, 1080),
            Self::P900 => (1600, 900),
            Self::P720 => (1280, 720),
            Self::P480 => (854, 480),
            Self::P360 => (640, 360),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::P1080 => "1080p (Full HD)",
            Self::P900 => "900p (HD+ Gamer)",
            Self::P720 => "720p (HD Turbo 60fps)",
            Self::P480 => "480p (SD Rápido)",
            Self::P360 => "360p (Ultra Leve)",
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            Self::P1080 => "1080p",
            Self::P900 => "900p",
            Self::P720 => "720p",
            Self::P480 => "480p",
            Self::P360 => "360p",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    Profile,
    Appearance,
    Stream,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ServerModalTab {
    CreateServer,
    JoinServer,
}

struct LiveStreamConfig {
    target_w: AtomicU32,
    target_h: AtomicU32,
    target_fps: AtomicU8,
    jpeg_quality: AtomicU8,
    eco_hide_preview: AtomicBool,
    auto_game_eco: AtomicBool,
    draw_cursor: AtomicBool,
    share_system_audio: Arc<AtomicBool>,
    target_audio_pid: Arc<AtomicU32>,
    encoded_frames_counter: Arc<AtomicU32>,
    active_source: Mutex<Option<CaptureSource>>,
}

struct LocalPreviewFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Estado visual (textura na GPU e telemetria) de cada amigo transmitindo na sala
struct RemoteTileState {
    texture: TextureHandle,
    video_size: (u32, u32),
    measured_fps: u32,
    measured_mbps: f32,
    frame_counter: u32,
    bytes_counter: usize,
}

/// Ações disparadas pelos botões de controle de cada tela no palco
#[derive(Default)]
struct TileUiActions {
    focus_tile: Option<u32>,
    unfocus: bool,
    enter_fullscreen: Option<u32>,
    exit_fullscreen: bool,
    move_tile: Option<u32>,
    popout_to_monitor: Option<u32>,
}

const LOCAL_PREVIEW_TILE_ID: u32 = 0;

struct Disc2App {
    // Personalização do Perfil, Conta e do Aplicativo
    custom: UserCustomization,
    user_avatar_tex: Option<TextureHandle>,
    user_banner_tex: Option<TextureHandle>,
    show_settings_modal: bool,
    settings_tab: SettingsTab,
    show_profile_popup: bool,

    // Modal de Criar / Entrar em um Servidor (Estilo Discord)
    show_server_modal: bool,
    server_modal_tab: ServerModalTab,
    create_srv_name_input: String,
    create_srv_rooms_input: String,
    create_srv_color_rgb: [u8; 3],
    join_srv_invite_input: String,

    // Seletor de Tela estilo Discord (Modal com Miniaturas + Pesquisa)
    show_share_modal: bool,
    share_tab: SourceCategory,
    share_search_query: String,
    sources: Vec<CaptureSource>,
    source_thumbnails: HashMap<String, TextureHandle>,
    selected_source_idx: usize,

    // Qualidade da Transmissão e Áudio
    res_preset: ResolutionPreset,
    fps_preset: u8,
    jpeg_quality: u8,
    share_audio_enabled: bool,
    eco_hide_preview: bool,
    auto_game_eco: bool,
    show_own_preview_in_grid: bool,

    // Sistema Multi-Servidor + Salas Isoladas + Multi-Monitor + Tela Cheia
    room_hub: Option<Arc<RoomHub>>,
    new_room_name_input: String,
    remote_tiles: HashMap<u32, RemoteTileState>,
    tile_order: Vec<u32>,
    focused_tile_id: Option<u32>,
    fullscreen_tile_id: Option<u32>,
    popout_tiles: HashSet<u32>,
    popout_fullscreen: HashSet<u32>,

    // Estado de Captura Local
    is_sharing: bool,
    stream_config: Arc<LiveStreamConfig>,
    capture_stop_flag: Option<Arc<AtomicBool>>,
    local_preview_rx: Option<Receiver<LocalPreviewFrame>>,

    // Textura do Vídeo Local e Telemetria
    local_video_texture: Option<TextureHandle>,
    local_video_size: (u32, u32),
    local_measured_fps: u32,
    local_measured_mbps: f32,
    local_frame_counter: u32,
    ram_mb: f32,
    last_metrics_tick: Instant,
    status_toast: Option<(String, Instant)>,

    // Auto-Updater pelo GitHub
    update_status: Arc<Mutex<UpdateStatus>>,
}

fn install_system_emoji_fonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguiemj.ttf") {
        fonts.font_data.insert(
            "segoe_ui_emoji".to_owned(),
            egui::FontData::from_owned(bytes),
        );
        if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            fam.push("segoe_ui_emoji".to_owned());
        }
        if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            fam.push("segoe_ui_emoji".to_owned());
        }
    }
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguisym.ttf") {
        fonts.font_data.insert(
            "segoe_ui_symbol".to_owned(),
            egui::FontData::from_owned(bytes),
        );
        if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            fam.push("segoe_ui_symbol".to_owned());
        }
        if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            fam.push("segoe_ui_symbol".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

impl Disc2App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_system_emoji_fonts(&cc.egui_ctx);
        cleanup_old_exe();
        let update_status = Arc::new(Mutex::new(UpdateStatus::Idle));
        check_for_updates_async(Arc::clone(&update_status));

        let mut custom = UserCustomization::load_from_disk();
        if custom.selected_server_id.is_empty() {
            if let Some(first) = custom.saved_servers.first() {
                custom.selected_server_id = first.server_id.clone();
            }
        }
        custom.apply_to_egui(&cc.egui_ctx);

        let user_avatar_tex = if !custom.avatar_image_path.is_empty() {
            load_circular_avatar_texture(&cc.egui_ctx, &custom.avatar_image_path)
        } else {
            None
        };

        let user_banner_tex = if !custom.banner_image_path.is_empty() {
            load_banner_texture(&cc.egui_ctx, &custom.banner_image_path)
        } else {
            None
        };

        let initial_name = if custom.display_name.trim().is_empty() {
            "Jogador".to_string()
        } else {
            custom.display_name.clone()
        };
        let initial_peer_id = derive_persistent_peer_id(&initial_name, &custom.user_tag);
        let room_hub = RoomHub::start(
            initial_peer_id,
            initial_name,
            &custom.saved_servers,
            &custom.selected_server_id,
            cc.egui_ctx.clone(),
        )
        .ok();

        let sources = list_capture_sources();
        let initial_pid = sources.first().map(|s| s.pid).unwrap_or(0);
        let share_system_audio = Arc::new(AtomicBool::new(true));
        let target_audio_pid = Arc::new(AtomicU32::new(initial_pid));
        let stream_config = Arc::new(LiveStreamConfig {
            target_w: AtomicU32::new(1280),
            target_h: AtomicU32::new(720),
            target_fps: AtomicU8::new(60),
            jpeg_quality: AtomicU8::new(65),
            eco_hide_preview: AtomicBool::new(false),
            auto_game_eco: AtomicBool::new(true),
            draw_cursor: AtomicBool::new(custom.show_cursor_in_stream),
            share_system_audio,
            target_audio_pid,
            encoded_frames_counter: Arc::new(AtomicU32::new(0)),
            active_source: Mutex::new(sources.first().cloned()),
        });

        Self {
            custom,
            user_avatar_tex,
            user_banner_tex,
            show_settings_modal: false,
            settings_tab: SettingsTab::Profile,
            show_profile_popup: false,
            show_server_modal: false,
            server_modal_tab: ServerModalTab::CreateServer,
            create_srv_name_input: String::new(),
            create_srv_rooms_input: "Sala Geral, Jogos, Cinema".to_string(),
            create_srv_color_rgb: [88, 101, 242],
            join_srv_invite_input: String::new(),
            show_share_modal: false,
            share_tab: SourceCategory::Application,
            share_search_query: String::new(),
            sources,
            source_thumbnails: HashMap::new(),
            selected_source_idx: 0,
            res_preset: ResolutionPreset::P720,
            fps_preset: 60,
            jpeg_quality: 65,
            share_audio_enabled: true,
            eco_hide_preview: false,
            auto_game_eco: true,
            show_own_preview_in_grid: true,
            room_hub,
            new_room_name_input: String::new(),
            remote_tiles: HashMap::new(),
            tile_order: Vec::new(),
            focused_tile_id: None,
            fullscreen_tile_id: None,
            popout_tiles: HashSet::new(),
            popout_fullscreen: HashSet::new(),
            is_sharing: false,
            stream_config,
            capture_stop_flag: None,
            local_preview_rx: None,
            local_video_texture: None,
            local_video_size: (0, 0),
            local_measured_fps: 0,
            local_measured_mbps: 0.0,
            local_frame_counter: 0,
            ram_mb: get_process_ram_mb(),
            last_metrics_tick: Instant::now(),
            status_toast: None,
            update_status,
        }
    }

    fn show_toast(&mut self, msg: impl Into<String>) {
        self.status_toast = Some((msg.into(), Instant::now()));
    }

    fn set_main_fullscreen(&mut self, ctx: &Context, target: Option<u32>) {
        self.fullscreen_tile_id = target;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(target.is_some()));
    }

    fn open_share_picker(&mut self, ctx: &Context) {
        self.refresh_sources_and_thumbnails(ctx);
        if !self.sources.iter().any(|s| s.category == self.share_tab) {
            self.share_tab = SourceCategory::Screen;
        }
        self.show_share_modal = true;
    }

    fn refresh_sources_and_thumbnails(&mut self, ctx: &Context) {
        let prev_id = self.sources.get(self.selected_source_idx).map(|s| s.id.clone());
        self.sources = list_capture_sources();
        self.source_thumbnails.clear();

        for src in &self.sources {
            if let Some((w, h, rgba)) = capture_thumbnail_rgba(src, 272, 153) {
                let img = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                let tex = ctx.load_texture(format!("thumb_{}", src.id), img, TextureOptions::LINEAR);
                self.source_thumbnails.insert(src.id.clone(), tex);
            }
        }

        if let Some(id) = prev_id {
            if let Some(pos) = self.sources.iter().position(|s| s.id == id) {
                self.selected_source_idx = pos;
                return;
            }
        }
        if let Some(pos) = self.sources.iter().position(|s| s.category == self.share_tab) {
            self.selected_source_idx = pos;
        } else {
            self.selected_source_idx = 0;
        }
    }

    fn select_first_in_category(&mut self, cat: SourceCategory) {
        self.share_tab = cat;
        if let Some(pos) = self.sources.iter().position(|s| s.category == cat) {
            self.selected_source_idx = pos;
        }
    }

    fn current_res_status_label(&self) -> String {
        if self.is_sharing {
            let audio_tag = if self.share_audio_enabled { " 🔊" } else { "" };
            format!(
                "{} {}fps{}",
                self.res_preset.short_label(),
                self.fps_preset,
                audio_tag
            )
        } else {
            "Na sala".to_string()
        }
    }

    fn sync_live_config(&mut self) {
        let (w, h) = self.res_preset.dimensions();
        self.stream_config.target_w.store(w, Ordering::Relaxed);
        self.stream_config.target_h.store(h, Ordering::Relaxed);
        self.stream_config
            .target_fps
            .store(self.fps_preset, Ordering::Relaxed);
        self.stream_config
            .jpeg_quality
            .store(self.jpeg_quality, Ordering::Relaxed);
        self.stream_config
            .eco_hide_preview
            .store(self.eco_hide_preview, Ordering::Relaxed);
        self.stream_config
            .auto_game_eco
            .store(self.auto_game_eco, Ordering::Relaxed);
        self.stream_config
            .draw_cursor
            .store(self.custom.show_cursor_in_stream, Ordering::Relaxed);
        self.stream_config
            .share_system_audio
            .store(self.share_audio_enabled, Ordering::Relaxed);

        if let Some(src) = self.sources.get(self.selected_source_idx).cloned() {
            self.stream_config
                .target_audio_pid
                .store(src.pid, Ordering::Relaxed);
            if let Ok(mut guard) = self.stream_config.active_source.lock() {
                *guard = Some(src);
            }
        }

        let res_lbl = self.current_res_status_label();
        if let Some(hub) = &self.room_hub {
            hub.update_my_status(
                self.custom.display_name.clone(),
                &self.custom.user_tag,
                self.is_sharing,
                res_lbl,
            );
        }
    }

    /// Inicia ou troca a janela transmitida SEM sair da sala!
    fn start_or_switch_sharing(&mut self, ctx: Context) {
        if self.sources.is_empty() {
            self.refresh_sources_and_thumbnails(&ctx);
        }
        if self.sources.is_empty() {
            self.show_toast("Nenhuma tela ou janela encontrada para capturar.");
            return;
        }

        // Se o usuário ainda não está conectado em uma sala, conecta automaticamente na primeira sala do servidor selecionado
        if let Some(hub) = &self.room_hub {
            if !hub.in_room.load(Ordering::Relaxed) {
                if let Some(srv) = self
                    .custom
                    .saved_servers
                    .iter()
                    .find(|s| {
                        s.server_id
                            .eq_ignore_ascii_case(&self.custom.selected_server_id)
                    })
                    .or_else(|| self.custom.saved_servers.first())
                {
                    let target_room = srv
                        .rooms
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "Sala Geral".to_string());
                    hub.enter_server_room(&srv.server_id, &target_room);
                } else {
                    self.show_share_modal = false;
                    self.show_server_modal = true;
                    self.show_toast("Crie ou entre em um Servidor primeiro para transmitir!");
                    return;
                }
            }
        }

        // Se já estamos transmitindo, apenas troca a janela/aplicativo em tempo real na MESMA sala!
        if self.is_sharing && self.capture_stop_flag.is_some() {
            self.sync_live_config();
            self.show_share_modal = false;
            let src_name = self
                .sources
                .get(self.selected_source_idx)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Nova tela".to_string());
            self.show_toast(format!("Tela alterada para: {}", src_name));
            return;
        }

        let Some(hub) = self.room_hub.as_ref().map(Arc::clone) else {
            self.show_toast("Erro ao acessar a sala.");
            return;
        };

        self.is_sharing = true;
        self.sync_live_config();

        let stop_flag = Arc::new(AtomicBool::new(false));
        self.capture_stop_flag = Some(Arc::clone(&stop_flag));

        let audio_stop = Arc::clone(&stop_flag);
        let audio_enabled = Arc::clone(&self.stream_config.share_system_audio);
        let audio_pid = Arc::clone(&self.stream_config.target_audio_pid);
        let hub_for_audio = Arc::clone(&hub);
        spawn_system_audio_loopback(audio_stop, audio_enabled, audio_pid, move |pcm_bytes| {
            hub_for_audio.broadcast_my_audio(pcm_bytes);
        });

        let (preview_tx, preview_rx) = sync_channel::<LocalPreviewFrame>(1);
        self.local_preview_rx = Some(preview_rx);

        // Canal de 1 slot (Zero-Lag) entre a Thread de Captura (DWM BitBlt) e a Thread de Codificação JPEG!
        let (encode_tx, encode_rx) = sync_channel::<(u32, u32, u8, u8, Vec<u8>)>(1);

        let cfg = Arc::clone(&self.stream_config);
        let bytes_per_sec = Arc::clone(&hub.bytes_sent_per_sec);
        let encoded_counter = Arc::clone(&self.stream_config.encoded_frames_counter);
        let stop_for_encoder = Arc::clone(&stop_flag);
        let hub_for_encoder = Arc::clone(&hub);

        // ESTÁGIO 2: Thread dedicada de Codificação JPEG + Controle Adaptativo de Bitrate (não bloqueia a captura!)
        thread::spawn(move || {
            let mut sec_timer = Instant::now();
            let mut sec_bytes: u32 = 0;
            let mut jpeg_buf = Vec::with_capacity(196_608);
            let mut adaptive_penalty: i16 = 0;

            while !stop_for_encoder.load(Ordering::Relaxed) {
                match encode_rx.recv_timeout(Duration::from_millis(60)) {
                    Ok((width, height, fps, base_quality, rgb)) => {
                        let eff_quality =
                            ((base_quality as i16) - adaptive_penalty).clamp(40, 92) as u8;
                        jpeg_buf.clear();
                        let mut encoder = Encoder::new(&mut jpeg_buf, eff_quality);
                        encoder.set_sampling_factor(SamplingFactor::F_2_2);

                        if encoder
                            .encode(&rgb, width as u16, height as u16, ColorType::Rgb)
                            .is_ok()
                        {
                            let frame_bytes = jpeg_buf.len();
                            // Bitrate Adaptativo Inteligente: evita engasgar o túnel bore.pub
                            if fps >= 45 && frame_bytes > 26_000 {
                                adaptive_penalty = (adaptive_penalty + 4).min(30);
                            } else if frame_bytes < 16_000 && adaptive_penalty > 0 {
                                adaptive_penalty = (adaptive_penalty - 2).max(0);
                            }

                            sec_bytes = sec_bytes.saturating_add(frame_bytes as u32);
                            encoded_counter.fetch_add(1, Ordering::Relaxed);
                            hub_for_encoder.broadcast_my_video(
                                width as u16,
                                height as u16,
                                fps,
                                &jpeg_buf,
                            );
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }

                if sec_timer.elapsed() >= Duration::from_secs(1) {
                    bytes_per_sec.store(sec_bytes, Ordering::Relaxed);
                    sec_bytes = 0;
                    sec_timer = Instant::now();
                }
            }
        });

        // ESTÁGIO 1: Thread de Captura Ultra-Rápida (DXGI GPU + fallback GDI)
        thread::spawn(move || {
            let mut fast_capturer = FastCapturer::new();
            let mut last_preview_sent = Instant::now() - Duration::from_secs(1);

            while !stop_flag.load(Ordering::Relaxed) {
                let frame_start = Instant::now();
                let fps = cfg.target_fps.load(Ordering::Relaxed).max(5);
                let frame_interval = Duration::from_nanos(1_000_000_000 / fps as u64);

                let target_w = cfg.target_w.load(Ordering::Relaxed);
                let target_h = cfg.target_h.load(Ordering::Relaxed);
                let quality = cfg.jpeg_quality.load(Ordering::Relaxed);
                let hide_preview = cfg.eco_hide_preview.load(Ordering::Relaxed);
                let auto_eco = cfg.auto_game_eco.load(Ordering::Relaxed);
                let draw_cursor = cfg.draw_cursor.load(Ordering::Relaxed);
                let has_peers = hub.has_connected_peers();

                // Quando você está focado dentro do jogo (SZdisc em 2º plano), pausa automaticamente
                // a prévia local para usar 0% de GPU/OpenGL extra e mandar 100% do FPS para seus amigos!
                let app_focused = is_szdisc_in_foreground();
                let need_local_preview = !hide_preview
                    && (!auto_eco || app_focused)
                    && last_preview_sent.elapsed() >= Duration::from_millis(33);

                if !need_local_preview && !has_peers {
                    thread::sleep(Duration::from_millis(60));
                    continue;
                }

                let source_opt = cfg.active_source.lock().ok().and_then(|g| g.clone());
                if let Some(source) = source_opt {
                    if let Some(raw) = fast_capturer.capture_live_frame(
                        &source,
                        target_w,
                        target_h,
                        need_local_preview,
                        draw_cursor,
                    ) {
                        if need_local_preview && !raw.rgba.is_empty() {
                            last_preview_sent = Instant::now();
                            let _ = preview_tx.try_send(LocalPreviewFrame {
                                width: raw.width,
                                height: raw.height,
                                rgba: raw.rgba,
                            });
                            ctx.request_repaint();
                        }

                        if has_peers {
                            let _ = encode_tx.try_send((
                                raw.width,
                                raw.height,
                                fps,
                                quality,
                                raw.rgb,
                            ));
                        } else {
                            cfg.encoded_frames_counter.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }

                let elapsed = frame_start.elapsed();
                if elapsed < frame_interval {
                    thread::sleep(frame_interval - elapsed);
                }
            }
        });

        self.show_share_modal = false;
        self.show_toast("Transmissão iniciada na sala!");
    }

    /// Para apenas a captura da nossa tela, mantendo você e seus amigos conectados na sala!
    fn stop_sharing(&mut self, ctx: &Context) {
        if let Some(flag) = self.capture_stop_flag.take() {
            flag.store(true, Ordering::Relaxed);
        }
        self.local_preview_rx = None;
        self.is_sharing = false;
        self.local_video_texture = None;
        self.local_measured_fps = 0;
        self.local_measured_mbps = 0.0;
        self.popout_tiles.remove(&LOCAL_PREVIEW_TILE_ID);
        self.popout_fullscreen.remove(&LOCAL_PREVIEW_TILE_ID);
        if self.focused_tile_id == Some(LOCAL_PREVIEW_TILE_ID) {
            self.focused_tile_id = None;
        }
        if self.fullscreen_tile_id == Some(LOCAL_PREVIEW_TILE_ID) {
            self.set_main_fullscreen(ctx, None);
        }
        if let Some(hub) = &self.room_hub {
            hub.update_my_status(
                self.custom.display_name.clone(),
                &self.custom.user_tag,
                false,
                "Na sala".to_string(),
            );
        }
    }

    /// Sai completamente da sala de voz/transmissão atual (estilo botão Desconectar do Discord)
    fn leave_room(&mut self, ctx: &Context) {
        self.stop_sharing(ctx);
        self.remote_tiles.clear();
        self.popout_tiles.clear();
        self.popout_fullscreen.clear();
        self.focused_tile_id = None;
        if self.fullscreen_tile_id.is_some() {
            self.set_main_fullscreen(ctx, None);
        }
        if let Some(hub) = &self.room_hub {
            hub.leave_room_completely();
        }
        self.show_toast("Você saiu da sala.");
    }

    fn upload_local_rgba_to_gpu(&mut self, ctx: &Context, width: u32, height: u32, rgba: Vec<u8>) {
        let image = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
        self.local_video_size = (width, height);
        match &mut self.local_video_texture {
            Some(tex) => tex.set(image, TextureOptions::LINEAR),
            None => {
                self.local_video_texture =
                    Some(ctx.load_texture("local_stream_frame", image, TextureOptions::LINEAR));
            }
        }
    }

    // ======================== MODAL DE CRIAR OU ENTRAR EM UM SERVIDOR (ESTILO DISCORD) ========================
    fn render_server_modal(&mut self, ctx: &Context) {
        if !self.show_server_modal {
            return;
        }

        let accent = self.custom.accent_color();
        let card_bg = self.custom.card_color();
        let sidebar_bg = self.custom.sidebar_color();

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
                        let active = self.server_modal_tab == tab;
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
                            self.server_modal_tab = tab;
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕").clicked() {
                            self.show_server_modal = false;
                        }
                    });
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(10.0);

                match self.server_modal_tab {
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
                                    egui::TextEdit::singleline(&mut self.create_srv_name_input)
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
                                ui.color_edit_button_srgb(&mut self.create_srv_color_rgb);
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
                            egui::TextEdit::singleline(&mut self.create_srv_rooms_input)
                                .hint_text("Ex: Sala Geral, Valorant, Cinema")
                                .desired_width(f32::INFINITY)
                                .margin(Vec2::new(10.0, 8.0)),
                        );

                        ui.add_space(20.0);

                        let valid = !self.create_srv_name_input.trim().is_empty();
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
                            let srv_name = self.create_srv_name_input.trim().to_string();
                            let srv_id = generate_server_id(&srv_name);
                            let mut rooms: Vec<String> = self
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
                                icon_rgb: self.create_srv_color_rgb,
                                rooms,
                            };
                            let invite = new_srv.invite_token();
                            ctx.copy_text(invite);

                            if let Some(hub) = &self.room_hub {
                                hub.register_and_select_server(&new_srv);
                            }
                            self.custom.selected_server_id = srv_id;
                            self.custom.saved_servers.push(new_srv);
                            self.custom.save_to_disk();

                            self.create_srv_name_input.clear();
                            self.show_server_modal = false;
                            self.show_toast(format!(
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
                            egui::TextEdit::singleline(&mut self.join_srv_invite_input)
                                .hint_text("Ex: SZ-48291/Tropa do Valorant/Geral,Jogos  ou  SZ-48291")
                                .desired_width(f32::INFINITY)
                                .margin(Vec2::new(10.0, 8.0)),
                        );

                        ui.add_space(22.0);

                        let parsed_srv = ServerProfile::from_invite_input(&self.join_srv_invite_input);
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

                                if let Some(existing) = self
                                    .custom
                                    .saved_servers
                                    .iter_mut()
                                    .find(|s| s.server_id.eq_ignore_ascii_case(&srv_id))
                                {
                                    for r in &new_srv.rooms {
                                        if !existing.rooms.iter().any(|x| x.eq_ignore_ascii_case(r))
                                        {
                                            existing.rooms.push(r.clone());
                                        }
                                    }
                                    if !new_srv.name.starts_with("Servidor SZ-") {
                                        existing.name = new_srv.name.clone();
                                    }
                                    if let Some(hub) = &self.room_hub {
                                        hub.register_and_select_server(existing);
                                    }
                                } else {
                                    if let Some(hub) = &self.room_hub {
                                        hub.register_and_select_server(&new_srv);
                                    }
                                    self.custom.saved_servers.push(new_srv);
                                }

                                self.custom.selected_server_id = srv_id;
                                self.custom.save_to_disk();
                                self.join_srv_invite_input.clear();
                                self.show_server_modal = false;
                                self.show_toast(format!(
                                    "Você entrou no servidor '{}'! Clique em qualquer sala dele para entrar.",
                                    srv_name
                                ));
                            }
                        }
                    }
                }
            });
    }

    // ======================== TELA INICIAL DE CRIAÇÃO DE CONTA / LOGIN ========================
    fn render_account_onboarding(&mut self, ctx: &Context) {
        let accent = self.custom.accent_color();
        let stage_bg = self.custom.stage_color();
        let card_bg = self.custom.card_color();
        let sidebar_bg = self.custom.sidebar_color();

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
                                    &self.custom,
                                    self.user_avatar_tex.as_ref(),
                                );
                                ui.add_space(8.0);
                                if ui.button("📷 Escolher Foto de Perfil (Opcional)").clicked() {
                                    if let Some(path) = pick_image_file_dialog() {
                                        if let Some(tex) = load_circular_avatar_texture(ctx, &path)
                                        {
                                            self.user_avatar_tex = Some(tex);
                                            self.custom.avatar_image_path = path;
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
                            let name_edit = egui::TextEdit::singleline(&mut self.custom.display_name)
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
                                    egui::TextEdit::singleline(&mut self.custom.username_login)
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
                                        egui::TextEdit::singleline(&mut self.custom.user_tag)
                                            .desired_width(85.0),
                                    );
                                    ui.color_edit_button_srgb(&mut self.custom.name_color_rgb);
                                    ui.color_edit_button_srgb(&mut self.custom.avatar_bg);
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
                                egui::TextEdit::singleline(&mut self.custom.custom_status)
                                    .hint_text("Ex: Jogando Valorant • 1080p 60fps")
                                    .desired_width(f32::INFINITY),
                            );

                            ui.add_space(18.0);

                            let valid_name = !self.custom.display_name.trim().is_empty();
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
                                self.custom.display_name =
                                    self.custom.display_name.trim().to_string();
                                if self.custom.username_login.trim().is_empty() {
                                    self.custom.username_login = self
                                        .custom
                                        .display_name
                                        .to_lowercase()
                                        .replace(' ', "_");
                                }
                                self.custom.account_created = true;
                                self.custom.save_to_disk();
                                let res_lbl = self.current_res_status_label();
                                if let Some(hub) = &self.room_hub {
                                    hub.update_my_status(
                                        self.custom.display_name.clone(),
                                        &self.custom.user_tag,
                                        self.is_sharing,
                                        res_lbl,
                                    );
                                }
                                self.show_toast(format!(
                                    "Bem-vindo(a), {}! Crie um Servidor ou entre com convite.",
                                    self.custom.display_name
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

    // ======================== MODAL VISUAL DE COMPARTILHAMENTO ESTILO DISCORD ========================
    fn render_share_picker_modal(&mut self, ctx: &Context) {
        if !self.show_share_modal {
            return;
        }

        let accent = self.custom.accent_color();
        let card_bg = self.custom.card_color();
        let sidebar_bg = self.custom.sidebar_color();

        egui::Window::new("🖥️ Compartilhar sua Tela na Sala")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(780.0, 545.0))
            .frame(
                egui::Frame::none()
                    .fill(card_bg)
                    .rounding(Rounding::same(self.custom.corner_radius.max(10.0)))
                    .stroke(Stroke::new(1.5_f32, accent))
                    .inner_margin(egui::Margin::same(18.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(if self.is_sharing {
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
                            self.show_share_modal = false;
                        }
                        if ui.button("↻ Atualizar Miniaturas").clicked() {
                            self.refresh_sources_and_thumbnails(ctx);
                        }
                    });
                });

                ui.add_space(10.0);

                let count_apps = self
                    .sources
                    .iter()
                    .filter(|s| s.category == SourceCategory::Application)
                    .count();
                let count_browsers = self
                    .sources
                    .iter()
                    .filter(|s| s.category == SourceCategory::Browser)
                    .count();
                let count_screens = self
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
                                let active = self.share_tab == cat;
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
                                    self.select_first_in_category(cat);
                                }
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.share_search_query)
                                        .hint_text("🔍 Filtrar app/janela...")
                                        .desired_width(155.0),
                                );
                            });
                        });
                    });

                ui.add_space(10.0);

                let q_lower = self.share_search_query.trim().to_lowercase();
                let filtered_indices: Vec<usize> = self
                    .sources
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| {
                        if !q_lower.is_empty() {
                            s.name.to_lowercase().contains(&q_lower)
                                || s.subtitle.to_lowercase().contains(&q_lower)
                                || s.exe_name.to_lowercase().contains(&q_lower)
                        } else {
                            s.category == self.share_tab
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
                                    RichText::new(match self.share_tab {
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
                                        let src = &self.sources[*src_idx];
                                        let is_selected = *src_idx == self.selected_source_idx;

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
                                                        self.source_thumbnails.get(&src.id)
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
                                                    let short_title = if src.name.chars().count() > 28
                                                    {
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
                                            self.selected_source_idx = *src_idx;
                                        }
                                        if resp.double_clicked() {
                                            self.selected_source_idx = *src_idx;
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
                                let sel = self.res_preset == preset;
                                if ui
                                    .selectable_label(sel, preset.short_label())
                                    .clicked()
                                {
                                    self.res_preset = preset;
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
                                let sel = self.fps_preset == fps;
                                if ui
                                    .selectable_label(sel, format!("{} FPS", fps))
                                    .clicked()
                                {
                                    self.fps_preset = fps;
                                }
                            }

                            ui.separator();

                            ui.checkbox(
                                &mut self.share_audio_enabled,
                                RichText::new("🔊 Compartilhar Áudio").size(12.0).strong(),
                            );

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let btn_lbl = if self.is_sharing {
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
                                },
                            );
                        });
                    });

                if confirm_go_live {
                    self.start_or_switch_sharing(ctx.clone());
                }
            });
    }

    // ======================== MODAL DE PERSONALIZAÇÃO LIVRE (PERFIL COM FOTO + TEMA DO APP + CONTA) ========================
    fn render_settings_modal(&mut self, ctx: &Context) {
        if !self.show_settings_modal {
            return;
        }

        let accent = self.custom.accent_color();
        let card_bg = self.custom.card_color();
        let sidebar_bg = self.custom.sidebar_color();
        let mut theme_changed = false;

        egui::Window::new("⚙️ Personalização Livre do Perfil, Conta e do Aplicativo")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(820.0, 550.0))
            .frame(
                egui::Frame::none()
                    .fill(card_bg)
                    .rounding(Rounding::same(self.custom.corner_radius.max(10.0)))
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
                        let active = self.settings_tab == tab;
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
                            self.settings_tab = tab;
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✓ Salvar e Fechar").clicked() {
                            self.custom.save_to_disk();
                            self.sync_live_config();
                            self.show_settings_modal = false;
                            self.show_toast("Personalizações salvas!");
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
                        .show(&mut cols[0], |ui| match self.settings_tab {
                            SettingsTab::Profile => {
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
                                            if let Some(tex) =
                                                load_circular_avatar_texture(ctx, &path)
                                            {
                                                self.user_avatar_tex = Some(tex);
                                                self.custom.avatar_image_path = path;
                                                self.custom.save_to_disk();
                                            }
                                        }
                                    }
                                    if self.user_avatar_tex.is_some()
                                        && ui.small_button("🗑️ Remover Foto").clicked()
                                    {
                                        self.user_avatar_tex = None;
                                        self.custom.avatar_image_path.clear();
                                        self.custom.save_to_disk();
                                    }
                                });

                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    if ui.button("🖼️ Escolher Imagem de Banner...").clicked() {
                                        if let Some(path) = pick_image_file_dialog() {
                                            if let Some(tex) = load_banner_texture(ctx, &path) {
                                                self.user_banner_tex = Some(tex);
                                                self.custom.banner_image_path = path;
                                                self.custom.save_to_disk();
                                            }
                                        }
                                    }
                                    if self.user_banner_tex.is_some()
                                        && ui.small_button("🗑️ Remover Banner").clicked()
                                    {
                                        self.user_banner_tex = None;
                                        self.custom.banner_image_path.clear();
                                        self.custom.save_to_disk();
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
                                        .add(
                                            egui::TextEdit::singleline(&mut self.custom.display_name)
                                                .desired_width(135.0),
                                        )
                                        .changed()
                                    {
                                        self.sync_live_config();
                                    }
                                    ui.label("Cor:");
                                    ui.color_edit_button_srgb(&mut self.custom.name_color_rgb);
                                    ui.label("Tag:");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.custom.user_tag)
                                            .desired_width(60.0),
                                    );
                                });

                                ui.add_space(4.0);
                                ui.label("Recado / Status Personalizado:");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.custom.custom_status)
                                        .desired_width(f32::INFINITY),
                                );

                                ui.add_space(4.0);
                                ui.label("Sobre Mim (Bio):");
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.custom.bio)
                                        .desired_rows(2)
                                        .desired_width(f32::INFINITY),
                                );

                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.label("Presença:");
                                    egui::ComboBox::from_id_salt("presence_combo")
                                        .selected_text(self.custom.presence.label())
                                        .show_ui(ui, |ui| {
                                            for p in [
                                                PresenceStatus::Online,
                                                PresenceStatus::Idle,
                                                PresenceStatus::DoNotDisturb,
                                                PresenceStatus::Invisible,
                                            ] {
                                                ui.selectable_value(
                                                    &mut self.custom.presence,
                                                    p,
                                                    p.label(),
                                                );
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
                                    ui.color_edit_button_srgb(&mut self.custom.avatar_bg);
                                    ui.label("Gradiente Banner:");
                                    ui.color_edit_button_srgb(&mut self.custom.banner_bg);
                                    ui.color_edit_button_srgb(&mut self.custom.banner_bg_2);
                                });

                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut self.custom.avatar_ring_glow, "Anel Neon:");
                                    ui.color_edit_button_srgb(&mut self.custom.ring_color_rgb);
                                    ui.add(
                                        egui::Slider::new(
                                            &mut self.custom.ring_thickness,
                                            0.5..=4.5,
                                        )
                                        .suffix(" px"),
                                    );
                                });

                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new("INSÍGNIAS (BADGES) E BADGE CUSTOMIZADA")
                                        .size(11.0)
                                        .strong()
                                        .color(Color32::from_rgb(148, 155, 164)),
                                );
                                ui.horizontal_wrapped(|ui| {
                                    ui.checkbox(&mut self.custom.badge_crown, "👑 Host");
                                    ui.checkbox(&mut self.custom.badge_1080p, "⚡ 1080p60");
                                    ui.checkbox(&mut self.custom.badge_rust, "🦀 Rust");
                                    ui.checkbox(&mut self.custom.badge_gamer, "🎮 Pro");
                                });

                                ui.horizontal(|ui| {
                                    ui.checkbox(
                                        &mut self.custom.custom_badge_enabled,
                                        "Badge Livre:",
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(
                                            &mut self.custom.custom_badge_text,
                                        )
                                        .desired_width(110.0),
                                    );
                                    ui.color_edit_button_srgb(&mut self.custom.custom_badge_rgb);
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
                                    self.leave_room(ctx);
                                    self.custom.account_created = false;
                                    self.custom.save_to_disk();
                                    self.show_settings_modal = false;
                                }
                            }

                            SettingsTab::Appearance => {
                                ui.label(
                                    RichText::new("TEMAS PREDEFINIDOS")
                                        .size(11.0)
                                        .strong()
                                        .color(Color32::from_rgb(148, 155, 164)),
                                );
                                ui.add_space(4.0);

                                for preset in [
                                    ThemePreset::DiscordDark,
                                    ThemePreset::MidnightOled,
                                    ThemePreset::CyberpunkNeon,
                                    ThemePreset::EmeraldGamer,
                                    ThemePreset::CrimsonRuby,
                                    ThemePreset::ArcticOcean,
                                ] {
                                    let is_sel = self.custom.theme_preset == preset;
                                    if ui.selectable_label(is_sel, preset.label()).clicked() {
                                        self.custom.apply_preset_colors(preset);
                                        theme_changed = true;
                                    }
                                }

                                ui.add_space(10.0);
                                ui.separator();
                                ui.add_space(6.0);

                                ui.label(
                                    RichText::new("PERSONALIZAÇÃO LIVRE DE CORES")
                                        .size(11.0)
                                        .strong()
                                        .color(Color32::from_rgb(148, 155, 164)),
                                );
                                ui.horizontal(|ui| {
                                    ui.label("Destaque:");
                                    if ui
                                        .color_edit_button_srgb(&mut self.custom.accent_rgb)
                                        .changed()
                                    {
                                        self.custom.theme_preset = ThemePreset::Custom;
                                        theme_changed = true;
                                    }
                                    ui.label("Texto Principal:");
                                    if ui
                                        .color_edit_button_srgb(&mut self.custom.text_rgb)
                                        .changed()
                                    {
                                        self.custom.theme_preset = ThemePreset::Custom;
                                        theme_changed = true;
                                    }
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Barra Lateral:");
                                    if ui
                                        .color_edit_button_srgb(&mut self.custom.bg_sidebar_rgb)
                                        .changed()
                                    {
                                        self.custom.theme_preset = ThemePreset::Custom;
                                        theme_changed = true;
                                    }
                                    ui.label("Palco Central:");
                                    if ui
                                        .color_edit_button_srgb(&mut self.custom.bg_stage_rgb)
                                        .changed()
                                    {
                                        self.custom.theme_preset = ThemePreset::Custom;
                                        theme_changed = true;
                                    }
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Painéis / Cards:");
                                    if ui
                                        .color_edit_button_srgb(&mut self.custom.bg_card_rgb)
                                        .changed()
                                    {
                                        self.custom.theme_preset = ThemePreset::Custom;
                                        theme_changed = true;
                                    }
                                });

                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new("GEOMETRIA E ESCALA DO APP")
                                        .size(11.0)
                                        .strong()
                                        .color(Color32::from_rgb(148, 155, 164)),
                                );
                                ui.horizontal(|ui| {
                                    ui.label("Cantos Arredondados:");
                                    if ui
                                        .add(
                                            egui::Slider::new(
                                                &mut self.custom.corner_radius,
                                                0.0..=16.0,
                                            )
                                            .suffix(" px"),
                                        )
                                        .changed()
                                    {
                                        theme_changed = true;
                                    }
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Espessura da Borda:");
                                    if ui
                                        .add(
                                            egui::Slider::new(
                                                &mut self.custom.border_stroke_width,
                                                0.5..=3.0,
                                            )
                                            .suffix(" px"),
                                        )
                                        .changed()
                                    {
                                        theme_changed = true;
                                    }
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Zoom da Interface:");
                                    if ui
                                        .add(
                                            egui::Slider::new(
                                                &mut self.custom.ui_scale,
                                                0.90..=1.15,
                                            ),
                                        )
                                        .changed()
                                    {
                                        theme_changed = true;
                                    }
                                });
                            }

                            SettingsTab::Stream => {
                                ui.label(
                                    RichText::new("ÁUDIO E CAPTURA DE TELA")
                                        .size(11.0)
                                        .strong()
                                        .color(Color32::from_rgb(148, 155, 164)),
                                );
                                ui.add_space(6.0);

                                if ui
                                    .checkbox(
                                        &mut self.share_audio_enabled,
                                        "🔊 Transmitir áudio isolado do jogo/app (WASAPI Process Loopback)",
                                    )
                                    .changed()
                                {
                                    self.sync_live_config();
                                }

                                ui.add_space(6.0);
                                if ui
                                    .checkbox(
                                        &mut self.custom.show_cursor_in_stream,
                                        "🖱️ Mostrar cursor do mouse na transmissão",
                                    )
                                    .changed()
                                {
                                    self.sync_live_config();
                                }

                                ui.add_space(6.0);
                                if ui
                                    .checkbox(
                                        &mut self.auto_game_eco,
                                        "⚡ Modo Jogo Inteligente (Pausar prévia local automaticamente quando você estiver focado no jogo)",
                                    )
                                    .changed()
                                {
                                    self.sync_live_config();
                                }

                                ui.add_space(6.0);
                                if ui
                                    .checkbox(
                                        &mut self.eco_hide_preview,
                                        "🎮 Ocultar prévia local sempre (mesmo com o SZdisc aberto na tela)",
                                    )
                                    .changed()
                                {
                                    self.sync_live_config();
                                }
                            }
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
                            &self.custom,
                            self.user_avatar_tex.as_ref(),
                            self.user_banner_tex.as_ref(),
                            self.is_sharing,
                        );
                    });
                });
            });

        if theme_changed {
            self.custom.apply_to_egui(ctx);
            self.custom.save_to_disk();
        }
    }

    /// Renderiza janelas nativas independentes para transmissões destacadas em Outros Monitores!
    fn render_popout_windows(&mut self, ctx: &Context) {
        if self.popout_tiles.is_empty() {
            return;
        }

        let popout_ids: Vec<u32> = self.popout_tiles.iter().copied().collect();
        let card_bg = self.custom.card_color();
        let accent = self.custom.accent_color();
        let mut to_dock_back = Vec::new();
        let mut toggle_fs_for = Vec::new();

        for tid in popout_ids {
            let (title, subtitle, tex_opt, vol_arc, mute_arc) = if tid == LOCAL_PREVIEW_TILE_ID {
                if !self.is_sharing {
                    to_dock_back.push(tid);
                    continue;
                }
                (
                    format!("{} (Sua Tela)", self.custom.display_name),
                    format!(
                        "{}×{} • {} FPS • {:.1} Mbps",
                        self.local_video_size.0,
                        self.local_video_size.1,
                        self.local_measured_fps,
                        self.local_measured_mbps
                    ),
                    self.local_video_texture.clone(),
                    None,
                    None,
                )
            } else if let Some(tile) = self.remote_tiles.get(&tid) {
                let peer_name = self
                    .room_hub
                    .as_ref()
                    .and_then(|h| {
                        h.peers
                            .lock()
                            .ok()
                            .and_then(|m| m.get(&tid).map(|p| p.name.clone()))
                    })
                    .unwrap_or_else(|| format!("Amigo #{}", tid % 1000));

                let (v, m) = self
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

            let is_popout_fs = self.popout_fullscreen.contains(&tid);
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
            self.popout_tiles.remove(&tid);
            self.popout_fullscreen.remove(&tid);
        }
        for tid in toggle_fs_for {
            if self.popout_fullscreen.contains(&tid) {
                self.popout_fullscreen.remove(&tid);
            } else {
                self.popout_fullscreen.insert(tid);
            }
        }
    }

    /// Renderiza um bloco de vídeo individual no palco (com controles de Tela Cheia, Mover Posição e Outro Monitor!)
    #[allow(clippy::too_many_arguments)]
    fn render_stream_tile(
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
}

impl eframe::App for Disc2App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if !self.custom.account_created {
            self.render_account_onboarding(ctx);
            return;
        }

        // Atalhos de Teclado para Tela Cheia (ESC sai da tela cheia, F11 alterna)
        if self.fullscreen_tile_id.is_some()
            && ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::F11))
        {
            self.set_main_fullscreen(ctx, None);
        }

        // Verifica avisos do RoomHub e sincroniza salas/nomes dos Servidores em que participamos
        let mut hub_notice = None;
        let mut servers_dirty = false;
        if let Some(hub) = &self.room_hub {
            if let Ok(mut n) = hub.status_notice.lock() {
                hub_notice = n.take();
            }
            if let (Ok(rmap), Ok(nmap)) = (hub.server_rooms_map.lock(), hub.server_names_map.lock())
            {
                for srv in &mut self.custom.saved_servers {
                    let key = srv.server_id.to_uppercase();
                    if let Some(hub_rooms) = rmap.get(&key) {
                        if *hub_rooms != srv.rooms && !hub_rooms.is_empty() {
                            srv.rooms = hub_rooms.clone();
                            servers_dirty = true;
                        }
                    }
                    if let Some(hub_name) = nmap.get(&key) {
                        if !hub_name.is_empty()
                            && !hub_name.starts_with("Servidor SZ-")
                            && *hub_name != srv.name
                        {
                            srv.name = hub_name.clone();
                            servers_dirty = true;
                        }
                    }
                }
            }
        }
        if servers_dirty {
            self.custom.save_to_disk();
        }
        if let Some(msg) = hub_notice {
            self.show_toast(msg);
        }

        // 1. Atualiza quadro da nossa própria transmissão local
        let mut new_local_frame = None;
        if let Some(rx) = &self.local_preview_rx {
            while let Ok(f) = rx.try_recv() {
                new_local_frame = Some(f);
                self.local_frame_counter += 1;
            }
        }
        if let Some(f) = new_local_frame {
            self.upload_local_rgba_to_gpu(ctx, f.width, f.height, f.rgba);
        }

        // 2. Atualiza quadros recebidos dos amigos na mesma sala (Multi-Stream Bidirecional)
        let mut active_remote_ids = Vec::new();
        if let Some(hub) = &self.room_hub {
            if let Ok(streams_map) = hub.remote_streams.lock() {
                for (&peer_id, stream_arc) in streams_map.iter() {
                    active_remote_ids.push(peer_id);
                    let mut maybe_frame = None;
                    if let Ok(mut latest) = stream_arc.latest_frame.lock() {
                        maybe_frame = latest.take();
                    }

                    if let Some(f) = maybe_frame {
                        let img = ColorImage::from_rgba_unmultiplied(
                            [f.width as usize, f.height as usize],
                            &f.rgba,
                        );
                        match self.remote_tiles.get_mut(&peer_id) {
                            Some(tile) => {
                                tile.texture.set(img, TextureOptions::LINEAR);
                                tile.video_size = (f.width, f.height);
                                tile.frame_counter += 1;
                                tile.bytes_counter += f.byte_len;
                            }
                            None => {
                                let tex = ctx.load_texture(
                                    format!("peer_video_{}", peer_id),
                                    img,
                                    TextureOptions::LINEAR,
                                );
                                self.remote_tiles.insert(
                                    peer_id,
                                    RemoteTileState {
                                        texture: tex,
                                        video_size: (f.width, f.height),
                                        measured_fps: 0,
                                        measured_mbps: 0.0,
                                        frame_counter: 1,
                                        bytes_counter: f.byte_len,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }

        // Remove texturas de amigos que pararam de transmitir ou saíram da sala
        self.remote_tiles
            .retain(|peer_id, _| active_remote_ids.contains(peer_id));
        self.popout_tiles
            .retain(|tid| *tid == LOCAL_PREVIEW_TILE_ID || active_remote_ids.contains(tid));

        if let Some(fs_id) = self.fullscreen_tile_id {
            let still_valid = (fs_id == LOCAL_PREVIEW_TILE_ID && self.is_sharing)
                || active_remote_ids.contains(&fs_id);
            if !still_valid {
                self.set_main_fullscreen(ctx, None);
            }
        }

        // Renderiza janelas destacadas em outros monitores (se houver)
        self.render_popout_windows(ctx);

        // 3. Atualiza telemetria e uso de RAM a cada 1 segundo
        let elapsed_sec = self.last_metrics_tick.elapsed().as_secs_f32();
        if elapsed_sec >= 1.0 {
            self.ram_mb = get_process_ram_mb();
            let enc_frames = self
                .stream_config
                .encoded_frames_counter
                .swap(0, Ordering::Relaxed);
            let prev_frames = self.local_frame_counter;
            self.local_frame_counter = 0;

            let best_frames = enc_frames.max(prev_frames);
            self.local_measured_fps = (best_frames as f32 / elapsed_sec).round() as u32;
            if self.is_sharing {
                if let Some(hub) = &self.room_hub {
                    let bps = hub.bytes_sent_per_sec.load(Ordering::Relaxed);
                    self.local_measured_mbps = (bps as f32 * 8.0) / 1_000_000.0;
                }
            }

            for tile in self.remote_tiles.values_mut() {
                tile.measured_fps = (tile.frame_counter as f32 / elapsed_sec).round() as u32;
                tile.measured_mbps =
                    ((tile.bytes_counter as f32 * 8.0) / elapsed_sec) / 1_000_000.0;
                tile.frame_counter = 0;
                tile.bytes_counter = 0;
            }

            self.last_metrics_tick = Instant::now();
        }

        let accent = self.custom.accent_color();
        let sidebar_bg = self.custom.sidebar_color();
        let stage_bg = self.custom.stage_color();
        let card_bg = self.custom.card_color();

        let in_room = self
            .room_hub
            .as_ref()
            .map(|h| h.in_room.load(Ordering::Relaxed))
            .unwrap_or(false);

        let connected_srv_id = self
            .room_hub
            .as_ref()
            .and_then(|h| h.connected_server_id.lock().ok().map(|g| g.clone()))
            .unwrap_or_default();

        let active_room_name = self
            .room_hub
            .as_ref()
            .and_then(|h| h.active_room_name.lock().ok().map(|g| g.clone()))
            .unwrap_or_default();

        // Se estivermos em MODO TELA CHEIA REAL, ocultamos a barra de servidores, a barra de salas e o topo para o vídeo ocupar 100% do monitor!
        if self.fullscreen_tile_id.is_none() {
            // ======================== COLUNA 1: BARRA DE ÍCONES DE SERVIDORES (EXATAMENTE IGUAL AO DISCORD!) ========================
            let rail_bg = Color32::from_rgb(
                self.custom.bg_sidebar_rgb[0].saturating_sub(11),
                self.custom.bg_sidebar_rgb[1].saturating_sub(11),
                self.custom.bg_sidebar_rgb[2].saturating_sub(12),
            );

            egui::SidePanel::left("discord_server_rail")
                .resizable(false)
                .exact_width(68.0)
                .frame(
                    egui::Frame::none()
                        .fill(rail_bg)
                        .inner_margin(egui::Margin::symmetric(8.0, 10.0)),
                )
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        // Lista de Servidores em que o usuário participa
                        let mut select_srv_action: Option<ServerProfile> = None;

                        egui::ScrollArea::vertical()
                            .id_salt("servers_rail_scroll")
                            .max_height(ui.available_height() - 115.0)
                            .show(ui, |ui| {
                                for srv in &self.custom.saved_servers {
                                    let is_selected = srv
                                        .server_id
                                        .eq_ignore_ascii_case(&self.custom.selected_server_id);
                                    let is_voice_here = in_room
                                        && connected_srv_id.eq_ignore_ascii_case(&srv.server_id);

                                    let (item_rect, resp) = ui.allocate_exact_size(
                                        Vec2::new(52.0, 50.0),
                                        egui::Sense::click(),
                                    );
                                    let icon_center = item_rect.center();
                                    let icon_rect = egui::Rect::from_center_size(
                                        icon_center,
                                        Vec2::splat(44.0),
                                    );

                                    // Barrinha branca lateral igual ao Discord
                                    if is_selected {
                                        let pill = egui::Rect::from_min_size(
                                            egui::pos2(item_rect.left() - 4.0, icon_center.y - 16.0),
                                            Vec2::new(4.0, 32.0),
                                        );
                                        ui.painter().rect_filled(
                                            pill,
                                            Rounding::same(2.0),
                                            Color32::WHITE,
                                        );
                                    } else if resp.hovered() {
                                        let pill = egui::Rect::from_min_size(
                                            egui::pos2(item_rect.left() - 4.0, icon_center.y - 9.0),
                                            Vec2::new(4.0, 18.0),
                                        );
                                        ui.painter().rect_filled(
                                            pill,
                                            Rounding::same(2.0),
                                            Color32::from_rgb(200, 205, 212),
                                        );
                                    }

                                    let srv_col = Color32::from_rgb(
                                        srv.icon_rgb[0],
                                        srv.icon_rgb[1],
                                        srv.icon_rgb[2],
                                    );
                                    let rounding = if is_selected || resp.hovered() {
                                        Rounding::same(14.0)
                                    } else {
                                        Rounding::same(22.0)
                                    };

                                    ui.painter().rect_filled(icon_rect, rounding, srv_col);

                                    // Sigla do Servidor (ex: "TV" para "Tropa do Valorant")
                                    let initials: String = srv
                                        .name
                                        .split_whitespace()
                                        .filter_map(|w| w.chars().next())
                                        .take(2)
                                        .collect::<String>()
                                        .to_uppercase();
                                    let label_txt = if initials.is_empty() {
                                        "SZ".to_string()
                                    } else {
                                        initials
                                    };
                                    ui.painter().text(
                                        icon_center,
                                        egui::Align2::CENTER_CENTER,
                                        label_txt,
                                        FontId::proportional(14.5),
                                        Color32::WHITE,
                                    );

                                    // Bolinha verde indicando que você está conectado numa sala deste servidor
                                    if is_voice_here {
                                        let badge_pos = egui::pos2(
                                            icon_rect.right() - 4.0,
                                            icon_rect.bottom() - 4.0,
                                        );
                                        ui.painter().circle_filled(
                                            badge_pos,
                                            6.5,
                                            rail_bg,
                                        );
                                        ui.painter().circle_filled(
                                            badge_pos,
                                            4.5,
                                            Color32::from_rgb(35, 165, 90),
                                        );
                                    }

                                    let clicked = resp
                                        .on_hover_text(format!(
                                            "{}\nCódigo: {}\n({} salas)",
                                            srv.name,
                                            srv.server_id,
                                            srv.rooms.len()
                                        ))
                                        .clicked();
                                    if clicked {
                                        select_srv_action = Some(srv.clone());
                                    }

                                    ui.add_space(4.0);
                                }
                            });

                        if let Some(chosen) = select_srv_action {
                            self.custom.selected_server_id = chosen.server_id.clone();
                            self.custom.save_to_disk();
                            if let Some(hub) = &self.room_hub {
                                hub.register_and_select_server(&chosen);
                            }
                        }

                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(6.0);

                        // Botão Redondo "+" para Criar um Servidor Novo ou Entrar com Convite!
                        let (add_rect, add_resp) =
                            ui.allocate_exact_size(Vec2::splat(44.0), egui::Sense::click());
                        let add_bg = if add_resp.hovered() {
                            Color32::from_rgb(35, 165, 90)
                        } else {
                            card_bg
                        };
                        let add_fg = if add_resp.hovered() {
                            Color32::WHITE
                        } else {
                            Color32::from_rgb(35, 165, 90)
                        };
                        ui.painter().rect_filled(
                            add_rect,
                            if add_resp.hovered() {
                                Rounding::same(14.0)
                            } else {
                                Rounding::same(22.0)
                            },
                            add_bg,
                        );
                        ui.painter().text(
                            add_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "➕",
                            FontId::proportional(18.0),
                            add_fg,
                        );
                        if add_resp
                            .on_hover_text("Criar um Novo Servidor ou Entrar em um Servidor com Convite")
                            .clicked()
                        {
                            self.server_modal_tab = ServerModalTab::CreateServer;
                            self.show_server_modal = true;
                        }

                        ui.add_space(6.0);

                        // Botão Redondo "🔑" direto para Entrar em Servidor com Convite
                        let (join_rect, join_resp) =
                            ui.allocate_exact_size(Vec2::splat(40.0), egui::Sense::click());
                        ui.painter().rect_filled(
                            join_rect,
                            if join_resp.hovered() {
                                Rounding::same(13.0)
                            } else {
                                Rounding::same(20.0)
                            },
                            if join_resp.hovered() { accent } else { card_bg },
                        );
                        ui.painter().text(
                            join_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "🔑",
                            FontId::proportional(15.0),
                            Color32::WHITE,
                        );
                        if join_resp
                            .on_hover_text("Entrar em um Servidor usando o Convite de um Amigo")
                            .clicked()
                        {
                            self.server_modal_tab = ServerModalTab::JoinServer;
                            self.show_server_modal = true;
                        }
                    });
                });

            // ======================== COLUNA 2: PAINEL DE SALAS DO SERVIDOR SELECIONADO ========================
            egui::SidePanel::left("sidebar_panel")
                .resizable(false)
                .exact_width(268.0)
                .frame(egui::Frame::none().fill(sidebar_bg))
                .show(ctx, |ui| {
                    // 1. Rodapé inferior fixo: Perfil do Usuário (igual ao Discord)
                    egui::TopBottomPanel::bottom("discord_user_footer")
                        .exact_height(60.0)
                        .frame(
                            egui::Frame::none()
                                .fill(Color32::from_rgb(
                                    self.custom.bg_sidebar_rgb[0].saturating_sub(8),
                                    self.custom.bg_sidebar_rgb[1].saturating_sub(8),
                                    self.custom.bg_sidebar_rgb[2].saturating_sub(9),
                                ))
                                .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
                        )
                        .show_inside(ui, |ui| {
                            ui.horizontal_centered(|ui| {
                                draw_discord_avatar(
                                    ui,
                                    36.0,
                                    &self.custom,
                                    self.user_avatar_tex.as_ref(),
                                );

                                let name_resp = ui
                                    .vertical(|ui| {
                                        ui.label(
                                            RichText::new(&self.custom.display_name)
                                                .strong()
                                                .size(13.0)
                                                .color(self.custom.name_color()),
                                        );
                                        let sub = if self.is_sharing {
                                            format!(
                                                "🔴 Transmitindo {}",
                                                self.res_preset.short_label()
                                            )
                                        } else if in_room {
                                            format!("🟢 Em: {}", active_room_name)
                                        } else {
                                            "⚪ Fora das salas".to_string()
                                        };
                                        ui.label(
                                            RichText::new(sub)
                                                .size(10.5)
                                                .color(Color32::from_rgb(160, 166, 174)),
                                        );
                                    })
                                    .response
                                    .interact(egui::Sense::click());

                                if name_resp.clicked() {
                                    self.show_profile_popup = !self.show_profile_popup;
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .button("⚙️")
                                            .on_hover_text("Configurações de Conta, Foto e Aparência")
                                            .clicked()
                                        {
                                            self.show_settings_modal = true;
                                        }
                                    },
                                );
                            });
                        });

                    // 2. Painel de Controle de Conexão (aparece quando conectado numa sala ou permite conectar rápido)
                    egui::TopBottomPanel::bottom("discord_voice_connected_panel")
                        .exact_height(if self.is_sharing { 114.0 } else { 80.0 })
                        .frame(
                            egui::Frame::none()
                                .fill(Color32::from_rgb(
                                    self.custom.bg_sidebar_rgb[0].saturating_sub(4),
                                    self.custom.bg_sidebar_rgb[1].saturating_sub(4),
                                    self.custom.bg_sidebar_rgb[2].saturating_sub(5),
                                ))
                                .stroke(Stroke::new(
                                    1.0_f32,
                                    Color32::from_rgba_unmultiplied(255, 255, 255, 15),
                                ))
                                .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
                        )
                        .show_inside(ui, |ui| {
                            if in_room {
                                let conn_srv_name = self
                                    .custom
                                    .saved_servers
                                    .iter()
                                    .find(|s| s.server_id.eq_ignore_ascii_case(&connected_srv_id))
                                    .map(|s| s.name.clone())
                                    .unwrap_or_else(|| connected_srv_id.clone());

                                ui.colored_label(
                                    Color32::from_rgb(35, 165, 90),
                                    RichText::new(format!(
                                        "🟢 {} / {}",
                                        active_room_name, conn_srv_name
                                    ))
                                    .strong()
                                    .size(11.5),
                                );
                                ui.add_space(5.0);

                                ui.columns(2, |cols| {
                                    let share_txt = if self.is_sharing {
                                        "🔄 Trocar Tela"
                                    } else {
                                        "🖥️ Compartilhar"
                                    };
                                    let share_btn = egui::Button::new(
                                        RichText::new(share_txt)
                                            .size(11.5)
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(if self.is_sharing {
                                        accent
                                    } else {
                                        Color32::from_rgb(43, 45, 49)
                                    })
                                    .min_size(Vec2::new(cols[0].available_width(), 30.0));

                                    if cols[0].add(share_btn).clicked() {
                                        self.open_share_picker(ctx);
                                    }

                                    let leave_btn = egui::Button::new(
                                        RichText::new("📞 Sair da Sala")
                                            .size(11.5)
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(Color32::from_rgb(218, 55, 60))
                                    .min_size(Vec2::new(cols[1].available_width(), 30.0));

                                    if cols[1].add(leave_btn).clicked() {
                                        self.leave_room(ctx);
                                    }
                                });

                                if self.is_sharing {
                                    ui.add_space(4.0);
                                    let stop_stream_btn = egui::Button::new(
                                        RichText::new("⏹ Parar Transmissão (Continuar na Sala)")
                                            .size(11.0)
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(Color32::from_rgb(180, 45, 50))
                                    .min_size(Vec2::new(ui.available_width(), 24.0));

                                    if ui.add(stop_stream_btn).clicked() {
                                        self.stop_sharing(ctx);
                                    }
                                }
                            } else {
                                ui.colored_label(
                                    Color32::from_rgb(148, 155, 164),
                                    RichText::new("⚪ Desconectado de Sala de Voz")
                                        .strong()
                                        .size(11.5),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(
                                        "Clique em qualquer sala do servidor acima para entrar!",
                                    )
                                    .size(10.5)
                                    .color(Color32::from_rgb(140, 146, 155)),
                                );
                            }
                        });

                    // 3. Conteúdo principal: Cabeçalho do Servidor Selecionado + Suas Salas Exclusivas!
                    egui::Frame::none()
                        .inner_margin(egui::Margin::same(11.0))
                        .show(ui, |ui| {
                            let selected_srv_opt = self
                                .custom
                                .saved_servers
                                .iter()
                                .find(|s| {
                                    s.server_id
                                        .eq_ignore_ascii_case(&self.custom.selected_server_id)
                                })
                                .cloned()
                                .or_else(|| self.custom.saved_servers.first().cloned());

                            if let Some(current_srv) = selected_srv_opt {
                                if !self
                                    .custom
                                    .selected_server_id
                                    .eq_ignore_ascii_case(&current_srv.server_id)
                                {
                                    self.custom.selected_server_id = current_srv.server_id.clone();
                                    if let Some(hub) = &self.room_hub {
                                        hub.register_and_select_server(&current_srv);
                                    }
                                }

                                // Cabeçalho do Servidor Atual + Botão Convidar Amigos
                                egui::Frame::none()
                                    .fill(card_bg)
                                    .rounding(Rounding::same(7.0))
                                    .inner_margin(egui::Margin::symmetric(9.0, 7.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(&current_srv.name)
                                                        .strong()
                                                        .size(14.5)
                                                        .color(Color32::WHITE),
                                                );
                                                ui.label(
                                                    RichText::new(format!(
                                                        "ID: {} • RAM {:.0}MB",
                                                        current_srv.server_id, self.ram_mb
                                                    ))
                                                    .size(10.0)
                                                    .color(Color32::from_rgb(148, 155, 164)),
                                                );
                                            });

                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui
                                                        .small_button("🗑️")
                                                        .on_hover_text(
                                                            "Sair / Remover este servidor da sua barra",
                                                        )
                                                        .clicked()
                                                    {
                                                        let del_id = current_srv.server_id.clone();
                                                        if let Some(hub) = &self.room_hub {
                                                            hub.remove_server(&del_id);
                                                        }
                                                        self.custom.saved_servers.retain(|s| {
                                                            !s.server_id
                                                                .eq_ignore_ascii_case(&del_id)
                                                        });
                                                        self.custom.selected_server_id = self
                                                            .custom
                                                            .saved_servers
                                                            .first()
                                                            .map(|s| s.server_id.clone())
                                                            .unwrap_or_default();
                                                        self.custom.save_to_disk();
                                                    }

                                                    let inv_btn = egui::Button::new(
                                                        RichText::new("📨 Convidar")
                                                            .size(11.0)
                                                            .strong()
                                                            .color(Color32::WHITE),
                                                    )
                                                    .fill(accent);

                                                    if ui
                                                        .add(inv_btn)
                                                        .on_hover_text(
                                                            "Copiar o Convite deste Servidor para mandar aos seus amigos!",
                                                        )
                                                        .clicked()
                                                    {
                                                        let token = current_srv.invite_token();
                                                        ctx.copy_text(token);
                                                        self.show_toast(format!(
                                                            "Convite de '{}' copiado! Mande para os amigos deste servidor.",
                                                            current_srv.name
                                                        ));
                                                    }
                                                },
                                            );
                                        });
                                    });

                                ui.add_space(8.0);

                                // Criar nova sala dentro deste Servidor específico
                                ui.label(
                                    RichText::new("SALAS DESTE SERVIDOR")
                                        .size(10.0)
                                        .strong()
                                        .color(Color32::from_rgb(148, 155, 164)),
                                );
                                ui.add_space(4.0);

                                ui.horizontal(|ui| {
                                    let resp = ui.add(
                                        egui::TextEdit::singleline(&mut self.new_room_name_input)
                                            .hint_text("➕ Nova sala neste servidor...")
                                            .desired_width(178.0),
                                    );
                                    let enter = resp.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                    let add_btn = egui::Button::new(
                                        RichText::new("➕ Criar")
                                            .size(11.0)
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(accent);

                                    if (ui.add(add_btn).clicked() || enter)
                                        && !self.new_room_name_input.trim().is_empty()
                                    {
                                        let rname = self.new_room_name_input.trim().to_string();
                                        self.new_room_name_input.clear();
                                        if let Some(hub) = &self.room_hub {
                                            let updated = hub.create_and_enter_room(
                                                &current_srv.server_id,
                                                &rname,
                                            );
                                            if let Some(srv_mut) = self
                                                .custom
                                                .saved_servers
                                                .iter_mut()
                                                .find(|s| {
                                                    s.server_id.eq_ignore_ascii_case(
                                                        &current_srv.server_id,
                                                    )
                                                })
                                            {
                                                srv_mut.rooms = updated;
                                            }
                                            self.custom.save_to_disk();
                                        }
                                        self.show_toast(format!(
                                            "Sala '{}' criada no servidor '{}'!",
                                            rname, current_srv.name
                                        ));
                                    }
                                });

                                ui.add_space(6.0);

                                let mut peers_in_my_room: Vec<PeerInfo> = self
                                    .room_hub
                                    .as_ref()
                                    .and_then(|h| {
                                        h.peers
                                            .lock()
                                            .ok()
                                            .map(|m| m.values().cloned().collect())
                                    })
                                    .unwrap_or_default();
                                peers_in_my_room.sort_by(|a, b| {
                                    a.name.to_lowercase().cmp(&b.name.to_lowercase())
                                });

                                let server_members_list: Vec<ServerMemberPresence> = self
                                    .room_hub
                                    .as_ref()
                                    .and_then(|h| {
                                        h.server_members
                                            .lock()
                                            .ok()
                                            .map(|m| m.values().cloned().collect())
                                    })
                                    .unwrap_or_default();

                                let mut room_to_join: Option<String> = None;
                                let mut room_to_delete: Option<String> = None;

                                egui::ScrollArea::vertical()
                                    .id_salt("server_rooms_scroll")
                                    .max_height(355.0)
                                    .show(ui, |ui| {
                                        for room_name in &current_srv.rooms {
                                            let is_my_current_room = in_room
                                                && connected_srv_id
                                                    .eq_ignore_ascii_case(&current_srv.server_id)
                                                && active_room_name
                                                    .eq_ignore_ascii_case(room_name);

                                            // Filtra APENAS membros deste Servidor (`current_srv.server_id`) que estão nesta sala!
                                            let mut room_members: Vec<(
                                                String,
                                                bool,
                                                String,
                                                bool,
                                            )> = Vec::new();

                                            if is_my_current_room {
                                                room_members.push((
                                                    format!("{} (Você)", self.custom.display_name),
                                                    self.is_sharing,
                                                    self.res_preset.short_label().to_string(),
                                                    true,
                                                ));
                                                for p in &peers_in_my_room {
                                                    room_members.push((
                                                        p.name.clone(),
                                                        p.is_sharing,
                                                        p.res_label.clone(),
                                                        false,
                                                    ));
                                                }
                                                for sm in &server_members_list {
                                                    if sm
                                                        .server_id
                                                        .eq_ignore_ascii_case(&current_srv.server_id)
                                                        && sm
                                                            .current_room
                                                            .eq_ignore_ascii_case(room_name)
                                                        && !peers_in_my_room.iter().any(|p| {
                                                            p.peer_id == sm.peer_id
                                                                || p.name
                                                                    .eq_ignore_ascii_case(&sm.name)
                                                        })
                                                        && !sm
                                                            .name
                                                            .eq_ignore_ascii_case(
                                                                &self.custom.display_name,
                                                            )
                                                    {
                                                        room_members.push((
                                                            sm.name.clone(),
                                                            sm.is_sharing,
                                                            sm.res_label.clone(),
                                                            false,
                                                        ));
                                                    }
                                                }
                                            } else {
                                                for sm in &server_members_list {
                                                    if sm
                                                        .server_id
                                                        .eq_ignore_ascii_case(&current_srv.server_id)
                                                        && sm
                                                            .current_room
                                                            .eq_ignore_ascii_case(room_name)
                                                    {
                                                        room_members.push((
                                                            sm.name.clone(),
                                                            sm.is_sharing,
                                                            sm.res_label.clone(),
                                                            false,
                                                        ));
                                                    }
                                                }
                                            }

                                            let header_fill = if is_my_current_room {
                                                Color32::from_rgba_unmultiplied(88, 101, 242, 42)
                                            } else {
                                                card_bg
                                            };
                                            let header_stroke = if is_my_current_room {
                                                Stroke::new(1.2_f32, accent)
                                            } else {
                                                Stroke::new(
                                                    1.0_f32,
                                                    Color32::from_rgba_unmultiplied(
                                                        255, 255, 255, 12,
                                                    ),
                                                )
                                            };

                                            let channel_resp = egui::Frame::none()
                                                .fill(header_fill)
                                                .stroke(header_stroke)
                                                .rounding(Rounding::same(6.0))
                                                .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                                                .show(ui, |ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.horizontal(|ui| {
                                                        let icon = if is_my_current_room {
                                                            "🟢 🔊"
                                                        } else {
                                                            "🔊"
                                                        };
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "{} {}",
                                                                icon, room_name
                                                            ))
                                                            .strong()
                                                            .size(12.5)
                                                            .color(Color32::WHITE),
                                                        );

                                                        ui.with_layout(
                                                            egui::Layout::right_to_left(
                                                                egui::Align::Center,
                                                            ),
                                                            |ui| {
                                                                if current_srv.rooms.len() > 1
                                                                    && !is_my_current_room
                                                                {
                                                                    if ui
                                                                        .small_button("✕")
                                                                        .on_hover_text(
                                                                            "Remover esta sala do servidor",
                                                                        )
                                                                        .clicked()
                                                                    {
                                                                        room_to_delete =
                                                                            Some(room_name.clone());
                                                                    }
                                                                }

                                                                if !is_my_current_room {
                                                                    if ui
                                                                        .small_button("Entrar")
                                                                        .clicked()
                                                                    {
                                                                        room_to_join =
                                                                            Some(room_name.clone());
                                                                    }
                                                                } else {
                                                                    ui.label(
                                                                        RichText::new(format!(
                                                                            "{} na sala",
                                                                            room_members.len()
                                                                        ))
                                                                        .size(9.5)
                                                                        .color(Color32::from_rgb(
                                                                            134, 239, 172,
                                                                        )),
                                                                    );
                                                                }
                                                            },
                                                        );
                                                    });
                                                })
                                                .response
                                                .interact(egui::Sense::click());

                                            if channel_resp.clicked()
                                                && !is_my_current_room
                                                && room_to_delete.is_none()
                                            {
                                                room_to_join = Some(room_name.clone());
                                            }

                                            // Lista os amigos conectados dentro desta sala
                                            for (m_name, m_sharing, m_res, is_me) in room_members {
                                                ui.add_space(2.0);
                                                ui.horizontal(|ui| {
                                                    ui.add_space(12.0);
                                                    egui::Frame::none()
                                                        .fill(card_bg.linear_multiply(0.92))
                                                        .rounding(Rounding::same(5.0))
                                                        .inner_margin(egui::Margin::symmetric(
                                                            7.0, 4.0,
                                                        ))
                                                        .show(ui, |ui| {
                                                            ui.set_width(ui.available_width());
                                                            ui.horizontal(|ui| {
                                                                if is_me {
                                                                    draw_discord_avatar(
                                                                        ui,
                                                                        22.0,
                                                                        &self.custom,
                                                                        self.user_avatar_tex
                                                                            .as_ref(),
                                                                    );
                                                                } else {
                                                                    let (rect, _) = ui
                                                                        .allocate_exact_size(
                                                                            Vec2::new(22.0, 22.0),
                                                                            egui::Sense::hover(),
                                                                        );
                                                                    ui.painter().circle_filled(
                                                                        rect.center(),
                                                                        11.0,
                                                                        accent,
                                                                    );
                                                                    let init: String = m_name
                                                                        .chars()
                                                                        .next()
                                                                        .unwrap_or('A')
                                                                        .to_uppercase()
                                                                        .collect();
                                                                    ui.painter().text(
                                                                        rect.center(),
                                                                        egui::Align2::CENTER_CENTER,
                                                                        init,
                                                                        FontId::proportional(11.0),
                                                                        Color32::WHITE,
                                                                    );
                                                                }

                                                                ui.vertical(|ui| {
                                                                    ui.label(
                                                                        RichText::new(&m_name)
                                                                            .strong()
                                                                            .size(11.0)
                                                                            .color(if is_me {
                                                                                self.custom
                                                                                    .name_color()
                                                                            } else {
                                                                                Color32::WHITE
                                                                            }),
                                                                    );
                                                                    if m_sharing {
                                                                        ui.label(
                                                                            RichText::new(format!(
                                                                                "Transmitindo ({})",
                                                                                m_res
                                                                            ))
                                                                            .size(9.0)
                                                                            .color(
                                                                                Color32::from_rgb(
                                                                                    134, 239, 172,
                                                                                ),
                                                                            ),
                                                                        );
                                                                    }
                                                                });

                                                                if m_sharing {
                                                                    ui.with_layout(
                                                                        egui::Layout::right_to_left(
                                                                            egui::Align::Center,
                                                                        ),
                                                                        |ui| {
                                                                            egui::Frame::none()
                                                                                .fill(
                                                                                    Color32::from_rgb(
                                                                                        242, 63, 67,
                                                                                    ),
                                                                                )
                                                                                .rounding(
                                                                                    Rounding::same(
                                                                                        4.0,
                                                                                    ),
                                                                                )
                                                                                .inner_margin(
                                                                                    egui::Margin::symmetric(
                                                                                        4.0, 1.0,
                                                                                    ),
                                                                                )
                                                                                .show(ui, |ui| {
                                                                                    ui.label(
                                                                                        RichText::new(
                                                                                            "LIVE",
                                                                                        )
                                                                                        .size(8.5)
                                                                                        .strong()
                                                                                        .color(
                                                                                            Color32::WHITE,
                                                                                        ),
                                                                                    );
                                                                                });
                                                                        },
                                                                    );
                                                                }
                                                            });
                                                        });
                                                });
                                            }

                                            ui.add_space(5.0);
                                        }
                                    });

                                if let Some(del_room) = room_to_delete {
                                    if let Some(srv_mut) = self
                                        .custom
                                        .saved_servers
                                        .iter_mut()
                                        .find(|s| {
                                            s.server_id
                                                .eq_ignore_ascii_case(&current_srv.server_id)
                                        })
                                    {
                                        if srv_mut.rooms.len() > 1 {
                                            srv_mut
                                                .rooms
                                                .retain(|r| !r.eq_ignore_ascii_case(&del_room));
                                            if let Some(hub) = &self.room_hub {
                                                if let Ok(mut map) = hub.server_rooms_map.lock() {
                                                    map.insert(
                                                        srv_mut.server_id.to_uppercase(),
                                                        srv_mut.rooms.clone(),
                                                    );
                                                }
                                            }
                                            self.custom.save_to_disk();
                                        }
                                    }
                                } else if let Some(target_room) = room_to_join {
                                    if let Some(hub) = &self.room_hub {
                                        hub.enter_server_room(&current_srv.server_id, &target_room);
                                    }
                                    self.show_toast(format!(
                                        "Conectado em '{}' ({})",
                                        target_room, current_srv.name
                                    ));
                                }

                            } else {
                                // Estado inicial quando o usuário ainda não entrou nem criou nenhum servidor!
                                ui.vertical_centered(|ui| {
                                    ui.add_space(36.0);
                                    ui.colored_label(
                                        accent,
                                        RichText::new("🌐 Nenhum Servidor")
                                            .size(16.0)
                                            .strong(),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(
                                            "Crie o seu próprio Servidor para um grupo de amigos ou entre usando o convite de um amigo!",
                                        )
                                        .size(12.0)
                                        .color(Color32::from_rgb(165, 170, 180)),
                                    );
                                    ui.add_space(16.0);

                                    let create_btn = egui::Button::new(
                                        RichText::new("🚀 Criar Novo Servidor")
                                            .size(13.0)
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(Color32::from_rgb(35, 165, 90))
                                    .min_size(Vec2::new(ui.available_width(), 36.0));

                                    if ui.add(create_btn).clicked() {
                                        self.server_modal_tab = ServerModalTab::CreateServer;
                                        self.show_server_modal = true;
                                    }

                                    ui.add_space(8.0);

                                    let join_btn = egui::Button::new(
                                        RichText::new("🔑 Entrar com Convite")
                                            .size(13.0)
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(accent)
                                    .min_size(Vec2::new(ui.available_width(), 36.0));

                                    if ui.add(join_btn).clicked() {
                                        self.server_modal_tab = ServerModalTab::JoinServer;
                                        self.show_server_modal = true;
                                    }
                                });
                            }
                        });
                });

            // ======================== BARRA SUPERIOR DO PALCO ========================
            egui::TopBottomPanel::top("stage_top_bar")
                .exact_height(46.0)
                .frame(
                    egui::Frame::none()
                        .fill(card_bg)
                        .inner_margin(egui::Margin::symmetric(12.0, 7.0)),
                )
                .show(ctx, |ui| {
                    ui.horizontal_centered(|ui| {
                        if self.is_sharing || !self.remote_tiles.is_empty() {
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
                        let prev_res = self.res_preset;
                        egui::ComboBox::from_id_salt("combo_res")
                            .selected_text(self.res_preset.short_label())
                            .width(70.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.res_preset,
                                    ResolutionPreset::P1080,
                                    ResolutionPreset::P1080.label(),
                                );
                                ui.selectable_value(
                                    &mut self.res_preset,
                                    ResolutionPreset::P900,
                                    ResolutionPreset::P900.label(),
                                );
                                ui.selectable_value(
                                    &mut self.res_preset,
                                    ResolutionPreset::P720,
                                    ResolutionPreset::P720.label(),
                                );
                                ui.selectable_value(
                                    &mut self.res_preset,
                                    ResolutionPreset::P480,
                                    ResolutionPreset::P480.label(),
                                );
                                ui.selectable_value(
                                    &mut self.res_preset,
                                    ResolutionPreset::P360,
                                    ResolutionPreset::P360.label(),
                                );
                            });
                        if self.res_preset != prev_res {
                            self.sync_live_config();
                        }

                        let prev_fps = self.fps_preset;
                        egui::ComboBox::from_id_salt("combo_fps")
                            .selected_text(format!("{} FPS", self.fps_preset))
                            .width(68.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.fps_preset, 60, "60 FPS");
                                ui.selectable_value(&mut self.fps_preset, 30, "30 FPS");
                                ui.selectable_value(&mut self.fps_preset, 15, "15 FPS");
                            });
                        if self.fps_preset != prev_fps {
                            self.sync_live_config();
                        }

                        let prev_q = self.jpeg_quality;
                        let q_label = match self.jpeg_quality {
                            90 => "Ultra",
                            75 => "Alta",
                            65 => "Turbo",
                            _ => "Leve",
                        };
                        egui::ComboBox::from_id_salt("combo_quality")
                            .selected_text(q_label)
                            .width(64.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.jpeg_quality, 90, "Ultra (90%)");
                                ui.selectable_value(&mut self.jpeg_quality, 75, "Alta (75%)");
                                ui.selectable_value(
                                    &mut self.jpeg_quality,
                                    65,
                                    "Turbo 60fps (65%)",
                                );
                                ui.selectable_value(&mut self.jpeg_quality, 48, "Leve (48%)");
                            });
                        if self.jpeg_quality != prev_q {
                            self.sync_live_config();
                        }

                        let is_turbo_preset = self.res_preset == ResolutionPreset::P720
                            && self.fps_preset == 60
                            && self.jpeg_quality == 65
                            && self.auto_game_eco;
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
                            self.res_preset = ResolutionPreset::P720;
                            self.fps_preset = 60;
                            self.jpeg_quality = 65;
                            self.auto_game_eco = true;
                            self.sync_live_config();
                            self.show_toast(
                                "⚡ Modo Turbo Jogos ativado: 720p 60 FPS com Zero Peso enquanto você joga!",
                            );
                        }

                        if self.is_sharing {
                            let prev_aud = self.share_audio_enabled;
                            ui.checkbox(&mut self.share_audio_enabled, "🔊 Meu Áudio");
                            if self.share_audio_enabled != prev_aud {
                                self.sync_live_config();
                            }

                            if !self.remote_tiles.is_empty() {
                                ui.checkbox(
                                    &mut self.show_own_preview_in_grid,
                                    "👁 Ver Minha Tela Junto",
                                );
                            } else {
                                let prev_eco = self.eco_hide_preview;
                                ui.checkbox(&mut self.eco_hide_preview, "Modo Jogo");
                                if self.eco_hide_preview != prev_eco {
                                    self.sync_live_config();
                                }
                            }
                        }

                        if self.focused_tile_id.is_some() {
                            if ui.button("🗗 Ver Telas Juntas").clicked() {
                                self.focused_tile_id = None;
                            }
                        }

                        if !self.popout_tiles.is_empty() {
                            if ui
                                .button(format!(
                                    "📥 Juntar Telas ({})",
                                    self.popout_tiles.len()
                                ))
                                .on_hover_text(
                                    "Trazer todas as telas destacadas em outros monitores de volta para o palco principal",
                                )
                                .clicked()
                            {
                                self.popout_tiles.clear();
                                self.popout_fullscreen.clear();
                            }
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let upd_state = self
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
                                        download_and_apply_update_async(Arc::clone(
                                            &self.update_status,
                                        ));
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
                                        check_for_updates_async(Arc::clone(&self.update_status));
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
                                        check_for_updates_async(Arc::clone(&self.update_status));
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
                                self.show_settings_modal = true;
                            }
                        });
                    });
                });
        }

        // ======================== PALCO CENTRAL DE VÍDEO (TELA CHEIA, MÚLTIPLAS TELAS E REORDENAÇÃO) ========================
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(stage_bg))
            .show(ctx, |ui| {
                let avail = ui.available_size();

                let include_local_tile = self.is_sharing
                    && !self.eco_hide_preview
                    && (self.remote_tiles.is_empty()
                        || self.show_own_preview_in_grid
                        || self.fullscreen_tile_id == Some(LOCAL_PREVIEW_TILE_ID));

                // Mantém a ordem escolhida pelo usuário (`self.tile_order`) para permitir mover as telas de lugar!
                let mut raw_active: Vec<u32> = self
                    .remote_tiles
                    .keys()
                    .copied()
                    .filter(|tid| !self.popout_tiles.contains(tid))
                    .collect();
                raw_active.sort_unstable();
                if include_local_tile && !self.popout_tiles.contains(&LOCAL_PREVIEW_TILE_ID) {
                    raw_active.push(LOCAL_PREVIEW_TILE_ID);
                }

                self.tile_order.retain(|id| raw_active.contains(id));
                for id in &raw_active {
                    if !self.tile_order.contains(id) {
                        self.tile_order.push(*id);
                    }
                }

                let active_tile_ids = self.tile_order.clone();
                let total_tiles = active_tile_ids.len();

                if total_tiles > 0 {
                    let is_fullscreen_mode = self.fullscreen_tile_id.is_some();
                    let visible_ids: Vec<u32> = if let Some(fs_id) = self.fullscreen_tile_id {
                        if active_tile_ids.contains(&fs_id) {
                            vec![fs_id]
                        } else {
                            active_tile_ids.clone()
                        }
                    } else if let Some(foc) = self.focused_tile_id {
                        if active_tile_ids.contains(&foc) {
                            vec![foc]
                        } else {
                            self.focused_tile_id = None;
                            active_tile_ids.clone()
                        }
                    } else {
                        active_tile_ids.clone()
                    };

                    let stage_rect = if is_fullscreen_mode {
                        ui.available_rect_before_wrap()
                    } else {
                        ui.available_rect_before_wrap().shrink(8.0)
                    };
                    let gap = 8.0;
                    let count = visible_ids.len();

                    let mut tile_rects: Vec<(u32, egui::Rect)> = Vec::with_capacity(count);
                    if count == 1 {
                        tile_rects.push((visible_ids[0], stage_rect));
                    } else if count == 2 {
                        let w = (stage_rect.width() - gap) * 0.5;
                        let r0 = egui::Rect::from_min_size(
                            stage_rect.min,
                            Vec2::new(w, stage_rect.height()),
                        );
                        let r1 = egui::Rect::from_min_size(
                            egui::pos2(stage_rect.min.x + w + gap, stage_rect.min.y),
                            Vec2::new(w, stage_rect.height()),
                        );
                        tile_rects.push((visible_ids[0], r0));
                        tile_rects.push((visible_ids[1], r1));
                    } else if count == 3 {
                        let h = (stage_rect.height() - gap) * 0.5;
                        let w_top = (stage_rect.width() - gap) * 0.5;
                        let r0 = egui::Rect::from_min_size(stage_rect.min, Vec2::new(w_top, h));
                        let r1 = egui::Rect::from_min_size(
                            egui::pos2(stage_rect.min.x + w_top + gap, stage_rect.min.y),
                            Vec2::new(w_top, h),
                        );
                        let w_bot = (stage_rect.width() * 0.62).max(w_top);
                        let x_bot = stage_rect.min.x + (stage_rect.width() - w_bot) * 0.5;
                        let r2 = egui::Rect::from_min_size(
                            egui::pos2(x_bot, stage_rect.min.y + h + gap),
                            Vec2::new(w_bot, h),
                        );
                        tile_rects.push((visible_ids[0], r0));
                        tile_rects.push((visible_ids[1], r1));
                        tile_rects.push((visible_ids[2], r2));
                    } else {
                        let cols = if count <= 4 { 2 } else { 3 };
                        let rows = (count + cols - 1) / cols;
                        let cell_w =
                            (stage_rect.width() - gap * (cols as f32 - 1.0)) / cols as f32;
                        let cell_h =
                            (stage_rect.height() - gap * (rows as f32 - 1.0)) / rows as f32;

                        for (idx, tid) in visible_ids.iter().enumerate() {
                            let c = idx % cols;
                            let r = idx / cols;
                            let min_pos = egui::pos2(
                                stage_rect.min.x + c as f32 * (cell_w + gap),
                                stage_rect.min.y + r as f32 * (cell_h + gap),
                            );
                            tile_rects.push((
                                *tid,
                                egui::Rect::from_min_size(min_pos, Vec2::new(cell_w, cell_h)),
                            ));
                        }
                    }

                    let mut actions = TileUiActions::default();

                    for (tid, rect) in tile_rects {
                        let is_focused = self.focused_tile_id == Some(tid);
                        let is_fs = self.fullscreen_tile_id == Some(tid);
                        if tid == LOCAL_PREVIEW_TILE_ID {
                            let title = format!("{} (Sua Tela)", self.custom.display_name);
                            let sub = format!(
                                "{}×{} • {} FPS • {:.1} Mbps",
                                self.local_video_size.0,
                                self.local_video_size.1,
                                self.local_measured_fps,
                                self.local_measured_mbps
                            );
                            Self::render_stream_tile(
                                ui,
                                rect,
                                tid,
                                &title,
                                &sub,
                                self.local_video_texture.as_ref(),
                                None,
                                None,
                                is_focused,
                                is_fs,
                                total_tiles,
                                card_bg,
                                accent,
                                &mut actions,
                            );
                        } else if let Some(tile) = self.remote_tiles.get(&tid) {
                            let peer_name = self
                                .room_hub
                                .as_ref()
                                .and_then(|h| {
                                    h.peers
                                        .lock()
                                        .ok()
                                        .and_then(|m| m.get(&tid).map(|p| p.name.clone()))
                                })
                                .unwrap_or_else(|| format!("Amigo #{}", tid % 1000));

                            let (vol_arc, mute_arc) = self
                                .room_hub
                                .as_ref()
                                .and_then(|h| {
                                    h.remote_streams.lock().ok().and_then(|m| {
                                        m.get(&tid).map(|s| {
                                            (
                                                Arc::clone(&s.volume_percent),
                                                Arc::clone(&s.muted),
                                            )
                                        })
                                    })
                                })
                                .unzip();

                            let sub = format!(
                                "{}×{} • {} FPS • {:.1} Mbps",
                                tile.video_size.0,
                                tile.video_size.1,
                                tile.measured_fps,
                                tile.measured_mbps
                            );

                            Self::render_stream_tile(
                                ui,
                                rect,
                                tid,
                                &peer_name,
                                &sub,
                                Some(&tile.texture),
                                vol_arc.as_ref(),
                                mute_arc.as_ref(),
                                is_focused,
                                is_fs,
                                total_tiles,
                                card_bg,
                                accent,
                                &mut actions,
                            );
                        }
                    }

                    if actions.exit_fullscreen {
                        self.set_main_fullscreen(ctx, None);
                    } else if let Some(fs_tid) = actions.enter_fullscreen {
                        self.set_main_fullscreen(ctx, Some(fs_tid));
                    }

                    if actions.unfocus {
                        self.focused_tile_id = None;
                    } else if let Some(fid) = actions.focus_tile {
                        self.focused_tile_id = Some(fid);
                    }

                    if let Some(move_tid) = actions.move_tile {
                        if let Some(pos) = self.tile_order.iter().position(|&x| x == move_tid) {
                            let next_pos = (pos + 1) % self.tile_order.len();
                            self.tile_order.swap(pos, next_pos);
                        }
                    }

                    if let Some(pop_tid) = actions.popout_to_monitor {
                        if self.fullscreen_tile_id == Some(pop_tid) {
                            self.set_main_fullscreen(ctx, None);
                        }
                        self.popout_tiles.insert(pop_tid);
                        self.show_toast(
                            "Janela destacada! Arraste-a para outro monitor ou deixe em tela cheia lá.",
                        );
                    }
                } else if !self.popout_tiles.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(avail.y * 0.33);
                        ui.colored_label(
                            accent,
                            RichText::new("🖥️ Transmissões abertas em janelas separadas (Outro Monitor)")
                                .size(17.0)
                                .strong(),
                        );
                        ui.add_space(10.0);
                        if ui
                            .button("📥 Trazer Todas as Telas de Volta para Este Palco")
                            .clicked()
                        {
                            self.popout_tiles.clear();
                            self.popout_fullscreen.clear();
                        }
                    });
                } else if self.is_sharing && self.eco_hide_preview {
                    ui.vertical_centered(|ui| {
                        ui.add_space(avail.y * 0.33);
                        ui.colored_label(
                            Color32::from_rgb(35, 165, 90),
                            RichText::new("● TRANSMITINDO AO VIVO NA SALA (MODO JOGO ATIVO)")
                                .size(16.5)
                                .strong(),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "A prévia local está oculta para economizar recursos enquanto você joga.",
                            )
                            .size(13.0)
                            .color(Color32::from_rgb(181, 186, 193)),
                        );
                        ui.add_space(10.0);
                        if ui.button("Mostrar Minha Tela Novamente").clicked() {
                            self.eco_hide_preview = false;
                            self.sync_live_config();
                        }
                    });
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space((avail.y * 0.12).max(14.0));
                        draw_discord_profile_card(
                            ui,
                            &self.custom,
                            self.user_avatar_tex.as_ref(),
                            self.user_banner_tex.as_ref(),
                            false,
                        );

                        ui.add_space(18.0);
                        ui.label(
                            RichText::new(if in_room {
                                format!("Conectado na sala: {}", active_room_name)
                            } else if self.custom.saved_servers.is_empty() {
                                "Crie um Servidor ou Entre com Convite na barra esquerda!".to_string()
                            } else {
                                "Selecione um Servidor e clique em uma Sala na esquerda".to_string()
                            })
                            .font(FontId::proportional(19.0))
                            .strong()
                            .color(Color32::WHITE),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "Você pode ter vários Servidores diferentes na coluna esquerda (cada um com suas próprias salas e amigos).\nBasta enviar o convite do servidor 1 vez para seus amigos e ele ficará salvo no app deles!",
                            )
                            .size(12.5)
                            .color(Color32::from_rgb(160, 166, 174)),
                        );
                        ui.add_space(14.0);

                        ui.horizontal(|ui| {
                            let pad = ((ui.available_width() - 560.0) * 0.5).max(0.0);
                            ui.add_space(pad);

                            let srv_btn = egui::Button::new(
                                RichText::new("🌐 Criar / Entrar em Servidor")
                                    .size(13.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(35, 165, 90))
                            .min_size(Vec2::new(205.0, 38.0));

                            if ui.add(srv_btn).clicked() {
                                self.show_server_modal = true;
                            }

                            let share_hero = egui::Button::new(
                                RichText::new("🖥️ Compartilhar Tela")
                                    .size(13.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(accent)
                            .min_size(Vec2::new(190.0, 38.0));

                            if ui.add(share_hero).clicked() {
                                self.open_share_picker(ctx);
                            }

                            let cust_btn = egui::Button::new(
                                RichText::new("📷 Conta & Perfil")
                                    .size(13.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(card_bg)
                            .min_size(Vec2::new(145.0, 38.0));

                            if ui.add(cust_btn).clicked() {
                                self.settings_tab = SettingsTab::Profile;
                                self.show_settings_modal = true;
                            }
                        });
                    });
                }

                if self.show_profile_popup {
                    egui::Area::new(egui::Id::new("discord_profile_popover"))
                        .fixed_pos(egui::Pos2::new(78.0, avail.y - 220.0))
                        .show(ctx, |ui| {
                            draw_discord_profile_card(
                                ui,
                                &self.custom,
                                self.user_avatar_tex.as_ref(),
                                self.user_banner_tex.as_ref(),
                                self.is_sharing,
                            );
                        });
                }

                if let Some((msg, when)) = &self.status_toast {
                    if when.elapsed() < Duration::from_secs(4) {
                        egui::Area::new(egui::Id::new("toast_area"))
                            .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -16.0))
                            .show(ctx, |ui| {
                                egui::Frame::none()
                                    .fill(card_bg)
                                    .stroke(Stroke::new(1.0_f32, accent))
                                    .rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(msg).size(12.0).color(Color32::WHITE),
                                        );
                                    });
                            });
                    }
                }
            });

        self.render_server_modal(ctx);
        self.render_share_picker_modal(ctx);
        self.render_settings_modal(ctx);
    }
}

fn main() -> eframe::Result<()> {
    unsafe {
        winapi::um::timeapi::timeBeginPeriod(1);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!(
                "SZdisc v{} — DXGI GPU Screen Share (Sem VPN)",
                CURRENT_VERSION
            ))
            .with_inner_size([1280.0, 760.0])
            .with_min_inner_size([960.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "SZdisc",
        options,
        Box::new(|cc| Ok(Box::new(Disc2App::new(cc)))),
    )
}
