use std::ffi::OsString;
use std::fs;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::ptr::null_mut;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{
    self, Color32, ColorImage, Context, Mesh, Pos2, Rect, RichText, Rounding, Stroke,
    TextureHandle, TextureOptions, Ui, Vec2,
};
use image::GenericImageView;
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

/// Abre a janela nativa do Windows para escolher uma imagem (PNG, JPG, JPEG, BMP)
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

/// Carrega uma imagem do disco e aplica máscara circular suave (estilo foto de perfil do Discord)
pub fn load_circular_avatar_texture(ctx: &Context, path: &str) -> Option<TextureHandle> {
    let img = image::open(path).ok()?;
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }

    let side = w.min(h);
    let crop_x = (w - side) / 2;
    let crop_y = (h - side) / 2;
    let cropped = img.crop_imm(crop_x, crop_y, side, side);
    let resized = cropped.resize_exact(128, 128, image::imageops::FilterType::Triangle);
    let mut rgba = resized.to_rgba8().into_raw();

    let center = 63.5_f32;
    let radius = 63.0_f32;
    for y in 0..128 {
        for x in 0..128 {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = (y * 128 + x) * 4;
            if dist > radius {
                rgba[idx + 3] = 0;
            } else if dist > radius - 1.5 {
                let factor = ((radius - dist) / 1.5).clamp(0.0, 1.0);
                rgba[idx + 3] = (rgba[idx + 3] as f32 * factor) as u8;
            }
        }
    }

    let color_img = ColorImage::from_rgba_unmultiplied([128, 128], &rgba);
    Some(ctx.load_texture("user_custom_avatar", color_img, TextureOptions::LINEAR))
}

/// Carrega uma imagem do disco para usar como Banner do Perfil
pub fn load_banner_texture(ctx: &Context, path: &str) -> Option<TextureHandle> {
    let img = image::open(path).ok()?;
    let resized = img.resize_to_fill(320, 84, image::imageops::FilterType::Triangle);
    let rgba = resized.to_rgba8().into_raw();
    let color_img = ColorImage::from_rgba_unmultiplied([320, 84], &rgba);
    Some(ctx.load_texture("user_custom_banner", color_img, TextureOptions::LINEAR))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PresenceStatus {
    Online,
    Idle,
    DoNotDisturb,
    Invisible,
}

impl PresenceStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Idle => "Ausente",
            Self::DoNotDisturb => "Não Perturbe",
            Self::Invisible => "Invisível",
        }
    }

    pub fn color(self) -> Color32 {
        match self {
            Self::Online => Color32::from_rgb(35, 165, 90),
            Self::Idle => Color32::from_rgb(240, 178, 50),
            Self::DoNotDisturb => Color32::from_rgb(242, 63, 67),
            Self::Invisible => Color32::from_rgb(128, 132, 142),
        }
    }

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
    pub fn label(self) -> &'static str {
        match self {
            Self::DiscordDark => "Escuro Clássico (Discord)",
            Self::MidnightOled => "Meia-Noite OLED (Preto Puro)",
            Self::CyberpunkNeon => "Cyberpunk Neon (Roxo/Rosa)",
            Self::EmeraldGamer => "Esmeralda Gamer (Verde)",
            Self::CrimsonRuby => "Rubi Carmesim (Vermelho)",
            Self::ArcticOcean => "Oceano Ártico (Azul)",
            Self::Custom => "Personalizado (Livre)",
        }
    }

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
    pub saved_rooms: Vec<String>,
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
            saved_rooms: vec![
                "Sala Geral #1".to_string(),
                "Sala de Jogos".to_string(),
                "Cinema & Vídeos".to_string(),
            ],
        };
        s.apply_preset_colors(ThemePreset::DiscordDark);
        s
    }
}

impl UserCustomization {
    pub fn accent_color(&self) -> Color32 {
        Color32::from_rgb(self.accent_rgb[0], self.accent_rgb[1], self.accent_rgb[2])
    }

    pub fn sidebar_color(&self) -> Color32 {
        Color32::from_rgb(
            self.bg_sidebar_rgb[0],
            self.bg_sidebar_rgb[1],
            self.bg_sidebar_rgb[2],
        )
    }

    pub fn stage_color(&self) -> Color32 {
        Color32::from_rgb(
            self.bg_stage_rgb[0],
            self.bg_stage_rgb[1],
            self.bg_stage_rgb[2],
        )
    }

    pub fn card_color(&self) -> Color32 {
        Color32::from_rgb(self.bg_card_rgb[0], self.bg_card_rgb[1], self.bg_card_rgb[2])
    }

    pub fn avatar_color(&self) -> Color32 {
        Color32::from_rgb(self.avatar_bg[0], self.avatar_bg[1], self.avatar_bg[2])
    }

    pub fn banner_color(&self) -> Color32 {
        Color32::from_rgb(self.banner_bg[0], self.banner_bg[1], self.banner_bg[2])
    }

