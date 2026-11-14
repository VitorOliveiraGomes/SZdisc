use std::ffi::OsString;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::ptr::null_mut;

use winapi::shared::minwindef::{BOOL, DWORD, FALSE, LPARAM, TRUE, UINT};
use winapi::shared::windef::{HDC, HMONITOR, HWND, RECT};
use winapi::um::dwmapi::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::{GetCurrentProcess, OpenProcess};
use winapi::um::psapi::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use winapi::um::winbase::QueryFullProcessImageNameW;
use winapi::um::wingdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
    SelectObject, SetStretchBltMode, StretchBlt, BI_RGB, BITMAPINFO, BITMAPINFOHEADER,
    COLORONCOLOR, DIB_RGB_COLORS, SRCCOPY,
};
use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;
use winapi::um::winuser::{
    DrawIconEx, EnumDisplayMonitors, EnumWindows, GetCursorInfo, GetDesktopWindow, GetMonitorInfoW,
    GetWindowDC, GetWindowLongW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, PrintWindow, ReleaseDC, CURSORINFO,
    CURSOR_SHOWING, GWL_EXSTYLE, MONITORINFOEXW, WS_EX_TOOLWINDOW,
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
    pub exe_name: String,
    pub pid: u32,
    pub category: SourceCategory,
    pub kind: CaptureSourceKind,
}

pub struct RawFrame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    pub rgba: Vec<u8>,
}

