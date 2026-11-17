use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::capture::SourceCategory;
use crate::core::AppCore;
use crate::updater::{UpdateStatus, CURRENT_VERSION};

use super::{ws::ws_video_client_count, BRIDGE_PORT};

#[derive(Deserialize)]
pub struct RpcRequest {
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProfileDto {
    pub server_id: String,
    pub name: String,
    pub icon_rgb: [u8; 3],
    pub rooms: Vec<String>,
    pub invite_token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserCustomizationDto {
    pub account_created: bool,
    pub username_login: String,
    pub display_name: String,
    pub user_tag: String,
    pub custom_status: String,
    pub bio: String,
    pub presence_code: u8,
    pub avatar_data_url: String,
    pub banner_data_url: String,
    pub avatar_bg: [u8; 3],
    pub banner_bg: [u8; 3],
    #[serde(rename = "bannerBg2")]
    pub banner_bg_2: [u8; 3],
    pub name_color_rgb: [u8; 3],
    pub ring_color_rgb: [u8; 3],
    pub avatar_ring_glow: bool,
    pub ring_thickness: f32,
    #[serde(rename = "badge1080p")]
    pub badge_1080p: bool,
    pub badge_rust: bool,
    pub badge_gamer: bool,
    pub badge_crown: bool,
    pub custom_badge_enabled: bool,
    pub custom_badge_text: String,
    pub custom_badge_rgb: [u8; 3],
    pub theme_preset_code: u8,
    pub accent_rgb: [u8; 3],
    pub bg_sidebar_rgb: [u8; 3],
    pub bg_stage_rgb: [u8; 3],
    pub bg_card_rgb: [u8; 3],
    pub text_rgb: [u8; 3],
    pub corner_radius: f32,
    pub border_stroke_width: f32,
    pub ui_scale: f32,
    pub show_cursor_in_stream: bool,
    pub selected_server_id: String,
    pub saved_servers: Vec<ServerProfileDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfoDto {
    pub peer_id: u32,
    pub name: String,
    pub is_sharing: bool,
    pub res_label: String,
    pub video_size: [u32; 2],
    pub fps: u32,
    pub mbps: f32,
    pub volume_percent: u32,
    pub muted: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerMemberPresenceDto {
    pub peer_id: u32,
    pub name: String,
    pub server_id: String,
    pub current_room: String,
    pub is_sharing: bool,
    pub res_label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomSnapshotDto {
    pub in_room: bool,
    pub connected_server_id: String,
    pub active_room_name: String,
    pub peers: Vec<PeerInfoDto>,
    pub server_members: Vec<ServerMemberPresenceDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSettingsDto {
    pub is_sharing: bool,
    pub resolution: String,
    pub fps: u8,
    pub jpeg_quality: u8,
    pub share_audio: bool,
    pub eco_hide_preview: bool,
    pub auto_game_eco: bool,
    pub show_own_preview_in_grid: bool,
    pub active_source_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryDto {
    pub ram_mb: f32,
    pub local_fps: u32,
    pub local_mbps: f32,
    pub local_video_size: [u32; 2],
    pub ws_clients: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSourceDto {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub exe_name: String,
    pub pid: u32,
    pub category: &'static str,
}

pub fn category_slug(cat: SourceCategory) -> &'static str {
    match cat {
        SourceCategory::Application => "Application",
        SourceCategory::Browser => "Browser",
        SourceCategory::Screen => "Screen",
    }
}

pub fn build_custom_dto(app: &AppCore) -> UserCustomizationDto {
    UserCustomizationDto {
        account_created: app.custom.account_created,
        username_login: app.custom.username_login.clone(),
        display_name: app.custom.display_name.clone(),
        user_tag: app.custom.user_tag.clone(),
        custom_status: app.custom.custom_status.clone(),
        bio: app.custom.bio.clone(),
        presence_code: app.custom.presence.code(),
        avatar_data_url: app.avatar_data_url.clone(),
        banner_data_url: app.banner_data_url.clone(),
        avatar_bg: app.custom.avatar_bg,
        banner_bg: app.custom.banner_bg,
        banner_bg_2: app.custom.banner_bg_2,
        name_color_rgb: app.custom.name_color_rgb,
        ring_color_rgb: app.custom.ring_color_rgb,
        avatar_ring_glow: app.custom.avatar_ring_glow,
        ring_thickness: (app.custom.ring_thickness * 100.0).round() / 100.0,
        badge_1080p: app.custom.badge_1080p,
        badge_rust: app.custom.badge_rust,
        badge_gamer: app.custom.badge_gamer,
        badge_crown: app.custom.badge_crown,
        custom_badge_enabled: app.custom.custom_badge_enabled,
        custom_badge_text: app.custom.custom_badge_text.clone(),
        custom_badge_rgb: app.custom.custom_badge_rgb,
        theme_preset_code: app.custom.theme_preset.code(),
        accent_rgb: app.custom.accent_rgb,
        bg_sidebar_rgb: app.custom.bg_sidebar_rgb,
        bg_stage_rgb: app.custom.bg_stage_rgb,
        bg_card_rgb: app.custom.bg_card_rgb,
        text_rgb: app.custom.text_rgb,
        corner_radius: (app.custom.corner_radius * 10.0).round() / 10.0,
        border_stroke_width: (app.custom.border_stroke_width * 100.0).round() / 100.0,
        ui_scale: (app.custom.ui_scale * 100.0).round() / 100.0,
        show_cursor_in_stream: app.custom.show_cursor_in_stream,
        selected_server_id: app.custom.selected_server_id.clone(),
        saved_servers: app
            .custom
            .saved_servers
            .iter()
            .map(|srv| ServerProfileDto {
                server_id: srv.server_id.clone(),
                name: srv.name.clone(),
                icon_rgb: srv.icon_rgb,
                rooms: srv.rooms.clone(),
                invite_token: srv.invite_token(),
            })
            .collect(),
    }
}

pub fn build_snapshot_json(app: &mut AppCore) -> String {
    if app.cached_custom_json.is_empty() {
        app.cached_custom_json =
            serde_json::to_string(&build_custom_dto(app)).unwrap_or_else(|_| "{}".to_string());
    }

    let in_room = app
        .room_hub
        .as_ref()
        .map(|h| h.in_room.load(Ordering::Relaxed))
        .unwrap_or(false);
    let connected_server_id = app
        .room_hub
        .as_ref()
        .and_then(|h| h.connected_server_id.lock().ok().map(|g| g.clone()))
        .unwrap_or_default();
    let active_room_name = app
        .room_hub
        .as_ref()
        .and_then(|h| h.active_room_name.lock().ok().map(|g| g.clone()))
        .unwrap_or_default();

    let peers = app
        .room_hub
        .as_ref()
        .and_then(|h| h.peers.lock().ok().map(|map| (h, map)))
        .map(|(h, map)| {
            let streams_guard = h.remote_streams.lock().ok();
            map.values()
                .map(|p| {
                    let (volume_percent, muted, vw, vh, fps, mbps) = streams_guard
                        .as_ref()
                        .and_then(|sm| {
                            sm.get(&p.peer_id).map(|s| {
                                (
                                    s.volume_percent.load(Ordering::Relaxed),
                                    s.muted.load(Ordering::Relaxed),
                                    s.video_width.load(Ordering::Relaxed),
                                    s.video_height.load(Ordering::Relaxed),
                                    s.measured_fps.load(Ordering::Relaxed),
                                    s.measured_mbps_x100.load(Ordering::Relaxed) as f32 / 100.0,
                                )
                            })
                        })
                        .unwrap_or((100, false, 0, 0, 0, 0.0));
                    PeerInfoDto {
                        peer_id: p.peer_id,
                        name: p.name.clone(),
                        is_sharing: p.is_sharing,
                        res_label: p.res_label.clone(),
                        video_size: [vw, vh],
                        fps,
                        mbps: (mbps * 100.0).round() / 100.0,
                        volume_percent,
                        muted,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let server_members = app
        .room_hub
        .as_ref()
        .and_then(|h| h.server_members.lock().ok())
        .map(|map| {
            map.values()
                .map(|m| ServerMemberPresenceDto {
                    peer_id: m.peer_id,
                    name: m.name.clone(),
                    server_id: m.server_id.clone(),
                    current_room: m.current_room.clone(),
                    is_sharing: m.is_sharing,
                    res_label: m.res_label.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    let room = RoomSnapshotDto {
        in_room,
        connected_server_id,
        active_room_name,
        peers,
        server_members,
    };

    let stream = StreamSettingsDto {
        is_sharing: app.is_sharing,
        resolution: app.res_preset.short_label().to_string(),
        fps: app.fps_preset,
        jpeg_quality: app.jpeg_quality,
        share_audio: app.share_audio_enabled,
        eco_hide_preview: app.eco_hide_preview,
        auto_game_eco: app.auto_game_eco,
        show_own_preview_in_grid: app.show_own_preview_in_grid,
        active_source_id: app.active_source_id.clone(),
    };

    let lw = app.stream_config.local_video_w.load(Ordering::Relaxed);
    let lh = app.stream_config.local_video_h.load(Ordering::Relaxed);
    let telemetry = TelemetryDto {
        ram_mb: (app.ram_mb * 10.0).round() / 10.0,
        local_fps: app.local_measured_fps,
        local_mbps: (app.local_measured_mbps * 100.0).round() / 100.0,
        local_video_size: [lw, lh],
        ws_clients: ws_video_client_count(),
    };

    let popout_tiles: Vec<u32> = app.popout_tiles.iter().copied().collect();
    let update_status = match app.update_status.lock().ok().map(|g| g.clone()) {
        Some(UpdateStatus::Checking) => "Checking".to_string(),
        Some(UpdateStatus::UpToDate) => "UpToDate".to_string(),
        Some(UpdateStatus::UpdateAvailable(v)) => format!("UpdateAvailable:{}", v),
        Some(UpdateStatus::Downloading) => "Downloading".to_string(),
        Some(UpdateStatus::Failed) => "Failed".to_string(),
        _ => "Idle".to_string(),
    };

    let active_toast = app
        .status_toast
        .as_ref()
        .filter(|(_, when)| when.elapsed() < Duration::from_secs(4))
        .map(|(msg, _)| msg.as_str())
        .unwrap_or("");

    let ver_json = serde_json::to_string(CURRENT_VERSION).unwrap_or_else(|_| "\"\"".to_string());
    let room_json = serde_json::to_string(&room).unwrap_or_else(|_| "{}".to_string());
    let stream_json = serde_json::to_string(&stream).unwrap_or_else(|_| "{}".to_string());
    let telemetry_json = serde_json::to_string(&telemetry).unwrap_or_else(|_| "{}".to_string());
    let popouts_json = serde_json::to_string(&popout_tiles).unwrap_or_else(|_| "[]".to_string());
    let upd_json = serde_json::to_string(&update_status).unwrap_or_else(|_| "\"Idle\"".to_string());
    let toast_json = serde_json::to_string(active_toast).unwrap_or_else(|_| "\"\"".to_string());

    format!(
        r#"{{"version":{},"bridgePort":{},"custom":{},"room":{},"stream":{},"telemetry":{},"popoutTiles":{},"updateStatus":{},"activeToast":{}}}"#,
        ver_json,
        BRIDGE_PORT,
        app.cached_custom_json,
        room_json,
        stream_json,
        telemetry_json,
        popouts_json,
        upd_json,
        toast_json,
    )
}

#[cfg(test)]
mod tests {
    use super::UserCustomizationDto;
    use crate::core::commands::SaveProfileParams;

    #[test]
    fn serde_camel_case_bate_com_frontend() {
        let json = r#"{"displayName":"Vitor","badge1080p":true,"bannerBg2":[10,20,30]}"#;
        let parsed: SaveProfileParams = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.display_name.as_deref(), Some("Vitor"));
        assert_eq!(parsed.badge_1080p, Some(true));
        assert_eq!(parsed.banner_bg2, Some([10, 20, 30]));

        let dto = UserCustomizationDto {
            account_created: true,
            username_login: "vitor".into(),
            display_name: "Vitor".into(),
            user_tag: "#1234".into(),
            custom_status: String::new(),
            bio: String::new(),
            presence_code: 0,
            avatar_data_url: String::new(),
            banner_data_url: String::new(),
            avatar_bg: [1, 2, 3],
            banner_bg: [4, 5, 6],
            banner_bg_2: [7, 8, 9],
            name_color_rgb: [255, 255, 255],
            ring_color_rgb: [88, 101, 242],
            avatar_ring_glow: true,
            ring_thickness: 2.0,
            badge_1080p: true,
            badge_rust: true,
            badge_gamer: false,
            badge_crown: false,
            custom_badge_enabled: false,
            custom_badge_text: String::new(),
            custom_badge_rgb: [0, 0, 0],
            theme_preset_code: 0,
            accent_rgb: [88, 101, 242],
            bg_sidebar_rgb: [30, 31, 34],
            bg_stage_rgb: [17, 18, 20],
            bg_card_rgb: [43, 45, 49],
            text_rgb: [242, 243, 245],
            corner_radius: 8.0,
            border_stroke_width: 1.0,
            ui_scale: 1.0,
            show_cursor_in_stream: true,
            selected_server_id: String::new(),
            saved_servers: Vec::new(),
        };
        let val = serde_json::to_value(&dto).unwrap();
        assert_eq!(val["badge1080p"], true);
        assert_eq!(val["bannerBg2"], serde_json::json!([7, 8, 9]));
        assert_eq!(val["displayName"], "Vitor");
    }
}
