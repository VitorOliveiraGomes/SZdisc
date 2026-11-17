use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::{sync_channel, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

use crate::audio::spawn_system_audio_loopback;
use crate::capture::{is_screenz_in_foreground, list_capture_sources, CaptureSource, FastCapturer};

use super::state::AppCore;

pub const LOCAL_PREVIEW_TILE_ID: u32 = 0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResolutionPreset {
    P1080,
    P900,
    P720,
    P480,
    P360,
}

impl ResolutionPreset {
    pub fn dimensions(self) -> (u32, u32) {
        match self {
            Self::P1080 => (1920, 1080),
            Self::P900 => (1600, 900),
            Self::P720 => (1280, 720),
            Self::P480 => (854, 480),
            Self::P360 => (640, 360),
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::P1080 => "1080p",
            Self::P900 => "900p",
            Self::P720 => "720p",
            Self::P480 => "480p",
            Self::P360 => "360p",
        }
    }
}

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
    pub local_video_w: Arc<AtomicU32>,
    pub local_video_h: Arc<AtomicU32>,
    pub active_source: Mutex<Option<CaptureSource>>,
}

struct AliveGuard(Arc<AtomicBool>);
impl Drop for AliveGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

impl AppCore {
    pub fn current_res_status_label(&self) -> String {
        if self.is_sharing {
            let audio_tag = if self.share_audio_enabled {
                " 🔊"
            } else {
                ""
            };
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

    pub fn sync_live_config(&mut self) {
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

        if let Some(src) = self.active_source().cloned() {
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

    pub fn start_or_switch_sharing(&mut self) -> Result<(), String> {
        if self.sources.is_empty() {
            self.sources = list_capture_sources();
        }
        if self.sources.is_empty() {
            return Err("Nenhuma tela ou janela encontrada para capturar.".into());
        }
        if self.active_source().is_none() {
            if let Some(first) = self.sources.first() {
                self.active_source_id = first.id.clone();
            }
        }

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
                    return Err("Crie ou entre em um Servidor primeiro para transmitir!".into());
                }
            }
        }

        let threads_alive = self
            .capture_alive
            .as_ref()
            .is_some_and(|f| f.load(Ordering::Relaxed))
            && self
                .encoder_alive
                .as_ref()
                .is_some_and(|f| f.load(Ordering::Relaxed));

        if self.is_sharing && self.capture_stop_flag.is_some() && threads_alive {
            self.sync_live_config();
            let src_name = self
                .active_source()
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Nova tela".to_string());
            self.show_toast(format!("Tela alterada para: {}", src_name));
            return Ok(());
        }

        if let Some(old_stop) = self.capture_stop_flag.take() {
            old_stop.store(true, Ordering::Relaxed);
        }

        let Some(hub) = self.room_hub.as_ref().map(Arc::clone) else {
            return Err("Erro ao acessar a sala.".into());
        };

        self.is_sharing = true;
        self.sync_live_config();

        let stop_flag = Arc::new(AtomicBool::new(false));
        let cap_alive = Arc::new(AtomicBool::new(true));
        let enc_alive = Arc::new(AtomicBool::new(true));
        self.capture_stop_flag = Some(Arc::clone(&stop_flag));
        self.capture_alive = Some(Arc::clone(&cap_alive));
        self.encoder_alive = Some(Arc::clone(&enc_alive));

        let audio_stop = Arc::clone(&stop_flag);
        let audio_enabled = Arc::clone(&self.stream_config.share_system_audio);
        let audio_pid = Arc::clone(&self.stream_config.target_audio_pid);
        let hub_for_audio = Arc::clone(&hub);
        spawn_system_audio_loopback(audio_stop, audio_enabled, audio_pid, move |pcm_bytes| {
            hub_for_audio.broadcast_my_audio(pcm_bytes);
        });

        let (encode_tx, encode_rx) = sync_channel::<(u32, u32, u8, u8, bool, bool, Vec<u8>)>(1);
        let (recycle_tx, recycle_rx) = sync_channel::<Vec<u8>>(2);

        let cfg = Arc::clone(&self.stream_config);
        let bytes_per_sec = Arc::clone(&hub.bytes_sent_per_sec);
        let encoded_counter = Arc::clone(&self.stream_config.encoded_frames_counter);
        let video_w_atomic = Arc::clone(&self.stream_config.local_video_w);
        let video_h_atomic = Arc::clone(&self.stream_config.local_video_h);
        let stop_for_encoder = Arc::clone(&stop_flag);
        let hub_for_encoder = Arc::clone(&hub);

        thread::spawn(move || {
            let _alive = AliveGuard(enc_alive);
            let mut sec_timer = Instant::now();
            let mut sec_bytes: u32 = 0;
            let mut jpeg_buf = Vec::with_capacity(196_608);
            let mut adaptive_penalty: i16 = 0;

            while !stop_for_encoder.load(Ordering::Relaxed) {
                match encode_rx.recv_timeout(Duration::from_millis(60)) {
                    Ok((width, height, fps, base_quality, send_to_peers, send_to_ws, rgb)) => {
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
                            if fps >= 45 && frame_bytes > 26_000 {
                                adaptive_penalty = (adaptive_penalty + 4).min(30);
                            } else if frame_bytes < 16_000 && adaptive_penalty > 0 {
                                adaptive_penalty = (adaptive_penalty - 2).max(0);
                            }

                            sec_bytes = sec_bytes.saturating_add(frame_bytes as u32);
                            encoded_counter.fetch_add(1, Ordering::Relaxed);
                            video_w_atomic.store(width, Ordering::Relaxed);
                            video_h_atomic.store(height, Ordering::Relaxed);

                            if send_to_ws {
                                crate::bridge::publish_video_frame(
                                    LOCAL_PREVIEW_TILE_ID,
                                    width as u16,
                                    height as u16,
                                    &jpeg_buf,
                                );
                            }
                            if send_to_peers {
                                hub_for_encoder.broadcast_my_video(
                                    width as u16,
                                    height as u16,
                                    fps,
                                    &jpeg_buf,
                                );
                            }
                        }
                        let _ = recycle_tx.try_send(rgb);
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

        thread::spawn(move || {
            let _alive = AliveGuard(cap_alive);
            let mut fast_capturer = FastCapturer::new();
            let mut last_preview_sent = Instant::now() - Duration::from_secs(1);
            let mut last_ws_seq = crate::bridge::ws_connect_seq();
            let mut last_source_id = String::new();
            let mut preview_grace_until = Instant::now() + Duration::from_millis(1500);

            while !stop_flag.load(Ordering::Relaxed) {
                let frame_start = Instant::now();
                let ws_seq = crate::bridge::ws_connect_seq();
                if ws_seq != last_ws_seq {
                    last_ws_seq = ws_seq;
                    fast_capturer.force_next_frame();
                    last_preview_sent = Instant::now() - Duration::from_secs(1);
                    preview_grace_until = Instant::now() + Duration::from_millis(1200);
                }

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

                let source_opt = cfg.active_source.lock().ok().and_then(|g| g.clone());
                if let Some(ref src) = source_opt {
                    if src.id != last_source_id {
                        last_source_id = src.id.clone();
                        fast_capturer.force_next_frame();
                        preview_grace_until = Instant::now() + Duration::from_millis(1500);
                    }
                }

                // Durante os primeiros 1.5s ou após conexão WS garante entrega inicial (evita 0x0)
                let in_grace = Instant::now() < preview_grace_until;
                let need_local_preview = has_ws
                    && !hide_preview
                    && last_preview_sent.elapsed() >= Duration::from_millis(16)
                    && (in_grace || !auto_eco || is_screenz_in_foreground());

                if !need_local_preview && !has_peers {
                    thread::sleep(Duration::from_millis(60));
                    continue;
                }

                while let Ok(buf) = recycle_rx.try_recv() {
                    fast_capturer.recycle_rgb(buf);
                }

                if let Some(source) = source_opt {
                    if let Some(raw) = fast_capturer.capture_live_frame(
                        &source,
                        target_w,
                        target_h,
                        false,
                        draw_cursor,
                    ) {
                        if need_local_preview {
                            last_preview_sent = Instant::now();
                        }

                        match encode_tx.try_send((
                            raw.width,
                            raw.height,
                            fps,
                            quality,
                            has_peers,
                            need_local_preview,
                            raw.rgb,
                        )) {
                            Ok(()) => {}
                            Err(TrySendError::Full(frame)) => fast_capturer.recycle_rgb(frame.6),
                            Err(TrySendError::Disconnected(_)) => break,
                        }
                    }
                }

                let elapsed = frame_start.elapsed();
                if elapsed < frame_interval {
                    thread::sleep(frame_interval - elapsed);
                }
            }
        });

        self.show_toast("Transmissão iniciada na sala!");
        Ok(())
    }

    pub fn stop_sharing(&mut self) {
        if let Some(flag) = self.capture_stop_flag.take() {
            flag.store(true, Ordering::Relaxed);
        }
        self.capture_alive = None;
        self.encoder_alive = None;
        self.is_sharing = false;
        self.local_measured_fps = 0;
        self.local_measured_mbps = 0.0;
        self.stream_config.local_video_w.store(0, Ordering::Relaxed);
        self.stream_config.local_video_h.store(0, Ordering::Relaxed);
        self.popout_tiles.remove(&LOCAL_PREVIEW_TILE_ID);
        if self.focused_tile_id == Some(LOCAL_PREVIEW_TILE_ID) {
            self.focused_tile_id = None;
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

    pub fn leave_room(&mut self) {
        self.stop_sharing();
        self.popout_tiles.clear();
        self.focused_tile_id = None;
        if let Some(hub) = &self.room_hub {
            hub.leave_room_completely();
        }
        self.show_toast("Você saiu da sala.");
    }
}
