use std::sync::atomic::Ordering;
use std::sync::Arc;

use eframe::egui::TextureHandle;

use crate::network::RoomHub;

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

    pub fn label(self) -> &'static str {
        match self {
            Self::P1080 => "1080p (Full HD)",
            Self::P900 => "900p (HD+ Gamer)",
            Self::P720 => "720p (HD Turbo 60fps)",
            Self::P480 => "480p (SD Rápido)",
            Self::P360 => "360p (Ultra Leve)",
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Profile,
    Appearance,
    Stream,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ServerModalTab {
    CreateServer,
    JoinServer,
}

/// Estado visual (textura na GPU e telemetria) de cada amigo transmitindo na sala
pub struct RemoteTileState {
    pub(crate) texture: TextureHandle,
    pub(crate) video_size: (u32, u32),
    pub(crate) measured_fps: u32,
    pub(crate) measured_mbps: f32,
    pub(crate) frame_counter: u32,
    pub(crate) bytes_counter: usize,
}

/// Ações disparadas pelos botões de controle de cada tela no palco
#[derive(Default)]
pub struct TileUiActions {
    pub focus_tile: Option<u32>,
    pub unfocus: bool,
    pub enter_fullscreen: Option<u32>,
    pub exit_fullscreen: bool,
    pub move_tile: Option<u32>,
    pub popout_to_monitor: Option<u32>,
}

/// Snapshot único por frame do estado de sala no `RoomHub`, evitando locks repetidos entre painéis
pub struct RoomStatusSnapshot {
    pub in_room: bool,
    pub connected_srv_id: String,
    pub active_room_name: String,
}

impl RoomStatusSnapshot {
    pub fn from_hub(room_hub: &Option<Arc<RoomHub>>) -> Self {
        let in_room = room_hub
            .as_ref()
            .map(|h| h.in_room.load(Ordering::Relaxed))
            .unwrap_or(false);

        let connected_srv_id = room_hub
            .as_ref()
            .and_then(|h| h.connected_server_id.lock().ok().map(|g| g.clone()))
            .unwrap_or_default();

        let active_room_name = room_hub
            .as_ref()
            .and_then(|h| h.active_room_name.lock().ok().map(|g| g.clone()))
            .unwrap_or_default();

        Self {
            in_room,
            connected_srv_id,
            active_room_name,
        }
    }
}
