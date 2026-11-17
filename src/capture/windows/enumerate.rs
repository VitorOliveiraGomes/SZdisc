use std::ffi::OsString;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::ptr::null_mut;

use winapi::shared::minwindef::{BOOL, DWORD, FALSE, LPARAM, TRUE};
use winapi::shared::windef::{HDC, HMONITOR, HWND, RECT};
use winapi::um::dwmapi::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::{GetCurrentProcessId, OpenProcess};
use winapi::um::winbase::QueryFullProcessImageNameW;
use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;
use winapi::um::winuser::{
    EnumDisplayMonitors, EnumWindows, GetForegroundWindow, GetMonitorInfoW, GetWindowLongW,
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, GWL_EXSTYLE, MONITORINFOEXW, WS_EX_TOOLWINDOW,
};

use super::super::{CaptureSource, CaptureSourceKind, SourceCategory};

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

unsafe fn get_window_title(hwnd: HWND) -> Option<String> {
    let mut buf = [0u16; 512];
    let copied = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if copied <= 0 {
        return None;
    }
    let title = OsString::from_wide(&buf[..copied as usize])
        .to_string_lossy()
        .trim()
        .to_string();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

fn is_browser_exe(exe_name: &str) -> bool {
    matches!(
        exe_name.to_lowercase().as_str(),
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
            | "msedgewebview2.exe"
            | "chromium.exe"
    )
}

pub(crate) fn is_screenz_frontend_title(title: &str) -> bool {
    let t = title.trim();
    t == "ScreenZ" || t.starts_with("ScreenZ —") || t.starts_with("ScreenZ Pop-out")
}

pub(crate) fn is_screenz_window_match(title: &str, exe_name: &str) -> bool {
    let t = title.trim();
    if t.starts_with("ScreenZ Pop-out") || t.starts_with("ScreenZ — Frontend") {
        return true;
    }
    is_screenz_frontend_title(t) && is_browser_exe(exe_name)
}

fn classify_window(
    raw_title: &str,
    exe_name: &str,
    w: i32,
    h: i32,
    minimized: bool,
) -> (String, String, SourceCategory) {
    let exe_lower = exe_name.to_lowercase();
    let is_browser = is_browser_exe(&exe_lower);

    let browser_suffixes = [
        (" - Google Chrome", "Google Chrome"),
        (" — Mozilla Firefox", "Mozilla Firefox"),
        (" - Microsoft\u{200B} Edge", "Microsoft Edge"),
        (" - Microsoft Edge", "Microsoft Edge"),
        (" - Pessoal — Microsoft\u{200B} Edge", "Microsoft Edge"),
        (" - Brave", "Brave"),
        (" - Opera", "Opera"),
        (" - Opera GX", "Opera GX"),
        (" - Vivaldi", "Vivaldi"),
    ];

    let state_str = if minimized {
        "Minimizado".to_string()
    } else {
        format!("{}×{}", w, h)
    };

    for (suffix, b_label) in browser_suffixes {
        if let Some(stripped) = raw_title.strip_suffix(suffix) {
            let clean_tab = if stripped.trim().is_empty() {
                raw_title.to_string()
            } else {
                stripped.trim().to_string()
            };
            return (
                clean_tab,
                format!("🌐 {} • {}", b_label, state_str),
                SourceCategory::Browser,
            );
        }
    }

    if is_browser {
        let friendly_browser = match exe_lower.as_str() {
            "chrome.exe" => "Google Chrome",
            "msedge.exe" => "Microsoft Edge",
            "firefox.exe" => "Mozilla Firefox",
            "brave.exe" => "Brave",
            "opera.exe" => "Opera",
            "vivaldi.exe" => "Vivaldi",
            _ => exe_name,
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

    (
        raw_title.to_string(),
        format!("🪟 {} • {}", friendly_app, state_str),
        SourceCategory::Application,
    )
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

unsafe extern "system" fn window_enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if IsWindowVisible(hwnd) == 0 {
        return TRUE;
    }

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
        || title == "ScreenZ"
        || title.starts_with("ScreenZ —")
        || title.starts_with("ScreenZ Pop-out")
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

    if !is_minimized && (w < 60 || h < 40) && (ex_style & WS_EX_TOOLWINDOW) != 0 {
        return TRUE;
    }

    let (pid, exe_name) = get_window_pid_and_exe(hwnd);
    if pid != 0 && pid == GetCurrentProcessId() {
        return TRUE;
    }
    if is_screenz_window_match(&title, &exe_name) {
        return TRUE;
    }

    let (clean_name, subtitle, category) = classify_window(&title, &exe_name, w, h, is_minimized);

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

pub fn is_screenz_in_foreground() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_null() {
            return false;
        }
        let mut fg_pid: DWORD = 0;
        GetWindowThreadProcessId(fg, &mut fg_pid);
        if fg_pid != 0 && fg_pid == GetCurrentProcessId() {
            return true;
        }
        let Some(title) = get_window_title(fg) else {
            return false;
        };
        if !is_screenz_frontend_title(&title) {
            return false;
        }
        let (_, exe_name) = get_window_pid_and_exe(fg);
        is_screenz_window_match(&title, &exe_name)
    }
}
