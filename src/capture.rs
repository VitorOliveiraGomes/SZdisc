use std::ffi::OsString;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::ptr::null_mut;

use winapi::shared::minwindef::{BOOL, DWORD, FALSE, LPARAM, TRUE, UINT};
use winapi::shared::windef::{HBITMAP, HDC, HGDIOBJ, HMONITOR, HWND, RECT};
use winapi::um::dwmapi::{DwmGetWindowAttribute, DWMWA_CLOAKED};
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

/// Retorna `true` apenas quando a janela do SZdisc está em primeiro plano (focada pelo usuário).
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
        fg_pid != 0 && fg_pid == GetCurrentProcessId()
    }
}

/// Capturador Persistente de Alta Velocidade (`CreateDIBSection` Zero-Copy + `BitBlt` 1:1 sem `PrintWindow`):
/// - Não cria nem destrói objetos GDI a cada frame (mantém `mem_dc` e `dib_bmp` mapeados em RAM).
/// - Nunca chama `PrintWindow` durante a transmissão ao vivo (evita travar a thread de renderização de jogos DirectX/Vulkan/OpenGL).
/// - Faz o downscaling (`1080p -> 720p/900p`) e a conversão `BGRA -> RGB` em uma única passagem direta via ponteiro de memória.
pub struct FastCapturer {
    mem_dc: HDC,
    dib_bmp: HBITMAP,
    old_obj: HGDIOBJ,
    dib_ptr: *mut u8,
    dib_w: i32,
    dib_h: i32,
    x_byte_offsets: Vec<usize>,
    cached_src_w: i32,
    cached_out_w: u32,
}

unsafe impl Send for FastCapturer {}

impl FastCapturer {
    pub fn new() -> Self {
        Self {
            mem_dc: null_mut(),
            dib_bmp: null_mut(),
            old_obj: null_mut(),
            dib_ptr: null_mut(),
            dib_w: 0,
            dib_h: 0,
            x_byte_offsets: Vec::new(),
            cached_src_w: 0,
            cached_out_w: 0,
        }
    }

    unsafe fn release_gdi(&mut self) {
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
                    if GetWindowRect(hwnd_ptr, &mut rect) == 0 {
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

            let screen_dc = GetDC(null_mut());
            if screen_dc.is_null() {
                return None;
            }

            if !self.ensure_dib_surface(screen_dc, src_w, src_h) {
                ReleaseDC(null_mut(), screen_dc);
                return None;
            }

            // Cópia 1:1 acelerada por GPU direto do compositor DWM (NUNCA trava o jogo!)
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
                        DrawIconEx(self.mem_dc, cx, cy, ci.hCursor, 0, 0, 0, null_mut(), DI_NORMAL);
                    }
                }
            }

            GdiFlush();

            let src_total_bytes = (src_w as usize) * (src_h as usize) * 4;
            let bgra_slice = std::slice::from_raw_parts(self.dib_ptr, src_total_bytes);

            let out_pixels = (out_w * out_h) as usize;
            let mut rgb = vec![0u8; out_pixels * 3];
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
                // Caminho Downscale em 1 passagem com tabela LUT pré-calculada (sem StretchBlt lento na GDI)
                if self.cached_src_w != src_w || self.cached_out_w != out_w {
                    self.x_byte_offsets.clear();
                    self.x_byte_offsets.reserve(out_w as usize);
                    for dx in 0..(out_w as usize) {
                        let sx = (dx * (src_w as usize)) / (out_w as usize);
                        self.x_byte_offsets.push(sx * 4);
                    }
                    self.cached_src_w = src_w;
                    self.cached_out_w = out_w;
                }

                let src_row_stride = (src_w as usize) * 4;
                let out_w_usize = out_w as usize;
                let out_h_usize = out_h as usize;
                let src_h_usize = src_h as usize;

                if need_rgba_preview {
                    for dy in 0..out_h_usize {
                        let sy = (dy * src_h_usize) / out_h_usize;
                        let src_row =
                            &bgra_slice[sy * src_row_stride..(sy + 1) * src_row_stride];
                        let dst_rgb_row =
                            &mut rgb[dy * out_w_usize * 3..(dy + 1) * out_w_usize * 3];
                        let dst_rgba_row =
                            &mut rgba[dy * out_w_usize * 4..(dy + 1) * out_w_usize * 4];

                        for ((&x_off, rgb_px), rgba_px) in self
                            .x_byte_offsets
                            .iter()
                            .zip(dst_rgb_row.chunks_exact_mut(3))
                            .zip(dst_rgba_row.chunks_exact_mut(4))
                        {
                            let b = src_row[x_off];
                            let g = src_row[x_off + 1];
                            let r = src_row[x_off + 2];
                            rgb_px[0] = r;
                            rgb_px[1] = g;
                            rgb_px[2] = b;
                            rgba_px[0] = r;
                            rgba_px[1] = g;
                            rgba_px[2] = b;
                            rgba_px[3] = 255;
                        }
                    }
                } else {
                    for dy in 0..out_h_usize {
                        let sy = (dy * src_h_usize) / out_h_usize;
                        let src_row =
                            &bgra_slice[sy * src_row_stride..(sy + 1) * src_row_stride];
                        let dst_rgb_row =
                            &mut rgb[dy * out_w_usize * 3..(dy + 1) * out_w_usize * 3];

                        for (&x_off, rgb_px) in self
                            .x_byte_offsets
                            .iter()
                            .zip(dst_rgb_row.chunks_exact_mut(3))
                        {
                            rgb_px[0] = src_row[x_off + 2];
                            rgb_px[1] = src_row[x_off + 1];
                            rgb_px[2] = src_row[x_off];
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
