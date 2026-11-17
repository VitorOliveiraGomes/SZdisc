use std::sync::{Arc, Mutex};

use tauri::{
    AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::bridge::{dto::build_snapshot_json, rpc::handle_rpc_request, BridgeServer};
use crate::core::AppCore;

type DesktopBridge = Arc<BridgeServer>;

#[tauri::command]
fn bridge_state(bridge: State<'_, DesktopBridge>) -> String {
    bridge.get_snapshot_json()
}

fn popout_tile_id(label: &str) -> Option<u32> {
    label.strip_prefix("stream-")?.parse().ok()
}

fn open_stream_window(app: &AppHandle, tile_id: u32) -> Result<WebviewWindow, String> {
    let label = format!("stream-{tile_id}");
    if let Some(window) = app.get_webview_window(&label) {
        window.show().map_err(|error| error.to_string())?;
        window.unminimize().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(window);
    }

    WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("index.html?popout={tile_id}").into()),
    )
    .title(format!("ScreenZ Pop-out — Transmissão {tile_id}"))
    .inner_size(960.0, 580.0)
    .min_inner_size(480.0, 300.0)
    .build()
    .map_err(|error| error.to_string())
}

// A criação de WebViews e o seletor nativo de imagens precisam liberar a thread da interface.
#[tauri::command]
async fn bridge_rpc(
    app: AppHandle,
    bridge: State<'_, DesktopBridge>,
    method: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let bridge = Arc::clone(bridge.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let tile_to_open = if method == "set_popout"
            && params.get("clearAll").and_then(|value| value.as_bool()) != Some(true)
            && params.get("popout").and_then(|value| value.as_bool()) != Some(false)
        {
            let tile_id = params
                .get("tileId")
                .and_then(|value| value.as_u64())
                .and_then(|id| u32::try_from(id).ok())
                .ok_or("tileId inválido")?;
            Some(tile_id)
        } else {
            None
        };

        let body = serde_json::json!({ "method": method, "params": params }).to_string();
        let result = handle_rpc_request(&bridge, &body)?;
        if let Some(tile_id) = tile_to_open {
            if let Err(error) = open_stream_window(&app, tile_id) {
                let rollback = serde_json::json!({
                    "method": "set_popout",
                    "params": { "tileId": tile_id, "popout": false }
                });
                let _ = handle_rpc_request(&bridge, &rollback.to_string());
                return Err(error);
            }
        }
        if method == "set_popout" {
            let docked = params.get("popout").and_then(|value| value.as_bool()) == Some(false);
            let clear_all = params.get("clearAll").and_then(|value| value.as_bool()) == Some(true);
            for (label, window) in app.webview_windows() {
                if let Some(tile_id) = popout_tile_id(&label) {
                    if clear_all
                        || docked
                            && params.get("tileId").and_then(|value| value.as_u64())
                                == Some(tile_id as u64)
                    {
                        let _ = window.close();
                    }
                }
            }
        }
        serde_json::from_str(&result).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            let core = Arc::new(Mutex::new(AppCore::new()));
            let bridge = BridgeServer::start(core).map_err(|error| {
                std::io::Error::other(format!(
                    "{error}. Feche a versão anterior do ScreenZ e tente novamente."
                ))
            })?;
            app.manage(bridge);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![bridge_state, bridge_rpc])
        .on_window_event(|window, event| {
            let Some(bridge) = window.try_state::<DesktopBridge>() else {
                return;
            };
            match event {
                WindowEvent::CloseRequested { .. } if window.label() == "main" => {
                    if let Ok(mut core) = bridge.app.lock() {
                        core.leave_room();
                    }
                    window.app_handle().exit(0);
                }
                WindowEvent::Destroyed => {
                    if let Some(tile_id) = popout_tile_id(window.label()) {
                        if let Ok(mut core) = bridge.app.lock() {
                            core.popout_tiles.remove(&tile_id);
                            let snapshot = build_snapshot_json(&mut core);
                            bridge.update_snapshot(snapshot);
                        }
                    }
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
}
