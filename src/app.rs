use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::capture::{get_process_ram_mb, list_capture_sources, CaptureSource};
use crate::network::{derive_persistent_peer_id, RoomHub};
use crate::profile_theme::{load_image_as_jpeg_data_url, UserCustomization};
use crate::stream_controller::{LiveStreamConfig, ResolutionPreset, LOCAL_PREVIEW_TILE_ID};
use crate::updater::{check_for_updates_async, cleanup_old_exe, UpdateStatus};

/// Núcleo headless do SZdisc (sem dependência de GUI nativa; controlado via SZweb).
pub struct AppCore {
    pub(crate) custom: UserCustomization,
    pub(crate) avatar_data_url: String,
    pub(crate) banner_data_url: String,

    pub(crate) sources: Vec<CaptureSource>,
    pub(crate) selected_source_idx: usize,

    pub(crate) res_preset: ResolutionPreset,
    pub(crate) fps_preset: u8,
    pub(crate) jpeg_quality: u8,
    pub(crate) share_audio_enabled: bool,
    pub(crate) eco_hide_preview: bool,
    pub(crate) auto_game_eco: bool,
    pub(crate) show_own_preview_in_grid: bool,

    pub(crate) room_hub: Option<Arc<RoomHub>>,
    pub(crate) tile_order: Vec<u32>,
    pub(crate) focused_tile_id: Option<u32>,
    pub(crate) popout_tiles: HashSet<u32>,

    pub(crate) is_sharing: bool,
    pub(crate) stream_config: Arc<LiveStreamConfig>,
    pub(crate) capture_stop_flag: Option<Arc<AtomicBool>>,

    pub(crate) local_measured_fps: u32,
    pub(crate) local_measured_mbps: f32,
    pub(crate) ram_mb: f32,
    pub(crate) last_metrics_tick: Instant,
    pub(crate) status_toast: Option<(String, Instant)>,

    pub(crate) update_status: Arc<Mutex<UpdateStatus>>,
}

impl AppCore {
    pub fn new() -> Self {
        cleanup_old_exe();
        let update_status = Arc::new(Mutex::new(UpdateStatus::Idle));
        check_for_updates_async(Arc::clone(&update_status));

        let mut custom = UserCustomization::load_from_disk();
        if custom.selected_server_id.is_empty() {
            if let Some(first) = custom.saved_servers.first() {
                custom.selected_server_id = first.server_id.clone();
            }
        }

        let avatar_data_url =
            load_image_as_jpeg_data_url(&custom.avatar_image_path, 128, 128).unwrap_or_default();
        let banner_data_url =
            load_image_as_jpeg_data_url(&custom.banner_image_path, 480, 120).unwrap_or_default();

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
        )
        .ok();

        let sources = list_capture_sources();
        let initial_pid = sources.first().map(|s| s.pid).unwrap_or(0);
        let stream_config = Arc::new(LiveStreamConfig {
            target_w: AtomicU32::new(1280),
            target_h: AtomicU32::new(720),
            target_fps: AtomicU8::new(60),
            jpeg_quality: AtomicU8::new(65),
            eco_hide_preview: AtomicBool::new(false),
            auto_game_eco: AtomicBool::new(true),
            draw_cursor: AtomicBool::new(custom.show_cursor_in_stream),
            share_system_audio: Arc::new(AtomicBool::new(true)),
            target_audio_pid: Arc::new(AtomicU32::new(initial_pid)),
            encoded_frames_counter: Arc::new(AtomicU32::new(0)),
            local_video_w: Arc::new(AtomicU32::new(0)),
            local_video_h: Arc::new(AtomicU32::new(0)),
            active_source: Mutex::new(sources.first().cloned()),
        });

        Self {
            custom,
            avatar_data_url,
            banner_data_url,
            sources,
            selected_source_idx: 0,
            res_preset: ResolutionPreset::P720,
            fps_preset: 60,
            jpeg_quality: 65,
            share_audio_enabled: true,
            eco_hide_preview: false,
            auto_game_eco: true,
            show_own_preview_in_grid: true,
            room_hub,
            tile_order: Vec::new(),
            focused_tile_id: None,
            popout_tiles: HashSet::new(),
            is_sharing: false,
            stream_config,
            capture_stop_flag: None,
            local_measured_fps: 0,
            local_measured_mbps: 0.0,
            ram_mb: get_process_ram_mb(),
            last_metrics_tick: Instant::now(),
            status_toast: None,
            update_status,
        }
    }

    pub(crate) fn show_toast(&mut self, msg: impl Into<String>) {
        self.status_toast = Some((msg.into(), Instant::now()));
    }

    /// Executa manutenção periódica de salas, pop-outs e telemetria (chamado a cada ~200ms).
    pub(crate) fn tick(&mut self) {
        let mut hub_notice = None;
        let mut servers_dirty = false;
        let mut active_remote_ids = Vec::new();

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
            if let Ok(streams_map) = hub.remote_streams.lock() {
                active_remote_ids.extend(streams_map.keys().copied());
            }
        }

        if servers_dirty {
            self.custom.save_to_disk();
        }
        if let Some(msg) = hub_notice {
            self.show_toast(msg);
        }

        self.popout_tiles
            .retain(|tid| *tid == LOCAL_PREVIEW_TILE_ID || active_remote_ids.contains(tid));

        if let Some(foc) = self.focused_tile_id {
            let valid = (foc == LOCAL_PREVIEW_TILE_ID && self.is_sharing)
                || active_remote_ids.contains(&foc);
            if !valid {
                self.focused_tile_id = None;
            }
        }

        let elapsed_sec = self.last_metrics_tick.elapsed().as_secs_f32();
        if elapsed_sec >= 1.0 {
            self.ram_mb = get_process_ram_mb();
            let enc_frames = self
                .stream_config
                .encoded_frames_counter
                .swap(0, Ordering::Relaxed);
            self.local_measured_fps = (enc_frames as f32 / elapsed_sec).round() as u32;
            if self.is_sharing {
                if let Some(hub) = &self.room_hub {
                    let bps = hub.bytes_sent_per_sec.load(Ordering::Relaxed);
                    self.local_measured_mbps = (bps as f32 * 8.0) / 1_000_000.0;
                }
            } else {
                self.local_measured_mbps = 0.0;
            }

            if let Some(hub) = &self.room_hub {
                if let Ok(streams_map) = hub.remote_streams.lock() {
                    for stream in streams_map.values() {
                        let fc = stream.frame_counter.swap(0, Ordering::Relaxed);
                        let bc = stream.bytes_counter.swap(0, Ordering::Relaxed);
                        let fps = (fc as f32 / elapsed_sec).round() as u32;
                        let mbps = ((bc as f32 * 8.0) / elapsed_sec) / 1_000_000.0;
                        stream.measured_fps.store(fps, Ordering::Relaxed);
                        stream
                            .measured_mbps_x100
                            .store((mbps * 100.0).round() as u32, Ordering::Relaxed);
                    }
                }
            }

            self.last_metrics_tick = Instant::now();
        }
    }
}
