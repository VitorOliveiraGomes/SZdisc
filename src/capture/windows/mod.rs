mod dxgi;
mod enumerate;
mod gdi;
mod scaler;

use std::mem::{size_of, zeroed};
use std::time::{Duration, Instant};

use winapi::shared::minwindef::DWORD;
use winapi::shared::windef::{HWND, RECT};
use winapi::um::dwmapi::DwmGetWindowAttribute;
use winapi::um::processthreadsapi::GetCurrentProcess;
use winapi::um::psapi::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use winapi::um::winuser::{
    GetSystemMetrics, GetWindowRect, IsIconic, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use super::{CaptureSource, CaptureSourceKind, RawFrame};
use dxgi::{DxgiCaptureResult, DxgiDuplicator};
use gdi::{capture_single_frame, CursorOverlay, GdiSurface};
use scaler::{compute_output_dimensions, convert_bgra_to_frame};

pub use enumerate::{is_screenz_in_foreground, list_capture_sources};

pub fn capture_thumbnail_rgba(
    source: &CaptureSource,
    thumb_w: u32,
    thumb_h: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    let frame = capture_single_frame(source, thumb_w, thumb_h, false, true, false)?;
    Some((frame.width, frame.height, frame.rgba))
}

pub fn get_process_ram_mb() -> f32 {
    unsafe {
        let mut pmc: PROCESS_MEMORY_COUNTERS = zeroed();
        pmc.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as DWORD;
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) != 0 {
            pmc.WorkingSetSize as f32 / (1024.0 * 1024.0)
        } else {
            0.0
        }
    }
}

pub struct FastCapturer {
    gdi: GdiSurface,
    cursor_overlay: CursorOverlay,
    dxgi: Option<DxgiDuplicator>,
    dxgi_bgra_buf: Vec<u8>,
    dxgi_retry_after: Instant,
    last_source_id: String,
    spare_rgb: Vec<u8>,
    last_sent_at: Instant,
    last_cursor_pos: Option<(i32, i32)>,
}

unsafe impl Send for FastCapturer {}

impl FastCapturer {
    pub fn new() -> Self {
        let past = Instant::now() - Duration::from_secs(2);
        Self {
            gdi: GdiSurface::new(),
            cursor_overlay: CursorOverlay::new(),
            dxgi: None,
            dxgi_bgra_buf: Vec::new(),
            dxgi_retry_after: past,
            last_source_id: String::new(),
            spare_rgb: Vec::new(),
            last_sent_at: past,
            last_cursor_pos: None,
        }
    }

    pub fn force_next_frame(&mut self) {
        self.last_sent_at = Instant::now() - Duration::from_secs(2);
    }

    pub fn recycle_rgb(&mut self, buf: Vec<u8>) {
        if buf.capacity() > self.spare_rgb.capacity() {
            self.spare_rgb = buf;
        }
    }

    unsafe fn resolve_source_rect(source: &CaptureSource) -> Option<(i32, i32, i32, i32)> {
        match source.kind {
            CaptureSourceKind::Monitor {
                left,
                top,
                width,
                height,
                ..
            } => {
                if width >= 2 && height >= 2 {
                    Some((left, top, width, height))
                } else {
                    None
                }
            }
            CaptureSourceKind::Window { hwnd } => {
                let hwnd_ptr = hwnd as HWND;
                if IsIconic(hwnd_ptr) != 0 {
                    return None;
                }
                let mut rect: RECT = zeroed();
                let dwm_ok = DwmGetWindowAttribute(
                    hwnd_ptr,
                    9, // DWMWA_EXTENDED_FRAME_BOUNDS
                    &mut rect as *mut _ as *mut _,
                    size_of::<RECT>() as DWORD,
                ) == 0;
                if !dwm_ok && GetWindowRect(hwnd_ptr, &mut rect) == 0 {
                    return None;
                }

                let v_left = GetSystemMetrics(SM_XVIRTUALSCREEN);
                let v_top = GetSystemMetrics(SM_YVIRTUALSCREEN);
                let v_cx = GetSystemMetrics(SM_CXVIRTUALSCREEN);
                let v_cy = GetSystemMetrics(SM_CYVIRTUALSCREEN);
                if v_cx <= 0 || v_cy <= 0 {
                    return None;
                }
                let v_right = v_left + v_cx;
                let v_bottom = v_top + v_cy;

                let cl_left = rect.left.max(v_left);
                let cl_top = rect.top.max(v_top);
                let cl_right = rect.right.min(v_right);
                let cl_bottom = rect.bottom.min(v_bottom);

                let w = cl_right - cl_left;
                let h = cl_bottom - cl_top;
                if w < 2 || h < 2 {
                    return None;
                }
                Some((cl_left, cl_top, w, h))
            }
        }
    }

