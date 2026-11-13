use std::fs;
use std::path::Path;

use eframe::egui::{self, Color32, Context, Pos2, Rect, RichText, Rounding, Stroke, Ui, Vec2};

const CONFIG_FILE: &str = "szdisc_settings.conf";

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
            Self::Custom => "Personalizado (Cores Livres)",
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
    // Perfil estilo Discord
    pub display_name: String,
    pub user_tag: String,
    pub custom_status: String,
    pub bio: String,
    pub presence: PresenceStatus,
    pub avatar_bg: [u8; 3],
    pub banner_bg: [u8; 3],
    pub avatar_ring_glow: bool,
    pub badge_1080p: bool,
    pub badge_rust: bool,
    pub badge_gamer: bool,
    pub badge_crown: bool,

    // Aparência do Aplicativo
    pub theme_preset: ThemePreset,
    pub accent_rgb: [u8; 3],
    pub bg_sidebar_rgb: [u8; 3],
    pub bg_stage_rgb: [u8; 3],
    pub bg_card_rgb: [u8; 3],
    pub corner_radius: f32,
    pub ui_scale: f32,
    pub show_cursor_in_stream: bool,
}

impl Default for UserCustomization {
    fn default() -> Self {
        let mut s = Self {
            display_name: "Vitor".to_string(),
            user_tag: "#0001".to_string(),
            custom_status: "Transmitindo em 1080p 60fps".to_string(),
            bio: "Transmissão ultra-leve nativa em Rust.".to_string(),
            presence: PresenceStatus::Online,
            avatar_bg: [88, 101, 242],
            banner_bg: [59, 68, 182],
            avatar_ring_glow: true,
            badge_1080p: true,
            badge_rust: true,
            badge_gamer: true,
            badge_crown: true,
            theme_preset: ThemePreset::DiscordDark,
            accent_rgb: [88, 101, 242],
            bg_sidebar_rgb: [30, 31, 34],
            bg_stage_rgb: [17, 18, 20],
            bg_card_rgb: [43, 45, 49],
            corner_radius: 8.0,
            ui_scale: 1.0,
            show_cursor_in_stream: true,
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

    pub fn apply_preset_colors(&mut self, preset: ThemePreset) {
        self.theme_preset = preset;
        match preset {
            ThemePreset::DiscordDark => {
                self.accent_rgb = [88, 101, 242];
                self.bg_sidebar_rgb = [30, 31, 34];
                self.bg_stage_rgb = [17, 18, 20];
                self.bg_card_rgb = [43, 45, 49];
            }
            ThemePreset::MidnightOled => {
                self.accent_rgb = [99, 102, 241];
                self.bg_sidebar_rgb = [10, 10, 13];
                self.bg_stage_rgb = [4, 4, 6];
                self.bg_card_rgb = [20, 21, 26];
            }
            ThemePreset::CyberpunkNeon => {
                self.accent_rgb = [217, 70, 239];
                self.bg_sidebar_rgb = [22, 15, 38];
                self.bg_stage_rgb = [13, 9, 24];
                self.bg_card_rgb = [35, 24, 58];
            }
            ThemePreset::EmeraldGamer => {
                self.accent_rgb = [16, 185, 129];
                self.bg_sidebar_rgb = [15, 28, 23];
                self.bg_stage_rgb = [9, 17, 14];
                self.bg_card_rgb = [23, 43, 35];
            }
            ThemePreset::CrimsonRuby => {
                self.accent_rgb = [244, 63, 94];
                self.bg_sidebar_rgb = [29, 15, 19];
                self.bg_stage_rgb = [18, 9, 12];
                self.bg_card_rgb = [45, 23, 29];
            }
            ThemePreset::ArcticOcean => {
                self.accent_rgb = [56, 189, 248];
                self.bg_sidebar_rgb = [15, 25, 40];
                self.bg_stage_rgb = [10, 16, 28];
                self.bg_card_rgb = [24, 38, 60];
            }
            ThemePreset::Custom => {}
        }
    }

    pub fn apply_to_egui(&self, ctx: &Context) {
        ctx.set_pixels_per_point(self.ui_scale.clamp(0.85, 1.25));

        let mut visuals = egui::Visuals::dark();
        let r = Rounding::same(self.corner_radius);

        visuals.panel_fill = self.sidebar_color();
        visuals.window_fill = self.card_color();
        visuals.window_rounding = Rounding::same((self.corner_radius + 4.0).min(16.0));
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
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.accent_color());

        visuals.widgets.active.bg_fill = self.accent_color();
        visuals.widgets.active.rounding = r;

        visuals.selection.bg_fill = self.accent_color();
        visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);

        ctx.set_visuals(visuals);
    }

