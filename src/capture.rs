use std::ffi::OsString;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStringExt;
use std::ptr::null_mut;

use winapi::shared::minwindef::{BOOL, DWORD, FALSE, LPARAM, TRUE, UINT};
use winapi::shared::windef::{HDC, HMONITOR, HWND, RECT};
use winapi::um::processthreadsapi::GetCurrentProcess;
use winapi::um::psapi::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use winapi::um::wingdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
    SelectObject, SetStretchBltMode, StretchBlt, BI_RGB, BITMAPINFO, BITMAPINFOHEADER,
    COLORONCOLOR, DIB_RGB_COLORS, SRCCOPY,
};
use winapi::um::winuser::{
    DrawIconEx, EnumDisplayMonitors, EnumWindows, GetCursorInfo, GetDesktopWindow, GetMonitorInfoW,
    GetWindowDC, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IsIconic, IsWindowVisible,
    PrintWindow, ReleaseDC, CURSORINFO, CURSOR_SHOWING, MONITORINFOEXW,
};

const DI_NORMAL: UINT = 0x0003;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceCategory {
    Application,
    Browser,
    Screen,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CaptureSourceKind {
    Monitor {
        index: usize,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
    },
    Window {
        hwnd: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaptureSource {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub category: SourceCategory,
    pub kind: CaptureSourceKind,
}

pub struct RawFrame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    pub rgba: Vec<u8>,
}

unsafe extern "system" fn monitor_enum_proc(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _lprect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let list = &mut *(lparam as *mut Vec<CaptureSource>);
    let mut info: MONITORINFOEXW = zeroed();
    info.cbSize = size_of::<MONITORINFOEXW>() as DWORD;

    if GetMonitorInfoW(hmonitor, &mut info as *mut _ as *mut _) != 0 {
        let rc = info.rcMonitor;
        let w = rc.right - rc.left;
        let h = rc.bottom - rc.top;
        let idx = list.iter().filter(|s| s.category == SourceCategory::Screen).count() + 1;
        let is_primary = (info.dwFlags & 1) != 0;
        let label = if is_primary {
            format!("Tela {} (Principal)", idx)
        } else {
            format!("Tela {}", idx)
        };

        list.push(CaptureSource {
            id: format!("screen:{}", idx),
            name: label,
            subtitle: format!("Monitor Completo • {}×{}", w, h),
            category: SourceCategory::Screen,
            kind: CaptureSourceKind::Monitor {
                index: idx,
                left: rc.left,
                top: rc.top,
                width: w,
                height: h,
            },
        });
    }
    TRUE
}

fn classify_window_title(raw_title: &str, w: i32, h: i32) -> (String, String, SourceCategory) {
    let browser_suffixes = [
        (" - Google Chrome", "Google Chrome"),
        (" — Mozilla Firefox", "Mozilla Firefox"),
        (" - Microsoft​ Edge", "Microsoft Edge"),
        (" - Microsoft Edge", "Microsoft Edge"),
        (" - Brave", "Brave Browser"),
        (" - Opera", "Opera"),
        (" - Vivaldi", "Vivaldi"),
    ];

    for (suffix, browser_name) in browser_suffixes {
        if let Some(stripped) = raw_title.strip_suffix(suffix) {
            let clean_tab = if stripped.trim().is_empty() {
                raw_title.to_string()
            } else {
                stripped.trim().to_string()
            };
            return (
                clean_tab,
                format!("🌐 {} • {}×{}", browser_name, w, h),
                SourceCategory::Browser,
            );
        }
    }

    let lower = raw_title.to_lowercase();
    if lower.contains("google chrome")
        || lower.contains("microsoft edge")
        || lower.contains("mozilla firefox")
        || lower.contains("brave")
        || lower.contains("opera")
    {
        return (
            raw_title.to_string(),
            format!("🌐 Navegador Web • {}×{}", w, h),
            SourceCategory::Browser,
        );
    }

    // Separa nome do documento e nome do aplicativo se houver " - "
    if let Some((left, right)) = raw_title.rsplit_once(" - ") {
        return (
            left.trim().to_string(),
            format!("🎮 {} • {}×{}", right.trim(), w, h),
            SourceCategory::Application,
        );
    }

    (
        raw_title.to_string(),
        format!("🪟 Aplicativo • {}×{}", w, h),
        SourceCategory::Application,
    )
}

unsafe extern "system" fn window_enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
        return TRUE;
    }

    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return TRUE;
    }

    let mut buf = vec![0u16; (len + 1) as usize];
    let copied = GetWindowTextW(hwnd, buf.as_mut_ptr(), len + 1);
    if copied <= 0 {
        return TRUE;
    }

    let title = OsString::from_wide(&buf[..copied as usize])
        .to_string_lossy()
        .trim()
        .to_string();

    if title.is_empty()
        || title == "Program Manager"
        || title == "Settings"
        || title == "Configurações"
        || title == "Windows Input Experience"
        || title.starts_with("DISC2")
        || title.starts_with("SZdisc")
    {
        return TRUE;
    }

    let mut rect: RECT = zeroed();
    if GetWindowRect(hwnd, &mut rect) == 0 {
        return TRUE;
    }
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;
    if w < 140 || h < 90 {
        return TRUE;
    }

    let (clean_name, subtitle, category) = classify_window_title(&title, w, h);

    let list = &mut *(lparam as *mut Vec<CaptureSource>);
    list.push(CaptureSource {
        id: format!("window:{}", hwnd as usize),
        name: clean_name,
        subtitle,
        category,
        kind: CaptureSourceKind::Window {
            hwnd: hwnd as usize,
        },
    });

    TRUE
}

