use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;

use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

use crate::capture::{capture_thumbnail_rgba, list_capture_sources};
use crate::config::{encode_base64, load_image_as_jpeg_data_url, pick_image_file_dialog};

use super::dto::{build_snapshot_json, category_slug, CaptureSourceDto, RpcRequest};
use super::BridgeServer;

pub(crate) fn handle_rpc_request(bridge: &BridgeServer, body: &str) -> Result<String, String> {
    let req: RpcRequest = serde_json::from_str(body).map_err(|e| format!("JSON inválido: {}", e))?;

    match req.method.as_str() {
        "get_state" => Ok(bridge.get_snapshot_json()),
        "list_capture_sources" => Ok(capture_sources_json()),
        "get_source_thumbnails" => Ok(capture_thumbnails_json()),
        "pick_profile_image" => {
            let kind = req
                .params
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("avatar")
                .to_string();

            let loaded = pick_image_file_dialog().and_then(|path| {
                let (w, h) = if kind == "banner" { (480, 120) } else { (128, 128) };
                load_image_as_jpeg_data_url(&path, w, h).map(|data_url| (path, data_url))
            });

            let mut app = bridge
                .app
                .lock()
                .map_err(|_| "Falha ao travar estado do AppCore".to_string())?;
            if let Some((path, data_url)) = loaded {
                if kind == "banner" {
                    app.custom.banner_image_path = path;
                    app.banner_data_url = data_url;
                } else {
                    app.custom.avatar_image_path = path;
                    app.avatar_data_url = data_url;
                }
                app.custom.save_to_disk();
                app.cached_custom_json.clear();
            }
            let snap = build_snapshot_json(&mut app);
            bridge.update_snapshot(snap.clone());
            Ok(snap)
        }
        _ => {
            let mut app = bridge
                .app
                .lock()
                .map_err(|_| "Falha ao travar estado do AppCore".to_string())?;
            app.execute_command(&req.method, &req.params)?;
            let snap = build_snapshot_json(&mut app);
            bridge.update_snapshot(snap.clone());
            Ok(snap)
        }
    }
}

fn capture_sources_json() -> String {
    let sources = list_capture_sources();
    let dtos: Vec<CaptureSourceDto> = sources
        .into_iter()
        .map(|s| CaptureSourceDto {
            id: s.id,
            name: s.name,
            subtitle: s.subtitle,
            exe_name: s.exe_name,
            pid: s.pid,
            category: category_slug(s.category),
        })
        .collect();
    serde_json::to_string(&dtos).unwrap_or_else(|_| "[]".to_string())
}

/// Captura miniaturas em paralelo (até 4 workers) sem alocar buffer RGB extra.
fn capture_thumbnails_json() -> String {
    let sources = list_capture_sources();
    let total = sources.len();
    if total == 0 {
        return "{}".to_string();
    }

    let next_idx = std::sync::atomic::AtomicUsize::new(0);
    let results: Mutex<HashMap<String, String>> = Mutex::new(HashMap::with_capacity(total));
    let worker_count = total.min(4);

    thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                let mut jpeg_buf = Vec::with_capacity(16_384);
                loop {
                    let idx = next_idx.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(src) = sources.get(idx) else {
                        break;
                    };
                    if let Some((w, h, rgba)) = capture_thumbnail_rgba(src, 272, 153) {
                        jpeg_buf.clear();
                        let mut enc = Encoder::new(&mut jpeg_buf, 70);
                        enc.set_sampling_factor(SamplingFactor::F_2_2);
                        if enc.encode(&rgba, w as u16, h as u16, ColorType::Rgba).is_ok() {
                            let data_url =
                                format!("data:image/jpeg;base64,{}", encode_base64(&jpeg_buf));
                            if let Ok(mut map) = results.lock() {
                                map.insert(src.id.clone(), data_url);
                            }
                        }
                    }
                }
            });
        }
    });

    let map = results.into_inner().unwrap_or_default();
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
}
