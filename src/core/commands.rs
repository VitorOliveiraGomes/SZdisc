use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::Deserialize;

use crate::capture::list_capture_sources;
use crate::config::{generate_server_id, PresenceStatus, ServerProfile, ThemePreset};
use crate::updater::{check_for_updates_async, download_and_apply_update_async};

use super::state::AppCore;
use super::stream::ResolutionPreset;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingParams {
    pub display_name: Option<String>,
    pub username_login: Option<String>,
    pub user_tag: Option<String>,
    pub custom_status: Option<String>,
    pub name_color_rgb: Option<[u8; 3]>,
    pub avatar_bg: Option<[u8; 3]>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SaveProfileParams {
    pub display_name: Option<String>,
    pub user_tag: Option<String>,
    pub custom_status: Option<String>,
    pub bio: Option<String>,
    pub presence_code: Option<u8>,
    pub name_color_rgb: Option<[u8; 3]>,
    pub avatar_bg: Option<[u8; 3]>,
    pub banner_bg: Option<[u8; 3]>,
    pub banner_bg2: Option<[u8; 3]>,
    pub avatar_ring_glow: Option<bool>,
    pub ring_color_rgb: Option<[u8; 3]>,
    pub ring_thickness: Option<f32>,
    pub badge_crown: Option<bool>,
    #[serde(rename = "badge1080p")]
    pub badge_1080p: Option<bool>,
    pub badge_rust: Option<bool>,
    pub badge_gamer: Option<bool>,
    pub custom_badge_enabled: Option<bool>,
    pub custom_badge_text: Option<String>,
    pub custom_badge_rgb: Option<[u8; 3]>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SaveAppearanceParams {
    pub accent_rgb: Option<[u8; 3]>,
    pub bg_sidebar_rgb: Option<[u8; 3]>,
    pub bg_stage_rgb: Option<[u8; 3]>,
    pub bg_card_rgb: Option<[u8; 3]>,
    pub text_rgb: Option<[u8; 3]>,
    pub corner_radius: Option<f32>,
    pub border_stroke_width: Option<f32>,
    pub ui_scale: Option<f32>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStreamParams {
    pub resolution: Option<String>,
    pub fps: Option<u8>,
    pub jpeg_quality: Option<u8>,
    pub share_audio: Option<bool>,
    pub eco_hide_preview: Option<bool>,
    pub auto_game_eco: Option<bool>,
    pub show_cursor: Option<bool>,
    pub show_own_preview_in_grid: Option<bool>,
}

impl AppCore {
    pub fn execute_command(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<(), String> {
        self.cached_custom_json.clear();

        match method {
            "complete_onboarding" => {
                let p: OnboardingParams =
                    serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
                let display_name = p.display_name.unwrap_or_default().trim().to_string();
                if display_name.is_empty() {
                    return Err("Nome de exibição é obrigatório".into());
                }
                self.custom.display_name = display_name.clone();
                let login = p.username_login.unwrap_or_default().trim().to_string();
                self.custom.username_login = if login.is_empty() {
                    display_name.to_lowercase().replace(' ', "_")
                } else {
                    login
                };
                if let Some(tag) = p.user_tag.filter(|t| !t.trim().is_empty()) {
                    self.custom.user_tag = tag.trim().to_string();
                }
                if let Some(status) = p.custom_status {
                    self.custom.custom_status = status;
                }
                if let Some(rgb) = p.name_color_rgb {
                    self.custom.name_color_rgb = rgb;
                }
                if let Some(rgb) = p.avatar_bg {
                    self.custom.avatar_bg = rgb;
                }
                self.custom.account_created = true;
                self.custom.save_to_disk();
                self.sync_identity_to_hub();
                self.sync_live_config();
                self.show_toast(format!("Bem-vindo(a), {}!", self.custom.display_name));
            }
            "clear_profile_image" => {
                let kind = params
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("avatar");
                if kind == "banner" {
                    self.custom.banner_image_path.clear();
                    self.banner_data_url.clear();
                } else {
                    self.custom.avatar_image_path.clear();
                    self.avatar_data_url.clear();
                }
                self.custom.save_to_disk();
            }
            "save_profile" => {
                let p: SaveProfileParams =
                    serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
                if let Some(v) = p.display_name {
                    self.custom.display_name = v;
                }
                if let Some(v) = p.user_tag {
                    self.custom.user_tag = v;
                }
                if let Some(v) = p.custom_status {
                    self.custom.custom_status = v;
                }
                if let Some(v) = p.bio {
                    self.custom.bio = v;
                }
                if let Some(code) = p.presence_code {
                    self.custom.presence = PresenceStatus::from_code(code);
                }
                if let Some(rgb) = p.name_color_rgb {
                    self.custom.name_color_rgb = rgb;
                }
                if let Some(rgb) = p.avatar_bg {
                    self.custom.avatar_bg = rgb;
                }
                if let Some(rgb) = p.banner_bg {
                    self.custom.banner_bg = rgb;
                }
                if let Some(rgb) = p.banner_bg2 {
                    self.custom.banner_bg_2 = rgb;
                }
                if let Some(b) = p.avatar_ring_glow {
                    self.custom.avatar_ring_glow = b;
                }
                if let Some(rgb) = p.ring_color_rgb {
                    self.custom.ring_color_rgb = rgb;
                }
                if let Some(t) = p.ring_thickness {
                    self.custom.ring_thickness = t.clamp(0.5, 4.5);
                }
                if let Some(b) = p.badge_crown {
                    self.custom.badge_crown = b;
                }
                if let Some(b) = p.badge_1080p {
                    self.custom.badge_1080p = b;
                }
                if let Some(b) = p.badge_rust {
                    self.custom.badge_rust = b;
                }
                if let Some(b) = p.badge_gamer {
                    self.custom.badge_gamer = b;
                }
                if let Some(b) = p.custom_badge_enabled {
                    self.custom.custom_badge_enabled = b;
                }
                if let Some(v) = p.custom_badge_text {
                    self.custom.custom_badge_text = v;
                }
                if let Some(rgb) = p.custom_badge_rgb {
                    self.custom.custom_badge_rgb = rgb;
                }
                self.custom.save_to_disk();
                self.sync_identity_to_hub();
                self.sync_live_config();
            }
            "logout" => {
                self.leave_room();
                self.custom.account_created = false;
                self.custom.save_to_disk();
                if let Some(hub) = &self.room_hub {
                    hub.set_account_active(false, "");
                }
            }
            "apply_theme_preset" => {
                let code = params
                    .get("presetCode")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u8;
                self.custom
                    .apply_preset_colors(ThemePreset::from_code(code));
                self.custom.save_to_disk();
            }
            "save_appearance" => {
                let p: SaveAppearanceParams =
                    serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
                if let Some(rgb) = p.accent_rgb {
                    self.custom.accent_rgb = rgb;
                    self.custom.theme_preset = ThemePreset::Custom;
                }
                if let Some(rgb) = p.bg_sidebar_rgb {
                    self.custom.bg_sidebar_rgb = rgb;
                    self.custom.theme_preset = ThemePreset::Custom;
                }
                if let Some(rgb) = p.bg_stage_rgb {
                    self.custom.bg_stage_rgb = rgb;
                    self.custom.theme_preset = ThemePreset::Custom;
                }
                if let Some(rgb) = p.bg_card_rgb {
                    self.custom.bg_card_rgb = rgb;
                    self.custom.theme_preset = ThemePreset::Custom;
                }
                if let Some(rgb) = p.text_rgb {
                    self.custom.text_rgb = rgb;
                    self.custom.theme_preset = ThemePreset::Custom;
                }
                if let Some(r) = p.corner_radius {
                    self.custom.corner_radius = r.clamp(0.0, 16.0);
                }
                if let Some(w) = p.border_stroke_width {
                    self.custom.border_stroke_width = w.clamp(0.5, 3.0);
                }
                if let Some(s) = p.ui_scale {
                    self.custom.ui_scale = s.clamp(0.90, 1.15);
                }
                self.custom.save_to_disk();
            }
            "create_server" => {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if name.is_empty() {
                    return Err("Nome do servidor vazio".into());
                }
                let icon_rgb: [u8; 3] = params
                    .get("iconRgb")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or([88, 101, 242]);
                let mut rooms: Vec<String> = params
                    .get("rooms")
                    .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty())
                    .collect();
                if rooms.is_empty() {
                    rooms.push("Sala Geral".to_string());
                }
                let srv_id = generate_server_id(&name);
                let new_srv = ServerProfile {
                    server_id: srv_id.clone(),
                    name: name.clone(),
                    icon_rgb,
                    rooms,
                };
                if let Some(hub) = &self.room_hub {
                    hub.register_and_select_server(&new_srv);
                }
                self.custom.selected_server_id = srv_id;
                self.custom.saved_servers.push(new_srv);
                self.custom.save_to_disk();
                self.show_toast(format!("Servidor '{}' criado!", name));
            }
            "join_server" => {
                let raw = params
                    .get("invite")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let new_srv = ServerProfile::from_invite_input(raw)
                    .ok_or_else(|| "Convite ou código inválido".to_string())?;
                let srv_id = new_srv.server_id.clone();
                let srv_name = new_srv.name.clone();

                if let Some(existing) = self
                    .custom
                    .saved_servers
                    .iter_mut()
                    .find(|s| s.server_id.eq_ignore_ascii_case(&srv_id))
                {
                    for r in &new_srv.rooms {
                        if !existing.rooms.iter().any(|x| x.eq_ignore_ascii_case(r)) {
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
                self.show_toast(format!("Você entrou no servidor '{}'!", srv_name));
            }
            "select_server" => {
                let server_id = params
                    .get("serverId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if let Some(chosen) = self
                    .custom
                    .saved_servers
                    .iter()
                    .find(|s| s.server_id.eq_ignore_ascii_case(server_id))
                    .cloned()
                {
                    self.custom.selected_server_id = chosen.server_id.clone();
                    self.custom.save_to_disk();
                    if let Some(hub) = &self.room_hub {
                        hub.register_and_select_server(&chosen);
                    }
                }
            }
            "remove_server" => {
                let server_id = params
                    .get("serverId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let is_connected_server = self.room_hub.as_ref().is_some_and(|hub| {
                    hub.connected_server_id
                        .lock()
                        .is_ok_and(|id| id.eq_ignore_ascii_case(server_id))
                });
                if is_connected_server {
                    self.leave_room();
                }
                if let Some(hub) = &self.room_hub {
                    hub.remove_server(server_id);
                }
                self.custom
                    .saved_servers
                    .retain(|s| !s.server_id.eq_ignore_ascii_case(server_id));
                if self
                    .custom
                    .selected_server_id
                    .eq_ignore_ascii_case(server_id)
                {
                    self.custom.selected_server_id = self
                        .custom
                        .saved_servers
                        .first()
                        .map(|s| s.server_id.clone())
                        .unwrap_or_default();
                    if let Some(hub) = &self.room_hub {
                        hub.set_account_active(
                            self.custom.account_created,
                            &self.custom.selected_server_id,
                        );
                    }
                }
                self.custom.save_to_disk();
            }
            "create_room" => {
                let server_id = params
                    .get("serverId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let room_name = params
                    .get("roomName")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if room_name.is_empty() {
                    return Err("Nome da sala vazio".into());
                }
                if let Some(hub) = &self.room_hub {
                    let updated = hub.create_and_enter_room(server_id, &room_name);
                    if let Some(srv_mut) = self
                        .custom
                        .saved_servers
                        .iter_mut()
                        .find(|s| s.server_id.eq_ignore_ascii_case(server_id))
                    {
                        srv_mut.rooms = updated;
                    }
                    self.custom.save_to_disk();
                }
                self.show_toast(format!("Sala '{}' criada!", room_name));
            }
            "delete_room" => {
                let server_id = params
                    .get("serverId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let room_name = params
                    .get("roomName")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if let Some(srv_mut) = self
                    .custom
                    .saved_servers
                    .iter_mut()
                    .find(|s| s.server_id.eq_ignore_ascii_case(server_id))
                {
                    if srv_mut.rooms.len() > 1 {
                        srv_mut.rooms.retain(|r| !r.eq_ignore_ascii_case(room_name));
                        if let Some(hub) = &self.room_hub {
                            if let Ok(mut map) = hub.server_rooms_map.lock() {
                                map.insert(srv_mut.server_id.to_uppercase(), srv_mut.rooms.clone());
                            }
                        }
                        self.custom.save_to_disk();
                    }
                }
            }
            "enter_room" => {
                let server_id = params
                    .get("serverId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let room_name = params
                    .get("roomName")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if let Some(hub) = &self.room_hub {
                    hub.enter_server_room(server_id, room_name);
                }
                self.show_toast(format!("Conectado em '{}'", room_name));
            }
            "leave_room" => {
                self.leave_room();
            }
            "start_sharing" => {
                let source_id = params
                    .get("sourceId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .trim();
                self.sources = list_capture_sources();
                if !source_id.is_empty() {
                    if !self.sources.iter().any(|s| s.id == source_id) {
                        return Err(format!(
                            "Fonte de captura '{}' não encontrada (pode ter sido fechada).",
                            source_id
                        ));
                    }
                    self.active_source_id = source_id.to_string();
                } else if self.active_source().is_none() {
                    if let Some(first) = self.sources.first() {
                        self.active_source_id = first.id.clone();
                    }
                }
                self.start_or_switch_sharing()?;
            }
            "stop_sharing" => {
                self.stop_sharing();
            }
            "update_stream_settings" => {
                let p: UpdateStreamParams =
                    serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
                if let Some(res) = p.resolution {
                    self.res_preset = match res.as_str() {
                        "1080p" => ResolutionPreset::P1080,
                        "900p" => ResolutionPreset::P900,
                        "720p" => ResolutionPreset::P720,
                        "480p" => ResolutionPreset::P480,
                        "360p" => ResolutionPreset::P360,
                        _ => self.res_preset,
                    };
                }
                if let Some(fps) = p.fps.filter(|f| matches!(f, 15 | 30 | 60)) {
                    self.fps_preset = fps;
                }
                if let Some(q) = p.jpeg_quality {
                    self.jpeg_quality = q.clamp(40, 95);
                }
                if let Some(b) = p.share_audio {
                    self.share_audio_enabled = b;
                }
                if let Some(b) = p.eco_hide_preview {
                    self.eco_hide_preview = b;
                }
                if let Some(b) = p.auto_game_eco {
                    self.auto_game_eco = b;
                }
                if let Some(b) = p.show_cursor {
                    self.custom.show_cursor_in_stream = b;
                    self.custom.save_to_disk();
                }
                if let Some(b) = p.show_own_preview_in_grid {
                    self.show_own_preview_in_grid = b;
                }
                self.sync_live_config();
            }
            "set_peer_audio" => {
                let peer_id = params
                    .get("peerId")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .ok_or_else(|| "peerId obrigatório".to_string())?;
                if let Some(hub) = &self.room_hub {
                    if let Ok(streams) = hub.remote_streams.lock() {
                        if let Some(stream) = streams.get(&peer_id) {
                            if let Some(vol) = params.get("volumePercent").and_then(|v| v.as_u64())
                            {
                                stream
                                    .volume_percent
                                    .store((vol as u32).min(150), Ordering::Relaxed);
                            }
                            if let Some(muted) = params.get("muted").and_then(|v| v.as_bool()) {
                                stream.muted.store(muted, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
            "set_popout" => {
                if params.get("clearAll").and_then(|v| v.as_bool()) == Some(true) {
                    self.popout_tiles.clear();
                } else if let Some(tile_id) = params
                    .get("tileId")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                {
                    let popout = params
                        .get("popout")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    if popout {
                        if self.focused_tile_id == Some(tile_id) {
                            self.focused_tile_id = None;
                        }
                        self.popout_tiles.insert(tile_id);
                    } else {
                        self.popout_tiles.remove(&tile_id);
                    }
                }
            }
            "show_toast" => {
                let msg = params
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Notificação")
                    .to_string();
                self.show_toast(msg);
            }
            "check_updates" => {
                check_for_updates_async(Arc::clone(&self.update_status));
            }
            "apply_update" => {
                download_and_apply_update_async(Arc::clone(&self.update_status));
            }
            _ => return Err(format!("Método RPC desconhecido: {}", method)),
        }
        Ok(())
    }
}
