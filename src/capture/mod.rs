#[cfg(windows)]
mod windows;

#[cfg(not(windows))]
mod unix;

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

#[cfg(windows)]
pub use windows::{
    capture_thumbnail_rgba, get_process_ram_mb, is_screenz_in_foreground, list_capture_sources,
    FastCapturer,
};

#[cfg(not(windows))]
pub use unix::{
    capture_thumbnail_rgba, get_process_ram_mb, is_screenz_in_foreground, list_capture_sources,
    FastCapturer,
};
