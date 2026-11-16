#[cfg(windows)]
mod platform {
use std::ffi::OsString;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::ptr::null_mut;

use std::time::Instant;

use winapi::shared::dxgi::{IDXGIDevice, IDXGIOutput, IDXGIResource};
use winapi::shared::dxgi1_2::{IDXGIOutput1, IDXGIOutputDuplication, DXGI_OUTDUPL_FRAME_INFO};
use winapi::shared::dxgiformat::DXGI_FORMAT_B8G8R8A8_UNORM;
use winapi::shared::dxgitype::DXGI_SAMPLE_DESC;
use winapi::shared::minwindef::{BOOL, DWORD, FALSE, LPARAM, TRUE, UINT};
use winapi::shared::windef::{HBITMAP, HDC, HGDIOBJ, HMONITOR, HWND, RECT};
use winapi::shared::winerror::{DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, SUCCEEDED};
use winapi::um::d3d11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use winapi::um::d3dcommon::D3D_DRIVER_TYPE_HARDWARE;
use winapi::um::dwmapi::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use winapi::um::unknwnbase::IUnknown;
use winapi::Interface;
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::{GetCurrentProcess, GetCurrentProcessId, OpenProcess};
use winapi::um::psapi::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use winapi::um::winbase::QueryFullProcessImageNameW;
use winapi::um::wingdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject,
    GdiFlush, GetDIBits, SelectObject, SetStretchBltMode, StretchBlt, BI_RGB, BITMAPINFO,
    BITMAPINFOHEADER, COLORONCOLOR, DIB_RGB_COLORS, SRCCOPY,
};
use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;
use winapi::um::winuser::{
    DrawIconEx, EnumDisplayMonitors, EnumWindows, GetCursorInfo, GetDC, GetDesktopWindow,
    GetForegroundWindow, GetMonitorInfoW, GetSystemMetrics, GetWindowDC, GetWindowLongW,
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, PrintWindow, ReleaseDC, CURSORINFO, CURSOR_SHOWING, GWL_EXSTYLE,
    MONITORINFOEXW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    WS_EX_TOOLWINDOW,
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

fn is_browser_exe(exe_name: &str) -> bool {
    let exe_lower = exe_name.to_lowercase();
    matches!(
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
    )
}

pub(super) fn is_szdisc_frontend_title(title: &str) -> bool {
    let t = title.trim();
    t == "SZdisc" || t.starts_with("SZdisc —")
}

pub(super) fn is_szdisc_window_match(title: &str, exe_name: &str) -> bool {
    let t = title.trim();
    if t.starts_with("SZdisc — Frontend") {
        return true;
    }
    is_szdisc_frontend_title(t) && is_browser_exe(exe_name)
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

fn classify_window(
    raw_title: &str,
    exe_name: &str,
    w: i32,
    h: i32,
    minimized: bool,
) -> (String, String, SourceCategory) {
    let exe_lower = exe_name.to_lowercase();
    let is_browser_exe = is_browser_exe(&exe_lower);

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

/// Retorna `true` apenas quando a janela do SZdisc (processo nativo ou janela do SZweb no navegador)
/// está em primeiro plano (focada pelo usuário).
/// Quando o jogador está dentro do jogo em tela cheia ou janela focada (`false`), o SZdisc
/// pausa automaticamente a renderização da prévia local para usar 0% de GPU extra!
pub fn is_szdisc_in_foreground() -> bool {
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
        // Como o processo Rust é headless, a UI ativa roda no navegador (`SZweb`).
        // Checa o título primeiro para evitar `OpenProcess` quando outra janela (ex: jogo) está em foco.
        let Some(title) = get_window_title(fg) else {
            return false;
        };
        if !is_szdisc_frontend_title(&title) {
            return false;
        }
        let (_, exe_name) = get_window_pid_and_exe(fg);
        is_szdisc_window_match(&title, &exe_name)
    }
}

enum DxgiCaptureResult {
    NewFrame,
    Unchanged,
    Unavailable,
}

struct DxgiDuplicator {
    device: *mut ID3D11Device,
    context: *mut ID3D11DeviceContext,
    dupl: *mut IDXGIOutputDuplication,
    staging: *mut ID3D11Texture2D,
    staging_w: u32,
    staging_h: u32,
    out_left: i32,
    out_top: i32,
    out_w: i32,
    out_h: i32,
}

impl DxgiDuplicator {
    unsafe fn release_all(&mut self) {
        if !self.staging.is_null() {
            (*self.staging).Release();
            self.staging = null_mut();
        }
        if !self.dupl.is_null() {
            (*self.dupl).Release();
            self.dupl = null_mut();
        }
        if !self.context.is_null() {
            (*self.context).Release();
            self.context = null_mut();
        }
        if !self.device.is_null() {
            (*self.device).Release();
            self.device = null_mut();
        }
        self.staging_w = 0;
        self.staging_h = 0;
    }

    fn contains_rect(&self, src_left: i32, src_top: i32, src_w: i32, src_h: i32) -> bool {
        src_left >= self.out_left
            && src_top >= self.out_top
            && src_left + src_w <= self.out_left + self.out_w
            && src_top + src_h <= self.out_top + self.out_h
    }

    unsafe fn init_for_region(
        src_left: i32,
        src_top: i32,
        src_w: i32,
        src_h: i32,
        fallback_idx: usize,
    ) -> Option<Self> {
        let mut device: *mut ID3D11Device = null_mut();
        let mut context: *mut ID3D11DeviceContext = null_mut();
        let hr = D3D11CreateDevice(
            null_mut(),
            D3D_DRIVER_TYPE_HARDWARE,
            null_mut(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            null_mut(),
            0,
            D3D11_SDK_VERSION,
            &mut device,
            null_mut(),
            &mut context,
        );
        if !SUCCEEDED(hr) || device.is_null() || context.is_null() {
            return None;
        }

        let mut dxgi_dev: *mut IDXGIDevice = null_mut();
        if !SUCCEEDED((*device).QueryInterface(
            &IDXGIDevice::uuidof(),
            &mut dxgi_dev as *mut _ as *mut _,
        )) || dxgi_dev.is_null()
        {
            (*context).Release();
            (*device).Release();
            return None;
        }

        let mut adapter = null_mut();
        let hr_ad = (*dxgi_dev).GetAdapter(&mut adapter);
        (*dxgi_dev).Release();
        if !SUCCEEDED(hr_ad) || adapter.is_null() {
            (*context).Release();
            (*device).Release();
            return None;
        }

        // Procura o output DXGI (0-based) que contém a região alvo; senão tenta o índice fallback.
        let mut chosen_output: *mut IDXGIOutput = null_mut();
        let mut fallback_output: *mut IDXGIOutput = null_mut();
        let mut i: UINT = 0;
        loop {
            let mut cand: *mut IDXGIOutput = null_mut();
            let hr_out = (*adapter).EnumOutputs(i, &mut cand);
            if !SUCCEEDED(hr_out) || cand.is_null() {
                break;
            }
            let mut desc = zeroed();
            let _ = (*cand).GetDesc(&mut desc);
            let r = desc.DesktopCoordinates;
            let matches = src_left >= r.left
                && src_top >= r.top
                && src_left + src_w <= r.right
                && src_top + src_h <= r.bottom;
            if matches {
                chosen_output = cand;
                break;
            } else if i as usize == fallback_idx && fallback_output.is_null() {
                fallback_output = cand;
            } else {
                (*cand).Release();
            }
            i += 1;
        }
        (*adapter).Release();

        let output = if !chosen_output.is_null() {
            if !fallback_output.is_null() {
                (*fallback_output).Release();
            }
            chosen_output
        } else {
            fallback_output
        };

        if output.is_null() {
            (*context).Release();
            (*device).Release();
            return None;
        }

        let mut desc = zeroed();
        let _ = (*output).GetDesc(&mut desc);
        let out_left = desc.DesktopCoordinates.left;
        let out_top = desc.DesktopCoordinates.top;
        let out_w = desc.DesktopCoordinates.right - desc.DesktopCoordinates.left;
        let out_h = desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top;

        let mut output1: *mut IDXGIOutput1 = null_mut();
        let hr_o1 = (*output).QueryInterface(
            &IDXGIOutput1::uuidof(),
            &mut output1 as *mut _ as *mut _,
        );
        (*output).Release();
        if !SUCCEEDED(hr_o1) || output1.is_null() {
            (*context).Release();
            (*device).Release();
            return None;
        }

        let mut dupl: *mut IDXGIOutputDuplication = null_mut();
        let hr_dup = (*output1).DuplicateOutput(device as *mut IUnknown, &mut dupl);
        (*output1).Release();
        if !SUCCEEDED(hr_dup) || dupl.is_null() {
            (*context).Release();
            (*device).Release();
            return None;
        }

        Some(Self {
            device,
            context,
            dupl,
            staging: null_mut(),
            staging_w: 0,
            staging_h: 0,
            out_left,
            out_top,
            out_w,
            out_h,
        })
    }

    unsafe fn ensure_staging(&mut self, w: u32, h: u32) -> bool {
        if !self.staging.is_null() && self.staging_w == w && self.staging_h == h {
            return true;
        }
        if !self.staging.is_null() {
            (*self.staging).Release();
            self.staging = null_mut();
        }
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ,
            MiscFlags: 0,
        };
        let hr = (*self.device).CreateTexture2D(&desc, null_mut(), &mut self.staging);
        if SUCCEEDED(hr) && !self.staging.is_null() {
            self.staging_w = w;
            self.staging_h = h;
            true
        } else {
            self.staging = null_mut();
            self.staging_w = 0;
            self.staging_h = 0;
            false
        }
    }

    unsafe fn capture_crop_bgra(
        &mut self,
        src_left: i32,
        src_top: i32,
        src_w: i32,
        src_h: i32,
        out_bgra: &mut Vec<u8>,
    ) -> DxgiCaptureResult {
        if src_left < self.out_left
            || src_top < self.out_top
            || src_left + src_w > self.out_left + self.out_w
            || src_top + src_h > self.out_top + self.out_h
        {
            return DxgiCaptureResult::Unavailable;
        }

        let mut frame_info: DXGI_OUTDUPL_FRAME_INFO = zeroed();
        let mut resource: *mut IDXGIResource = null_mut();
        let hr = (*self.dupl).AcquireNextFrame(0, &mut frame_info, &mut resource);
        if hr == DXGI_ERROR_WAIT_TIMEOUT {
            return DxgiCaptureResult::Unchanged;
        }
        if hr == DXGI_ERROR_ACCESS_LOST || !SUCCEEDED(hr) || resource.is_null() {
            if !resource.is_null() {
                (*resource).Release();
            }
            return DxgiCaptureResult::Unavailable;
        }

        if frame_info.AccumulatedFrames == 0 && frame_info.TotalMetadataBufferSize == 0 {
            (*resource).Release();
            let _ = (*self.dupl).ReleaseFrame();
            return DxgiCaptureResult::Unchanged;
        }

        let mut tex: *mut ID3D11Texture2D = null_mut();
        let hr_tex = (*resource).QueryInterface(
            &ID3D11Texture2D::uuidof(),
            &mut tex as *mut _ as *mut _,
        );
        (*resource).Release();
        if !SUCCEEDED(hr_tex) || tex.is_null() {
            let _ = (*self.dupl).ReleaseFrame();
            return DxgiCaptureResult::Unavailable;
        }

        let uw = src_w as u32;
        let uh = src_h as u32;
        if !self.ensure_staging(uw, uh) {
            (*tex).Release();
            let _ = (*self.dupl).ReleaseFrame();
            return DxgiCaptureResult::Unavailable;
        }

        let rel_left = (src_left - self.out_left) as u32;
        let rel_top = (src_top - self.out_top) as u32;
        let src_box = D3D11_BOX {
            left: rel_left,
            top: rel_top,
            front: 0,
            right: rel_left + uw,
            bottom: rel_top + uh,
            back: 1,
        };
        (*self.context).CopySubresourceRegion(
            self.staging as *mut ID3D11Resource,
            0,
            0,
            0,
            0,
            tex as *mut ID3D11Resource,
            0,
            &src_box,
        );
        (*tex).Release();
        let _ = (*self.dupl).ReleaseFrame();

        let mut mapped: D3D11_MAPPED_SUBRESOURCE = zeroed();
        let hr_map = (*self.context).Map(
            self.staging as *mut ID3D11Resource,
            0,
            D3D11_MAP_READ,
            0,
            &mut mapped,
        );
        if !SUCCEEDED(hr_map) || mapped.pData.is_null() {
            return DxgiCaptureResult::Unavailable;
        }

        let row_bytes = (uw as usize) * 4;
        out_bgra.resize(row_bytes * (uh as usize), 0);
        let src_ptr = mapped.pData as *const u8;
        let pitch = mapped.RowPitch as usize;
        if pitch == row_bytes {
            std::ptr::copy_nonoverlapping(src_ptr, out_bgra.as_mut_ptr(), out_bgra.len());
        } else {
            for y in 0..(uh as usize) {
                std::ptr::copy_nonoverlapping(
                    src_ptr.add(y * pitch),
                    out_bgra.as_mut_ptr().add(y * row_bytes),
                    row_bytes,
                );
            }
        }
        (*self.context).Unmap(self.staging as *mut ID3D11Resource, 0);
        DxgiCaptureResult::NewFrame
    }
}

/// Capturador Persistente de Alta Velocidade (DXGI Desktop Duplication GPU-side + fallback GDI `BitBlt`):
pub struct FastCapturer {
    mem_dc: HDC,
    dib_bmp: HBITMAP,
    old_obj: HGDIOBJ,
    dib_ptr: *mut u8,
    dib_w: i32,
    dib_h: i32,
    dxgi: Option<DxgiDuplicator>,
    dxgi_bgra_buf: Vec<u8>,
    dxgi_retry_after: Instant,
    last_source_id: String,
    /// Buffer RGB reaproveitado entre frames (devolvido pelo encoder via `recycle_rgb`).
    spare_rgb: Vec<u8>,
    last_sent_at: Instant,
}

unsafe impl Send for FastCapturer {}

impl FastCapturer {
    pub fn new() -> Self {
        let past = Instant::now() - std::time::Duration::from_secs(2);
        Self {
            mem_dc: null_mut(),
            dib_bmp: null_mut(),
            old_obj: null_mut(),
            dib_ptr: null_mut(),
            dib_w: 0,
            dib_h: 0,
            dxgi: None,
            dxgi_bgra_buf: Vec::new(),
            dxgi_retry_after: past,
            last_source_id: String::new(),
            spare_rgb: Vec::new(),
            last_sent_at: past,
        }
    }

    /// Força o envio do próximo quadro mesmo se o DXGI reportar tela estática (`Unchanged`).
    pub fn force_next_frame(&mut self) {
        self.last_sent_at = Instant::now() - std::time::Duration::from_secs(2);
    }

    /// Devolve um buffer RGB já usado para ser reaproveitado no próximo frame (zero alocação).
    pub fn recycle_rgb(&mut self, buf: Vec<u8>) {
        if buf.capacity() > self.spare_rgb.capacity() {
            self.spare_rgb = buf;
        }
    }

    unsafe fn release_gdi(&mut self) {
        if let Some(mut d) = self.dxgi.take() {
            d.release_all();
        }
        if !self.mem_dc.is_null() {
            if !self.old_obj.is_null() {
                SelectObject(self.mem_dc, self.old_obj);
                self.old_obj = null_mut();
            }
            if !self.dib_bmp.is_null() {
                DeleteObject(self.dib_bmp as *mut _);
                self.dib_bmp = null_mut();
            }
            DeleteDC(self.mem_dc);
            self.mem_dc = null_mut();
        }
        self.dib_ptr = null_mut();
        self.dib_w = 0;
        self.dib_h = 0;
    }

    unsafe fn ensure_dib_surface(&mut self, screen_dc: HDC, w: i32, h: i32) -> bool {
        if !self.mem_dc.is_null()
            && !self.dib_bmp.is_null()
            && !self.dib_ptr.is_null()
            && self.dib_w == w
            && self.dib_h == h
        {
            return true;
        }

        self.release_gdi();

        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_null() {
            return false;
        }

        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as DWORD;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h; // Top-down DIB
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;

        let mut bits_ptr: *mut winapi::ctypes::c_void = null_mut();
        let dib_bmp = CreateDIBSection(
            screen_dc,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            null_mut(),
            0,
        );

        if dib_bmp.is_null() || bits_ptr.is_null() {
            if !dib_bmp.is_null() {
                DeleteObject(dib_bmp as *mut _);
            }
            DeleteDC(mem_dc);
            return false;
        }

        let old_obj = SelectObject(mem_dc, dib_bmp as *mut _);
        self.mem_dc = mem_dc;
        self.dib_bmp = dib_bmp;
        self.old_obj = old_obj;
        self.dib_ptr = bits_ptr as *mut u8;
        self.dib_w = w;
        self.dib_h = h;
        true
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
            let (src_left, src_top, src_w, src_h) = match source.kind {
                CaptureSourceKind::Monitor {
                    left,
                    top,
                    width,
                    height,
                    ..
                } => (left, top, width, height),
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
                    // Recorta bordas invisíveis do Windows 10/11 (ex: -8px em janelas maximizadas)
                    let v_left = GetSystemMetrics(SM_XVIRTUALSCREEN);
                    let v_top = GetSystemMetrics(SM_YVIRTUALSCREEN);
                    let v_right = v_left + GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1920);
                    let v_bottom = v_top + GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1080);

                    let cl_left = rect.left.max(v_left);
                    let cl_top = rect.top.max(v_top);
                    let cl_right = rect.right.min(v_right);
                    let cl_bottom = rect.bottom.min(v_bottom);

                    let w = (cl_right - cl_left).max(16);
                    let h = (cl_bottom - cl_top).max(16);
                    (cl_left, cl_top, w, h)
                }
            };

            if src_w <= 0 || src_h <= 0 {
                return None;
            }

            let scale_x = target_w as f32 / src_w as f32;
            let scale_y = target_h as f32 / src_h as f32;
            let scale = scale_x.min(scale_y).min(1.0);

            let out_w = (((src_w as f32 * scale) as u32) & !1).max(64);
            let out_h = (((src_h as f32 * scale) as u32) & !1).max(64);

            if self.last_source_id != source.id {
                self.last_source_id = source.id.clone();
                self.force_next_frame();
                self.dxgi_retry_after = Instant::now() - std::time::Duration::from_secs(1);
            }

            // Tenta primeiro via DXGI Desktop Duplication (GPU-side, sem travar DWM)
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
                    self.dxgi_retry_after = Instant::now() + std::time::Duration::from_secs(3);
                }
            }

            let mut used_dxgi = false;
            if let Some(ref mut dupl) = self.dxgi {
                match dupl.capture_crop_bgra(
                    src_left,
                    src_top,
                    src_w,
                    src_h,
                    &mut self.dxgi_bgra_buf,
                ) {
                    DxgiCaptureResult::NewFrame => {
                        used_dxgi = true;
                        self.last_sent_at = Instant::now();
                    }
                    DxgiCaptureResult::Unchanged => {
                        if self.last_sent_at.elapsed() < std::time::Duration::from_secs(1) {
                            return None;
                        }
                        // Reenvio periódico (1s) para quem acabou de entrar na sala
                    }
                    DxgiCaptureResult::Unavailable => {
                        if let Some(mut old) = self.dxgi.take() {
                            old.release_all();
                        }
                        self.dxgi_retry_after = Instant::now() + std::time::Duration::from_secs(2);
                    }
                }
            }

            let bgra_slice: &[u8] = if used_dxgi {
                &self.dxgi_bgra_buf
            } else {
                let screen_dc = GetDC(null_mut());
                if screen_dc.is_null() {
                    return None;
                }

                if !self.ensure_dib_surface(screen_dc, src_w, src_h) {
                    ReleaseDC(null_mut(), screen_dc);
                    return None;
                }

                let blt_ok = BitBlt(
                    self.mem_dc,
                    0,
                    0,
                    src_w,
                    src_h,
                    screen_dc,
                    src_left,
                    src_top,
                    SRCCOPY,
                );
                ReleaseDC(null_mut(), screen_dc);

                if blt_ok == 0 {
                    return None;
                }

                if draw_cursor {
                    let mut ci: CURSORINFO = zeroed();
                    ci.cbSize = size_of::<CURSORINFO>() as DWORD;
                    if GetCursorInfo(&mut ci) != 0 && (ci.flags & CURSOR_SHOWING) != 0 {
                        let cx = ci.ptScreenPos.x - src_left;
                        let cy = ci.ptScreenPos.y - src_top;
                        if cx >= 0 && cy >= 0 && cx < src_w && cy < src_h {
                            DrawIconEx(
                                self.mem_dc,
                                cx,
                                cy,
                                ci.hCursor,
                                0,
                                0,
                                0,
                                null_mut(),
                                DI_NORMAL,
                            );
                        }
                    }
                }

                GdiFlush();
                self.last_sent_at = Instant::now();
                let src_total_bytes = (src_w as usize) * (src_h as usize) * 4;
                std::slice::from_raw_parts(self.dib_ptr, src_total_bytes)
            };

            let out_pixels = (out_w * out_h) as usize;
            // Reaproveita o buffer devolvido pelo encoder: todos os bytes são sobrescritos abaixo,
            // então só redimensiona quando o tamanho de saída muda.
            let mut rgb = std::mem::take(&mut self.spare_rgb);
            if rgb.len() != out_pixels * 3 {
                rgb.resize(out_pixels * 3, 0);
            }
            let mut rgba = if need_rgba_preview {
                vec![0u8; out_pixels * 4]
            } else {
                Vec::new()
            };

            if out_w as i32 == src_w && out_h as i32 == src_h {
                // Caminho 1:1 ultra-rápido (vetorizado via chunks_exact)
                if need_rgba_preview {
                    for ((bgra_px, rgb_px), rgba_px) in bgra_slice
                        .chunks_exact(4)
                        .zip(rgb.chunks_exact_mut(3))
                        .zip(rgba.chunks_exact_mut(4))
                    {
                        let b = bgra_px[0];
                        let g = bgra_px[1];
                        let r = bgra_px[2];
                        rgb_px[0] = r;
                        rgb_px[1] = g;
                        rgb_px[2] = b;
                        rgba_px[0] = r;
                        rgba_px[1] = g;
                        rgba_px[2] = b;
                        rgba_px[3] = 255;
                    }
                } else {
                    for (bgra_px, rgb_px) in
                        bgra_slice.chunks_exact(4).zip(rgb.chunks_exact_mut(3))
                    {
                        rgb_px[0] = bgra_px[2];
                        rgb_px[1] = bgra_px[1];
                        rgb_px[2] = bgra_px[0];
                    }
                }
            } else {
                // Downscale Bilinear 2x2 em inteiros (texto muito mais nítido e JPEG menor!)
                let src_row_stride = (src_w as usize) * 4;
                let out_w_usize = out_w as usize;
                let out_h_usize = out_h as usize;
                let max_sx = (src_w as usize).saturating_sub(2);
                let max_sy = (src_h as usize).saturating_sub(2);
                let scale_x_fp = ((src_w as usize) << 16) / out_w_usize.max(1);
                let scale_y_fp = ((src_h as usize) << 16) / out_h_usize.max(1);

                for dy in 0..out_h_usize {
                    let sy_fp = dy * scale_y_fp;
                    let sy = (sy_fp >> 16).min(max_sy);
                    let wy = (sy_fp & 0xFFFF) >> 8; // 0..255
                    let inv_wy = 256 - wy;

                    let row0 = &bgra_slice[sy * src_row_stride..(sy + 1) * src_row_stride];
                    let row1 = &bgra_slice[(sy + 1) * src_row_stride..(sy + 2) * src_row_stride];
                    let dst_rgb_row =
                        &mut rgb[dy * out_w_usize * 3..(dy + 1) * out_w_usize * 3];

                    if need_rgba_preview {
                        let dst_rgba_row =
                            &mut rgba[dy * out_w_usize * 4..(dy + 1) * out_w_usize * 4];
                        for dx in 0..out_w_usize {
                            let sx_fp = dx * scale_x_fp;
                            let sx = (sx_fp >> 16).min(max_sx);
                            let wx = (sx_fp & 0xFFFF) >> 8;
                            let inv_wx = 256 - wx;

                            let i0 = sx * 4;
                            let i1 = i0 + 4;
                            let w00 = inv_wx * inv_wy;
                            let w10 = wx * inv_wy;
                            let w01 = inv_wx * wy;
                            let w11 = wx * wy;

                            let b = ((row0[i0] as usize * w00
                                + row0[i1] as usize * w10
                                + row1[i0] as usize * w01
                                + row1[i1] as usize * w11)
                                >> 16) as u8;
                            let g = ((row0[i0 + 1] as usize * w00
                                + row0[i1 + 1] as usize * w10
                                + row1[i0 + 1] as usize * w01
                                + row1[i1 + 1] as usize * w11)
                                >> 16) as u8;
                            let r = ((row0[i0 + 2] as usize * w00
                                + row0[i1 + 2] as usize * w10
                                + row1[i0 + 2] as usize * w01
                                + row1[i1 + 2] as usize * w11)
                                >> 16) as u8;

                            let r_off = dx * 3;
                            dst_rgb_row[r_off] = r;
                            dst_rgb_row[r_off + 1] = g;
                            dst_rgb_row[r_off + 2] = b;

                            let a_off = dx * 4;
                            dst_rgba_row[a_off] = r;
                            dst_rgba_row[a_off + 1] = g;
                            dst_rgba_row[a_off + 2] = b;
                            dst_rgba_row[a_off + 3] = 255;
                        }
                    } else {
                        for dx in 0..out_w_usize {
                            let sx_fp = dx * scale_x_fp;
                            let sx = (sx_fp >> 16).min(max_sx);
                            let wx = (sx_fp & 0xFFFF) >> 8;
                            let inv_wx = 256 - wx;

                            let i0 = sx * 4;
                            let i1 = i0 + 4;
                            let w00 = inv_wx * inv_wy;
                            let w10 = wx * inv_wy;
                            let w01 = inv_wx * wy;
                            let w11 = wx * wy;

                            let b = ((row0[i0] as usize * w00
                                + row0[i1] as usize * w10
                                + row1[i0] as usize * w01
                                + row1[i1] as usize * w11)
                                >> 16) as u8;
                            let g = ((row0[i0 + 1] as usize * w00
                                + row0[i1 + 1] as usize * w10
                                + row1[i0 + 1] as usize * w01
                                + row1[i1 + 1] as usize * w11)
                                >> 16) as u8;
                            let r = ((row0[i0 + 2] as usize * w00
                                + row0[i1 + 2] as usize * w10
                                + row1[i0 + 2] as usize * w01
                                + row1[i1 + 2] as usize * w11)
                                >> 16) as u8;

                            let r_off = dx * 3;
                            dst_rgb_row[r_off] = r;
                            dst_rgb_row[r_off + 1] = g;
                            dst_rgb_row[r_off + 2] = b;
                        }
                    }
                }
            }

            Some(RawFrame {
                width: out_w,
                height: out_h,
                rgb,
                rgba,
            })
        }
    }
}

