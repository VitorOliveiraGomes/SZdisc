#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod network;
mod profile_theme;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use capture::{
    capture_frame, capture_thumbnail_rgba, get_process_ram_mb, list_capture_sources, CaptureSource,
    SourceCategory,
};
use eframe::egui::{
    self, Color32, ColorImage, Context, FontId, RichText, Rounding, Stroke, TextureHandle,
    TextureOptions, Vec2,
};
use jpeg_encoder::{ColorType, Encoder};
use network::{
    start_lan_discovery_listener, DiscoveredRoom, StreamHostServer, StreamViewerClient,
};
use profile_theme::{
    draw_discord_avatar, draw_discord_profile_card, PresenceStatus, ThemePreset, UserCustomization,
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
    active_source: Mutex<Option<CaptureSource>>,
}

struct LocalPreviewFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

struct Disc2App {
    // Personalização do Perfil e do Aplicativo
    custom: UserCustomization,
    show_settings_modal: bool,
    settings_tab: SettingsTab,
    show_profile_popup: bool,

    // Seletor de Tela estilo Discord (Modal com Miniaturas)
    show_share_modal: bool,
    share_tab: SourceCategory,
    sources: Vec<CaptureSource>,
    source_thumbnails: HashMap<String, TextureHandle>,
    selected_source_idx: usize,

    // Qualidade da Transmissão (Padrão: 1080p 60fps, alterável em tempo real)
    res_preset: ResolutionPreset,
    fps_preset: u8,
    jpeg_quality: u8,
    eco_hide_preview: bool,

    // Estado de Transmissão Local (Host)
    is_sharing: bool,
    stream_config: Arc<LiveStreamConfig>,
    capture_stop_flag: Option<Arc<AtomicBool>>,
    local_preview_rx: Option<Receiver<LocalPreviewFrame>>,
    host_server: Option<Arc<StreamHostServer>>,
    host_res_label: Arc<Mutex<String>>,

    // Estado de Visualização Remota (Viewer)
    join_addr_input: String,
    viewer_client: Option<StreamViewerClient>,

    // Salas Descobertas na Rede / VPN
    discovered_rooms: Arc<Mutex<HashMap<String, DiscoveredRoom>>>,

    // Textura do Vídeo no Palco e Telemetria
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
        let custom = UserCustomization::load_from_disk();
        custom.apply_to_egui(&cc.egui_ctx);

        let sources = list_capture_sources();
        let stream_config = Arc::new(LiveStreamConfig {
            target_w: AtomicU32::new(1920),
            target_h: AtomicU32::new(1080),
            target_fps: AtomicU8::new(60),
            jpeg_quality: AtomicU8::new(78),
            eco_hide_preview: AtomicBool::new(false),
            draw_cursor: AtomicBool::new(custom.show_cursor_in_stream),
            active_source: Mutex::new(sources.first().cloned()),
        });

