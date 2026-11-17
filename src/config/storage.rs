use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::models::{PresenceStatus, ServerProfile, ThemePreset, UserCustomization};

fn config_base_dir() -> PathBuf {
    let base = std::env::var("APPDATA")
        .or_else(|_| std::env::var("XDG_CONFIG_HOME"))
        .or_else(|_| std::env::var("HOME").map(|h| format!("{h}/.config")))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base)
}

fn account_path(base: &Path) -> PathBuf {
    base.join("ScreenZ").join("account_v2.conf")
}

fn load_account_conf(base: &Path) -> io::Result<String> {
    let path = account_path(base);
    match fs::read_to_string(&path) {
        Ok(raw) => return Ok(raw),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    // Importa o perfil anterior sem apagar o original nem sobrescrever um perfil ScreenZ.
    let raw = fs::read_to_string(base.join("SZdisc").join("account_v2.conf"))?;
    if fs::create_dir_all(base.join("ScreenZ")).is_ok() {
        if let Ok(mut file) = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            if file.write_all(raw.as_bytes()).is_err() {
                drop(file);
                let _ = fs::remove_file(&path);
            }
        }
    }
    Ok(raw)
}

fn fmt_rgb(rgb: [u8; 3]) -> String {
    format!("{},{},{}", rgb[0], rgb[1], rgb[2])
}

fn parse_rgb(s: &str) -> Option<[u8; 3]> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() == 3 {
        Some([
            parts[0].trim().parse().ok()?,
            parts[1].trim().parse().ok()?,
            parts[2].trim().parse().ok()?,
        ])
    } else {
        None
    }
}

fn serialize_servers(servers: &[ServerProfile]) -> String {
    servers
        .iter()
        .map(|srv| {
            let clean_id = srv
                .server_id
                .replace(['~', '|', '\n'], "")
                .trim()
                .to_string();
            let clean_name = srv.name.replace(['~', '|', '\n'], " ").trim().to_string();
            let rooms_str = srv
                .rooms
                .iter()
                .map(|r| r.replace([';', '~', '|', '\n'], "").trim().to_string())
                .filter(|r| !r.is_empty())
                .collect::<Vec<_>>()
                .join(";");
            format!(
                "{}~{}~{}~{}",
                clean_id,
                clean_name,
                fmt_rgb(srv.icon_rgb),
                rooms_str
            )
        })
        .collect::<Vec<_>>()
        .join("||")
}

fn deserialize_servers(raw: &str) -> Vec<ServerProfile> {
    let mut list = Vec::new();
    for chunk in raw.split("||") {
        let fields: Vec<&str> = chunk.split('~').collect();
        if fields.len() >= 4 {
            let server_id = fields[0].trim().to_string();
            let name = fields[1].trim().to_string();
            let icon_rgb = parse_rgb(fields[2]).unwrap_or([88, 101, 242]);
            let mut rooms: Vec<String> = fields[3]
                .split(';')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if rooms.is_empty() {
                rooms.push("Sala Geral".to_string());
            }
            if !server_id.is_empty() && !name.is_empty() {
                list.push(ServerProfile {
                    server_id,
                    name,
                    icon_rgb,
                    rooms,
                });
            }
        }
    }
    list
}

impl UserCustomization {
    pub fn serialize_conf(&self) -> String {
        let lines = [
            format!("account_created={}", self.account_created),
            format!("username_login={}", self.username_login.replace('\n', " ")),
            format!("display_name={}", self.display_name.replace('\n', " ")),
            format!("user_tag={}", self.user_tag.replace('\n', " ")),
            format!("custom_status={}", self.custom_status.replace('\n', " ")),
            format!("bio={}", self.bio.replace('\n', " ")),
            format!("presence={}", self.presence.code()),
            format!("avatar_image_path={}", self.avatar_image_path),
            format!("banner_image_path={}", self.banner_image_path),
            format!("avatar_bg={}", fmt_rgb(self.avatar_bg)),
            format!("banner_bg={}", fmt_rgb(self.banner_bg)),
            format!("banner_bg_2={}", fmt_rgb(self.banner_bg_2)),
            format!("name_color_rgb={}", fmt_rgb(self.name_color_rgb)),
            format!("ring_color_rgb={}", fmt_rgb(self.ring_color_rgb)),
            format!("avatar_ring_glow={}", self.avatar_ring_glow),
            format!("ring_thickness={}", self.ring_thickness),
            format!("badge_1080p={}", self.badge_1080p),
            format!("badge_rust={}", self.badge_rust),
            format!("badge_gamer={}", self.badge_gamer),
            format!("badge_crown={}", self.badge_crown),
            format!("custom_badge_enabled={}", self.custom_badge_enabled),
            format!(
                "custom_badge_text={}",
                self.custom_badge_text.replace('\n', " ")
            ),
            format!("custom_badge_rgb={}", fmt_rgb(self.custom_badge_rgb)),
            format!("theme_preset={}", self.theme_preset.code()),
            format!("accent_rgb={}", fmt_rgb(self.accent_rgb)),
            format!("bg_sidebar_rgb={}", fmt_rgb(self.bg_sidebar_rgb)),
            format!("bg_stage_rgb={}", fmt_rgb(self.bg_stage_rgb)),
            format!("bg_card_rgb={}", fmt_rgb(self.bg_card_rgb)),
            format!("text_rgb={}", fmt_rgb(self.text_rgb)),
            format!("corner_radius={}", self.corner_radius),
            format!("border_stroke_width={}", self.border_stroke_width),
            format!("ui_scale={}", self.ui_scale),
            format!("show_cursor_in_stream={}", self.show_cursor_in_stream),
            format!(
                "selected_server_id={}",
                self.selected_server_id.replace('\n', "")
            ),
            format!("saved_servers={}", serialize_servers(&self.saved_servers)),
        ];
        format!("{}\n", lines.join("\n"))
    }

