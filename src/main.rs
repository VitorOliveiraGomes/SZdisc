#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod bridge;
mod capture;
mod network;
mod profile_theme;
mod stream_controller;
mod updater;

use std::net::TcpStream;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use app::AppCore;
use bridge::{BridgeServer, BRIDGE_PORT};

fn launch_szweb_window(url: &str) {
    #[cfg(windows)]
    {
        let app_arg = format!("--app={}", url);
        let browsers = [
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        ];
        for bin in browsers {
            if std::path::Path::new(bin).is_file() {
                if Command::new(bin).arg(&app_arg).spawn().is_ok() {
                    return;
                }
            }
        }
        let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(url).spawn();
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = Command::new("xdg-open").arg(url).spawn();
    }
}

fn main() {
    #[cfg(windows)]
    unsafe {
        winapi::um::timeapi::timeBeginPeriod(1);
    }

    let core = Arc::new(Mutex::new(AppCore::new()));
    let _bridge = BridgeServer::start(Arc::clone(&core));

    // Em dev, se o Vite (porta 5173) já estiver rodando, abre direto nele; senão usa o dist servido pelo Rust
    let dev_vite_up = TcpStream::connect_timeout(
        &"127.0.0.1:5173".parse().unwrap(),
        Duration::from_millis(120),
    )
    .is_ok();
    let url = if dev_vite_up {
        "http://localhost:5173".to_string()
    } else {
        format!("http://127.0.0.1:{}", BRIDGE_PORT)
    };

    launch_szweb_window(&url);

    // Mantém o processo nativo (captura, áudio, rede e bridge) ativo
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
