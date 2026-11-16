use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::mem::{size_of, zeroed};
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::ptr::null_mut;
#[cfg(windows)]
use winapi::um::commdlg::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

fn get_user_config_path() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(base).join("SZdisc");
    let _ = fs::create_dir_all(&dir);
    dir.join("account_v2.conf")
}

fn generate_random_tag() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(4829);
    let num = 1000 + (nanos % 9000);
    format!("#{}", num)
}

/// Gera um código único de Servidor (ex: "SZ-48291")
pub fn generate_server_id(server_name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(982451653);
    let mut hash: u32 = 2166136261;
    for b in server_name.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16777619);
    }
    hash ^= (nanos & 0xFFFF_FFFF) as u32;
    hash = hash.wrapping_mul(16777619);
    let code = 10000 + (hash % 90000);
    format!("SZ-{}", code)
}

/// Representa um Servidor isolado (estilo Servidor do Discord), com seu próprio Código de Convite e suas próprias Salas!
#[derive(Clone, Debug, PartialEq)]
pub struct ServerProfile {
    pub server_id: String,
    pub name: String,
    pub icon_rgb: [u8; 3],
    pub rooms: Vec<String>,
}

impl ServerProfile {
    /// Gera o código/link de convite completo deste servidor para mandar para os amigos daquele grupo
    pub fn invite_token(&self) -> String {
        let clean_name = self.name.replace('/', "-").replace('|', "-").trim().to_string();
        let rooms_csv = self
            .rooms
            .iter()
            .map(|r| r.replace(',', " ").replace('/', "-").trim().to_string())
            .filter(|r| !r.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        format!("{}/{}/{}", self.server_id, clean_name, rooms_csv)
    }

    /// Interpreta um convite colado por um amigo (aceita tanto o convite completo `SZ-12345/Nome/Sala1,Sala2` quanto só o código `SZ-12345` ou `12345`)
    pub fn from_invite_input(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        let parts: Vec<&str> = trimmed.split('/').collect();
        let raw_id = parts[0].trim().to_uppercase();
        let server_id = if raw_id.starts_with("SZ-") {
            raw_id
        } else if raw_id.chars().all(|c| c.is_ascii_alphanumeric()) && raw_id.len() >= 3 {
            format!("SZ-{}", raw_id)
        } else {
            return None;
        };

        let name = if parts.len() >= 2 && !parts[1].trim().is_empty() {
            parts[1].trim().to_string()
        } else {
            format!("Servidor {}", server_id)
        };

        let mut rooms = Vec::new();
        if parts.len() >= 3 {
            for r in parts[2].split(',') {
                let cr = r.trim();
                if !cr.is_empty() && !rooms.iter().any(|x: &String| x.eq_ignore_ascii_case(cr)) {
                    rooms.push(cr.to_string());
                }
            }
        }
        if rooms.is_empty() {
            rooms.push("Sala Geral".to_string());
            rooms.push("Jogos".to_string());
        }

        let mut color_hash: u32 = 2166136261;
        for b in server_id.as_bytes() {
            color_hash ^= *b as u32;
            color_hash = color_hash.wrapping_mul(16777619);
        }
        let palette: [[u8; 3]; 6] = [
            [88, 101, 242],
            [16, 185, 129],
            [236, 72, 153],
            [245, 158, 11],
            [56, 189, 248],
            [168, 85, 247],
        ];
        let icon_rgb = palette[(color_hash as usize) % palette.len()];

        Some(Self {
            server_id,
            name,
            icon_rgb,
            rooms,
        })
    }
}

/// Abre a janela nativa do sistema para escolher uma imagem (PNG, JPG, JPEG, BMP)
#[cfg(windows)]
pub fn pick_image_file_dialog() -> Option<String> {
    unsafe {
        let mut file_buf = [0u16; 1024];
        let filter_str =
            "Imagens (PNG, JPG, BMP)\0*.png;*.jpg;*.jpeg;*.bmp\0Todos os Arquivos\0*.*\0\0";
        let filter_wide: Vec<u16> = OsString::from(filter_str).encode_wide().collect();
        let title_wide: Vec<u16> = OsString::from("Escolher Imagem\0").encode_wide().collect();

        let mut ofn: OPENFILENAMEW = zeroed();
        ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = null_mut();
        ofn.lpstrFile = file_buf.as_mut_ptr();
        ofn.nMaxFile = file_buf.len() as u32;
        ofn.lpstrFilter = filter_wide.as_ptr();
        ofn.nFilterIndex = 1;
        ofn.lpstrTitle = title_wide.as_ptr();
        ofn.Flags = OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR;

        if GetOpenFileNameW(&mut ofn) != 0 {
            let len = file_buf.iter().position(|&c| c == 0).unwrap_or(0);
            if len > 0 {
                return Some(
                    OsString::from_wide(&file_buf[..len])
                        .to_string_lossy()
                        .to_string(),
                );
            }
        }
        None
    }
}

#[cfg(not(windows))]
pub fn pick_image_file_dialog() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Imagens", &["png", "jpg", "jpeg", "bmp"])
        .pick_file()
        .map(|p| p.to_string_lossy().to_string())
}