    pub fn capture_live_frame(
        &mut self,
        source: &CaptureSource,
        target_w: u32,
        target_h: u32,
        need_rgba_preview: bool,
        draw_cursor: bool,
    ) -> Option<RawFrame> {
        unsafe {
            let (src_left, src_top, src_w, src_h) = Self::resolve_source_rect(source)?;
            let (out_w, out_h, _) = compute_output_dimensions(src_w, src_h, target_w, target_h)?;

            if self.last_source_id != source.id {
                self.last_source_id = source.id.clone();
                self.force_next_frame();
                self.dxgi_retry_after = Instant::now() - Duration::from_secs(1);
                self.last_cursor_pos = None;
            }

            let cursor_pos_now = if draw_cursor {
                CursorOverlay::current_relative_pos(src_left, src_top, src_w, src_h)
            } else {
                None
            };
            let cursor_moved = draw_cursor && cursor_pos_now != self.last_cursor_pos;
            self.last_cursor_pos = cursor_pos_now;

            let fallback_output = match source.kind {
                CaptureSourceKind::Monitor { index, .. } => index.saturating_sub(1),
                CaptureSourceKind::Window { .. } => 0,
            };
            let need_reinit = self
                .dxgi
                .as_ref()
                .map(|d| !d.contains_rect(src_left, src_top, src_w, src_h))
                .unwrap_or(true);
            if need_reinit && Instant::now() >= self.dxgi_retry_after {
                if let Some(mut old) = self.dxgi.take() {
                    old.release_all();
                }
                self.dxgi = DxgiDuplicator::init_for_region(
                    src_left,
                    src_top,
                    src_w,
                    src_h,
                    fallback_output,
                );
                if self.dxgi.is_none() {
                    self.dxgi_retry_after = Instant::now() + Duration::from_secs(3);
                }
            }

            let allow_cached =
                cursor_moved || self.last_sent_at.elapsed() >= Duration::from_millis(800);
            let mut used_dxgi = false;

            if let Some(ref mut dupl) = self.dxgi {
                match dupl.capture_crop_bgra(
                    src_left,
                    src_top,
                    src_w,
                    src_h,
                    allow_cached,
                    &mut self.dxgi_bgra_buf,
                ) {
                    DxgiCaptureResult::NewFrame | DxgiCaptureResult::CachedFrame => {
                        used_dxgi = true;
                        self.last_sent_at = Instant::now();
                    }
                    DxgiCaptureResult::Unchanged => {
                        if self.last_sent_at.elapsed() < Duration::from_millis(800) {
                            return None;
                        }
                    }
                    DxgiCaptureResult::Unavailable => {
                        if let Some(mut old) = self.dxgi.take() {
                            old.release_all();
                        }
                        self.dxgi_retry_after = Instant::now() + Duration::from_secs(2);
                    }
                }
            }

            let bgra_slice: &[u8] = if used_dxgi {
                if draw_cursor {
                    self.cursor_overlay.stamp_onto_bgra(
                        &mut self.dxgi_bgra_buf,
                        src_left,
                        src_top,
                        src_w,
                        src_h,
                    );
                }
                &self.dxgi_bgra_buf
            } else {
                let slice =
                    self.gdi
                        .capture_screen_bgra(src_left, src_top, src_w, src_h, draw_cursor)?;
                self.last_sent_at = Instant::now();
                slice
            };

            convert_bgra_to_frame(
                bgra_slice,
                (src_w, src_h),
                (out_w, out_h),
                true,
                need_rgba_preview,
                &mut self.spare_rgb,
            )
        }
    }
}

impl Drop for FastCapturer {
    fn drop(&mut self) {
        unsafe {
            if let Some(mut d) = self.dxgi.take() {
                d.release_all();
            }
            self.gdi.release();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::enumerate::{is_screenz_frontend_title, is_screenz_window_match};

    #[test]
    fn reconhece_janela_screenz_web_e_ignora_ide_ou_explorer() {
        assert!(is_screenz_window_match("ScreenZ", "msedgewebview2.exe"));
        assert!(is_screenz_window_match(
            "ScreenZ — Estúdio de transmissão - Google Chrome",
            "chrome.exe"
        ));
        assert!(is_screenz_window_match(
            "ScreenZ Pop-out — Tela",
            "screenz.exe"
        ));
        assert!(!is_screenz_frontend_title("ScreenZ - Visual Studio Code"));
        assert!(!is_screenz_window_match("ScreenZ", "explorer.exe"));
        assert!(is_screenz_window_match(
            "ScreenZ — Frontend Vite + shadcn/ui",
            "msedge.exe"
        ));
        assert!(is_screenz_window_match(
            "ScreenZ — Frontend Vite + shadcn/ui - Google Chrome",
            "chrome.exe"
        ));
        assert!(is_screenz_window_match(
            "ScreenZ — Frontend Vite + shadcn/ui",
            "custom_webview.exe"
        ));
        assert!(!is_screenz_frontend_title("ScreenZ - Visual Studio Code"));
        assert!(!is_screenz_window_match(
            "ScreenZ - Visual Studio Code",
            "Code.exe"
        ));
        assert!(!is_screenz_window_match(
            "C:\\Users\\Venti\\ScreenZ",
            "explorer.exe"
        ));
    }
}
