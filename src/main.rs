#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod capture;
mod network;
mod profile_theme;
mod stream_controller;
mod ui;
mod updater;

use app::Disc2App;
use eframe::egui;
use updater::CURRENT_VERSION;

fn main() -> eframe::Result<()> {
    #[cfg(windows)]
    unsafe {
        winapi::um::timeapi::timeBeginPeriod(1);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!(
                "SZdisc v{} — DXGI GPU Screen Share (Sem VPN)",
                CURRENT_VERSION
            ))
            .with_inner_size([1280.0, 760.0])
            .with_min_inner_size([960.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "SZdisc",
        options,
        Box::new(|cc| Ok(Box::new(Disc2App::new(cc)))),
    )
}
