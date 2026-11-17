pub mod dto;
pub mod http;
pub mod rpc;
pub mod ws;

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::core::AppCore;

pub use ws::{has_ws_video_clients, publish_video_frame, ws_connect_seq};

pub const BRIDGE_PORT: u16 = 47839;

pub struct BridgeServer {
    pub(crate) app: Arc<Mutex<AppCore>>,
    pub(crate) latest_snapshot_json: Mutex<String>,
    pub(crate) dist_dir: Option<PathBuf>,
}

impl BridgeServer {
    pub fn start(app: Arc<Mutex<AppCore>>) -> Result<Arc<Self>, String> {
        let listener = TcpListener::bind(("127.0.0.1", BRIDGE_PORT))
            .map_err(|e| format!("Porta {} já está em uso: {}", BRIDGE_PORT, e))?;

        let initial_json = app
            .lock()
            .map(|mut a| dto::build_snapshot_json(&mut a))
            .unwrap_or_else(|_| "{}".to_string());

        let server = Arc::new(Self {
            app: Arc::clone(&app),
            latest_snapshot_json: Mutex::new(initial_json),
            dist_dir: http::find_screenz_web_dist_dir(),
        });

        let srv_listener = Arc::clone(&server);
        thread::spawn(move || {
            for incoming in listener.incoming().flatten() {
                let srv = Arc::clone(&srv_listener);
                thread::spawn(move || http::handle_http_connection(incoming, srv));
            }
        });

        let srv_ticker = Arc::clone(&server);
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(200));
            if let Ok(mut guard) = srv_ticker.app.lock() {
                guard.tick();
                let json = dto::build_snapshot_json(&mut guard);
                srv_ticker.update_snapshot(json);
            }
        });

        Ok(server)
    }

    pub fn update_snapshot(&self, json: String) {
        if let Ok(mut guard) = self.latest_snapshot_json.lock() {
            *guard = json;
        }
    }

    pub fn get_snapshot_json(&self) -> String {
        self.latest_snapshot_json
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| "{}".to_string())
    }
}
