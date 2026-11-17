use std::mem::{size_of, zeroed};
use std::ptr::null_mut;

use winapi::shared::minwindef::{DWORD, FALSE, UINT};
use winapi::shared::windef::{HBITMAP, HDC, HGDIOBJ, HWND, RECT};
use winapi::um::wingdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject,
    GdiFlush, GetDIBits, SelectObject, SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, COLORONCOLOR, DIB_RGB_COLORS, SRCCOPY,
};
use winapi::um::winuser::{
    DrawIconEx, GetCursorInfo, GetDC, GetDesktopWindow, GetIconInfo, GetWindowDC, GetWindowRect,
    IsIconic, PrintWindow, ReleaseDC, CURSORINFO, CURSOR_SHOWING, ICONINFO,
};

use super::super::{CaptureSource, CaptureSourceKind, RawFrame};
use super::scaler::{compute_output_dimensions, convert_bgra_to_frame};

const DI_NORMAL: UINT = 0x0003;
const CURSOR_BOX: i32 = 64;

pub(crate) struct GdiSurface {
    mem_dc: HDC,
    dib_bmp: HBITMAP,
    old_obj: HGDIOBJ,
    dib_ptr: *mut u8,
    dib_w: i32,
    dib_h: i32,
}

impl GdiSurface {
    pub(crate) fn new() -> Self {
        Self {
            mem_dc: null_mut(),
            dib_bmp: null_mut(),
            old_obj: null_mut(),
            dib_ptr: null_mut(),
            dib_w: 0,
            dib_h: 0,
        }
    }