        Self {
            custom,
            show_settings_modal: false,
            settings_tab: SettingsTab::Profile,
            show_profile_popup: false,
            show_share_modal: false,
            share_tab: SourceCategory::Screen,
            sources,
            source_thumbnails: HashMap::new(),
            selected_source_idx: 0,
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

    fn open_share_picker(&mut self, ctx: &Context) {
        self.refresh_sources_and_thumbnails(ctx);
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
                                Vec::with_capacity((raw.width * raw.height / 4) as usize);
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
        self.show_toast(format!("Transmitindo em {}!", addr_msg));
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
            .fixed_size(Vec2::new(760.0, 520.0))
            .frame(
                egui::Frame::none()
                    .fill(card_bg)
                    .rounding(Rounding::same(12.0))
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
                                "Selecione um aplicativo/jogo, uma guia do navegador (Google Chrome/Edge/etc.) ou a tela inteira",
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

                // Abas estilo Discord: Aplicativos | Guias do Navegador | Telas Inteiras
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
                                    format!("🌐 Guias do Google / Navegador ({})", count_browsers),
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
                                        .size(12.5)
                                        .strong()
                                        .color(if active {
                                            Color32::WHITE
                                        } else {
                                            Color32::from_rgb(181, 186, 193)
                                        }),
                                )
                                .fill(if active { accent } else { Color32::TRANSPARENT })
                                .min_size(Vec2::new(220.0, 30.0));

                                if ui.add(btn).clicked() {
                                    self.select_first_in_category(cat);
                                }
                            }
                        });
                    });

                ui.add_space(10.0);

                // Grade de Cards com Miniaturas Reais (3 colunas igual ao Discord)
                let filtered_indices: Vec<usize> = self
                    .sources
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.category == self.share_tab)
                    .map(|(i, _)| i)
                    .collect();

                let mut double_clicked_go_live = false;

                egui::ScrollArea::vertical()
                    .id_salt("modal_sources_grid")
                    .max_height(280.0)
                    .show(ui, |ui| {
                        if filtered_indices.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(60.0);
                                ui.label(
                                    RichText::new(match self.share_tab {
                                        SourceCategory::Browser => {
                                            "Nenhum navegador aberto no momento (abra o Google Chrome, Edge, Brave ou Firefox e clique em Atualizar)."
                                        }
                                        SourceCategory::Application => {
                                            "Nenhum outro aplicativo aberto encontrado."
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
                                            Color32::from_rgba_unmultiplied(255, 255, 255, 18)
                                        };
                                        let card_fill = if is_selected {
                                            sidebar_bg. linear_multiply(1.25)
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
                                                ui.set_width(222.0);
                                                ui.vertical(|ui| {
                                                    if let Some(tex) =
                                                        self.source_thumbnails.get(&src.id)
                                                    {
                                                        ui.image((
                                                            tex.id(),
                                                            Vec2::new(222.0, 124.0),
                                                        ));
                                                    } else {
                                                        let (r, _) = ui.allocate_exact_size(
                                                            Vec2::new(222.0, 124.0),
                                                            egui::Sense::hover(),
                                                        );
                                                        ui.painter().rect_filled(
                                                            r,
                                                            Rounding::same(5.0),
                                                            Color32::from_rgb(12, 13, 15),
                                                        );
                                                    }

                                                    ui.add_space(5.0);
                                                    let short_title = if src.name.chars().count() > 27
                                                    {
                                                        let s: String =
                                                            src.name.chars().take(26).collect();
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

                // Painel de Qualidade da Transmissão estilo Discord
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
                                RichText::new("Taxa de Quadros:")
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
                                    .min_size(Vec2::new(135.0, 32.0));

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

    // ======================== MODAL DE PERSONALIZAÇÃO (PERFIL + TEMA DO APP) ========================
    fn render_settings_modal(&mut self, ctx: &Context) {
        if !self.show_settings_modal {
            return;
        }

        let accent = self.custom.accent_color();
        let card_bg = self.custom.card_color();
        let sidebar_bg = self.custom.sidebar_color();
        let mut theme_changed = false;

        egui::Window::new("⚙️ Personalização do Perfil e Aparência (Estilo Discord)")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .fixed_size(Vec2::new(760.0, 490.0))
            .frame(
                egui::Frame::none()
                    .fill(card_bg)
                    .rounding(Rounding::same(12.0))
                    .stroke(Stroke::new(1.5_f32, accent))
                    .inner_margin(egui::Margin::same(16.0)),
            )
            .show(ctx, |ui| {
                // Topo do Modal
                ui.horizontal(|ui| {
                    let tabs = [
                        (SettingsTab::Profile, "👤 Meu Perfil Discord"),
                        (SettingsTab::Appearance, "🎨 Temas e Cores do App"),
                        (SettingsTab::Stream, "⚡ Captura e Performance"),
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
                        .min_size(Vec2::new(180.0, 32.0));

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
                ui.add_space(10.0);

                ui.columns(2, |cols| {
                    // Coluna da Esquerda: Controles de Edição
                    match self.settings_tab {
                        SettingsTab::Profile => {
                            cols[0].label(
                                RichText::new("PERSONALIZAR SEU PERFIL")
                                    .size(11.0)
                                    .strong()
                                    .color(Color32::from_rgb(148, 155, 164)),
                            );
                            cols[0].add_space(6.0);

                            cols[0].horizontal(|ui| {
                                ui.label("Nome:");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.custom.display_name)
                                        .desired_width(150.0),
                                );
                                ui.label("Tag:");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.custom.user_tag)
                                        .desired_width(70.0),
                                );
                            });

                            cols[0].add_space(6.0);
                            cols[0].label("Recado / Status Personalizado:");
                            cols[0].add(
                                egui::TextEdit::singleline(&mut self.custom.custom_status)
                                    .desired_width(f32::INFINITY),
                            );

                            cols[0].add_space(6.0);
                            cols[0].label("Sobre Mim (Bio do Perfil):");
                            cols[0].add(
                                egui::TextEdit::multiline(&mut self.custom.bio)
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY),
                            );

                            cols[0].add_space(8.0);
                            cols[0].horizontal(|ui| {
                                ui.label("Status de Presença:");
                                egui::ComboBox::from_id_salt("presence_combo")
                                    .selected_text(self.custom.presence.label())
                                    .show_ui(ui, |ui| {
                                        for p in [
                                            PresenceStatus::Online,
                                            PresenceStatus::Idle,
                                            PresenceStatus::DoNotDisturb,
                                            PresenceStatus::Invisible,
                                        ] {
                                            ui.selectable_value(&mut self.custom.presence, p, p.label());
                                        }
                                    });
                            });

                            cols[0].add_space(8.0);
                            cols[0].horizontal(|ui| {
                                ui.label("Cor do Avatar:");
                                ui.color_edit_button_srgb(&mut self.custom.avatar_bg);
                                ui.add_space(12.0);
                                ui.label("Cor do Banner:");
                                ui.color_edit_button_srgb(&mut self.custom.banner_bg);
                            });

                            cols[0].add_space(6.0);
                            cols[0].checkbox(
                                &mut self.custom.avatar_ring_glow,
                                "Anel Neon em volta do Avatar",
                            );

                            cols[0].add_space(8.0);
                            cols[0].label(
                                RichText::new("INSÍGNIAS DO PERFIL (BADGES)")
                                    .size(11.0)
                                    .strong()
                                    .color(Color32::from_rgb(148, 155, 164)),
                            );
                            cols[0].horizontal_wrapped(|ui| {
                                ui.checkbox(&mut self.custom.badge_crown, "👑 Host");
                                ui.checkbox(&mut self.custom.badge_1080p, "⚡ 1080p60");
                                ui.checkbox(&mut self.custom.badge_rust, "🦀 Rust");
                                ui.checkbox(&mut self.custom.badge_gamer, "🎮 Pro");
                            });
                        }

                        SettingsTab::Appearance => {
                            cols[0].label(
                                RichText::new("TEMAS DO APLICATIVO")
                                    .size(11.0)
                                    .strong()
                                    .color(Color32::from_rgb(148, 155, 164)),
                            );
                            cols[0].add_space(6.0);

                            for preset in [
                                ThemePreset::DiscordDark,
                                ThemePreset::MidnightOled,
                                ThemePreset::CyberpunkNeon,
                                ThemePreset::EmeraldGamer,
                                ThemePreset::CrimsonRuby,
                                ThemePreset::ArcticOcean,
                            ] {
                                let is_sel = self.custom.theme_preset == preset;
                                if cols[0].selectable_label(is_sel, preset.label()).clicked() {
                                    self.custom.apply_preset_colors(preset);
                                    theme_changed = true;
                                }
                            }

                            cols[0].add_space(10.0);
                            cols[0].label(
                                RichText::new("CORES PERSONALIZADAS")
                                    .size(11.0)
                                    .strong()
                                    .color(Color32::from_rgb(148, 155, 164)),
                            );
                            cols[0].horizontal(|ui| {
                                ui.label("Destaque:");
                                if ui.color_edit_button_srgb(&mut self.custom.accent_rgb).changed() {
                                    self.custom.theme_preset = ThemePreset::Custom;
                                    theme_changed = true;
                                }
                                ui.label("Barra Lateral:");
                                if ui
                                    .color_edit_button_srgb(&mut self.custom.bg_sidebar_rgb)
                                    .changed()
                                {
                                    self.custom.theme_preset = ThemePreset::Custom;
                                    theme_changed = true;
                                }
                            });

                            cols[0].horizontal(|ui| {
                                ui.label("Palco:");
                                if ui.color_edit_button_srgb(&mut self.custom.bg_stage_rgb).changed() {
                                    self.custom.theme_preset = ThemePreset::Custom;
                                    theme_changed = true;
                                }
                                ui.label("Painéis:");
                                if ui.color_edit_button_srgb(&mut self.custom.bg_card_rgb).changed() {
                                    self.custom.theme_preset = ThemePreset::Custom;
                                    theme_changed = true;
                                }
                            });

                            cols[0].add_space(8.0);
                            cols[0].horizontal(|ui| {
                                ui.label("Arredondamento:");
                                if ui
                                    .add(egui::Slider::new(&mut self.custom.corner_radius, 0.0..=14.0).suffix(" px"))
                                    .changed()
                                {
                                    theme_changed = true;
                                }
                            });

                            cols[0].horizontal(|ui| {
                                ui.label("Escala da Interface:");
                                for (scale, lbl) in [(0.95_f32, "Compacta"), (1.0_f32, "Padrão"), (1.08_f32, "Grande")] {
                                    let active = (self.custom.ui_scale - scale).abs() < 0.02;
                                    if ui.selectable_label(active, lbl).clicked() {
                                        self.custom.ui_scale = scale;
                                        theme_changed = true;
                                    }
                                }
                            });
                        }

                        SettingsTab::Stream => {
                            cols[0].label(
                                RichText::new("PREFERÊNCIAS DE TRANSMISSÃO")
                                    .size(11.0)
                                    .strong()
                                    .color(Color32::from_rgb(148, 155, 164)),
                            );
                            cols[0].add_space(8.0);

                            if cols[0]
                                .checkbox(
                                    &mut self.custom.show_cursor_in_stream,
                                    "Mostrar cursor do mouse na transmissão de tela",
                                )
                                .changed()
                            {
                                self.sync_live_config();
                            }

                            cols[0].add_space(6.0);
                            cols[0].checkbox(
                                &mut self.eco_hide_preview,
                                "Modo Jogo (Ocultar prévia local para poupar GPU)",
                            );

                            cols[0].add_space(10.0);
                            cols[0].label(
                                RichText::new(
                                    "Dica: Você pode alterar entre 1080p, 720p, 480p e 360p e entre 60, 30 e 15 FPS diretamente no topo do palco a qualquer momento, mesmo com a transmissão aberta.",
                                )
                                .size(11.5)
                                .color(Color32::from_rgb(160, 166, 174)),
                            );
                        }
                    }

                    // Coluna da Direita: Prévia ao Vivo do Card de Perfil Estilo Discord
                    cols[1].vertical_centered(|ui| {
                        ui.label(
                            RichText::new("PRÉVIA DO SEU PERFIL NO APP")
                                .size(11.0)
                                .strong()
                                .color(Color32::from_rgb(148, 155, 164)),
                        );
                        ui.add_space(8.0);
                        draw_discord_profile_card(ui, &self.custom, self.is_sharing);
                    });
                });
            });

        if theme_changed {
            self.custom.apply_to_egui(ctx);
            self.custom.save_to_disk();
        }
    }
}

impl eframe::App for Disc2App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Recebe quadros locais
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

        // Recebe quadros remotos
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
                self.measured_mbps =
                    ((self.bytes_counter as f32 * 8.0) / elapsed_sec) / 1_000_000.0;
            }
            self.frame_counter = 0;
            self.bytes_counter = 0;
            self.last_metrics_tick = Instant::now();
        }

        let accent = self.custom.accent_color();
        let sidebar_bg = self.custom.sidebar_color();
        let stage_bg = self.custom.stage_color();
        let card_bg = self.custom.card_color();

        // ======================== BARRA LATERAL ESQUERDA (ESTILO DISCORD) ========================
        egui::SidePanel::left("sidebar_panel")
            .resizable(false)
            .exact_width(285.0)
            .frame(egui::Frame::none().fill(sidebar_bg))
            .show(ctx, |ui| {
                // Rodapé fixo do Perfil de Usuário (igual à barra inferior do Discord)
                egui::TopBottomPanel::bottom("discord_user_footer")
                    .exact_height(60.0)
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
                            let initial = self.custom.display_name.chars().next().unwrap_or('U');
                            draw_discord_avatar(
                                ui,
                                36.0,
                                initial,
                                self.custom.avatar_color(),
                                self.custom.presence,
                                self.custom.avatar_ring_glow,
                                accent,
                            );

                            let name_resp = ui
                                .vertical(|ui| {
                                    ui.label(
                                        RichText::new(&self.custom.display_name)
                                            .strong()
                                            .size(13.0)
                                            .color(Color32::WHITE),
                                    );
                                    let sub = if self.is_sharing {
                                        format!(
                                            "🔴 Ao Vivo {} {}fps",
                                            self.res_preset.short_label(),
                                            self.fps_preset
                                        )
                                    } else if !self.custom.custom_status.is_empty() {
                                        self.custom.custom_status.clone()
                                    } else {
                                        self.custom.presence.label().to_string()
                                    };
                                    let short_sub: String = sub.chars().take(21).collect();
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
                                        .on_hover_text("Personalizar Perfil e Tema do App")
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
                        // Cabeçalho da Sidebar
                        ui.horizontal(|ui| {
                            ui.colored_label(accent, RichText::new("● SZdisc").strong().size(17.0));
                            ui.label(
                                RichText::new("Rust Screen")
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

                        // Botão Principal de Compartilhar Tela (abre o modal visual estilo Discord)
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
                                let viewers = srv.viewer_count.load(Ordering::Relaxed);
                                egui::Frame::none()
                                    .fill(Color32::from_rgb(20, 42, 30))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(35, 165, 90)))
                                    .rounding(Rounding::same(6.0))
                                    .inner_margin(egui::Margin::same(8.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.colored_label(
                                                Color32::from_rgb(134, 239, 172),
                                                RichText::new(format!("Sala: {}", addr))
                                                    .strong()
                                                    .size(12.0),
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
                                            RichText::new(format!(
                                                "Assistindo agora: {}",
                                                viewers
                                            ))
                                            .size(11.0)
                                            .color(Color32::from_rgb(181, 186, 193)),
                                        );
                                    });
                            }

                            ui.add_space(6.0);
                            let stop_btn = egui::Button::new(
                                RichText::new("⏹ Encerrar Transmissão")
                                    .size(12.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(Color32::from_rgb(242, 63, 67))
                            .min_size(Vec2::new(ui.available_width(), 30.0));

                            if ui.add(stop_btn).clicked() {
                                self.stop_sharing();
                            }
                        }

                        ui.add_space(12.0);

                        // Participantes / Canal de Transmissão
                        ui.label(
                            RichText::new("NO PALCO AGORA")
                                .size(10.5)
                                .strong()
                                .color(Color32::from_rgb(148, 155, 164)),
                        );
                        ui.add_space(4.0);

                        egui::Frame::none()
                            .fill(card_bg)
                            .rounding(Rounding::same(7.0))
                            .inner_margin(egui::Margin::same(8.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let initial =
                                        self.custom.display_name.chars().next().unwrap_or('U');
                                    draw_discord_avatar(
                                        ui,
                                        30.0,
                                        initial,
                                        self.custom.avatar_color(),
                                        self.custom.presence,
                                        self.custom.avatar_ring_glow,
                                        accent,
                                    );
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(format!(
                                                "{} (Você)",
                                                self.custom.display_name
                                            ))
                                            .strong()
                                            .size(12.5)
                                            .color(Color32::WHITE),
                                        );
                                        let src_name = self
                                            .sources
                                            .get(self.selected_source_idx)
                                            .map(|s| s.name.as_str())
                                            .unwrap_or("Pronto");
                                        let short_src: String = src_name.chars().take(22).collect();
                                        ui.label(
                                            RichText::new(if self.is_sharing {
                                                short_src
                                            } else {
                                                "Na sala".to_string()
                                            })
                                            .size(10.5)
                                            .color(Color32::from_rgb(160, 166, 174)),
                                        );
                                    });

                                    if self.is_sharing {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                egui::Frame::none()
                                                    .fill(Color32::from_rgb(242, 63, 67))
                                                    .rounding(Rounding::same(4.0))
                                                    .inner_margin(egui::Margin::symmetric(5.0, 2.0))
                                                    .show(ui, |ui| {
                                                        ui.label(
                                                            RichText::new("LIVE")
                                                                .size(9.5)
                                                                .strong()
                                                                .color(Color32::WHITE),
                                                        );
                                                    });
                                            },
                                        );
                                    }
                                });
                            });

                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // Assistir Transmissão de Amigo
                        ui.label(
                            RichText::new("CONECTAR EM UMA SALA")
                                .size(10.5)
                                .strong()
                                .color(Color32::from_rgb(148, 155, 164)),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.join_addr_input)
                                    .hint_text("IP ou IP:Porta do amigo")
                                    .desired_width(175.0),
                            );
                            if ui.button("Entrar").clicked()
                                && !self.join_addr_input.trim().is_empty()
                            {
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
                            RichText::new("SALAS ATIVAS NA REDE / VPN")
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
                                RichText::new("Nenhuma outra transmissão ativa na rede.")
                                    .size(11.0)
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
                                                            if ui.small_button("Assistir").clicked()
                                                            {
                                                                self.start_watching(
                                                                    room.addr.clone(),
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

        // ======================== BARRA SUPERIOR DO PALCO (QUALIDADE EM TEMPO REAL) ========================
        egui::TopBottomPanel::top("stage_top_bar")
            .exact_height(48.0)
            .frame(
                egui::Frame::none()
                    .fill(card_bg)
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
                        .color(Color32::WHITE),
                    );

                    ui.separator();

                    // Seletor de Resolução em Tempo Real (1080p, 720p, 480p, 360p)
                    ui.label(RichText::new("Resolução:").size(12.0));
                    let prev_res = self.res_preset;
                    egui::ComboBox::from_id_salt("combo_res")
                        .selected_text(self.res_preset.label())
                        .width(122.0)
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

                    // Seletor de FPS em Tempo Real (60, 30, 15)
                    ui.label(RichText::new("Quadros:").size(12.0));
                    let prev_fps = self.fps_preset;
                    egui::ComboBox::from_id_salt("combo_fps")
                        .selected_text(format!("{} FPS", self.fps_preset))
                        .width(74.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.fps_preset, 60, "60 FPS");
                            ui.selectable_value(&mut self.fps_preset, 30, "30 FPS");
                            ui.selectable_value(&mut self.fps_preset, 15, "15 FPS");
                        });
                    if self.fps_preset != prev_fps {
                        self.sync_live_config();
                    }

                    // Seletor de Nitidez
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
                        .width(96.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.jpeg_quality, 90, "Ultra (90%)");
                            ui.selectable_value(&mut self.jpeg_quality, 78, "Alta (78%)");
                            ui.selectable_value(&mut self.jpeg_quality, 62, "Média (62%)");
                            ui.selectable_value(&mut self.jpeg_quality, 45, "Leve (45%)");
                        });
                    if self.jpeg_quality != prev_q {
                        self.sync_live_config();
                    }

                    if self.is_sharing {
                        let prev_eco = self.eco_hide_preview;
                        ui.checkbox(&mut self.eco_hide_preview, "Modo Jogo");
                        if self.eco_hide_preview != prev_eco {
                            self.sync_live_config();
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("🎨 Personalizar").clicked() {
                            self.show_settings_modal = true;
                        }

                        if self.is_sharing || self.viewer_client.is_some() {
                            ui.label(
                                RichText::new(format!(
                                    "{}×{} • {} FPS • {:.1} Mbps",
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
            .frame(egui::Frame::none().fill(stage_bg))
            .show(ctx, |ui| {
                let avail = ui.available_size();

                if self.is_sharing && self.eco_hide_preview {
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
                    // Tela Inicial Elegante Estilo Palco do Discord + Card do Seu Perfil
                    ui.vertical_centered(|ui| {
                        ui.add_space((avail.y * 0.14).max(16.0));
                        draw_discord_profile_card(ui, &self.custom, false);

                        ui.add_space(18.0);
                        ui.label(
                            RichText::new("Pronto para Compartilhar sua Tela em 1080p 60fps")
                                .font(FontId::proportional(19.0))
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "Escolha entre Aplicativos/Jogos, Guias do Google Chrome/Navegadores ou Tela Inteira.\nVocê pode alterar para 720p, 480p ou 360p a qualquer momento.",
                            )
                            .size(12.5)
                            .color(Color32::from_rgb(160, 166, 174)),
                        );
                        ui.add_space(14.0);

                        ui.horizontal(|ui| {
                            let pad = ((ui.available_width() - 390.0) * 0.5).max(0.0);
                            ui.add_space(pad);

                            let share_hero = egui::Button::new(
                                RichText::new("🖥️ Compartilhar Tela / App / Guia")
                                    .size(14.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(accent)
                            .min_size(Vec2::new(235.0, 38.0));

                            if ui.add(share_hero).clicked() {
                                self.open_share_picker(ctx);
                            }

                            let cust_btn = egui::Button::new(
                                RichText::new("🎨 Personalizar")
                                    .size(13.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(card_bg)
                            .min_size(Vec2::new(140.0, 38.0));

                            if ui.add(cust_btn).clicked() {
                                self.show_settings_modal = true;
                            }
                        });
                    });
                }

                // Popup do Perfil quando clicado no rodapé da barra lateral
                if self.show_profile_popup {
                    egui::Area::new(egui::Id::new("discord_profile_popover"))
                        .fixed_pos(egui::Pos2::new(14.0, avail.y - 210.0))
                        .show(ctx, |ui| {
                            draw_discord_profile_card(ui, &self.custom, self.is_sharing);
                        });
                }

                // Toast de status no rodapé
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

        // Modais Flutuantes
        self.render_share_picker_modal(ctx);
        self.render_settings_modal(ctx);
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SZdisc — Screen Share Leve em Rust (1080p 60fps)")
            .with_inner_size([1200.0, 740.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "SZdisc",
        options,
        Box::new(|cc| Ok(Box::new(Disc2App::new(cc)))),
    )
}