pub fn list_capture_sources() -> Vec<CaptureSource> {
    let mut sources = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            null_mut(),
            null_mut(),
            Some(monitor_enum_proc),
            &mut sources as *mut _ as LPARAM,
        );
        EnumWindows(Some(window_enum_proc), &mut sources as *mut _ as LPARAM);
    }
    sources
}

/// Gera uma miniatura RGBA rápida para o seletor visual de telas/aplicativos/guias estilo Discord
pub fn capture_thumbnail_rgba(source: &CaptureSource, thumb_w: u32, thumb_h: u32) -> Option<(u32, u32, Vec<u8>)> {
    let frame = capture_frame(source, thumb_w, thumb_h, true, false)?;
    Some((frame.width, frame.height, frame.rgba))
}

/// Captura um quadro da tela ou janela selecionada já escalado para `(target_w, target_h)`
pub fn capture_frame(
    source: &CaptureSource,
    target_w: u32,
    target_h: u32,
    need_rgba_preview: bool,
    draw_cursor: bool,
) -> Option<RawFrame> {
    unsafe {
        match source.kind {
            CaptureSourceKind::Monitor {
                left,
                top,
                width,
                height,
                ..
            } => capture_region(
                null_mut(),
                left,
                top,
                width,
                height,
                target_w,
                target_h,
                false,
                need_rgba_preview,
                draw_cursor,
            ),
            CaptureSourceKind::Window { hwnd } => {
                let hwnd_ptr = hwnd as HWND;
                let mut rect: RECT = zeroed();
                if GetWindowRect(hwnd_ptr, &mut rect) == 0 {
                    return None;
                }
                let w = (rect.right - rect.left).max(16);
                let h = (rect.bottom - rect.top).max(16);
                capture_region(
                    hwnd_ptr,
                    rect.left,
                    rect.top,
                    w,
                    h,
                    target_w,
                    target_h,
                    true,
                    need_rgba_preview,
                    draw_cursor,
                )
            }
        }
    }
}