    /// Libera apenas recursos GDI sem tocar em DXGI.
    pub(crate) unsafe fn release(&mut self) {
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

    pub(crate) unsafe fn ensure_surface(&mut self, screen_dc: HDC, w: i32, h: i32) -> bool {
        if !self.mem_dc.is_null()
            && !self.dib_bmp.is_null()
            && !self.dib_ptr.is_null()
            && self.dib_w == w
            && self.dib_h == h
        {
            return true;
        }
        self.release();

        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_null() {
            return false;
        }

        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as DWORD;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h;
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

    pub(crate) unsafe fn capture_screen_bgra(
        &mut self,
        src_left: i32,
        src_top: i32,
        src_w: i32,
        src_h: i32,
        draw_cursor: bool,
    ) -> Option<&[u8]> {
        let screen_dc = GetDC(null_mut());
        if screen_dc.is_null() {
            return None;
        }

        if !self.ensure_surface(screen_dc, src_w, src_h) {
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
            draw_cursor_on_dc(self.mem_dc, src_left, src_top, src_w, src_h, 1.0);
        }

        GdiFlush();
        let total = (src_w as usize) * (src_h as usize) * 4;
        Some(std::slice::from_raw_parts(self.dib_ptr, total))
    }
}

impl Drop for GdiSurface {
    fn drop(&mut self) {
        unsafe {
            self.release();
        }
    }
}

/// Captura e compõe o cursor do Windows diretamente sobre um buffer BGRA (para uso com DXGI).
pub(crate) struct CursorOverlay {
    black: GdiSurface,
    white: GdiSurface,
}

impl CursorOverlay {
    pub(crate) fn new() -> Self {
        Self {
            black: GdiSurface::new(),
            white: GdiSurface::new(),
        }
    }

    /// Retorna a posição atual do cursor relativa ao retângulo (`Some((cx, cy))`) se visível dentro dele.
    pub(crate) unsafe fn current_relative_pos(
        src_left: i32,
        src_top: i32,
        src_w: i32,
        src_h: i32,
    ) -> Option<(i32, i32)> {
        let mut ci: CURSORINFO = zeroed();
        ci.cbSize = size_of::<CURSORINFO>() as DWORD;
        if GetCursorInfo(&mut ci) == 0 || (ci.flags & CURSOR_SHOWING) == 0 || ci.hCursor.is_null() {
            return None;
        }
        let cx = ci.ptScreenPos.x - src_left;
        let cy = ci.ptScreenPos.y - src_top;
        if cx >= 0 && cy >= 0 && cx < src_w && cy < src_h {
            Some((cx, cy))
        } else {
            None
        }
    }

    pub(crate) unsafe fn stamp_onto_bgra(
        &mut self,
        bgra: &mut [u8],
        src_left: i32,
        src_top: i32,
        src_w: i32,
        src_h: i32,
    ) {
        let mut ci: CURSORINFO = zeroed();
        ci.cbSize = size_of::<CURSORINFO>() as DWORD;
        if GetCursorInfo(&mut ci) == 0 || (ci.flags & CURSOR_SHOWING) == 0 || ci.hCursor.is_null() {
            return;
        }

        let mut hotspot_x = 0i32;
        let mut hotspot_y = 0i32;
        let mut icon_info: ICONINFO = zeroed();
        if GetIconInfo(ci.hCursor, &mut icon_info) != 0 {
            hotspot_x = icon_info.xHotspot as i32;
            hotspot_y = icon_info.yHotspot as i32;
            if !icon_info.hbmMask.is_null() {
                DeleteObject(icon_info.hbmMask as *mut _);
            }
            if !icon_info.hbmColor.is_null() {
                DeleteObject(icon_info.hbmColor as *mut _);
            }
        }

        let draw_x = ci.ptScreenPos.x - src_left - hotspot_x;
        let draw_y = ci.ptScreenPos.y - src_top - hotspot_y;
        if draw_x + CURSOR_BOX <= 0
            || draw_y + CURSOR_BOX <= 0
            || draw_x >= src_w
            || draw_y >= src_h
        {
            return;
        }

        let screen_dc = GetDC(null_mut());
        if screen_dc.is_null() {
            return;
        }
        let ok = self.black.ensure_surface(screen_dc, CURSOR_BOX, CURSOR_BOX)
            && self.white.ensure_surface(screen_dc, CURSOR_BOX, CURSOR_BOX);
        ReleaseDC(null_mut(), screen_dc);
        if !ok {
            return;
        }

        let box_bytes = (CURSOR_BOX * CURSOR_BOX * 4) as usize;
        std::ptr::write_bytes(self.black.dib_ptr, 0x00, box_bytes);
        std::ptr::write_bytes(self.white.dib_ptr, 0xFF, box_bytes);

        DrawIconEx(
            self.black.mem_dc,
            0,
            0,
            ci.hCursor,
            0,
            0,
            0,
            null_mut(),
            DI_NORMAL,
        );
        DrawIconEx(
            self.white.mem_dc,
            0,
            0,
            ci.hCursor,
            0,
            0,
            0,
            null_mut(),
            DI_NORMAL,
        );
        GdiFlush();

        let b_slice = std::slice::from_raw_parts(self.black.dib_ptr, box_bytes);
        let w_slice = std::slice::from_raw_parts(self.white.dib_ptr, box_bytes);

        for sy in 0..CURSOR_BOX {
            let dy = draw_y + sy;
            if dy < 0 || dy >= src_h {
                continue;
            }
            for sx in 0..CURSOR_BOX {
                let dx = draw_x + sx;
                if dx < 0 || dx >= src_w {
                    continue;
                }
                let si = ((sy * CURSOR_BOX + sx) * 4) as usize;
                let (bb, bg, br) = (b_slice[si], b_slice[si + 1], b_slice[si + 2]);
                let (wb, wg, wr) = (w_slice[si], w_slice[si + 1], w_slice[si + 2]);

                if bb == 0 && bg == 0 && br == 0 && wb == 255 && wg == 255 && wr == 255 {
                    continue;
                }

                let di = ((dy * src_w + dx) * 4) as usize;
                if di + 3 >= bgra.len() {
                    continue;
                }

                // Cursor XOR (ex: I-beam de texto): preto vira branco e branco vira preto
                if bb > wb && bg > wg && br > wr {
                    bgra[di] ^= bb;
                    bgra[di + 1] ^= bg;
                    bgra[di + 2] ^= br;
                    continue;
                }

                let diff = (wg as i32 - bg as i32).clamp(0, 255);
                let alpha = (255 - diff) as u32;
                let inv = 255 - alpha;
                bgra[di] = ((bb as u32 * 255 + bgra[di] as u32 * inv) / 255).min(255) as u8;
                bgra[di + 1] = ((bg as u32 * 255 + bgra[di + 1] as u32 * inv) / 255).min(255) as u8;
                bgra[di + 2] = ((br as u32 * 255 + bgra[di + 2] as u32 * inv) / 255).min(255) as u8;
            }
        }
    }
}

unsafe fn draw_cursor_on_dc(
    mem_dc: HDC,
    src_left: i32,
    src_top: i32,
    out_w: i32,
    out_h: i32,
    scale: f32,
) {
    let mut ci: CURSORINFO = zeroed();
    ci.cbSize = size_of::<CURSORINFO>() as DWORD;
    if GetCursorInfo(&mut ci) != 0 && (ci.flags & CURSOR_SHOWING) != 0 {
        let cx = ((ci.ptScreenPos.x - src_left) as f32 * scale) as i32;
        let cy = ((ci.ptScreenPos.y - src_top) as f32 * scale) as i32;
        if cx >= 0 && cy >= 0 && cx < out_w && cy < out_h {
            DrawIconEx(mem_dc, cx, cy, ci.hCursor, 0, 0, 0, null_mut(), DI_NORMAL);
        }
    }
}

/// Captura única para miniaturas (`capture_rgb = false`, `capture_rgba = true`).
pub(crate) fn capture_single_frame(
    source: &CaptureSource,
    target_w: u32,
    target_h: u32,
    capture_rgb: bool,
    capture_rgba: bool,
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
                capture_rgb,
                capture_rgba,
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
                let w = rect.right - rect.left;
                let h = rect.bottom - rect.top;
                if w < 2 || h < 2 {
                    return None;
                }
                capture_region(
                    hwnd_ptr,
                    rect.left,
                    rect.top,
                    w,
                    h,
                    target_w,
                    target_h,
                    true,
                    capture_rgb,
                    capture_rgba,
                    draw_cursor,
                )
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn capture_region(
    hwnd: HWND,
    src_left: i32,
    src_top: i32,
    src_w: i32,
    src_h: i32,
    target_w: u32,
    target_h: u32,
    is_window: bool,
    capture_rgb: bool,
    capture_rgba: bool,
    draw_cursor: bool,
) -> Option<RawFrame> {
    let (out_w, out_h, scale) = compute_output_dimensions(src_w, src_h, target_w, target_h)?;

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
    } else if out_w as i32 == src_w && out_h as i32 == src_h {
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

    if draw_cursor {
        draw_cursor_on_dc(mem_dc, src_left, src_top, out_w as i32, out_h as i32, scale);
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

    if is_window && lines > 0 && bgra.iter().step_by(64).all(|&b| b == 0) {
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

    SelectObject(mem_dc, old_obj);
    DeleteObject(bmp as *mut _);
    DeleteDC(mem_dc);
    ReleaseDC(desktop_hwnd, desktop_dc);

    if lines == 0 {
        return None;
    }

    let mut dummy_spare = Vec::new();
    convert_bgra_to_frame(
        &bgra,
        (out_w as i32, out_h as i32),
        (out_w, out_h),
        capture_rgb,
        capture_rgba,
        &mut dummy_spare,
    )
}