    pub fn save_to_disk(&self) {
        let content = format!(
            "display_name={}\nuser_tag={}\ncustom_status={}\nbio={}\npresence={}\navatar_bg={},{},{}\nbanner_bg={},{},{}\navatar_ring_glow={}\nbadge_1080p={}\nbadge_rust={}\nbadge_gamer={}\nbadge_crown={}\ntheme_preset={}\naccent_rgb={},{},{}\nbg_sidebar_rgb={},{},{}\nbg_stage_rgb={},{},{}\nbg_card_rgb={},{},{}\ncorner_radius={}\nui_scale={}\nshow_cursor_in_stream={}\n",
            self.display_name.replace('\n', " "),
            self.user_tag.replace('\n', " "),
            self.custom_status.replace('\n', " "),
            self.bio.replace('\n', " "),
            self.presence.code(),
            self.avatar_bg[0], self.avatar_bg[1], self.avatar_bg[2],
            self.banner_bg[0], self.banner_bg[1], self.banner_bg[2],
            self.avatar_ring_glow,
            self.badge_1080p,
            self.badge_rust,
            self.badge_gamer,
            self.badge_crown,
            self.theme_preset.code(),
            self.accent_rgb[0], self.accent_rgb[1], self.accent_rgb[2],
            self.bg_sidebar_rgb[0], self.bg_sidebar_rgb[1], self.bg_sidebar_rgb[2],
            self.bg_stage_rgb[0], self.bg_stage_rgb[1], self.bg_stage_rgb[2],
            self.bg_card_rgb[0], self.bg_card_rgb[1], self.bg_card_rgb[2],
            self.corner_radius,
            self.ui_scale,
            self.show_cursor_in_stream
        );
        let _ = fs::write(CONFIG_FILE, content);
    }