unsafe fn capture_region(
    hwnd: HWND,
    src_left: i32,
    src_top: i32,
    src_w: i32,
    src_h: i32,
    target_w: u32,
    target_h: u32,
    is_window: bool,
    need_rgba_preview: bool,
    draw_cursor: bool,
) -> Option<RawFrame> {
    if src_w <= 0 || src_h <= 0 {
        return None;
    }

    let scale_x = target_w as f32 / src_w as f32;
    let scale_y = target_h as f32 / src_h as f32;
    let scale = scale_x.min(scale_y).min(1.0);

    let out_w = (((src_w as f32 * scale) as u32) & !1).max(64);
    let out_h = (((src_h as f32 * scale) as u32) & !1).max(64);

    let desktop_hwnd = GetDesktopWindow();
    let src_dc = if is_window && !hwnd.is_null() {
        GetWindowDC(hwnd)
    } else {
        GetWindowDC(desktop_hwnd)
    };
    if src_dc.is_null() {
        return None;
    }

    let mem_dc = CreateCompatibleDC(src_dc);
    let bmp = CreateCompatibleBitmap(src_dc, out_w as i32, out_h as i32);
    if mem_dc.is_null() || bmp.is_null() {
        if !mem_dc.is_null() {
            DeleteDC(mem_dc);
        }
        if !bmp.is_null() {
            DeleteObject(bmp as *mut _);
        }
        ReleaseDC(if is_window { hwnd } else { desktop_hwnd }, src_dc);
        return None;
    }

    let old_obj = SelectObject(mem_dc, bmp as *mut _);

    if is_window && !hwnd.is_null() {
        let full_dc = CreateCompatibleDC(src_dc);
        let full_bmp = CreateCompatibleBitmap(src_dc, src_w, src_h);
        let old_full = SelectObject(full_dc, full_bmp as *mut _);

        let ok = PrintWindow(hwnd, full_dc, 2);
        if ok == FALSE {
            BitBlt(full_dc, 0, 0, src_w, src_h, src_dc, 0, 0, SRCCOPY);
        }

        if out_w as i32 == src_w && out_h as i32 == src_h {
            BitBlt(mem_dc, 0, 0, out_w as i32, out_h as i32, full_dc, 0, 0, SRCCOPY);
        } else {
            SetStretchBltMode(mem_dc, COLORONCOLOR);
            StretchBlt(
                mem_dc,
                0,
                0,
                out_w as i32,
                out_h as i32,
                full_dc,
                0,
                0,
                src_w,
                src_h,
                SRCCOPY,
            );
        }

        SelectObject(full_dc, old_full);
        DeleteObject(full_bmp as *mut _);
        DeleteDC(full_dc);
    } else {
        if out_w as i32 == src_w && out_h as i32 == src_h {
            BitBlt(
                mem_dc,
                0,
                0,
                out_w as i32,
                out_h as i32,
                src_dc,
                src_left,
                src_top,
                SRCCOPY,
            );
        } else {
            SetStretchBltMode(mem_dc, COLORONCOLOR);
            StretchBlt(
                mem_dc,
                0,
                0,
                out_w as i32,
                out_h as i32,
                src_dc,
                src_left,
                src_top,
                src_w,
                src_h,
                SRCCOPY,
            );
        }

        if draw_cursor {
            let mut ci: CURSORINFO = zeroed();
            ci.cbSize = size_of::<CURSORINFO>() as DWORD;
            if GetCursorInfo(&mut ci) != 0 && (ci.flags & CURSOR_SHOWING) != 0 {
                let cx = ((ci.ptScreenPos.x - src_left) as f32 * scale) as i32;
                let cy = ((ci.ptScreenPos.y - src_top) as f32 * scale) as i32;
                if cx >= 0 && cy >= 0 && cx < out_w as i32 && cy < out_h as i32 {
                    DrawIconEx(mem_dc, cx, cy, ci.hCursor, 0, 0, 0, null_mut(), DI_NORMAL);
                }
            }
        }
    }

    let mut bmi: BITMAPINFO = zeroed();
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as DWORD;
    bmi.bmiHeader.biWidth = out_w as i32;
    bmi.bmiHeader.biHeight = -(out_h as i32);
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let pixel_count = (out_w * out_h) as usize;
    let mut bgra = vec![0u8; pixel_count * 4];

    let lines = GetDIBits(
        mem_dc,
        bmp,
        0,
        out_h,
        bgra.as_mut_ptr() as *mut _,
        &mut bmi,
        DIB_RGB_COLORS,
    );

    SelectObject(mem_dc, old_obj);
    DeleteObject(bmp as *mut _);
    DeleteDC(mem_dc);
    ReleaseDC(if is_window { hwnd } else { desktop_hwnd }, src_dc);

    if lines == 0 {
        return None;
    }

    let mut rgb = vec![0u8; pixel_count * 3];
    let mut rgba = if need_rgba_preview {
        vec![0u8; pixel_count * 4]
    } else {
        Vec::new()
    };

    if need_rgba_preview {
        for i in 0..pixel_count {
            let b = bgra[i * 4];
            let g = bgra[i * 4 + 1];
            let r = bgra[i * 4 + 2];

            rgb[i * 3] = r;
            rgb[i * 3 + 1] = g;
            rgb[i * 3 + 2] = b;

            rgba[i * 4] = r;
            rgba[i * 4 + 1] = g;
            rgba[i * 4 + 2] = b;
            rgba[i * 4 + 3] = 255;
        }
    } else {
        for i in 0..pixel_count {
            rgb[i * 3] = bgra[i * 4 + 2];
            rgb[i * 3 + 1] = bgra[i * 4 + 1];
            rgb[i * 3 + 2] = bgra[i * 4];
        }
    }

    Some(RawFrame {
        width: out_w,
        height: out_h,
        rgb,
        rgba,
    })
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
