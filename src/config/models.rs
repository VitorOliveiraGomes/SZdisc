use std::time::{SystemTime, UNIX_EPOCH};

pub fn generate_random_tag() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(4829);
    format!("#{}", 1000 + (nanos % 9000))
}

/// Gera um código único de Servidor (ex: `"SZ-48291"`).
pub fn generate_server_id(server_name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(982_451_653);
    let mut hash: u32 = 2_166_136_261;
    for b in server_name.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16_777_619);
    }
    hash ^= (nanos & 0xFFFF_FFFF) as u32;
    hash = hash.wrapping_mul(16_777_619);
    format!("SZ-{}", 10000 + (hash % 90000))
}

/// Representa um Servidor isolado com seu Código de Convite e Salas.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerProfile {
    pub server_id: String,
    pub name: String,
    pub icon_rgb: [u8; 3],
    pub rooms: Vec<String>,
}

impl ServerProfile {
    pub fn invite_token(&self) -> String {
        let clean_name = self.name.replace(['/', '|'], "-").trim().to_string();
        let rooms_csv = self
            .rooms
            .iter()
            .map(|r| r.replace(',', " ").replace('/', "-").trim().to_string())
            .filter(|r| !r.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        format!("{}/{}/{}", self.server_id, clean_name, rooms_csv)
    }

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

        let mut color_hash: u32 = 2_166_136_261;
        for b in server_id.as_bytes() {
            color_hash ^= *b as u32;
            color_hash = color_hash.wrapping_mul(16_777_619);
        }
        const PALETTE: [[u8; 3]; 6] = [
            [88, 101, 242],
            [16, 185, 129],
            [236, 72, 153],
            [245, 158, 11],
            [56, 189, 248],
            [168, 85, 247],
        ];
        let icon_rgb = PALETTE[(color_hash as usize) % PALETTE.len()];

        Some(Self {
            server_id,
            name,
            icon_rgb,
            rooms,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemePreset {
    StudioDark,
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
            Self::StudioDark => 0,
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
            _ => Self::StudioDark,
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
            custom_status: String::new(),
            bio: String::new(),
            presence: PresenceStatus::Online,
            avatar_image_path: String::new(),
            banner_image_path: String::new(),
            avatar_bg: [57, 76, 49],
            banner_bg: [42, 59, 43],
            banner_bg_2: [93, 112, 55],
            name_color_rgb: [255, 255, 255],
            ring_color_rgb: [213, 247, 128],
            avatar_ring_glow: false,
            ring_thickness: 2.2,
            badge_1080p: false,
            badge_rust: false,
            badge_gamer: false,
            badge_crown: false,
            custom_badge_enabled: false,
            custom_badge_text: "💎 VIP".to_string(),
            custom_badge_rgb: [236, 72, 153],
            theme_preset: ThemePreset::StudioDark,
            accent_rgb: [213, 247, 128],
            bg_sidebar_rgb: [21, 27, 22],
            bg_stage_rgb: [16, 20, 17],
            bg_card_rgb: [28, 36, 30],
            text_rgb: [237, 241, 230],
            corner_radius: 14.0,
            border_stroke_width: 1.0,
            ui_scale: 1.0,
            show_cursor_in_stream: true,
            saved_servers: Vec::new(),
            selected_server_id: String::new(),
        };
        s.apply_preset_colors(ThemePreset::StudioDark);
        s
    }
}

impl UserCustomization {
    pub fn apply_preset_colors(&mut self, preset: ThemePreset) {
        self.theme_preset = preset;
        match preset {
            ThemePreset::StudioDark => {
                self.accent_rgb = [213, 247, 128];
                self.bg_sidebar_rgb = [21, 27, 22];
                self.bg_stage_rgb = [16, 20, 17];
                self.bg_card_rgb = [28, 36, 30];
                self.text_rgb = [237, 241, 230];
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
}