    pub fn load_from_disk() -> Self {
        let mut cfg = Self::default();
        if !Path::new(CONFIG_FILE).exists() {
            return cfg;
        }
        let Ok(raw) = fs::read_to_string(CONFIG_FILE) else {
            return cfg;
        };

        for line in raw.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k.trim() {
                "display_name" => cfg.display_name = v.to_string(),
                "user_tag" => cfg.user_tag = v.to_string(),
                "custom_status" => cfg.custom_status = v.to_string(),
                "bio" => cfg.bio = v.to_string(),
                "presence" => cfg.presence = PresenceStatus::from_code(v.parse().unwrap_or(0)),
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
                "avatar_ring_glow" => cfg.avatar_ring_glow = v == "true",
                "badge_1080p" => cfg.badge_1080p = v == "true",
                "badge_rust" => cfg.badge_rust = v == "true",
                "badge_gamer" => cfg.badge_gamer = v == "true",
                "badge_crown" => cfg.badge_crown = v == "true",
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
                "corner_radius" => cfg.corner_radius = v.parse().unwrap_or(8.0),
                "ui_scale" => cfg.ui_scale = v.parse().unwrap_or(1.0),
                "show_cursor_in_stream" => cfg.show_cursor_in_stream = v == "true",
                _ => {}
            }
        }
        cfg
    }

    pub fn active_badges(&self) -> Vec<(&'static str, Color32)> {
        let mut list = Vec::new();
        if self.badge_crown {
            list.push(("👑 Host", Color32::from_rgb(245, 158, 11)));
        }
        if self.badge_1080p {
            list.push(("⚡ 1080p60", Color32::from_rgb(88, 101, 242)));
        }
        if self.badge_rust {
            list.push(("🦀 Rust", Color32::from_rgb(234, 88, 12)));
        }
        if self.badge_gamer {
            list.push(("🎮 Pro", Color32::from_rgb(16, 185, 129)));
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

/// Desenha um avatar redondo estilo Discord com anel neon opcional e bolinha de status
pub fn draw_discord_avatar(
    ui: &mut Ui,
    size: f32,
    initial: char,
    bg_color: Color32,
    presence: PresenceStatus,
    ring_glow: bool,
    ring_color: Color32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let painter = ui.painter();
    let center = rect.center();
    let radius = size * 0.5;

    if ring_glow {
        painter.circle_stroke(center, radius, Stroke::new(2.0_f32, ring_color));
    }

    painter.circle_filled(center, radius - 2.0, bg_color);

    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        initial.to_uppercase().to_string(),
        egui::FontId::proportional(size * 0.44),
        Color32::WHITE,
    );

    // Bolinha de status no canto inferior direito (com borda escura igual ao Discord)
    let dot_r = (size * 0.16).max(4.5);
    let dot_pos = Pos2::new(rect.right() - dot_r * 0.75, rect.bottom() - dot_r * 0.75);
    painter.circle_filled(dot_pos, dot_r + 2.0, Color32::from_rgb(20, 21, 24));
    painter.circle_filled(dot_pos, dot_r, presence.color());
}

/// Desenha o Card Completo de Perfil Estilo Discord (com Banner colorido, Avatar sobreposto, Insígnias e Bio)
pub fn draw_discord_profile_card(ui: &mut Ui, cust: &UserCustomization, is_live: bool) {
    let card_width = ui.available_width().min(310.0);
    egui::Frame::none()
        .fill(Color32::from_rgb(17, 18, 20))
        .rounding(Rounding::same(10.0))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 22)))
        .show(ui, |ui| {
            ui.set_width(card_width);

            // Banner Superior
            let (banner_rect, _) =
                ui.allocate_exact_size(Vec2::new(card_width, 64.0), egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(
                banner_rect,
                Rounding {
                    nw: 10.0,
                    ne: 10.0,
                    sw: 0.0,
                    se: 0.0,
                },
                cust.banner_color(),
            );

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

            // Corpo do Card
            egui::Frame::none()
                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let initial = cust.display_name.chars().next().unwrap_or('U');
                        draw_discord_avatar(
                            ui,
                            48.0,
                            initial,
                            cust.avatar_color(),
                            cust.presence,
                            cust.avatar_ring_glow,
                            cust.accent_color(),
                        );

                        ui.vertical(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(&cust.display_name)
                                        .strong()
                                        .size(15.5)
                                        .color(Color32::WHITE),
                                );
                                ui.label(
                                    RichText::new(&cust.user_tag)
                                        .size(12.0)
                                        .color(Color32::from_rgb(148, 155, 164)),
                                );
                            });

                            // Insígnias (Badges)
                            let badges = cust.active_badges();
                            if !badges.is_empty() {
                                ui.horizontal_wrapped(|ui| {
                                    for (txt, col) in badges {
                                        egui::Frame::none()
                                            .fill(col.gamma_multiply(0.22))
                                            .stroke(Stroke::new(1.0_f32, col))
                                            .rounding(Rounding::same(4.0))
                                            .inner_margin(egui::Margin::symmetric(5.0, 1.5))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(txt).size(10.0).strong().color(Color32::WHITE),
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
