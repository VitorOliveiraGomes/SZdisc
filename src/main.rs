#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod capture;
mod network;
mod profile_theme;
mod updater;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use audio::spawn_system_audio_loopback;
use capture::{
    capture_frame, capture_thumbnail_rgba, get_process_ram_mb, list_capture_sources, CaptureSource,
    SourceCategory,
};
use eframe::egui::{
    self, Color32, ColorImage, Context, FontId, RichText, Rounding, Stroke, TextureHandle,
    TextureOptions, Vec2,
};
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
use network::{
    start_lan_discovery_listener, DiscoveredRoom, StreamHostServer, StreamViewerClient,
};
use profile_theme::{
    draw_discord_avatar, draw_discord_profile_card, load_banner_texture,
    load_circular_avatar_texture, pick_image_file_dialog, PresenceStatus, ThemePreset,
    UserCustomization,
};
use updater::{
    check_for_updates_async, cleanup_old_exe, download_and_apply_update_async, UpdateStatus,
    CURRENT_VERSION,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResolutionPreset {
    P1080,
    P720,
    P480,
    P360,
}

impl ResolutionPreset {
    fn dimensions(self) -> (u32, u32) {
        match self {
            Self::P1080 => (1920, 1080),
            Self::P720 => (1280, 720),
            Self::P480 => (854, 480),
            Self::P360 => (640, 360),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::P1080 => "1080p (Full HD)",
            Self::P720 => "720p (HD Leve)",
            Self::P480 => "480p (SD Rápido)",
            Self::P360 => "360p (Ultra Leve)",
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            Self::P1080 => "1080p",
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

struct LiveStreamConfig {
    target_w: AtomicU32,
    target_h: AtomicU32,
    target_fps: AtomicU8,
    jpeg_quality: AtomicU8,
    eco_hide_preview: AtomicBool,
    draw_cursor: AtomicBool,
    share_system_audio: Arc<AtomicBool>,
    target_audio_pid: Arc<AtomicU32>,
    active_source: Mutex<Option<CaptureSource>>,
}

struct LocalPreviewFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Representa uma transmissão remota assistida simultaneamente no palco (Multi-Stream)
struct RemoteStreamEntry {
    id: u64,
    label: String,
    client: StreamViewerClient,
    texture: Option<TextureHandle>,
    video_size: (u32, u32),
    measured_fps: u32,
    measured_mbps: f32,
    frame_counter: u32,
    bytes_counter: usize,
}

const LOCAL_PREVIEW_TILE_ID: u64 = 0;

struct Disc2App {
    // Personalização do Perfil, Conta e do Aplicativo
    custom: UserCustomization,
    user_avatar_tex: Option<TextureHandle>,
    user_banner_tex: Option<TextureHandle>,
    show_settings_modal: bool,
    settings_tab: SettingsTab,
    show_profile_popup: bool,

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
    show_own_preview_in_grid: bool,

    // Estado de Transmissão Local (Host)
    is_sharing: bool,
    stream_config: Arc<LiveStreamConfig>,
    capture_stop_flag: Option<Arc<AtomicBool>>,
    local_preview_rx: Option<Receiver<LocalPreviewFrame>>,
    host_server: Option<Arc<StreamHostServer>>,
    host_res_label: Arc<Mutex<String>>,

    // Estado de Visualização Remota Simultânea (Multi-Stream: 3+ telas ao mesmo tempo)
    join_addr_input: String,
    remote_streams: Vec<RemoteStreamEntry>,
    next_stream_id: u64,
    focused_tile_id: Option<u64>,

    // Salas Descobertas na Rede / VPN
    discovered_rooms: Arc<Mutex<HashMap<String, DiscoveredRoom>>>,

    // Textura do Vídeo Local (Host) e Telemetria
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

impl Disc2App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cleanup_old_exe();
        let update_status = Arc::new(Mutex::new(UpdateStatus::Idle));
        check_for_updates_async(Arc::clone(&update_status));

        let custom = UserCustomization::load_from_disk();
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

        let sources = list_capture_sources();
        let initial_pid = sources.first().map(|s| s.pid).unwrap_or(0);
        let share_system_audio = Arc::new(AtomicBool::new(true));
        let target_audio_pid = Arc::new(AtomicU32::new(initial_pid));
        let stream_config = Arc::new(LiveStreamConfig {
            target_w: AtomicU32::new(1920),
            target_h: AtomicU32::new(1080),
            target_fps: AtomicU8::new(60),
            jpeg_quality: AtomicU8::new(75),
            eco_hide_preview: AtomicBool::new(false),
            draw_cursor: AtomicBool::new(custom.show_cursor_in_stream),
            share_system_audio,
            target_audio_pid,
            active_source: Mutex::new(sources.first().cloned()),
        });

        Self {
            custom,
            user_avatar_tex,
            user_banner_tex,
            show_settings_modal: false,
            settings_tab: SettingsTab::Profile,
            show_profile_popup: false,
            show_share_modal: false,
            share_tab: SourceCategory::Application,
            share_search_query: String::new(),
            sources,
            source_thumbnails: HashMap::new(),
            selected_source_idx: 0,
            res_preset: ResolutionPreset::P1080,
            fps_preset: 60,
            jpeg_quality: 75,
            share_audio_enabled: true,
            eco_hide_preview: false,
            show_own_preview_in_grid: true,
            is_sharing: false,
            stream_config,
            capture_stop_flag: None,
            local_preview_rx: None,
            host_server: None,
            host_res_label: Arc::new(Mutex::new("1080p 60fps".to_string())),
            join_addr_input: String::new(),
            remote_streams: Vec::new(),
            next_stream_id: 1,
            focused_tile_id: None,
            discovered_rooms: start_lan_discovery_listener(),
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

        if let Ok(mut lbl) = self.host_res_label.lock() {
            let audio_tag = if self.share_audio_enabled { " 🔊" } else { "" };
            *lbl = format!(
                "{} {}fps{}",
                self.res_preset.short_label(),
                self.fps_preset,
                audio_tag
            );
        }
    }

    fn start_sharing(&mut self, ctx: Context) {
        // Permite transmitir sua tela E continuar assistindo as telas dos seus amigos ao mesmo tempo!
        self.stop_sharing();

        if self.sources.is_empty() {
            self.refresh_sources_and_thumbnails(&ctx);
        }
        if self.sources.is_empty() {
            self.show_toast("Nenhuma tela ou janela encontrada para capturar.");
            return;
        }

        self.sync_live_config();

        let server = match StreamHostServer::start(
            self.custom.display_name.clone(),
            Arc::clone(&self.host_res_label),
        ) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                self.show_toast(e);
                return;
            }
        };

        let addr_msg = server.listen_addr.clone();
        self.host_server = Some(Arc::clone(&server));

        let stop_flag = Arc::new(AtomicBool::new(false));
        self.capture_stop_flag = Some(Arc::clone(&stop_flag));

        let audio_stop = Arc::clone(&stop_flag);
        let audio_enabled = Arc::clone(&self.stream_config.share_system_audio);
        let audio_pid = Arc::clone(&self.stream_config.target_audio_pid);
        let server_for_audio = Arc::clone(&server);
        spawn_system_audio_loopback(audio_stop, audio_enabled, audio_pid, move |pcm_bytes| {
            server_for_audio.broadcast_audio(pcm_bytes);
        });

        let (preview_tx, preview_rx) = sync_channel::<LocalPreviewFrame>(2);
        self.local_preview_rx = Some(preview_rx);

        let cfg = Arc::clone(&self.stream_config);
        let bytes_per_sec = Arc::clone(&server.bytes_sent_per_sec);

        thread::spawn(move || {
            let mut sec_timer = Instant::now();
            let mut sec_bytes: u32 = 0;

            while !stop_flag.load(Ordering::Relaxed) {
                let frame_start = Instant::now();
                let fps = cfg.target_fps.load(Ordering::Relaxed).max(5);
                let frame_interval = Duration::from_nanos(1_000_000_000 / fps as u64);

                let target_w = cfg.target_w.load(Ordering::Relaxed);
                let target_h = cfg.target_h.load(Ordering::Relaxed);
                let quality = cfg.jpeg_quality.load(Ordering::Relaxed);
                let hide_preview = cfg.eco_hide_preview.load(Ordering::Relaxed);
                let draw_cursor = cfg.draw_cursor.load(Ordering::Relaxed);
                let has_viewers = server.viewer_count.load(Ordering::Relaxed) > 0;

                if hide_preview && !has_viewers {
                    thread::sleep(Duration::from_millis(80));
                    continue;
                }

                let source_opt = cfg.active_source.lock().ok().and_then(|g| g.clone());
                if let Some(source) = source_opt {
                    if let Some(raw) =
                        capture_frame(&source, target_w, target_h, !hide_preview, draw_cursor)
                    {
                        if has_viewers {
                            let mut jpeg_buf =
                                Vec::with_capacity((raw.width * raw.height / 5) as usize);
                            let mut encoder = Encoder::new(&mut jpeg_buf, quality);
                            // Subsampling 4:2:0 reduz o tamanho do quadro em ~45% mantendo nitidez,
                            // evitando qualquer gargalo no túnel de internet!
                            encoder.set_sampling_factor(SamplingFactor::F_2_2);
                            if encoder
                                .encode(
                                    &raw.rgb,
                                    raw.width as u16,
                                    raw.height as u16,
                                    ColorType::Rgb,
                                )
                                .is_ok()
                            {
                                sec_bytes = sec_bytes.saturating_add(jpeg_buf.len() as u32);
                                server.broadcast_frame(
                                    raw.width as u16,
                                    raw.height as u16,
                                    fps,
                                    &jpeg_buf,
                                );
                            }
                        }

                        if !hide_preview {
                            let _ = preview_tx.try_send(LocalPreviewFrame {
                                width: raw.width,
                                height: raw.height,
                                rgba: raw.rgba,
                            });
                            ctx.request_repaint();
                        }
                    }
                }

                if sec_timer.elapsed() >= Duration::from_secs(1) {
                    bytes_per_sec.store(sec_bytes, Ordering::Relaxed);
                    sec_bytes = 0;
                    sec_timer = Instant::now();
                }

                let elapsed = frame_start.elapsed();
                if elapsed < frame_interval {
                    thread::sleep(frame_interval - elapsed);
                }
            }
        });

        self.is_sharing = true;
        self.show_share_modal = false;
        self.show_toast(format!("Transmitindo vídeo e áudio em {}!", addr_msg));
    }

    fn stop_sharing(&mut self) {
        if let Some(flag) = self.capture_stop_flag.take() {
            flag.store(true, Ordering::Relaxed);
        }
        if let Some(server) = self.host_server.take() {
            server.stop();
        }
        self.local_preview_rx = None;
        self.is_sharing = false;
        self.local_video_texture = None;
        self.local_measured_fps = 0;
        self.local_measured_mbps = 0.0;
        if self.focused_tile_id == Some(LOCAL_PREVIEW_TILE_ID) {
            self.focused_tile_id = None;
        }
    }

    /// Adiciona uma nova transmissão remota ao palco sem fechar as outras que já estão abertas
    fn add_remote_stream(&mut self, ctx: &Context, raw_addr: String, custom_label: Option<String>) {
        let trimmed = raw_addr.trim().to_string();
        if trimmed.is_empty() {
            return;
        }

        // Evita abrir duas conexões idênticas para o mesmo código/endereço
        if self
            .remote_streams
            .iter()
            .any(|s| s.client.display_label.eq_ignore_ascii_case(&trimmed) || s.client.remote_addr.eq_ignore_ascii_case(&trimmed))
        {
            self.show_toast(format!("Você já está assistindo {}!", trimmed));
            return;
        }

        if self.remote_streams.len() >= 6 {
            self.show_toast("Limite máximo de 6 transmissões simultâneas atingido.");
            return;
        }

        let client = StreamViewerClient::connect(&trimmed, ctx.clone());
        let id = self.next_stream_id;
        self.next_stream_id += 1;

        let label = custom_label.unwrap_or_else(|| format!("Sala {}", client.display_label));
        self.show_toast(format!("Conectando a {}...", label));

        self.remote_streams.push(RemoteStreamEntry {
            id,
            label,
            client,
            texture: None,
            video_size: (0, 0),
            measured_fps: 0,
            measured_mbps: 0.0,
            frame_counter: 0,
            bytes_counter: 0,
        });
    }

    fn remove_remote_stream(&mut self, id: u64) {
        if let Some(pos) = self.remote_streams.iter().position(|s| s.id == id) {
            let entry = self.remote_streams.remove(pos);
            entry.client.stop();
            if self.focused_tile_id == Some(id) {
                self.focused_tile_id = None;
            }
        }
    }

    fn stop_all_remote_streams(&mut self) {
        for entry in self.remote_streams.drain(..) {
            entry.client.stop();
        }
        if self.focused_tile_id != Some(LOCAL_PREVIEW_TILE_ID) {
            self.focused_tile_id = None;
        }
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
                                        "Crie sua conta para entrar e compartilhar telas com seus amigos",
                                    )
                                    .size(13.0)
                                    .color(Color32::from_rgb(181, 186, 193)),
                                );
                            });

                            ui.add_space(16.0);

                            // Avatar + Botão de escolher foto
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
                                RichText::new("SEU NOME DE EXIBIÇÃO (COMO SEUS AMIGOS VÃO TE VER) *")
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
                                self.show_toast(format!(
                                    "Bem-vindo(a), {}!",
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

        egui::Window::new("🖥️ Compartilhar sua Tela")
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
                            RichText::new("Escolha o que deseja transmitir")
                                .size(17.0)
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.label(
                            RichText::new(
                                "Selecione um aplicativo/jogo, uma guia do navegador (Chrome, Edge, Brave, Opera, Firefox) ou a tela inteira",
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

                let mut double_clicked_go_live = false;

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
                                            double_clicked_go_live = true;
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
                                    let go_btn = egui::Button::new(
                                        RichText::new("🔴 Ir Ao Vivo")
                                            .size(13.5)
                                            .strong()
                                            .color(Color32::WHITE),
                                    )
                                    .fill(Color32::from_rgb(35, 165, 90))
                                    .min_size(Vec2::new(125.0, 32.0));

                                    if ui.add(go_btn).clicked() {
                                        double_clicked_go_live = true;
                                    }
                                },
                            );
                        });
                    });

                if double_clicked_go_live {
                    self.start_sharing(ctx.clone());
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
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.custom.display_name)
                                            .desired_width(135.0),
                                    );
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
                                        RichText::new("🚪 Trocar de Conta / Sair")
                                            .color(Color32::from_rgb(242, 63, 67)),
                                    )
                                    .clicked()
                                {
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
                                        &mut self.eco_hide_preview,
                                        "🎮 Modo Jogo (Ocultar prévia local enquanto transmite)",
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

    /// Renderiza um bloco de vídeo individual (seja transmissão remota de um amigo ou sua própria prévia)
    fn render_stream_tile(
        ui: &mut egui::Ui,
        tile_rect: egui::Rect,
        tile_id: u64,
        title: &str,
        subtitle: &str,
        texture: Option<&TextureHandle>,
        is_remote: bool,
        volume_atomic: Option<&Arc<AtomicU32>>,
        muted_atomic: Option<&Arc<AtomicBool>>,
        is_focused: bool,
        total_tiles: usize,
        card_bg: Color32,
        accent: Color32,
        focus_action: &mut Option<u64>,
        unfocus_action: &mut bool,
        close_action: &mut Option<u64>,
    ) {
        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(tile_rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );

        let rounding = Rounding::same(8.0);
        child_ui.painter().rect(
            tile_rect,
            rounding,
            Color32::from_rgb(12, 13, 16),
            Stroke::new(1.2, Color32::from_rgba_unmultiplied(255, 255, 255, 24)),
        );

        let header_h = 34.0;
        let video_area_h = (tile_rect.height() - header_h).max(20.0);
        let video_rect = egui::Rect::from_min_size(
            tile_rect.min,
            Vec2::new(tile_rect.width(), video_area_h),
        );

        // Desenha o quadro de vídeo mantendo a proporção 16:9 / nativa
        if let Some(tex) = texture {
            let tex_size = tex.size_vec2();
            if tex_size.x > 0.0 && tex_size.y > 0.0 {
                let scale =
                    ((video_rect.width() - 6.0) / tex_size.x).min((video_rect.height() - 6.0) / tex_size.y);
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
                "⏳ Recebendo transmissão...",
                FontId::proportional(13.5),
                Color32::from_rgb(160, 166, 174),
            );
        }

        // Clique duplo no vídeo para expandir/voltar para a grade
        let video_resp = child_ui.interact(
            video_rect,
            egui::Id::new(("tile_video_click", tile_id)),
            egui::Sense::click(),
        );
        if video_resp.double_clicked() && total_tiles > 1 {
            if is_focused {
                *unfocus_action = true;
            } else {
                *focus_action = Some(tile_id);
            }
        }

        // Barra inferior de controles individuais da tela
        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(tile_rect.min.x, tile_rect.max.y - header_h),
            Vec2::new(tile_rect.width(), header_h),
        );
        child_ui.painter().rect_filled(bar_rect, Rounding::same(6.0), card_bg);

        let mut bar_ui = child_ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar_rect.shrink2(Vec2::new(8.0, 4.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );

        bar_ui.colored_label(
            Color32::from_rgb(242, 63, 67),
            RichText::new("●").size(12.0).strong(),
        );
        bar_ui.label(
            RichText::new(title)
                .size(12.0)
                .strong()
                .color(Color32::WHITE),
        );
        bar_ui.label(
            RichText::new(subtitle)
                .size(11.0)
                .color(Color32::from_rgb(165, 170, 180)),
        );

        bar_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if is_remote {
                if ui
                    .small_button("✕")
                    .on_hover_text("Parar de assistir esta tela")
                    .clicked()
                {
                    *close_action = Some(tile_id);
                }
            }

            if total_tiles > 1 {
                let focus_lbl = if is_focused { "🗗 Grade" } else { "🔍 Expandir" };
                let btn = egui::Button::new(RichText::new(focus_lbl).size(11.0).color(Color32::WHITE))
                    .fill(if is_focused { accent } else { Color32::from_rgb(45, 48, 56) });
                if ui.add(btn).clicked() {
                    if is_focused {
                        *unfocus_action = true;
                    } else {
                        *focus_action = Some(tile_id);
                    }
                }
            }

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
                    .on_hover_text("Mutar/Desmutar som apenas desta tela")
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
        // Se o usuário ainda não criou sua conta neste PC, exibe a tela de criação de conta
        if !self.custom.account_created {
            self.render_account_onboarding(ctx);
            return;
        }

        // Recebe quadros locais (Host)
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

        // Recebe quadros de TODAS as transmissões remotas simultâneas (Multi-Stream)
        let mut disconnected_ids = Vec::new();
        let mut toast_to_show = None;
        let mut any_remote_active = false;

        for entry in &mut self.remote_streams {
            let mut latest_frame = None;
            while let Ok(f) = entry.client.frame_rx.try_recv() {
                entry.frame_counter += 1;
                entry.bytes_counter += f.byte_len;
                latest_frame = Some(f);
            }
            if let Some(f) = latest_frame {
                entry.video_size = (f.width, f.height);
                let img = ColorImage::from_rgba_unmultiplied(
                    [f.width as usize, f.height as usize],
                    &f.rgba,
                );
                match &mut entry.texture {
                    Some(tex) => tex.set(img, TextureOptions::LINEAR),
                    None => {
                        entry.texture = Some(ctx.load_texture(
                            format!("remote_stream_{}", entry.id),
                            img,
                            TextureOptions::LINEAR,
                        ));
                    }
                }
            }

            if let Ok(mut err_guard) = entry.client.error_msg.lock() {
                if let Some(e) = err_guard.take() {
                    toast_to_show = Some(format!("{}: {}", entry.label, e));
                    disconnected_ids.push(entry.id);
                }
            }

            if entry.client.connected.load(Ordering::Relaxed) {
                any_remote_active = true;
            }
        }

        if any_remote_active {
            ctx.request_repaint_after(Duration::from_millis(12));
        }

        for id in disconnected_ids {
            self.remove_remote_stream(id);
        }
        if let Some(msg) = toast_to_show {
            self.show_toast(msg);
        }

        // Atualiza telemetria e uso de RAM a cada 1 segundo
        let elapsed_sec = self.last_metrics_tick.elapsed().as_secs_f32();
        if elapsed_sec >= 1.0 {
            self.ram_mb = get_process_ram_mb();
            self.local_measured_fps =
                (self.local_frame_counter as f32 / elapsed_sec).round() as u32;
            if self.is_sharing {
                if let Some(srv) = &self.host_server {
                    let bps = srv.bytes_sent_per_sec.load(Ordering::Relaxed);
                    self.local_measured_mbps = (bps as f32 * 8.0) / 1_000_000.0;
                }
                if self.eco_hide_preview {
                    self.local_measured_fps = self.fps_preset as u32;
                }
            }
            self.local_frame_counter = 0;

            for entry in &mut self.remote_streams {
                entry.measured_fps = (entry.frame_counter as f32 / elapsed_sec).round() as u32;
                entry.measured_mbps =
                    ((entry.bytes_counter as f32 * 8.0) / elapsed_sec) / 1_000_000.0;
                entry.frame_counter = 0;
                entry.bytes_counter = 0;
            }

            self.last_metrics_tick = Instant::now();
        }

        let accent = self.custom.accent_color();
        let sidebar_bg = self.custom.sidebar_color();
        let stage_bg = self.custom.stage_color();
        let card_bg = self.custom.card_color();

        // ======================== BARRA LATERAL ESQUERDA (ESTILO DISCORD) ========================
        egui::SidePanel::left("sidebar_panel")
            .resizable(false)
            .exact_width(290.0)
            .frame(egui::Frame::none().fill(sidebar_bg))
            .show(ctx, |ui| {
                // Rodapé fixo do Perfil de Usuário
                egui::TopBottomPanel::bottom("discord_user_footer")
                    .exact_height(62.0)
                    .frame(
                        egui::Frame::none()
                            .fill(Color32::from_rgb(
                                self.custom.bg_sidebar_rgb[0].saturating_sub(7),
                                self.custom.bg_sidebar_rgb[1].saturating_sub(7),
                                self.custom.bg_sidebar_rgb[2].saturating_sub(8),
                            ))
                            .inner_margin(egui::Margin::symmetric(10.0, 8.0)),
                    )
                    .show_inside(ui, |ui| {
                        ui.horizontal_centered(|ui| {
                            draw_discord_avatar(
                                ui,
                                38.0,
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
                                            "🔴 {} {}fps",
                                            self.res_preset.short_label(),
                                            self.fps_preset
                                        )
                                    } else if !self.custom.custom_status.is_empty() {
                                        self.custom.custom_status.clone()
                                    } else {
                                        self.custom.presence.label().to_string()
                                    };
                                    let short_sub: String = sub.chars().take(20).collect();
                                    ui.label(
                                        RichText::new(short_sub)
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
                                        .on_hover_text("Personalizar Conta, Foto e Tema do App")
                                        .clicked()
                                    {
                                        self.show_settings_modal = true;
                                    }
                                    if ui
                                        .button("🖥️")
                                        .on_hover_text("Escolher Tela, Aplicativo ou Guia do Google")
                                        .clicked()
                                    {
                                        self.open_share_picker(ctx);
                                    }
                                },
                            );
                        });
                    });

                // Conteúdo principal da Barra Lateral
                egui::Frame::none()
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(accent, RichText::new("● SZdisc").strong().size(17.0));
                            ui.label(
                                RichText::new(format!("v{}", CURRENT_VERSION))
                                    .size(11.0)
                                    .color(Color32::from_rgb(148, 155, 164)),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(format!("RAM {:.0}MB", self.ram_mb))
                                            .size(10.5)
                                            .color(Color32::from_rgb(35, 165, 90)),
                                    );
                                },
                            );
                        });

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);

                        let share_label = if self.is_sharing {
                            "🖥️ Trocar Tela / Aplicativo / Guia"
                        } else {
                            "🖥️ Compartilhar Sua Tela"
                        };
                        let share_btn = egui::Button::new(
                            RichText::new(share_label)
                                .size(13.5)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(if self.is_sharing {
                            accent
                        } else {
                            Color32::from_rgb(35, 165, 90)
                        })
                        .min_size(Vec2::new(ui.available_width(), 36.0));

                        if ui.add(share_btn).clicked() {
                            self.open_share_picker(ctx);
                        }

                        if self.is_sharing {
                            ui.add_space(6.0);
                            if let Some(srv) = &self.host_server {
                                let addr = srv.listen_addr.clone();
                                let net_code = srv
                                    .internet_code
                                    .lock()
                                    .ok()
                                    .and_then(|g| g.clone());
                                let viewers = srv.viewer_count.load(Ordering::Relaxed);

                                egui::Frame::none()
                                    .fill(Color32::from_rgb(20, 42, 30))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(35, 165, 90)))
                                    .rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::same(8.0))
                                    .show(ui, |ui| {
                                        if let Some(code) = net_code {
                                            ui.horizontal(|ui| {
                                                ui.colored_label(
                                                    Color32::from_rgb(134, 239, 172),
                                                    RichText::new(format!(
                                                        "🌐 Código (Sem Radmin): {}",
                                                        code
                                                    ))
                                                    .strong()
                                                    .size(12.5),
                                                );
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        if ui.small_button("Copiar").clicked() {
                                                            ctx.copy_text(code.clone());
                                                            self.show_toast(format!(
                                                                "Código {} copiado!",
                                                                code
                                                            ));
                                                        }
                                                    },
                                                );
                                            });
                                        } else {
                                            ui.label(
                                                RichText::new("🌐 Gerando código de internet...")
                                                    .size(11.5)
                                                    .color(Color32::from_rgb(250, 204, 21)),
                                            );
                                        }

                                        ui.add_space(2.0);
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!("🏠 LAN/VPN: {}", addr))
                                                    .size(11.0)
                                                    .color(Color32::from_rgb(181, 186, 193)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui.small_button("IP").clicked() {
                                                        ctx.copy_text(addr.clone());
                                                        self.show_toast("IP local copiado!");
                                                    }
                                                },
                                            );
                                        });

                                        ui.label(
                                            RichText::new(format!(
                                                "Assistindo você agora: {}",
                                                viewers
                                            ))
                                            .size(11.0)
                                            .color(Color32::from_rgb(134, 239, 172)),
                                        );
                                    });
                            }

                            ui.add_space(6.0);
                            let stop_btn = egui::Button::new(
                                RichText::new("⏹ Encerrar Minha Transmissão")
                                    .size(12.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(242, 63, 67))
                            .min_size(Vec2::new(ui.available_width(), 28.0));

                            if ui.add(stop_btn).clicked() {
                                self.stop_sharing();
                            }
                        }

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // Conectar em múltiplas transmissões ao mesmo tempo (Multi-Stream)
                        ui.label(
                            RichText::new("ASSISTIR AMIGOS (ATÉ 3+ TELAS SIMULTÂNEAS)")
                                .size(10.0)
                                .strong()
                                .color(Color32::from_rgb(148, 155, 164)),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.join_addr_input)
                                    .hint_text("Código (ex: 52735) ou IP")
                                    .desired_width(175.0),
                            );
                            let enter = resp.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if (ui.button("➕ Assistir").clicked() || enter)
                                && !self.join_addr_input.trim().is_empty()
                            {
                                let target = self.join_addr_input.trim().to_string();
                                self.join_addr_input.clear();
                                self.add_remote_stream(ctx, target, None);
                            }
                        });

                        // Lista de telas remotas abertas simultaneamente
                        if !self.remote_streams.is_empty() {
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "TELAS ABERTAS NO PALCO ({})",
                                        self.remote_streams.len()
                                    ))
                                    .size(10.0)
                                    .strong()
                                    .color(Color32::from_rgb(134, 239, 172)),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if self.remote_streams.len() > 1
                                            && ui.small_button("Fechar Todas").clicked()
                                        {
                                            self.stop_all_remote_streams();
                                        }
                                    },
                                );
                            });
                            ui.add_space(4.0);

                            let mut id_to_close = None;
                            let mut id_to_focus = None;
                            for s in &self.remote_streams {
                                egui::Frame::none()
                                    .fill(card_bg)
                                    .rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.colored_label(
                                                Color32::from_rgb(242, 63, 67),
                                                "●",
                                            );
                                            ui.label(
                                                RichText::new(&s.label)
                                                    .size(11.5)
                                                    .strong()
                                                    .color(Color32::WHITE),
                                            );
                                            ui.label(
                                                RichText::new(format!("{}fps", s.measured_fps))
                                                    .size(10.5)
                                                    .color(Color32::from_rgb(160, 166, 174)),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui.small_button("✕").clicked() {
                                                        id_to_close = Some(s.id);
                                                    }
                                                    if ui.small_button("🔍").clicked() {
                                                        if self.focused_tile_id == Some(s.id) {
                                                            id_to_focus = Some(u64::MAX);
                                                        } else {
                                                            id_to_focus = Some(s.id);
                                                        }
                                                    }
                                                },
                                            );
                                        });
                                    });
                                ui.add_space(3.0);
                            }

                            if let Some(id) = id_to_close {
                                self.remove_remote_stream(id);
                            }
                            if let Some(id) = id_to_focus {
                                if id == u64::MAX {
                                    self.focused_tile_id = None;
                                } else {
                                    self.focused_tile_id = Some(id);
                                }
                            }
                        }

                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("SALAS DESCOBERTAS NA REDE / VPN")
                                .size(10.0)
                                .strong()
                                .color(Color32::from_rgb(148, 155, 164)),
                        );
                        ui.add_space(4.0);

                        let rooms_snapshot: Vec<DiscoveredRoom> = self
                            .discovered_rooms
                            .lock()
                            .map(|m| m.values().cloned().collect())
                            .unwrap_or_default();

                        if rooms_snapshot.is_empty() {
                            ui.label(
                                RichText::new(
                                    "Use o Código de 5 dígitos acima para assistir amigos pela internet.",
                                )
                                .size(10.5)
                                .color(Color32::from_rgb(128, 132, 142)),
                            );
                        } else {
                            egui::ScrollArea::vertical()
                                .id_salt("lan_rooms_scroll")
                                .max_height(130.0)
                                .show(ui, |ui| {
                                    for room in rooms_snapshot {
                                        egui::Frame::none()
                                            .fill(card_bg)
                                            .rounding(Rounding::same(6.0))
                                            .inner_margin(egui::Margin::same(7.0))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.vertical(|ui| {
                                                        ui.label(
                                                            RichText::new(&room.host_name)
                                                                .strong()
                                                                .size(12.0)
                                                                .color(Color32::WHITE),
                                                        );
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "🔴 {} • {}",
                                                                room.res_label, room.addr
                                                            ))
                                                            .size(10.5)
                                                            .color(Color32::from_rgb(181, 186, 193)),
                                                        );
                                                    });
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            if ui
                                                                .small_button("➕ Assistir")
                                                                .clicked()
                                                            {
                                                                self.add_remote_stream(
                                                                    ctx,
                                                                    room.addr.clone(),
                                                                    Some(room.host_name.clone()),
                                                                );
                                                            }
                                                        },
                                                    );
                                                });
                                            });
                                        ui.add_space(4.0);
                                    }
                                });
                        }
                    });
            });

        // ======================== BARRA SUPERIOR DO PALCO (QUALIDADE + MULTI-STREAM + AUTO-UPDATER) ========================
        egui::TopBottomPanel::top("stage_top_bar")
            .exact_height(48.0)
            .frame(
                egui::Frame::none()
                    .fill(card_bg)
                    .inner_margin(egui::Margin::symmetric(14.0, 8.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if self.is_sharing || !self.remote_streams.is_empty() {
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

                    // Seletor de Resolução em Tempo Real (1080p, 720p, 480p, 360p)
                    ui.label(RichText::new("Resolução:").size(12.0));
                    let prev_res = self.res_preset;
                    egui::ComboBox::from_id_salt("combo_res")
                        .selected_text(self.res_preset.label())
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.res_preset,
                                ResolutionPreset::P1080,
                                ResolutionPreset::P1080.label(),
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

                    // Seletor de FPS em Tempo Real
                    ui.label(RichText::new("Quadros:").size(12.0));
                    let prev_fps = self.fps_preset;
                    egui::ComboBox::from_id_salt("combo_fps")
                        .selected_text(format!("{} FPS", self.fps_preset))
                        .width(72.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.fps_preset, 60, "60 FPS");
                            ui.selectable_value(&mut self.fps_preset, 30, "30 FPS");
                            ui.selectable_value(&mut self.fps_preset, 15, "15 FPS");
                        });
                    if self.fps_preset != prev_fps {
                        self.sync_live_config();
                    }

                    // Seletor de Nitidez
                    let prev_q = self.jpeg_quality;
                    let q_label = match self.jpeg_quality {
                        90 => "Ultra (90%)",
                        75 => "Alta (75%)",
                        62 => "Média (62%)",
                        _ => "Leve (45%)",
                    };
                    egui::ComboBox::from_id_salt("combo_quality")
                        .selected_text(q_label)
                        .width(92.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.jpeg_quality, 90, "Ultra (90%)");
                            ui.selectable_value(&mut self.jpeg_quality, 75, "Alta (75%)");
                            ui.selectable_value(&mut self.jpeg_quality, 62, "Média (62%)");
                            ui.selectable_value(&mut self.jpeg_quality, 45, "Leve (45%)");
                        });
                    if self.jpeg_quality != prev_q {
                        self.sync_live_config();
                    }

                    if self.is_sharing {
                        let prev_aud = self.share_audio_enabled;
                        ui.checkbox(&mut self.share_audio_enabled, "🔊 Áudio");
                        if self.share_audio_enabled != prev_aud {
                            self.sync_live_config();
                        }

                        if !self.remote_streams.is_empty() {
                            ui.checkbox(&mut self.show_own_preview_in_grid, "👁 Minha Tela");
                        } else {
                            let prev_eco = self.eco_hide_preview;
                            ui.checkbox(&mut self.eco_hide_preview, "Modo Jogo");
                            if self.eco_hide_preview != prev_eco {
                                self.sync_live_config();
                            }
                        }
                    }

                    if self.focused_tile_id.is_some() {
                        if ui
                            .button("🗗 Voltar para Grade Multi-Telas")
                            .clicked()
                        {
                            self.focused_tile_id = None;
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Botão de Auto-Update pelo GitHub
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
                            _ => {
                                if ui
                                    .button(format!("🔄 Atualizar (v{})", CURRENT_VERSION))
                                    .on_hover_text(
                                        "Clique para baixar a versão mais recente direto do GitHub e reiniciar automaticamente",
                                    )
                                    .clicked()
                                {
                                    download_and_apply_update_async(Arc::clone(
                                        &self.update_status,
                                    ));
                                }
                            }
                        }

                        if ui.button("🎨 Conta & Tema").clicked() {
                            self.show_settings_modal = true;
                        }
                    });
                });
            });

        // ======================== PALCO CENTRAL DE VÍDEO (SUPORTE A 1, 2, 3, 4+ TELAS SIMULTÂNEAS) ========================
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(stage_bg))
            .show(ctx, |ui| {
                let avail = ui.available_size();

                let include_local_tile = self.is_sharing
                    && !self.eco_hide_preview
                    && (self.remote_streams.is_empty() || self.show_own_preview_in_grid);

                let mut active_tile_ids: Vec<u64> = Vec::new();
                for s in &self.remote_streams {
                    active_tile_ids.push(s.id);
                }
                if include_local_tile {
                    active_tile_ids.push(LOCAL_PREVIEW_TILE_ID);
                }

                let total_tiles = active_tile_ids.len();

                if total_tiles > 0 {
                    // Verifica se há uma tela focada/expandida
                    let visible_ids: Vec<u64> = if let Some(foc) = self.focused_tile_id {
                        if active_tile_ids.contains(&foc) {
                            vec![foc]
                        } else {
                            self.focused_tile_id = None;
                            active_tile_ids.clone()
                        }
                    } else {
                        active_tile_ids.clone()
                    };

                    let stage_rect = ui.available_rect_before_wrap().shrink(8.0);
                    let gap = 8.0;
                    let count = visible_ids.len();

                    // Calcula os retângulos responsivos para 1, 2, 3 ou 4+ transmissões simultâneas
                    let mut tile_rects: Vec<(u64, egui::Rect)> = Vec::with_capacity(count);
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
                        // 2 telas em cima e 1 tela centralizada embaixo (estilo Discord Multi-Stream)
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
                        // Grade 2x2 ou 3x2 para 4 a 6 telas simultâneas
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
                            tile_rects.push((*tid, egui::Rect::from_min_size(min_pos, Vec2::new(cell_w, cell_h))));
                        }
                    }

                    let mut focus_action = None;
                    let mut unfocus_action = false;
                    let mut close_action = None;

                    for (tid, rect) in tile_rects {
                        let is_focused = self.focused_tile_id == Some(tid);
                        if tid == LOCAL_PREVIEW_TILE_ID {
                            let title = format!("{} (Sua Transmissão)", self.custom.display_name);
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
                                false,
                                None,
                                None,
                                is_focused,
                                total_tiles,
                                card_bg,
                                accent,
                                &mut focus_action,
                                &mut unfocus_action,
                                &mut close_action,
                            );
                        } else if let Some(entry) =
                            self.remote_streams.iter().find(|s| s.id == tid)
                        {
                            let sub = if entry.video_size.0 > 0 {
                                format!(
                                    "{}×{} • {} FPS • {:.1} Mbps",
                                    entry.video_size.0,
                                    entry.video_size.1,
                                    entry.measured_fps,
                                    entry.measured_mbps
                                )
                            } else {
                                "Conectado".to_string()
                            };
                            Self::render_stream_tile(
                                ui,
                                rect,
                                tid,
                                &entry.label,
                                &sub,
                                entry.texture.as_ref(),
                                true,
                                Some(&entry.client.volume_percent),
                                Some(&entry.client.muted),
                                is_focused,
                                total_tiles,
                                card_bg,
                                accent,
                                &mut focus_action,
                                &mut unfocus_action,
                                &mut close_action,
                            );
                        }
                    }

                    if unfocus_action {
                        self.focused_tile_id = None;
                    } else if let Some(fid) = focus_action {
                        self.focused_tile_id = Some(fid);
                    }
                    if let Some(cid) = close_action {
                        self.remove_remote_stream(cid);
                    }
                } else if self.is_sharing && self.eco_hide_preview {
                    ui.vertical_centered(|ui| {
                        ui.add_space(avail.y * 0.33);
                        ui.colored_label(
                            Color32::from_rgb(35, 165, 90),
                            RichText::new("● TRANSMITINDO AO VIVO (MODO JOGO ATIVO)")
                                .size(16.5)
                                .strong(),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "A prévia local está oculta para economizar sua placa de vídeo enquanto joga.",
                            )
                            .size(13.0)
                            .color(Color32::from_rgb(181, 186, 193)),
                        );
                        ui.add_space(10.0);
                        if ui.button("Mostrar Prévia Novamente").clicked() {
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
                            RichText::new(
                                "Compartilhe sua Tela ou Assista até 3+ Telas Simultaneamente",
                            )
                            .font(FontId::proportional(18.5))
                            .strong()
                            .color(Color32::WHITE),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "Transmita Jogos com Áudio Isolado ou cole os Códigos de 5 dígitos dos seus amigos na barra lateral\npara assistir várias telas ao mesmo tempo em grade (Multi-Stream).",
                            )
                            .size(12.5)
                            .color(Color32::from_rgb(160, 166, 174)),
                        );
                        ui.add_space(14.0);

                        ui.horizontal(|ui| {
                            let pad = ((ui.available_width() - 395.0) * 0.5).max(0.0);
                            ui.add_space(pad);

                            let share_hero = egui::Button::new(
                                RichText::new("🖥️ Compartilhar Tela / App / Guia")
                                    .size(14.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(accent)
                            .min_size(Vec2::new(240.0, 38.0));

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
                        .fixed_pos(egui::Pos2::new(14.0, avail.y - 220.0))
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

        self.render_share_picker_modal(ctx);
        self.render_settings_modal(ctx);
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SZdisc — Screen Share & Multi-Stream Leve em Rust (1080p 60fps + Áudio)")
            .with_inner_size([1220.0, 750.0])
            .with_min_inner_size([920.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "SZdisc",
        options,
        Box::new(|cc| Ok(Box::new(Disc2App::new(cc)))),
    )
}
