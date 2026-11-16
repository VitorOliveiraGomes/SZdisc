use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{self, ColorImage, Context, TextureOptions};
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

use crate::app::Disc2App;
use crate::audio::spawn_system_audio_loopback;
use crate::capture::{
    capture_thumbnail_rgba, is_szdisc_in_foreground, list_capture_sources, CaptureSource,
    FastCapturer, SourceCategory,
};
use crate::ui::state::LOCAL_PREVIEW_TILE_ID;

pub struct LiveStreamConfig {
    pub target_w: AtomicU32,
    pub target_h: AtomicU32,
    pub target_fps: AtomicU8,
    pub jpeg_quality: AtomicU8,
    pub eco_hide_preview: AtomicBool,
    pub auto_game_eco: AtomicBool,
    pub draw_cursor: AtomicBool,
    pub share_system_audio: Arc<AtomicBool>,
    pub target_audio_pid: Arc<AtomicU32>,
    pub encoded_frames_counter: Arc<AtomicU32>,
    pub active_source: Mutex<Option<CaptureSource>>,
}

pub struct LocalPreviewFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Disc2App {
    pub(crate) fn set_main_fullscreen(&mut self, ctx: &Context, target: Option<u32>) {
        self.fullscreen_tile_id = target;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(target.is_some()));
    }

    pub(crate) fn open_share_picker(&mut self, ctx: &Context) {
        self.refresh_sources_and_thumbnails(ctx);
        if !self.sources.iter().any(|s| s.category == self.share_tab) {
            self.share_tab = SourceCategory::Screen;
        }
        self.show_share_modal = true;
    }

    pub(crate) fn refresh_sources_and_thumbnails(&mut self, ctx: &Context) {
        let prev_id = self.sources.get(self.selected_source_idx).map(|s| s.id.clone());
        self.sources = list_capture_sources();
        self.source_thumbnails.clear();

        for src in &self.sources {
            if let Some((w, h, rgba)) = capture_thumbnail_rgba(src, 272, 153) {
                let img = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                let tex =
                    ctx.load_texture(format!("thumb_{}", src.id), img, TextureOptions::LINEAR);
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

    pub(crate) fn select_first_in_category(&mut self, cat: SourceCategory) {
        self.share_tab = cat;
        if let Some(pos) = self.sources.iter().position(|s| s.category == cat) {
            self.selected_source_idx = pos;
        }
    }

    pub(crate) fn current_res_status_label(&self) -> String {
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

    pub(crate) fn sync_live_config(&mut self) {
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
    pub(crate) fn start_or_switch_sharing(&mut self, ctx: Context) {
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
        let (encode_tx, encode_rx) = sync_channel::<(u32, u32, u8, u8, bool, Vec<u8>)>(1);

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
                    Ok((width, height, fps, base_quality, send_to_peers, rgb)) => {
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
                            crate::bridge::publish_video_frame(
                                LOCAL_PREVIEW_TILE_ID,
                                width as u16,
                                height as u16,
                                &jpeg_buf,
                            );
                            if send_to_peers {
                                hub_for_encoder.broadcast_my_video(
                                    width as u16,
                                    height as u16,
                                    fps,
                                    &jpeg_buf,
                                );
                            }
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
                let has_ws = crate::bridge::has_ws_video_clients();

                // Quando você está focado dentro do jogo (SZdisc em 2º plano), pausa automaticamente
                // a prévia local para usar 0% de GPU/OpenGL extra e mandar 100% do FPS para seus amigos!
                let app_focused = is_szdisc_in_foreground();
                let need_local_preview = !hide_preview
                    && (!auto_eco || app_focused)
                    && last_preview_sent.elapsed() >= Duration::from_millis(33);

                if !need_local_preview && !has_peers && !has_ws {
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

                        if has_peers || has_ws {
                            let _ = encode_tx.try_send((
                                raw.width,
                                raw.height,
                                fps,
                                quality,
                                has_peers,
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
    pub(crate) fn stop_sharing(&mut self, ctx: &Context) {
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
    pub(crate) fn leave_room(&mut self, ctx: &Context) {
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

    pub(crate) fn upload_local_rgba_to_gpu(
        &mut self,
        ctx: &Context,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) {
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
}