/// Carrega uma imagem do disco, redimensiona para o tamanho alvo e retorna um Data URL JPEG base64.
pub fn load_image_as_jpeg_data_url(path: &str, target_w: u32, target_h: u32) -> Option<String> {
    if path.trim().is_empty() {
        return None;
    }
    let img = image::open(path).ok()?;
    let resized = img.resize_to_fill(target_w, target_h, image::imageops::FilterType::Triangle);
    let rgb = resized.to_rgb8().into_raw();
    let mut jpeg_buf = Vec::new();
    let mut enc = Encoder::new(&mut jpeg_buf, 85);
    enc.set_sampling_factor(SamplingFactor::F_2_2);
    enc.encode(&rgb, target_w as u16, target_h as u16, ColorType::Rgb)
        .ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        crate::bridge::base64_encode(&jpeg_buf)
    ))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PresenceStatus {
    Online,
    Idle,
    DoNotDisturb,
    Invisible,
}

impl PresenceStatus {
    pub fn code(self) -> u8 {
        match self {
            Self::Online => 0,
            Self::Idle => 1,
            Self::DoNotDisturb => 2,
            Self::Invisible => 3,
        }
    }

    pub fn from_code(c: u8) -> Self {
        match c {
            1 => Self::Idle,
            2 => Self::DoNotDisturb,
            3 => Self::Invisible,
            _ => Self::Online,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemePreset {
    DiscordDark,
    MidnightOled,
    CyberpunkNeon,
    EmeraldGamer,
    CrimsonRuby,
    ArcticOcean,
    Custom,
}

impl ThemePreset {
    pub fn code(self) -> u8 {
        match self {
            Self::DiscordDark => 0,
            Self::MidnightOled => 1,
            Self::CyberpunkNeon => 2,
            Self::EmeraldGamer => 3,
            Self::CrimsonRuby => 4,
            Self::ArcticOcean => 5,
            Self::Custom => 6,
        }
    }

    pub fn from_code(c: u8) -> Self {
        match c {
            1 => Self::MidnightOled,
            2 => Self::CyberpunkNeon,
            3 => Self::EmeraldGamer,
            4 => Self::CrimsonRuby,
            5 => Self::ArcticOcean,
            6 => Self::Custom,
            _ => Self::DiscordDark,
        }
    }
}

#[derive(Clone)]
pub struct UserCustomization {
    pub account_created: bool,
    pub username_login: String,
    pub display_name: String,
    pub user_tag: String,
    pub custom_status: String,
    pub bio: String,
    pub presence: PresenceStatus,
    pub avatar_image_path: String,
    pub banner_image_path: String,
    pub avatar_bg: [u8; 3],
    pub banner_bg: [u8; 3],
    pub banner_bg_2: [u8; 3],
    pub name_color_rgb: [u8; 3],
    pub ring_color_rgb: [u8; 3],
    pub avatar_ring_glow: bool,
    pub ring_thickness: f32,
    pub badge_1080p: bool,
    pub badge_rust: bool,
    pub badge_gamer: bool,
    pub badge_crown: bool,
    pub custom_badge_enabled: bool,
    pub custom_badge_text: String,
    pub custom_badge_rgb: [u8; 3],

    // Aparência do Aplicativo
    pub theme_preset: ThemePreset,
    pub accent_rgb: [u8; 3],
    pub bg_sidebar_rgb: [u8; 3],
    pub bg_stage_rgb: [u8; 3],
    pub bg_card_rgb: [u8; 3],
    pub text_rgb: [u8; 3],
    pub corner_radius: f32,
    pub border_stroke_width: f32,
    pub ui_scale: f32,
    pub show_cursor_in_stream: bool,
    pub saved_servers: Vec<ServerProfile>,
    pub selected_server_id: String,
}

impl Default for UserCustomization {
    fn default() -> Self {
        let mut s = Self {
            account_created: false,
            username_login: String::new(),
            display_name: String::new(),
            user_tag: generate_random_tag(),
            custom_status: "Transmitindo em 1080p 60fps".to_string(),
            bio: "Membro do SZdisc • 1080p 60fps".to_string(),
            presence: PresenceStatus::Online,
            avatar_image_path: String::new(),
            banner_image_path: String::new(),
            avatar_bg: [88, 101, 242],
            banner_bg: [59, 68, 182],
            banner_bg_2: [139, 92, 246],
            name_color_rgb: [255, 255, 255],
            ring_color_rgb: [88, 101, 242],
            avatar_ring_glow: true,
            ring_thickness: 2.2,
            badge_1080p: true,
            badge_rust: true,
            badge_gamer: true,
            badge_crown: false,
            custom_badge_enabled: false,
            custom_badge_text: "💎 VIP".to_string(),
            custom_badge_rgb: [236, 72, 153],
            theme_preset: ThemePreset::DiscordDark,
            accent_rgb: [88, 101, 242],
            bg_sidebar_rgb: [30, 31, 34],
            bg_stage_rgb: [17, 18, 20],
            bg_card_rgb: [43, 45, 49],
            text_rgb: [242, 243, 245],
            corner_radius: 8.0,
            border_stroke_width: 1.0,
            ui_scale: 1.0,
            show_cursor_in_stream: true,
            saved_servers: Vec::new(),
            selected_server_id: String::new(),
        };
        s.apply_preset_colors(ThemePreset::DiscordDark);
        s
    }
}

impl UserCustomization {
    pub fn apply_preset_colors(&mut self, preset: ThemePreset) {
        self.theme_preset = preset;
        match preset {
            ThemePreset::DiscordDark => {
                self.accent_rgb = [88, 101, 242];
                self.bg_sidebar_rgb = [30, 31, 34];
                self.bg_stage_rgb = [17, 18, 20];
                self.bg_card_rgb = [43, 45, 49];
                self.text_rgb = [242, 243, 245];
            }
            ThemePreset::MidnightOled => {
                self.accent_rgb = [99, 102, 241];
                self.bg_sidebar_rgb = [10, 10, 13];
                self.bg_stage_rgb = [4, 4, 6];
                self.bg_card_rgb = [20, 21, 26];
                self.text_rgb = [245, 245, 250];
            }
            ThemePreset::CyberpunkNeon => {
                self.accent_rgb = [217, 70, 239];
                self.bg_sidebar_rgb = [22, 15, 38];
                self.bg_stage_rgb = [13, 9, 24];
                self.bg_card_rgb = [35, 24, 58];
                self.text_rgb = [250, 232, 255];
            }
            ThemePreset::EmeraldGamer => {
                self.accent_rgb = [16, 185, 129];
                self.bg_sidebar_rgb = [15, 28, 23];
                self.bg_stage_rgb = [9, 17, 14];
                self.bg_card_rgb = [23, 43, 35];
                self.text_rgb = [236, 253, 245];
            }
            ThemePreset::CrimsonRuby => {
                self.accent_rgb = [244, 63, 94];
                self.bg_sidebar_rgb = [29, 15, 19];
                self.bg_stage_rgb = [18, 9, 12];
                self.bg_card_rgb = [45, 23, 29];
                self.text_rgb = [255, 241, 242];
            }
            ThemePreset::ArcticOcean => {
                self.accent_rgb = [56, 189, 248];
                self.bg_sidebar_rgb = [15, 25, 40];
                self.bg_stage_rgb = [10, 16, 28];
                self.bg_card_rgb = [24, 38, 60];
                self.text_rgb = [240, 249, 255];
            }
            ThemePreset::Custom => {}
        }
    }

    pub fn save_to_disk(&self) {
        let cfg_path = get_user_config_path();
        let servers_serialized = self
            .saved_servers
            .iter()
            .map(|srv| {
                let clean_id = srv
                    .server_id
                    .replace('~', "")
                    .replace('|', "")
                    .replace('\n', "")
                    .trim()
                    .to_string();
                let clean_name = srv
                    .name
                    .replace('~', " ")
                    .replace('|', " ")
                    .replace('\n', " ")
                    .trim()
                    .to_string();
                let rooms_str = srv
                    .rooms
                    .iter()
                    .map(|r| {
                        r.replace(';', "")
                            .replace('~', "")
                            .replace('|', "")
                            .replace('\n', "")
                            .trim()
                            .to_string()
                    })
                    .filter(|r| !r.is_empty())
                    .collect::<Vec<_>>()
                    .join(";");
                format!(
                    "{}~{}~{},{},{}~{}",
                    clean_id,
                    clean_name,
                    srv.icon_rgb[0],
                    srv.icon_rgb[1],
                    srv.icon_rgb[2],
                    rooms_str
                )
            })
            .collect::<Vec<_>>()
            .join("||");

        let content = format!(
            "account_created={}\nusername_login={}\ndisplay_name={}\nuser_tag={}\ncustom_status={}\nbio={}\npresence={}\navatar_image_path={}\nbanner_image_path={}\navatar_bg={},{},{}\nbanner_bg={},{},{}\nbanner_bg_2={},{},{}\nname_color_rgb={},{},{}\nring_color_rgb={},{},{}\navatar_ring_glow={}\nring_thickness={}\nbadge_1080p={}\nbadge_rust={}\nbadge_gamer={}\nbadge_crown={}\ncustom_badge_enabled={}\ncustom_badge_text={}\ncustom_badge_rgb={},{},{}\ntheme_preset={}\naccent_rgb={},{},{}\nbg_sidebar_rgb={},{},{}\nbg_stage_rgb={},{},{}\nbg_card_rgb={},{},{}\ntext_rgb={},{},{}\ncorner_radius={}\nborder_stroke_width={}\nui_scale={}\nshow_cursor_in_stream={}\nselected_server_id={}\nsaved_servers={}\n",
            self.account_created,
            self.username_login.replace('\n', " "),
            self.display_name.replace('\n', " "),
            self.user_tag.replace('\n', " "),
            self.custom_status.replace('\n', " "),
            self.bio.replace('\n', " "),
            self.presence.code(),
            self.avatar_image_path,
            self.banner_image_path,
            self.avatar_bg[0], self.avatar_bg[1], self.avatar_bg[2],
            self.banner_bg[0], self.banner_bg[1], self.banner_bg[2],
            self.banner_bg_2[0], self.banner_bg_2[1], self.banner_bg_2[2],
            self.name_color_rgb[0], self.name_color_rgb[1], self.name_color_rgb[2],
            self.ring_color_rgb[0], self.ring_color_rgb[1], self.ring_color_rgb[2],
            self.avatar_ring_glow,
            self.ring_thickness,
            self.badge_1080p,
            self.badge_rust,
            self.badge_gamer,
            self.badge_crown,
            self.custom_badge_enabled,
            self.custom_badge_text.replace('\n', " "),
            self.custom_badge_rgb[0], self.custom_badge_rgb[1], self.custom_badge_rgb[2],
            self.theme_preset.code(),
            self.accent_rgb[0], self.accent_rgb[1], self.accent_rgb[2],
            self.bg_sidebar_rgb[0], self.bg_sidebar_rgb[1], self.bg_sidebar_rgb[2],
            self.bg_stage_rgb[0], self.bg_stage_rgb[1], self.bg_stage_rgb[2],
            self.bg_card_rgb[0], self.bg_card_rgb[1], self.bg_card_rgb[2],
            self.text_rgb[0], self.text_rgb[1], self.text_rgb[2],
            self.corner_radius,
            self.border_stroke_width,
            self.ui_scale,
            self.show_cursor_in_stream,
            self.selected_server_id.replace('\n', ""),
            servers_serialized
        );
        let _ = fs::write(cfg_path, content);
    }

    pub fn load_from_disk() -> Self {
        let mut cfg = Self::default();
        let cfg_path = get_user_config_path();
        if !cfg_path.exists() {
            return cfg;
        }
        let Ok(raw) = fs::read_to_string(cfg_path) else {
            return cfg;
        };

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
                "theme_preset" => cfg.theme_preset = ThemePreset::from_code(v.parse().unwrap_or(0)),
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
                "saved_servers" => {
                    let mut list = Vec::new();
                    for chunk in v.split("||") {
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
                    cfg.saved_servers = list;
                }
                _ => {}
            }
        }
        if cfg.display_name.trim().is_empty() {
            cfg.account_created = false;
        }
        cfg
    }
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

