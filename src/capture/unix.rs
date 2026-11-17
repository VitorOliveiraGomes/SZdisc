use xcap::{Monitor, Window};

use super::{CaptureSource, CaptureSourceKind, RawFrame, SourceCategory};

pub fn list_capture_sources() -> Vec<CaptureSource> {
    let mut sources = Vec::new();
    if let Ok(monitors) = Monitor::all() {
        for (idx, m) in monitors.into_iter().enumerate() {
            let w = m.width().unwrap_or(0) as i32;
            let h = m.height().unwrap_or(0) as i32;
            if w <= 0 || h <= 0 {
                continue;
            }
            sources.push(CaptureSource {
                id: format!("mon_{}", idx),
                name: format!("Tela {} ({}x{})", idx + 1, w, h),
                subtitle: m.name().unwrap_or_default(),
                exe_name: "Monitor".to_string(),
                pid: 0,
                category: SourceCategory::Screen,
                kind: CaptureSourceKind::Monitor {
                    index: idx,
                    left: m.x().unwrap_or(0),
                    top: m.y().unwrap_or(0),
                    width: w,
                    height: h,
                },
            });
        }
    }
    if let Ok(windows) = Window::all() {
        for w in windows {
            if w.is_minimized().unwrap_or(false)
                || w.width().unwrap_or(0) < 80
                || w.height().unwrap_or(0) < 80
            {
                continue;
            }
            let title = w.title().unwrap_or_default().trim().to_string();
            if title.is_empty()
                || title == "ScreenZ"
                || title.starts_with("ScreenZ —")
                || title.starts_with("ScreenZ Pop-out")
            {
                continue;
            }
            let app = w.app_name().unwrap_or_default();
            let lower = app.to_lowercase();
            let category = if lower.contains("chrome")
                || lower.contains("firefox")
                || lower.contains("safari")
                || lower.contains("edge")
                || lower.contains("brave")
            {
                SourceCategory::Browser
            } else {
                SourceCategory::Application
            };
            let wid = w.id().unwrap_or(0);
            sources.push(CaptureSource {
                id: format!("win_{}", wid),
                name: title,
                subtitle: app.clone(),
                exe_name: app,
                pid: w.pid().unwrap_or(0),
                category,
                kind: CaptureSourceKind::Window { hwnd: wid as usize },
            });
        }
    }
    sources
}

pub fn capture_thumbnail_rgba(
    source: &CaptureSource,
    max_w: u32,
    max_h: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    let img = capture_xcap_image(source)?;
    let dyn_img = image::DynamicImage::ImageRgba8(img);
    let resized = dyn_img.thumbnail(max_w, max_h).to_rgba8();
    let (w, h) = resized.dimensions();
    Some((w, h, resized.into_raw()))
}

pub fn is_screenz_in_foreground() -> bool {
    true
}

pub fn get_process_ram_mb() -> f32 {
    0.0
}

pub struct FastCapturer;

impl FastCapturer {
    pub fn new() -> Self {
        Self
    }

    pub fn recycle_rgb(&mut self, _buf: Vec<u8>) {}

    pub fn force_next_frame(&mut self) {}

    pub fn capture_live_frame(
        &mut self,
        source: &CaptureSource,
        target_w: u32,
        target_h: u32,
        need_rgba_preview: bool,
        _draw_cursor: bool,
    ) -> Option<RawFrame> {
        let img = capture_xcap_image(source)?;
        let src_w = img.width();
        let src_h = img.height();
        if src_w == 0 || src_h == 0 {
            return None;
        }
        let scale = (target_w as f32 / src_w as f32)
            .min(target_h as f32 / src_h as f32)
            .min(1.0);
        let out_w = (((src_w as f32 * scale) as u32) & !1).max(2);
        let out_h = (((src_h as f32 * scale) as u32) & !1).max(2);

        let dyn_img = image::DynamicImage::ImageRgba8(img);
        let resized = if out_w != src_w || out_h != src_h {
            dyn_img.resize_exact(out_w, out_h, image::imageops::FilterType::Triangle)
        } else {
            dyn_img
        };
        let rgb = resized.to_rgb8().into_raw();
        let rgba = if need_rgba_preview {
            resized.to_rgba8().into_raw()
        } else {
            Vec::new()
        };
        Some(RawFrame {
            width: out_w,
            height: out_h,
            rgb,
            rgba,
        })
    }
}

fn capture_xcap_image(source: &CaptureSource) -> Option<image::RgbaImage> {
    match source.kind {
        CaptureSourceKind::Monitor { index, .. } => Monitor::all()
            .ok()?
            .into_iter()
            .nth(index)?
            .capture_image()
            .ok(),
        CaptureSourceKind::Window { hwnd } => Window::all()
            .ok()?
            .into_iter()
            .find(|w| w.id().ok() == Some(hwnd as u32))?
            .capture_image()
            .ok(),
    }
}