    pub fn banner_color_2(&self) -> Color32 {
        Color32::from_rgb(self.banner_bg_2[0], self.banner_bg_2[1], self.banner_bg_2[2])
    }

    pub fn name_color(&self) -> Color32 {
        Color32::from_rgb(
            self.name_color_rgb[0],
            self.name_color_rgb[1],
            self.name_color_rgb[2],
        )
    }

    pub fn ring_color(&self) -> Color32 {
        Color32::from_rgb(
            self.ring_color_rgb[0],
            self.ring_color_rgb[1],
            self.ring_color_rgb[2],
        )
    }

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

    pub fn apply_to_egui(&self, ctx: &Context) {
        ctx.set_pixels_per_point(self.ui_scale.clamp(0.85, 1.25));

        let mut visuals = egui::Visuals::dark();
        let r = Rounding::same(self.corner_radius);
        let text_col = Color32::from_rgb(self.text_rgb[0], self.text_rgb[1], self.text_rgb[2]);

        visuals.override_text_color = Some(text_col);
        visuals.panel_fill = self.sidebar_color();
        visuals.window_fill = self.card_color();
        visuals.window_rounding = Rounding::same((self.corner_radius + 4.0).min(18.0));
        visuals.menu_rounding = r;

        visuals.widgets.noninteractive.bg_fill = self.card_color();
        visuals.widgets.noninteractive.rounding = r;

        visuals.widgets.inactive.bg_fill = Color32::from_rgb(
            self.bg_card_rgb[0].saturating_add(10),
            self.bg_card_rgb[1].saturating_add(10),
            self.bg_card_rgb[2].saturating_add(12),
        );
        visuals.widgets.inactive.rounding = r;

        visuals.widgets.hovered.bg_fill = Color32::from_rgb(
            self.bg_card_rgb[0].saturating_add(22),
            self.bg_card_rgb[1].saturating_add(22),
            self.bg_card_rgb[2].saturating_add(26),
        );
        visuals.widgets.hovered.rounding = r;
        visuals.widgets.hovered.bg_stroke =
            Stroke::new(self.border_stroke_width, self.accent_color());

        visuals.widgets.active.bg_fill = self.accent_color();
        visuals.widgets.active.rounding = r;

        visuals.selection.bg_fill = self.accent_color();
        visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);