unsafe fn get_window_pid_and_exe(hwnd: HWND) -> (u32, String) {
    let mut pid: DWORD = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == 0 {
        return (0, String::new());
    }

    let h_proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if h_proc.is_null() {
        return (pid, String::new());
    }

    let mut buf = [0u16; 512];
    let mut size: DWORD = buf.len() as DWORD;
    let ok = QueryFullProcessImageNameW(h_proc, 0, buf.as_mut_ptr(), &mut size);
    CloseHandle(h_proc);

    if ok == 0 || size == 0 {
        return (pid, String::new());
    }

    let full_path = OsString::from_wide(&buf[..size as usize])
        .to_string_lossy()
        .to_string();

    let exe = Path::new(&full_path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    (pid, exe)
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
        let idx = list
            .iter()
            .filter(|s| s.category == SourceCategory::Screen)
            .count()
            + 1;
        let is_primary = (info.dwFlags & 1) != 0;
        let label = if is_primary {
            format!("Tela {} (Principal)", idx)
        } else {
            format!("Tela {}", idx)
        };

        list.push(CaptureSource {
            id: format!("screen:{}", idx),
            name: label,
            subtitle: format!("🖥️ Monitor Completo • {}×{}", w, h),
            exe_name: "Monitor".to_string(),
            pid: 0,
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

fn classify_window(
    raw_title: &str,
    exe_name: &str,
    w: i32,
    h: i32,
    minimized: bool,
) -> (String, String, SourceCategory) {
    let exe_lower = exe_name.to_lowercase();
    let is_browser_exe = matches!(
        exe_lower.as_str(),
        "chrome.exe"
            | "msedge.exe"
            | "firefox.exe"
            | "brave.exe"
            | "opera.exe"
            | "vivaldi.exe"
            | "waterfox.exe"
            | "librewolf.exe"
            | "thorium.exe"
            | "arc.exe"
            | "zen.exe"
    );

    let browser_suffixes = [
        (" - Google Chrome", "Google Chrome"),
        (" — Mozilla Firefox", "Mozilla Firefox"),
        (" - Microsoft​ Edge", "Microsoft Edge"),
        (" - Microsoft Edge", "Microsoft Edge"),
        (" - Pessoal — Microsoft​ Edge", "Microsoft Edge"),
        (" - Brave", "Brave"),
        (" - Opera", "Opera"),
        (" - Opera GX", "Opera GX"),
        (" - Vivaldi", "Vivaldi"),
    ];

    for (suffix, b_label) in browser_suffixes {
        if let Some(stripped) = raw_title.strip_suffix(suffix) {
            let clean_tab = if stripped.trim().is_empty() {
                raw_title.to_string()
            } else {
                stripped.trim().to_string()
            };
            let state_str = if minimized {
                "Minimizado".to_string()
            } else {
                format!("{}×{}", w, h)
            };
            return (
                clean_tab,
                format!("🌐 {} • {}", b_label, state_str),
                SourceCategory::Browser,
            );
        }
    }

    if is_browser_exe {
        let friendly_browser = match exe_lower.as_str() {
            "chrome.exe" => "Google Chrome",
            "msedge.exe" => "Microsoft Edge",
            "firefox.exe" => "Mozilla Firefox",
            "brave.exe" => "Brave",
            "opera.exe" => "Opera",
            "vivaldi.exe" => "Vivaldi",
            _ => exe_name,
        };
        let state_str = if minimized {
            "Minimizado".to_string()
        } else {
            format!("{}×{}", w, h)
        };
        return (
            raw_title.to_string(),
            format!("🌐 {} • {}", friendly_browser, state_str),
            SourceCategory::Browser,
        );
    }

    let friendly_app = if !exe_name.is_empty() {
        exe_name.trim_end_matches(".exe").trim_end_matches(".EXE")
    } else {
        "Aplicativo"
    };

    let state_str = if minimized {
        "Minimizado".to_string()
    } else {
        format!("{}×{}", w, h)
    };

    (
        raw_title.to_string(),
        format!("🪟 {} • {}", friendly_app, state_str),
        SourceCategory::Application,
    )
}

unsafe extern "system" fn window_enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if IsWindowVisible(hwnd) == 0 {
        return TRUE;
    }

    // Filtra apenas janelas ocultas do sistema operacional (DWM Cloaked), mas mantém janelas reais e minimizadas
    let mut cloaked: DWORD = 0;
    let _ = DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut _ as *mut _,
        size_of::<DWORD>() as DWORD,
    );
    if cloaked != 0 {
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
        || title == "Windows Input Experience"
        || title == "MSCTFIME UI"
        || title == "Default IME"
        || title.starts_with("SZdisc —")
        || title.starts_with("DISC2 —")
    {
        return TRUE;
    }

    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    let is_minimized = IsIconic(hwnd) != 0;

    let mut rect: RECT = zeroed();
    if GetWindowRect(hwnd, &mut rect) == 0 {
        return TRUE;
    }
    let w = (rect.right - rect.left).max(0);
    let h = (rect.bottom - rect.top).max(0);

    // Ignora apenas pequenos tooltips flutuantes invisíveis quando não estão minimizados
    if !is_minimized && (w < 60 || h < 40) && (ex_style & WS_EX_TOOLWINDOW) != 0 {
        return TRUE;
    }

    let (pid, exe_name) = get_window_pid_and_exe(hwnd);
    let (clean_name, subtitle, category) =
        classify_window(&title, &exe_name, w, h, is_minimized);

    let list = &mut *(lparam as *mut Vec<CaptureSource>);
    list.push(CaptureSource {
        id: format!("window:{}", hwnd as usize),
        name: clean_name,
        subtitle,
        exe_name,
        pid,
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

pub fn capture_thumbnail_rgba(
    source: &CaptureSource,
    thumb_w: u32,
    thumb_h: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    let frame = capture_frame(source, thumb_w, thumb_h, true, false)?;
    Some((frame.width, frame.height, frame.rgba))
}

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
                if IsIconic(hwnd_ptr) != 0 {
                    return None;
                }
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
    let desktop_dc = GetWindowDC(desktop_hwnd);
    if desktop_dc.is_null() {
        return None;
    }

    let mem_dc = CreateCompatibleDC(desktop_dc);
    let bmp = CreateCompatibleBitmap(desktop_dc, out_w as i32, out_h as i32);
    if mem_dc.is_null() || bmp.is_null() {
        if !mem_dc.is_null() {
            DeleteDC(mem_dc);
        }
        if !bmp.is_null() {
            DeleteObject(bmp as *mut _);
        }
        ReleaseDC(desktop_hwnd, desktop_dc);
        return None;
    }

    let old_obj = SelectObject(mem_dc, bmp as *mut _);

    if is_window && !hwnd.is_null() {
        let win_dc = GetWindowDC(hwnd);
        let full_dc = CreateCompatibleDC(desktop_dc);
        let full_bmp = CreateCompatibleBitmap(desktop_dc, src_w, src_h);
        let old_full = SelectObject(full_dc, full_bmp as *mut _);

        let ok = PrintWindow(hwnd, full_dc, 2);
        if ok == FALSE && !win_dc.is_null() {
            BitBlt(full_dc, 0, 0, src_w, src_h, win_dc, 0, 0, SRCCOPY);
        }

        if out_w as i32 == src_w && out_h as i32 == src_h {
            BitBlt(
                mem_dc,
                0,
                0,
                out_w as i32,
                out_h as i32,
                full_dc,
                0,
                0,
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
        if !win_dc.is_null() {
            ReleaseDC(hwnd, win_dc);
        }
    } else {
        if out_w as i32 == src_w && out_h as i32 == src_h {
            BitBlt(
                mem_dc,
                0,
                0,
                out_w as i32,
                out_h as i32,
                desktop_dc,
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
                desktop_dc,
                src_left,
                src_top,
                src_w,
                src_h,
                SRCCOPY,
            );
        }
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

    let mut bmi: BITMAPINFO = zeroed();
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as DWORD;
    bmi.bmiHeader.biWidth = out_w as i32;
    bmi.bmiHeader.biHeight = -(out_h as i32);
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let pixel_count = (out_w * out_h) as usize;
    let mut bgra = vec![0u8; pixel_count * 4];

    let mut lines = GetDIBits(
        mem_dc,
        bmp,
        0,
        out_h,
        bgra.as_mut_ptr() as *mut _,
        &mut bmi,
        DIB_RGB_COLORS,
    );

    // Se a janela acelerada por GPU retornou imagem totalmente preta no PrintWindow, faz fallback automático pelo Desktop DC
    if is_window && lines > 0 {
        let is_all_black = bgra
            .iter()
            .step_by(64)
            .all(|&b| b == 0);
        if is_all_black {
            SetStretchBltMode(mem_dc, COLORONCOLOR);
            StretchBlt(
                mem_dc,
                0,
                0,
                out_w as i32,
                out_h as i32,
                desktop_dc,
                src_left,
                src_top,
                src_w,
                src_h,
                SRCCOPY,
            );
            lines = GetDIBits(
                mem_dc,
                bmp,
                0,
                out_h,
                bgra.as_mut_ptr() as *mut _,
                &mut bmi,
                DIB_RGB_COLORS,
            );
        }
    }

    SelectObject(mem_dc, old_obj);
    DeleteObject(bmp as *mut _);
    DeleteDC(mem_dc);
    ReleaseDC(desktop_hwnd, desktop_dc);

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