impl Drop for FastCapturer {
    fn drop(&mut self) {
        unsafe {
            self.release_gdi();
        }
    }
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
}

#[cfg(not(windows))]
mod platform {
    use xcap::{Monitor, Window};

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
                if title.is_empty() || title.contains("SZdisc") {
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
                    kind: CaptureSourceKind::Window {
                        hwnd: wid as usize,
                    },
                });
            }
        }
        sources
    }

    pub fn capture_thumbnail_rgba(source: &CaptureSource, max_w: u32, max_h: u32) -> Option<(u32, u32, Vec<u8>)> {
        let img = match source.kind {
            CaptureSourceKind::Monitor { index, .. } => {
                Monitor::all().ok()?.into_iter().nth(index)?.capture_image().ok()?
            }
            CaptureSourceKind::Window { hwnd } => {
                Window::all()
                    .ok()?
                    .into_iter()
                    .find(|w| w.id().ok() == Some(hwnd as u32))?
                    .capture_image()
                    .ok()?
            }
        };
        let dyn_img = image::DynamicImage::ImageRgba8(img);
        let resized = dyn_img.thumbnail(max_w, max_h).to_rgba8();
        let (w, h) = resized.dimensions();
        Some((w, h, resized.into_raw()))
    }

    pub fn is_szdisc_in_foreground() -> bool {
        true
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
            let img = match source.kind {
                CaptureSourceKind::Monitor { index, .. } => {
                    Monitor::all().ok()?.into_iter().nth(index)?.capture_image().ok()?
                }
                CaptureSourceKind::Window { hwnd } => {
                    Window::all()
                        .ok()?
                        .into_iter()
                        .find(|w| w.id().ok() == Some(hwnd as u32))?
                        .capture_image()
                        .ok()?
                }
            };
            let src_w = img.width();
            let src_h = img.height();
            if src_w == 0 || src_h == 0 {
                return None;
            }
            let scale = (target_w as f32 / src_w as f32)
                .min(target_h as f32 / src_h as f32)
                .min(1.0);
            let out_w = (((src_w as f32 * scale) as u32) & !1).max(64);
            let out_h = (((src_h as f32 * scale) as u32) & !1).max(64);

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

    pub fn get_process_ram_mb() -> f32 {
        0.0
    }
}

pub use platform::*;

#[cfg(all(test, windows))]
mod tests {
    use super::platform::{is_szdisc_frontend_title, is_szdisc_window_match};

    #[test]
    fn reconhece_janela_szweb_e_ignora_ide_ou_explorer() {
        assert!(is_szdisc_window_match(
            "SZdisc — Frontend Vite + shadcn/ui",
            "msedge.exe"
        ));
        assert!(is_szdisc_window_match(
            "SZdisc — Frontend Vite + shadcn/ui - Google Chrome",
            "chrome.exe"
        ));
        assert!(is_szdisc_window_match(
            "SZdisc — Frontend Vite + shadcn/ui",
            "custom_webview.exe"
        ));
        assert!(!is_szdisc_frontend_title("SZdisc - Visual Studio Code"));
        assert!(!is_szdisc_window_match(
            "SZdisc - Visual Studio Code",
            "Code.exe"
        ));
        assert!(!is_szdisc_window_match(
            "C:\\Users\\Venti\\SZdisc",
            "explorer.exe"
        ));
    }
}
