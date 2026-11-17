#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod bridge;
mod capture;
mod config;
mod core;
mod desktop;
mod network;
mod updater;

use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

use bridge::BRIDGE_PORT;

#[cfg(windows)]
fn init_windows_runtime() {
    use winapi::um::libloaderapi::{GetModuleHandleA, GetProcAddress};
    use winapi::um::timeapi::timeBeginPeriod;
    use winapi::um::winuser::SetProcessDPIAware;

    unsafe {
        timeBeginPeriod(1);

        let user32 = GetModuleHandleA(c"user32.dll".as_ptr());
        if !user32.is_null() {
            let proc = GetProcAddress(user32, c"SetProcessDpiAwarenessContext".as_ptr());
            if !proc.is_null() {
                let set_ctx: unsafe extern "system" fn(*mut std::ffi::c_void) -> i32 =
                    std::mem::transmute(proc);
                // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4
                if set_ctx((-4isize) as *mut std::ffi::c_void) != 0 {
                    return;
                }
            }
        }
        SetProcessDPIAware();
    }
}

#[cfg(not(windows))]
fn init_windows_runtime() {}

fn wait_for_bridge_port_release(max_wait: Duration) {
    let deadline = Instant::now() + max_wait;
    while Instant::now() < deadline {
        if TcpListener::bind(("127.0.0.1", BRIDGE_PORT)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn main() {
    init_windows_runtime();

    if std::env::args().any(|a| a == "--wait-replace") {
        wait_for_bridge_port_release(Duration::from_secs(4));
    }

    if let Err(error) = desktop::run() {
        eprintln!("Falha ao iniciar ScreenZ: {error}");
        #[cfg(windows)]
        unsafe {
            use winapi::um::winuser::{MessageBoxW, MB_ICONERROR, MB_OK};
            let message: Vec<u16> = format!("Falha ao iniciar ScreenZ:\n{error}\0")
                .encode_utf16()
                .collect();
            MessageBoxW(
                std::ptr::null_mut(),
                message.as_ptr(),
                "ScreenZ\0".encode_utf16().collect::<Vec<_>>().as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}