    pub fn deserialize_conf(raw: &str) -> Self {
        let mut cfg = Self::default();
        for line in raw.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k.trim() {
                "account_created" => cfg.account_created = v == "true",
                "username_login" => cfg.username_login = v.to_string(),
                "display_name" => cfg.display_name = v.to_string(),
                "user_tag" => cfg.user_tag = v.to_string(),
                "custom_status" => cfg.custom_status = v.to_string(),
                "bio" => cfg.bio = v.to_string(),
                "presence" => cfg.presence = PresenceStatus::from_code(v.parse().unwrap_or(0)),
                "avatar_image_path" => cfg.avatar_image_path = v.to_string(),
                "banner_image_path" => cfg.banner_image_path = v.to_string(),
                "avatar_bg" => {
                    if let Some(rgb) = parse_rgb(v) {
                        cfg.avatar_bg = rgb;
                    }
                }
                "banner_bg" => {
                    if let Some(rgb) = parse_rgb(v) {
                        cfg.banner_bg = rgb;
                    }
                }
                "banner_bg_2" => {
                    if let Some(rgb) = parse_rgb(v) {
                        cfg.banner_bg_2 = rgb;
                    }
                }
                "name_color_rgb" => {
                    if let Some(rgb) = parse_rgb(v) {
                        cfg.name_color_rgb = rgb;
                    }
                }
                "ring_color_rgb" => {
                    if let Some(rgb) = parse_rgb(v) {
                        cfg.ring_color_rgb = rgb;
                    }
                }
                "avatar_ring_glow" => cfg.avatar_ring_glow = v == "true",
                "ring_thickness" => cfg.ring_thickness = v.parse().unwrap_or(2.2),
                "badge_1080p" => cfg.badge_1080p = v == "true",
                "badge_rust" => cfg.badge_rust = v == "true",
                "badge_gamer" => cfg.badge_gamer = v == "true",
                "badge_crown" => cfg.badge_crown = v == "true",
                "custom_badge_enabled" => cfg.custom_badge_enabled = v == "true",
                "custom_badge_text" => cfg.custom_badge_text = v.to_string(),
                "custom_badge_rgb" => {
                    if let Some(rgb) = parse_rgb(v) {
                        cfg.custom_badge_rgb = rgb;
                    }
                }
                "theme_preset" => {
                    cfg.theme_preset = ThemePreset::from_code(v.parse().unwrap_or(0));
                }
                "accent_rgb" => {
                    if let Some(rgb) = parse_rgb(v) {
                        cfg.accent_rgb = rgb;
                    }
                }
                "bg_sidebar_rgb" => {
                    if let Some(rgb) = parse_rgb(v) {
                        cfg.bg_sidebar_rgb = rgb;
                    }
                }
                "bg_stage_rgb" => {
                    if let Some(rgb) = parse_rgb(v) {
                        cfg.bg_stage_rgb = rgb;
                    }
                }
                "bg_card_rgb" => {
                    if let Some(rgb) = parse_rgb(v) {
                        cfg.bg_card_rgb = rgb;
                    }
                }
                "text_rgb" => {
                    if let Some(rgb) = parse_rgb(v) {
                        cfg.text_rgb = rgb;
                    }
                }
                "corner_radius" => cfg.corner_radius = v.parse().unwrap_or(8.0),
                "border_stroke_width" => cfg.border_stroke_width = v.parse().unwrap_or(1.0),
                "ui_scale" => cfg.ui_scale = v.parse().unwrap_or(1.0),
                "show_cursor_in_stream" => cfg.show_cursor_in_stream = v == "true",
                "selected_server_id" => cfg.selected_server_id = v.trim().to_string(),
                "saved_servers" => cfg.saved_servers = deserialize_servers(v),
                _ => {}
            }
        }
        if cfg.display_name.trim().is_empty() {
            cfg.account_created = false;
        }
        if cfg.theme_preset == ThemePreset::StudioDark
            && cfg.accent_rgb == [88, 101, 242]
            && cfg.bg_sidebar_rgb == [30, 31, 34]
            && cfg.bg_stage_rgb == [17, 18, 20]
            && cfg.bg_card_rgb == [43, 45, 49]
            && cfg.text_rgb == [242, 243, 245]
        {
            cfg.apply_preset_colors(ThemePreset::StudioDark);
        }
        cfg
    }

    pub fn save_to_disk(&self) {
        let base = config_base_dir();
        if fs::create_dir_all(base.join("ScreenZ")).is_ok() {
            let _ = fs::write(account_path(&base), self.serialize_conf());
        }
    }

    pub fn load_from_disk() -> Self {
        let Ok(raw) = load_account_conf(&config_base_dir()) else {
            return Self::default();
        };
        Self::deserialize_conf(&raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migra_perfil_antigo_e_prioriza_perfil_screenz() {
        let base = std::env::temp_dir().join(format!(
            "screenz-profile-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(load_account_conf(&base).is_err());
        assert!(!account_path(&base).exists());

        let legacy_path = base.join("SZdisc").join("account_v2.conf");
        fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        let raw = "account_created=true\ndisplay_name=Perfil antigo\nselected_server_id=SZ-12345\nsaved_servers=SZ-12345~Amigos~10,20,30~Geral;Jogos\naccent_rgb=12,80,220\n";
        fs::write(&legacy_path, raw).unwrap();
        assert_eq!(load_account_conf(&base).unwrap(), raw);
        assert_eq!(fs::read_to_string(account_path(&base)).unwrap(), raw);
        assert_eq!(fs::read_to_string(&legacy_path).unwrap(), raw);
        let profile = UserCustomization::deserialize_conf(raw);
        assert!(profile.account_created);
        assert_eq!(profile.saved_servers[0].rooms, vec!["Geral", "Jogos"]);
        assert_eq!(profile.accent_rgb, [12, 80, 220]);

        let current = raw.replace("Perfil antigo", "Perfil ScreenZ");
        fs::write(account_path(&base), &current).unwrap();
        assert_eq!(load_account_conf(&base).unwrap(), current);
        assert_eq!(fs::read_to_string(&legacy_path).unwrap(), raw);
        // Remove somente os arquivos temporários criados neste teste.
        fs::remove_file(account_path(&base)).unwrap();
        fs::remove_file(legacy_path).unwrap();
        fs::remove_dir(base.join("ScreenZ")).unwrap();
        fs::remove_dir(base.join("SZdisc")).unwrap();
        fs::remove_dir(base).unwrap();
    }

    #[test]
    fn roundtrip_account_conf() {
        let mut cfg = UserCustomization {
            account_created: true,
            display_name: "Vitor".to_string(),
            ..UserCustomization::default()
        };
        cfg.saved_servers.push(ServerProfile {
            server_id: "SZ-12345".to_string(),
            name: "Lab".to_string(),
            icon_rgb: [10, 20, 30],
            rooms: vec!["Geral".to_string(), "Jogos".to_string()],
        });
        cfg.selected_server_id = "SZ-12345".to_string();

        let text = cfg.serialize_conf();
        let parsed = UserCustomization::deserialize_conf(&text);
        assert!(parsed.account_created);
        assert_eq!(parsed.display_name, "Vitor");
        assert_eq!(parsed.saved_servers.len(), 1);
        assert_eq!(parsed.saved_servers[0].rooms, vec!["Geral", "Jogos"]);
    }

    #[test]
    fn atualiza_somente_a_paleta_padrao_antiga() {
        let legacy = "theme_preset=0\naccent_rgb=88,101,242\nbg_sidebar_rgb=30,31,34\nbg_stage_rgb=17,18,20\nbg_card_rgb=43,45,49\ntext_rgb=242,243,245\ndisplay_name=Vitor\naccount_created=true\nbio=Minha bio\n";
        let migrated = UserCustomization::deserialize_conf(legacy);
        assert_eq!(migrated.accent_rgb, [213, 247, 128]);
        assert_eq!(migrated.display_name, "Vitor");
        assert_eq!(migrated.bio, "Minha bio");
        assert!(migrated.account_created);

        let customized = UserCustomization::deserialize_conf(
            &legacy.replace("accent_rgb=88,101,242", "accent_rgb=12,80,220"),
        );
        assert_eq!(customized.accent_rgb, [12, 80, 220]);
        assert_eq!(customized.bg_stage_rgb, [17, 18, 20]);

        let custom_preset = UserCustomization::deserialize_conf(
            &legacy.replace("theme_preset=0", "theme_preset=6"),
        );
        assert_eq!(custom_preset.accent_rgb, [88, 101, 242]);
    }
}