        ctx.set_visuals(visuals);
    }

    pub fn save_to_disk(&self) {
        let cfg_path = get_user_config_path();
        let rooms_joined = self
            .saved_rooms
            .iter()
            .map(|r| r.replace(';', "").replace('\n', "").trim().to_string())
            .filter(|r| !r.is_empty())
            .collect::<Vec<_>>()
            .join(";");
        let content = format!(
            "account_created={}\nusername_login={}\ndisplay_name={}\nuser_tag={}\ncustom_status={}\nbio={}\npresence={}\navatar_image_path={}\nbanner_image_path={}\navatar_bg={},{},{}\nbanner_bg={},{},{}\nbanner_bg_2={},{},{}\nname_color_rgb={},{},{}\nring_color_rgb={},{},{}\navatar_ring_glow={}\nring_thickness={}\nbadge_1080p={}\nbadge_rust={}\nbadge_gamer={}\nbadge_crown={}\ncustom_badge_enabled={}\ncustom_badge_text={}\ncustom_badge_rgb={},{},{}\ntheme_preset={}\naccent_rgb={},{},{}\nbg_sidebar_rgb={},{},{}\nbg_stage_rgb={},{},{}\nbg_card_rgb={},{},{}\ntext_rgb={},{},{}\ncorner_radius={}\nborder_stroke_width={}\nui_scale={}\nshow_cursor_in_stream={}\nsaved_rooms={}\n",
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
            rooms_joined
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
                "saved_rooms" => {
                    let parsed: Vec<String> = v
                        .split(';')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if !parsed.is_empty() {
                        cfg.saved_rooms = parsed;
                    }
                }
                _ => {}
            }
        }
        if cfg.display_name.trim().is_empty() {
            cfg.account_created = false;
        }
        cfg
    }

    pub fn active_badges(&self) -> Vec<(String, Color32)> {
        let mut list = Vec::new();
        if self.badge_crown {
            list.push(("👑 Host".to_string(), Color32::from_rgb(245, 158, 11)));
        }
        if self.badge_1080p {
            list.push(("⚡ 1080p60".to_string(), Color32::from_rgb(88, 101, 242)));
        }
        if self.badge_rust {
            list.push(("🦀 Rust".to_string(), Color32::from_rgb(234, 88, 12)));
        }
        if self.badge_gamer {
            list.push(("🎮 Pro".to_string(), Color32::from_rgb(16, 185, 129)));
        }
        if self.custom_badge_enabled && !self.custom_badge_text.trim().is_empty() {
            list.push((
                self.custom_badge_text.clone(),
                Color32::from_rgb(
                    self.custom_badge_rgb[0],
                    self.custom_badge_rgb[1],
                    self.custom_badge_rgb[2],
                ),
            ));
        }
        list
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

pub fn draw_discord_avatar(
    ui: &mut Ui,
    size: f32,
    cust: &UserCustomization,
    avatar_tex: Option<&TextureHandle>,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let painter = ui.painter();
    let center = rect.center();
    let radius = size * 0.5;

    if let Some(tex) = avatar_tex {
        let img_rect = Rect::from_center_size(center, Vec2::splat(size - 3.0));
        painter.image(
            tex.id(),
            img_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        painter.circle_filled(center, radius - 2.0, cust.avatar_color());
        let initial = cust.display_name.chars().next().unwrap_or('?');
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            initial.to_uppercase().to_string(),
            egui::FontId::proportional(size * 0.44),
            Color32::WHITE,
        );
    }

    if cust.avatar_ring_glow && cust.ring_thickness > 0.1 {
        painter.circle_stroke(
            center,
            radius - 1.0,
            Stroke::new(cust.ring_thickness, cust.ring_color()),
        );
    }

    let dot_r = (size * 0.16).max(4.5);
    let dot_pos = Pos2::new(rect.right() - dot_r * 0.75, rect.bottom() - dot_r * 0.75);
    painter.circle_filled(dot_pos, dot_r + 2.0, Color32::from_rgb(17, 18, 20));
    painter.circle_filled(dot_pos, dot_r, cust.presence.color());
}

pub fn draw_discord_profile_card(
    ui: &mut Ui,
    cust: &UserCustomization,
    avatar_tex: Option<&TextureHandle>,
    banner_tex: Option<&TextureHandle>,
    is_live: bool,
) {
    let card_width = ui.available_width().min(320.0);
    egui::Frame::none()
        .fill(Color32::from_rgb(17, 18, 20))
        .rounding(Rounding::same(cust.corner_radius.max(8.0)))
        .stroke(Stroke::new(
            cust.border_stroke_width.max(1.0),
            cust.accent_color().gamma_multiply(0.55),
        ))
        .show(ui, |ui| {
            ui.set_width(card_width);

            let (banner_rect, _) =
                ui.allocate_exact_size(Vec2::new(card_width, 72.0), egui::Sense::hover());
            let painter = ui.painter();

            if let Some(btex) = banner_tex {
                painter.image(
                    btex.id(),
                    banner_rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else {
                let mut mesh = Mesh::default();
                let c1 = cust.banner_color();
                let c2 = cust.banner_color_2();
                mesh.colored_vertex(banner_rect.left_top(), c1);
                mesh.colored_vertex(banner_rect.right_top(), c2);
                mesh.colored_vertex(banner_rect.right_bottom(), c2);
                mesh.colored_vertex(banner_rect.left_bottom(), c1);
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                painter.add(egui::Shape::mesh(mesh));
            }

            if is_live {
                let badge_rect = Rect::from_min_size(
                    Pos2::new(banner_rect.right() - 74.0, banner_rect.top() + 10.0),
                    Vec2::new(64.0, 20.0),
                );
                painter.rect_filled(badge_rect, Rounding::same(4.0), Color32::from_rgb(242, 63, 67));
                painter.text(
                    badge_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "AO VIVO",
                    egui::FontId::proportional(10.5),
                    Color32::WHITE,
                );
            }

            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        draw_discord_avatar(ui, 54.0, cust, avatar_tex);

                        ui.vertical(|ui| {
                            let shown_name = if cust.display_name.trim().is_empty() {
                                "Novo Usuário"
                            } else {
                                cust.display_name.as_str()
                            };
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(shown_name)
                                        .strong()
                                        .size(16.0)
                                        .color(cust.name_color()),
                                );
                                ui.label(
                                    RichText::new(&cust.user_tag)
                                        .size(12.0)
                                        .color(Color32::from_rgb(148, 155, 164)),
                                );
                            });

                            let badges = cust.active_badges();
                            if !badges.is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    for (txt, col) in badges {
                                        egui::Frame::none()
                                            .fill(col.gamma_multiply(0.24))
                                            .stroke(Stroke::new(1.0_f32, col))
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::symmetric(5.0, 1.5))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(txt)
                                                        .size(10.0)
                                                        .strong()
                                                        .color(Color32::WHITE),
                                                );
                                            });
                                    }
                                });
                            }
                        });
                    });

                    if !cust.custom_status.trim().is_empty() {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!("💬 {}", cust.custom_status))
                                .size(12.0)
                                .color(Color32::from_rgb(215, 220, 228)),
                        );
                    }

                    if !cust.bio.trim().is_empty() {
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("SOBRE MIM")
                                .size(10.0)
                                .strong()
                                .color(Color32::from_rgb(148, 155, 164)),
                        );
                        ui.label(
                            RichText::new(&cust.bio)
                                .size(11.5)
                                .color(Color32::from_rgb(195, 200, 208)),
                        );
                    }
                });
        });
}
