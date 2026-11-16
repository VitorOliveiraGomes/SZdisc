use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui::{self, ColorImage, Context, TextureHandle, TextureOptions};

use crate::bridge::BridgeServer;
use crate::capture::{get_process_ram_mb, list_capture_sources, CaptureSource, SourceCategory};
use crate::network::{derive_persistent_peer_id, RoomHub};
use crate::profile_theme::UserCustomization;
use crate::stream_controller::{LiveStreamConfig, LocalPreviewFrame};
use crate::ui;
use crate::ui::image_utils::{load_banner_texture, load_circular_avatar_texture};
use crate::ui::state::{
    RemoteTileState, ResolutionPreset, RoomStatusSnapshot, ServerModalTab, SettingsTab,
    LOCAL_PREVIEW_TILE_ID,
};
use crate::ui::theme::{apply_theme_to_egui, install_system_emoji_fonts};
use crate::updater::{check_for_updates_async, cleanup_old_exe, UpdateStatus};

pub struct Disc2App {
    // Personalização do Perfil, Conta e do Aplicativo
    pub(crate) custom: UserCustomization,
    pub(crate) user_avatar_tex: Option<TextureHandle>,
    pub(crate) user_banner_tex: Option<TextureHandle>,
    pub(crate) show_settings_modal: bool,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) show_profile_popup: bool,

    // Modal de Criar / Entrar em um Servidor (Estilo Discord)
    pub(crate) show_server_modal: bool,
    pub(crate) server_modal_tab: ServerModalTab,
    pub(crate) create_srv_name_input: String,
    pub(crate) create_srv_rooms_input: String,
    pub(crate) create_srv_color_rgb: [u8; 3],
    pub(crate) join_srv_invite_input: String,

    // Seletor de Tela estilo Discord (Modal com Miniaturas + Pesquisa)
    pub(crate) show_share_modal: bool,
    pub(crate) share_tab: SourceCategory,
    pub(crate) share_search_query: String,
    pub(crate) sources: Vec<CaptureSource>,
    pub(crate) source_thumbnails: HashMap<String, TextureHandle>,
    pub(crate) selected_source_idx: usize,

    // Qualidade da Transmissão e Áudio
    pub(crate) res_preset: ResolutionPreset,
    pub(crate) fps_preset: u8,
    pub(crate) jpeg_quality: u8,
    pub(crate) share_audio_enabled: bool,
    pub(crate) eco_hide_preview: bool,
    pub(crate) auto_game_eco: bool,
    pub(crate) show_own_preview_in_grid: bool,

    // Sistema Multi-Servidor + Salas Isoladas + Multi-Monitor + Tela Cheia
    pub(crate) room_hub: Option<Arc<RoomHub>>,
    pub(crate) new_room_name_input: String,
    pub(crate) remote_tiles: HashMap<u32, RemoteTileState>,
    pub(crate) tile_order: Vec<u32>,
    pub(crate) focused_tile_id: Option<u32>,
    pub(crate) fullscreen_tile_id: Option<u32>,
    pub(crate) popout_tiles: HashSet<u32>,
    pub(crate) popout_fullscreen: HashSet<u32>,

    // Estado de Captura Local
    pub(crate) is_sharing: bool,
    pub(crate) stream_config: Arc<LiveStreamConfig>,
    pub(crate) capture_stop_flag: Option<Arc<AtomicBool>>,
    pub(crate) local_preview_rx: Option<Receiver<LocalPreviewFrame>>,

    // Textura do Vídeo Local e Telemetria
    pub(crate) local_video_texture: Option<TextureHandle>,
    pub(crate) local_video_size: (u32, u32),
    pub(crate) local_measured_fps: u32,
    pub(crate) local_measured_mbps: f32,
    pub(crate) local_frame_counter: u32,
    pub(crate) ram_mb: f32,
    pub(crate) last_metrics_tick: Instant,
    pub(crate) status_toast: Option<(String, Instant)>,

    // Auto-Updater pelo GitHub
    pub(crate) update_status: Arc<Mutex<UpdateStatus>>,

    // Ponte HTTP RPC + WebSocket local para o SZweb
    pub(crate) bridge: Arc<BridgeServer>,
}

impl Disc2App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
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
        apply_theme_to_egui(&custom, &cc.egui_ctx);

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

        let bridge = BridgeServer::start(cc.egui_ctx.clone());

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
            bridge,
        }
    }

    pub(crate) fn show_toast(&mut self, msg: impl Into<String>) {
        self.status_toast = Some((msg.into(), Instant::now()));
    }
}

impl eframe::App for Disc2App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.poll_bridge_and_sync_snapshot(ctx);

        if !self.custom.account_created {
            ui::modals::render_account_onboarding(self, ctx);
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
        ui::windows::render_popout_windows(self, ctx);

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

        let room_snap = RoomStatusSnapshot::from_hub(&self.room_hub);

        // Ordem estrita de painéis egui:
        // server_rail -> sidebar -> top_bar -> stage (CentralPanel)
        if self.fullscreen_tile_id.is_none() {
            ui::panels::render_server_rail(self, ctx, &room_snap);
            ui::panels::render_sidebar(self, ctx, &room_snap);
            ui::panels::render_top_bar(self, ctx);
        }

        ui::panels::render_stage(self, ctx, &room_snap);
        ui::overlays::render_profile_popover(self, ctx);
        ui::widgets::render_toast(
            ctx,
            &self.status_toast,
            self.custom.card_color(),
            self.custom.accent_color(),
        );

        ui::modals::render_server_modal(self, ctx);
        ui::modals::render_share_picker_modal(self, ctx);
        ui::modals::render_settings_modal(self, ctx);
    }
}
