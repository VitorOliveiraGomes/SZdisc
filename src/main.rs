#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod network;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use capture::{capture_frame, get_process_ram_mb, list_capture_sources, CaptureSource};
use eframe::egui::{
    self, Color32, ColorImage, Context, FontId, RichText, Rounding, Stroke, TextureHandle,
    TextureOptions, Vec2,
};
use jpeg_encoder::{ColorType, Encoder};
use network::{
    start_lan_discovery_listener, DiscoveredRoom, StreamHostServer, StreamViewerClient,
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

struct LiveStreamConfig {
    target_w: AtomicU32,
    target_h: AtomicU32,
    target_fps: AtomicU8,
    jpeg_quality: AtomicU8,
    eco_hide_preview: AtomicBool,
    active_source: Mutex<Option<CaptureSource>>,
}

struct LocalPreviewFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

struct Disc2App {
    username: String,
    sources: Vec<CaptureSource>,
    selected_source_idx: usize,
    filter_screens_only: bool,

    // Qualidade (alterável em tempo real)
    res_preset: ResolutionPreset,
    fps_preset: u8,
    jpeg_quality: u8,
    eco_hide_preview: bool,

    // Estado de transmissão local (Host)
    is_sharing: bool,
    stream_config: Arc<LiveStreamConfig>,
    capture_stop_flag: Option<Arc<AtomicBool>>,
    local_preview_rx: Option<Receiver<LocalPreviewFrame>>,
    host_server: Option<Arc<StreamHostServer>>,
    host_res_label: Arc<Mutex<String>>,

    // Estado de visualização remota (Viewer)
    join_addr_input: String,
    viewer_client: Option<StreamViewerClient>,

    // Salas descobertas na rede / VPN
    discovered_rooms: Arc<Mutex<HashMap<String, DiscoveredRoom>>>,

    // Textura na GPU e telemetria
    video_texture: Option<TextureHandle>,
    current_video_size: (u32, u32),
    measured_fps: u32,
    measured_mbps: f32,
    ram_mb: f32,
    frame_counter: u32,
    bytes_counter: usize,
    last_metrics_tick: Instant,
    status_toast: Option<(String, Instant)>,
}

impl Disc2App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_discord_dark_theme(&cc.egui_ctx);

        let sources = list_capture_sources();
        let stream_config = Arc::new(LiveStreamConfig {
            target_w: AtomicU32::new(1920),
            target_h: AtomicU32::new(1080),
            target_fps: AtomicU8::new(60),
            jpeg_quality: AtomicU8::new(78),
            eco_hide_preview: AtomicBool::new(false),
            active_source: Mutex::new(sources.first().cloned()),
        });

        Self {
            username: "Jogador".to_string(),
            sources,
            selected_source_idx: 0,
            filter_screens_only: true,
            res_preset: ResolutionPreset::P1080,
            fps_preset: 60,
            jpeg_quality: 78,
            eco_hide_preview: false,
            is_sharing: false,
            stream_config,
            capture_stop_flag: None,
            local_preview_rx: None,
            host_server: None,
            host_res_label: Arc::new(Mutex::new("1080p 60fps".to_string())),
            join_addr_input: String::new(),
            viewer_client: None,
            discovered_rooms: start_lan_discovery_listener(),
            video_texture: None,
            current_video_size: (0, 0),
            measured_fps: 0,
            measured_mbps: 0.0,
            ram_mb: get_process_ram_mb(),
            frame_counter: 0,
            bytes_counter: 0,
            last_metrics_tick: Instant::now(),
            status_toast: None,
        }
    }

    fn show_toast(&mut self, msg: impl Into<String>) {
        self.status_toast = Some((msg.into(), Instant::now()));
    }

    fn refresh_sources(&mut self) {
        let prev_id = self.sources.get(self.selected_source_idx).map(|s| s.id.clone());
        self.sources = list_capture_sources();
        if let Some(id) = prev_id {
            if let Some(pos) = self.sources.iter().position(|s| s.id == id) {
                self.selected_source_idx = pos;
                return;
            }
        }
        self.selected_source_idx = 0;
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

        if let Some(src) = self.sources.get(self.selected_source_idx).cloned() {
            if let Ok(mut guard) = self.stream_config.active_source.lock() {
                *guard = Some(src);
            }
        }

        if let Ok(mut lbl) = self.host_res_label.lock() {
            *lbl = format!("{} {}fps", self.res_preset.short_label(), self.fps_preset);
        }
    }

    fn start_sharing(&mut self, ctx: Context) {
        self.stop_watching();
        self.stop_sharing();

        if self.sources.is_empty() {
            self.refresh_sources();
        }
        if self.sources.is_empty() {
            self.show_toast("Nenhuma tela ou janela encontrada para capturar.");
            return;
        }

        self.sync_live_config();

        let server = match StreamHostServer::start(
            self.username.clone(),
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
                let has_viewers = server.viewer_count.load(Ordering::Relaxed) > 0;

                // Se a prévia local estiver oculta (Modo Jogo) e não houver espectadores ainda, dorme sem gastar CPU/GPU
                if hide_preview && !has_viewers {
                    thread::sleep(Duration::from_millis(80));
                    continue;
                }

                let source_opt = cfg.active_source.lock().ok().and_then(|g| g.clone());
                if let Some(source) = source_opt {
                    if let Some(raw) = capture_frame(&source, target_w, target_h, !hide_preview) {
                        // Codifica e envia para os espectadores conectados
                        if has_viewers {
                            let mut jpeg_buf = Vec::with_capacity((raw.width * raw.height / 4) as usize);
                            let encoder = Encoder::new(&mut jpeg_buf, quality);
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

                        // Envia prévia para a própria tela se não estiver em Modo Jogo
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
        self.show_toast(format!("Transmitindo ao vivo em {}!", addr_msg));
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
        self.video_texture = None;
        self.measured_fps = 0;
        self.measured_mbps = 0.0;
    }

    fn start_watching(&mut self, addr: String) {
        self.stop_sharing();
        self.stop_watching();

        let client = StreamViewerClient::connect(&addr);
        self.show_toast(format!("Conectando a {}...", client.remote_addr));
        self.viewer_client = Some(client);
    }

    fn stop_watching(&mut self) {
        if let Some(client) = self.viewer_client.take() {
            client.stop();
        }
        self.video_texture = None;
        self.measured_fps = 0;
        self.measured_mbps = 0.0;
    }

    fn upload_rgba_to_gpu(&mut self, ctx: &Context, width: u32, height: u32, rgba: Vec<u8>) {
        let image = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
        self.current_video_size = (width, height);
        match &mut self.video_texture {
            Some(tex) => tex.set(image, TextureOptions::LINEAR),
            None => {
                self.video_texture =
                    Some(ctx.load_texture("stream_frame", image, TextureOptions::LINEAR));
            }
        }
    }
}

impl eframe::App for Disc2App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Recebe quadros da prévia local (Host)
        let mut new_local_frame = None;
        if let Some(rx) = &self.local_preview_rx {
            while let Ok(f) = rx.try_recv() {
                new_local_frame = Some(f);
                self.frame_counter += 1;
            }
        }
        if let Some(f) = new_local_frame {
            self.upload_rgba_to_gpu(ctx, f.width, f.height, f.rgba);
        }

        // Recebe quadros de uma transmissão remota (Viewer)
        let mut new_remote_frame = None;
        let mut remote_err = None;
        if let Some(client) = &self.viewer_client {
            while let Ok(f) = client.frame_rx.try_recv() {
                self.frame_counter += 1;
                self.bytes_counter += f.byte_len;
                new_remote_frame = Some(f);
            }
            if let Ok(mut err_guard) = client.error_msg.lock() {
                if let Some(e) = err_guard.take() {
                    remote_err = Some(e);
                }
            }
            if client.connected.load(Ordering::Relaxed) {
                ctx.request_repaint_after(Duration::from_millis(8));
            }
        }
        if let Some(f) = new_remote_frame {
            self.upload_rgba_to_gpu(ctx, f.width, f.height, f.rgba);
        }
        if let Some(e) = remote_err {
            self.stop_watching();
            self.show_toast(e);
        }

        // Atualiza telemetria e uso de RAM a cada 1 segundo
        let elapsed_sec = self.last_metrics_tick.elapsed().as_secs_f32();
        if elapsed_sec >= 1.0 {
            self.ram_mb = get_process_ram_mb();
            self.measured_fps = (self.frame_counter as f32 / elapsed_sec).round() as u32;
            if self.is_sharing {
                if let Some(srv) = &self.host_server {
                    let bps = srv.bytes_sent_per_sec.load(Ordering::Relaxed);
                    self.measured_mbps = (bps as f32 * 8.0) / 1_000_000.0;
                }
                if self.eco_hide_preview {
                    self.measured_fps = self.fps_preset as u32;
                }
            } else {
                self.measured_mbps = ((self.bytes_counter as f32 * 8.0) / elapsed_sec) / 1_000_000.0;
            }
            self.frame_counter = 0;
            self.bytes_counter = 0;
            self.last_metrics_tick = Instant::now();
        }

        // ======================== BARRA LATERAL ESQUERDA ========================
        egui::SidePanel::left("sidebar_panel")
            .resizable(false)
            .exact_width(275.0)
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(30, 31, 34))
                    .inner_margin(egui::Margin::same(12.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        Color32::from_rgb(88, 101, 242),
                        RichText::new("● DISC2").strong().size(16.0),
                    );
                    ui.label(
                        RichText::new("Rust Nativo")
                            .size(11.0)
                            .color(Color32::from_rgb(148, 155, 164)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("RAM: {:.1} MB", self.ram_mb))
                                .size(11.0)
                                .color(Color32::from_rgb(35, 165, 90)),
                        );
                    });
                });

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);

                // Perfil
                ui.label(
                    RichText::new("SEU APELIDO NA SALA")
                        .size(10.5)
                        .strong()
                        .color(Color32::from_rgb(148, 155, 164)),
                );
                ui.add_space(3.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.username)
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(12.0);

                // Seção: Fonte de Captura (Telas / Janelas)
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("FONTE DE TRANSMISSÃO")
                            .size(10.5)
                            .strong()
                            .color(Color32::from_rgb(148, 155, 164)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("↻ Atualizar").clicked() {
                            self.refresh_sources();
                        }
                    });
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(self.filter_screens_only, "Telas Inteiras")
                        .clicked()
                    {
                        self.filter_screens_only = true;
                        if let Some(pos) = self.sources.iter().position(|s| s.is_screen) {
                            self.selected_source_idx = pos;
                            self.sync_live_config();
                        }
                    }
                    if ui
                        .selectable_label(!self.filter_screens_only, "Janelas / Jogos")
                        .clicked()
                    {
                        self.filter_screens_only = false;
                        if let Some(pos) = self.sources.iter().position(|s| !s.is_screen) {
                            self.selected_source_idx = pos;
                            self.sync_live_config();
                        }
                    }
                });

                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("sources_scroll")
                    .max_height(135.0)
                    .show(ui, |ui| {
                        let mut changed_source = false;
                        for (idx, src) in self.sources.iter().enumerate() {
                            if src.is_screen != self.filter_screens_only {
                                continue;
                            }
                            let is_sel = idx == self.selected_source_idx;
                            let resp = ui.selectable_label(is_sel, &src.name);
                            if resp.clicked() {
                                self.selected_source_idx = idx;
                                changed_source = true;
                            }
                        }
                        if changed_source {
                            self.sync_live_config();
                        }
                    });

                ui.add_space(8.0);
                if !self.is_sharing {
                    let btn = egui::Button::new(
                        RichText::new("▶ Transmitir Minha Tela")
                            .size(13.5)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(35, 165, 90))
                    .min_size(Vec2::new(ui.available_width(), 34.0));

                    if ui.add(btn).clicked() {
                        self.start_sharing(ctx.clone());
                    }
                } else {
                    if let Some(srv) = &self.host_server {
                        let addr = srv.listen_addr.clone();
                        let viewers = srv.viewer_count.load(Ordering::Relaxed);
                        egui::Frame::none()
                            .fill(Color32::from_rgb(20, 42, 30))
                            .stroke(Stroke::new(1.0, Color32::from_rgb(35, 165, 90)))
                            .rounding(Rounding::same(6.0))
                            .inner_margin(egui::Margin::same(8.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.colored_label(
                                        Color32::from_rgb(134, 239, 172),
                                        RichText::new(format!("Sala: {}", addr)).strong().size(12.0),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.small_button("Copiar").clicked() {
                                                ctx.copy_text(addr.clone());
                                                self.show_toast("Endereço copiado!");
                                            }
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(format!("Espectadores conectados: {}", viewers))
                                        .size(11.0)
                                        .color(Color32::from_rgb(181, 186, 193)),
                                );
                            });
                    }

                    ui.add_space(6.0);
                    let stop_btn = egui::Button::new(
                        RichText::new("⏹ Parar Transmissão")
                            .size(13.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(242, 63, 67))
                    .min_size(Vec2::new(ui.available_width(), 32.0));

                    if ui.add(stop_btn).clicked() {
                        self.stop_sharing();
                    }
                }

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                // Seção: Assistir Transmissão de Amigo
                ui.label(
                    RichText::new("ASSISTIR TRANSMISSÃO")
                        .size(10.5)
                        .strong()
                        .color(Color32::from_rgb(148, 155, 164)),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.join_addr_input)
                            .hint_text("IP ou IP:Porta (ex: 192.168.1.10)")
                            .desired_width(175.0),
                    );
                    if ui.button("Conectar").clicked() && !self.join_addr_input.trim().is_empty() {
                        let target = self.join_addr_input.trim().to_string();
                        self.start_watching(target);
                    }
                });

                if self.viewer_client.is_some() {
                    ui.add_space(4.0);
                    if ui.button("Desconectar da Sala").clicked() {
                        self.stop_watching();
                    }
                }

                ui.add_space(10.0);
                ui.label(
                    RichText::new("SALAS DESCOBERTAS NA REDE / VPN")
                        .size(10.5)
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
                        RichText::new("Nenhuma outra sala aberta na rede local.")
                            .size(11.5)
                            .color(Color32::from_rgb(128, 132, 142)),
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("lan_rooms_scroll")
                        .max_height(130.0)
                        .show(ui, |ui| {
                            for room in rooms_snapshot {
                                egui::Frame::none()
                                    .fill(Color32::from_rgb(43, 45, 49))
                                    .rounding(Rounding::same(5.0))
                                    .inner_margin(egui::Margin::same(6.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(&room.host_name)
                                                        .strong()
                                                        .size(12.0),
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
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    if ui.small_button("Assistir").clicked() {
                                                        self.start_watching(room.addr.clone());
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

        // ======================== BARRA SUPERIOR DO PALCO (QUALIDADE EM TEMPO REAL) ========================
        egui::TopBottomPanel::top("stage_top_bar")
            .exact_height(48.0)
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(43, 45, 49))
                    .inner_margin(egui::Margin::symmetric(14.0, 8.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if self.is_sharing || self.viewer_client.is_some() {
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

                    ui.label(
                        RichText::new(format!(
                            "{} • {} FPS",
                            self.res_preset.short_label(),
                            self.fps_preset
                        ))
                        .strong()
                        .size(13.0)
                        .color(Color32::from_rgb(165, 180, 252)),
                    );

                    ui.separator();

                    // Seletor de Resolução (1080p, 720p, 480p, 360p)
                    ui.label(RichText::new("Resolução:").size(12.0));
                    let prev_res = self.res_preset;
                    egui::ComboBox::from_id_salt("combo_res")
                        .selected_text(self.res_preset.label())
                        .width(125.0)
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

                    // Seletor de FPS (60, 30, 15)
                    ui.label(RichText::new("Quadros:").size(12.0));
                    let prev_fps = self.fps_preset;
                    egui::ComboBox::from_id_salt("combo_fps")
                        .selected_text(format!("{} FPS", self.fps_preset))
                        .width(78.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.fps_preset, 60, "60 FPS");
                            ui.selectable_value(&mut self.fps_preset, 30, "30 FPS");
                            ui.selectable_value(&mut self.fps_preset, 15, "15 FPS");
                        });
                    if self.fps_preset != prev_fps {
                        self.sync_live_config();
                    }

                    // Seletor de Qualidade / Compressão
                    ui.label(RichText::new("Nitidez:").size(12.0));
                    let prev_q = self.jpeg_quality;
                    let q_label = match self.jpeg_quality {
                        90 => "Ultra (90%)",
                        78 => "Alta (78%)",
                        62 => "Média (62%)",
                        _ => "Leve (45%)",
                    };
                    egui::ComboBox::from_id_salt("combo_quality")
                        .selected_text(q_label)
                        .width(100.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.jpeg_quality, 90, "Ultra (90%)");
                            ui.selectable_value(&mut self.jpeg_quality, 78, "Alta (78%)");
                            ui.selectable_value(&mut self.jpeg_quality, 62, "Média (62%)");
                            ui.selectable_value(&mut self.jpeg_quality, 45, "Leve (45%)");
                        });
                    if self.jpeg_quality != prev_q {
                        self.sync_live_config();
                    }

                    // Modo Jogo (Ocultar Prévia Local)
                    if self.is_sharing {
                        let prev_eco = self.eco_hide_preview;
                        ui.checkbox(&mut self.eco_hide_preview, "Modo Jogo (Ocultar Prévia)");
                        if self.eco_hide_preview != prev_eco {
                            self.sync_live_config();
                        }
                    }

                    // Telemetria à direita
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.is_sharing || self.viewer_client.is_some() {
                            ui.label(
                                RichText::new(format!(
                                    "{}x{} • {} FPS • {:.1} Mbps",
                                    self.current_video_size.0,
                                    self.current_video_size.1,
                                    self.measured_fps,
                                    self.measured_mbps
                                ))
                                .size(11.5)
                                .color(Color32::from_rgb(181, 186, 193)),
                            );
                        }
                    });
                });
            });

        // ======================== PALCO CENTRAL DE VÍDEO ========================
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::from_rgb(14, 15, 18)))
            .show(ctx, |ui| {
                let avail = ui.available_size();

                if self.is_sharing && self.eco_hide_preview {
                    ui.vertical_centered(|ui| {
                        ui.add_space(avail.y * 0.35);
                        ui.colored_label(
                            Color32::from_rgb(35, 165, 90),
                            RichText::new("● TRANSMITINDO AO VIVO (MODO JOGO ATIVO)")
                                .size(16.0)
                                .strong(),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "A prévia local está desativada para consumir praticamente 0% de GPU enquanto você joga.",
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
                } else if let Some(tex) = &self.video_texture {
                    let tex_size = tex.size_vec2();
                    if tex_size.x > 0.0 && tex_size.y > 0.0 {
                        let scale = (avail.x / tex_size.x).min(avail.y / tex_size.y);
                        let draw_size = Vec2::new(tex_size.x * scale, tex_size.y * scale);
                        let pad_y = ((avail.y - draw_size.y) * 0.5).max(0.0);

                        ui.vertical_centered(|ui| {
                            ui.add_space(pad_y);
                            ui.image((tex.id(), draw_size));
                        });
                    }
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(avail.y * 0.30);
                        ui.label(
                            RichText::new("DISC2 — Transmissão Nativa em Rust (1080p 60fps)")
                                .font(FontId::proportional(20.0))
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(
                                "Escolha uma tela ou janela na barra lateral para transmitir, ou conecte na sala de um amigo.\nVocê pode reduzir a resolução (1080p → 720p → 480p → 360p) e os quadros (60 → 30 → 15 FPS) em tempo real.",
                            )
                            .size(13.0)
                            .color(Color32::from_rgb(160, 166, 174)),
                        );
                        ui.add_space(16.0);
                        let hero_btn = egui::Button::new(
                            RichText::new("▶ Iniciar Transmissão 1080p 60fps")
                                .size(14.5)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(88, 101, 242))
                        .min_size(Vec2::new(250.0, 38.0));

                        if ui.add(hero_btn).clicked() {
                            self.start_sharing(ctx.clone());
                        }
                    });
                }

                // Toast de status no rodapé
                if let Some((msg, when)) = &self.status_toast {
                    if when.elapsed() < Duration::from_secs(4) {
                        egui::Area::new(egui::Id::new("toast_area"))
                            .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -16.0))
                            .show(ctx, |ui| {
                                egui::Frame::none()
                                    .fill(Color32::from_rgb(49, 51, 56))
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(88, 101, 242)))
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
    }
}

fn configure_discord_dark_theme(ctx: &Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(30, 31, 34);
    visuals.window_fill = Color32::from_rgb(43, 45, 49);
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(43, 45, 49);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(49, 51, 56);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(64, 66, 73);
    visuals.widgets.active.bg_fill = Color32::from_rgb(88, 101, 242);
    visuals.selection.bg_fill = Color32::from_rgb(88, 101, 242);
    ctx.set_visuals(visuals);
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DISC2 — Screen Share Leve em Rust (1080p 60fps)")
            .with_inner_size([1180.0, 720.0])
            .with_min_inner_size([860.0, 520.0]),
        ..Default::default()
    };

    eframe::run_native(
        "DISC2",
        options,
        Box::new(|cc| Ok(Box::new(Disc2App::new(cc)))),
    )
}
