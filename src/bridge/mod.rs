use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

use crate::app::AppCore;
use crate::capture::{capture_thumbnail_rgba, list_capture_sources, SourceCategory};
use crate::profile_theme::{
    generate_server_id, load_image_as_jpeg_data_url, pick_image_file_dialog, PresenceStatus,
    ServerProfile, ThemePreset,
};
use crate::stream_controller::ResolutionPreset;
use crate::updater::{
    check_for_updates_async, download_and_apply_update_async, UpdateStatus, CURRENT_VERSION,
};

pub const BRIDGE_PORT: u16 = 47839;

// ============================================================================
// Pure-Rust SHA-1 & Base64 (for RFC 6455 WebSocket handshake & JPEG data URLs)
// ============================================================================

fn sha1_digest(data: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(data.len() + 72);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    out[0..4].copy_from_slice(&h0.to_be_bytes());
    out[4..8].copy_from_slice(&h1.to_be_bytes());
    out[8..12].copy_from_slice(&h2.to_be_bytes());
    out[12..16].copy_from_slice(&h3.to_be_bytes());
    out[16..20].copy_from_slice(&h4.to_be_bytes());
    out
}

pub(crate) fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

// ============================================================================
// Minimal JSON Parser & Serializer (Zero external dependencies)
// ============================================================================

#[derive(Clone, Debug)]
pub enum JsonVal {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JsonVal>),
    Obj(HashMap<String, JsonVal>),
}

impl JsonVal {
    fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }
    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }
    fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Num(n) => Some(*n),
            _ => None,
        }
    }
    fn as_u32(&self) -> Option<u32> {
        self.as_f64().map(|n| n.max(0.0).round() as u32)
    }
    fn as_u8(&self) -> Option<u8> {
        self.as_f64().map(|n| n.clamp(0.0, 255.0).round() as u8)
    }
    fn get(&self, key: &str) -> Option<&JsonVal> {
        match self {
            Self::Obj(map) => map.get(key),
            _ => None,
        }
    }
    fn get_str(&self, key: &str) -> Option<String> {
        self.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
    }
    fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(|v| v.as_bool())
    }
    fn get_u32(&self, key: &str) -> Option<u32> {
        self.get(key).and_then(|v| v.as_u32())
    }
    fn get_u8(&self, key: &str) -> Option<u8> {
        self.get(key).and_then(|v| v.as_u8())
    }
    fn get_f32(&self, key: &str) -> Option<f32> {
        self.get(key).and_then(|v| v.as_f64()).map(|n| n as f32)
    }
    fn get_rgb(&self, key: &str) -> Option<[u8; 3]> {
        match self.get(key) {
            Some(JsonVal::Arr(arr)) if arr.len() >= 3 => {
                Some([arr[0].as_u8()?, arr[1].as_u8()?, arr[2].as_u8()?])
            }
            _ => None,
        }
    }
    fn get_str_vec(&self, key: &str) -> Option<Vec<String>> {
        match self.get(key) {
            Some(JsonVal::Arr(arr)) => Some(
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect(),
            ),
            _ => None,
        }
    }
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            pos: 0,
        }
    }
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.bytes.get(self.pos).copied()
    }
    fn parse_val(&mut self) -> Result<JsonVal, String> {
        match self.peek() {
            Some(b'{') => self.parse_obj(),
            Some(b'[') => self.parse_arr(),
            Some(b'"') => self.parse_str().map(JsonVal::Str),
            Some(b't') => {
                if self.bytes[self.pos..].starts_with(b"true") {
                    self.pos += 4;
                    Ok(JsonVal::Bool(true))
                } else {
                    Err("Invalid literal".into())
                }
            }
            Some(b'f') => {
                if self.bytes[self.pos..].starts_with(b"false") {
                    self.pos += 5;
                    Ok(JsonVal::Bool(false))
                } else {
                    Err("Invalid literal".into())
                }
            }
            Some(b'n') => {
                if self.bytes[self.pos..].starts_with(b"null") {
                    self.pos += 4;
                    Ok(JsonVal::Null)
                } else {
                    Err("Invalid literal".into())
                }
            }
            Some(b) if b == b'-' || b.is_ascii_digit() => self.parse_num(),
            _ => Err("Unexpected token in JSON".into()),
        }
    }
    fn parse_str(&mut self) -> Result<String, String> {
        self.skip_ws();
        if self.bytes.get(self.pos) != Some(&b'"') {
            return Err("Expected quote".into());
        }
        self.pos += 1;
        let mut out = String::new();
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            self.pos += 1;
            if b == b'"' {
                return Ok(out);
            }
            if b == b'\\' {
                if self.pos >= self.bytes.len() {
                    return Err("Incomplete escape".into());
                }
                let esc = self.bytes[self.pos];
                self.pos += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        if self.pos + 4 <= self.bytes.len() {
                            if let Ok(hex) = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4]) {
                                if let Ok(cp) = u16::from_str_radix(hex, 16) {
                                    if let Some(ch) = char::from_u32(cp as u32) {
                                        out.push(ch);
                                    }
                                }
                            }
                            self.pos += 4;
                        }
                    }
                    _ => out.push(esc as char),
                }
            } else {
                let start = self.pos - 1;
                while self.pos < self.bytes.len() && (self.bytes[self.pos] & 0xC0) == 0x80 {
                    self.pos += 1;
                }
                if let Ok(s) = std::str::from_utf8(&self.bytes[start..self.pos]) {
                    out.push_str(s);
                }
            }
        }
        Err("Unterminated string".into())
    }
    fn parse_num(&mut self) -> Result<JsonVal, String> {
        let start = self.pos;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E') {
                self.pos += 1;
            } else {
                break;
            }
        }
        let s = std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| "UTF-8")?;
        let n: f64 = s.parse().map_err(|_| "Invalid number")?;
        Ok(JsonVal::Num(n))
    }
    fn parse_arr(&mut self) -> Result<JsonVal, String> {
        self.pos += 1;
        let mut items = Vec::new();
        loop {
            if self.peek() == Some(b']') {
                self.pos += 1;
                return Ok(JsonVal::Arr(items));
            }
            items.push(self.parse_val()?);
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(JsonVal::Arr(items));
                }
                _ => return Err("Expected , or ]".into()),
            }
        }
    }
    fn parse_obj(&mut self) -> Result<JsonVal, String> {
        self.pos += 1;
        let mut map = HashMap::new();
        loop {
            if self.peek() == Some(b'}') {
                self.pos += 1;
                return Ok(JsonVal::Obj(map));
            }
            let key = self.parse_str()?;
            if self.peek() != Some(b':') {
                return Err("Expected :".into());
            }
            self.pos += 1;
            let val = self.parse_val()?;
            map.insert(key, val);
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(JsonVal::Obj(map));
                }
                _ => return Err("Expected , or }".into()),
            }
        }
    }
}

fn parse_json(s: &str) -> Result<JsonVal, String> {
    JsonParser::new(s).parse_val()
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_rgb(rgb: [u8; 3]) -> String {
    format!("[{},{},{}]", rgb[0], rgb[1], rgb[2])
}

// ============================================================================
// WebSocket Video Broadcaster (Zero-Backpressure, 1 Latest Slot per tile_id)
// ============================================================================

struct WsClientSlot {
    filter_tile_id: Option<u32>,
    slots: Mutex<HashMap<u32, Vec<u8>>>,
    wakeup: Condvar,
    alive: AtomicBool,
}

impl WsClientSlot {
    fn new(filter_tile_id: Option<u32>) -> Arc<Self> {
        Arc::new(Self {
            filter_tile_id,
            slots: Mutex::new(HashMap::new()),
            wakeup: Condvar::new(),
            alive: AtomicBool::new(true),
        })
    }

    fn push_packet(&self, tile_id: u32, packet: &[u8]) {
        if let Some(filter) = self.filter_tile_id {
            if filter != tile_id {
                return;
            }
        }
        if let Ok(mut guard) = self.slots.lock() {
            guard.insert(tile_id, packet.to_vec());
            self.wakeup.notify_one();
        }
    }
}

pub struct VideoBroadcaster {
    clients: Mutex<Vec<Arc<WsClientSlot>>>,
    client_count: AtomicUsize,
}

static GLOBAL_BROADCASTER: OnceLock<Arc<VideoBroadcaster>> = OnceLock::new();

fn broadcaster() -> &'static Arc<VideoBroadcaster> {
    GLOBAL_BROADCASTER.get_or_init(|| {
        Arc::new(VideoBroadcaster {
            clients: Mutex::new(Vec::new()),
            client_count: AtomicUsize::new(0),
        })
    })
}

pub fn has_ws_video_clients() -> bool {
    broadcaster().client_count.load(Ordering::Relaxed) > 0
}

pub fn ws_video_client_count() -> usize {
    broadcaster().client_count.load(Ordering::Relaxed)
}

/// Publica um frame JPEG para todos os clientes WebSocket conectados (Zero-Backpressure).
/// Formato binário: [tile_id: u32 LE][width: u16 LE][height: u16 LE][jpeg_bytes...]
pub fn publish_video_frame(tile_id: u32, width: u16, height: u16, jpeg_bytes: &[u8]) {
    let b = broadcaster();
    if b.client_count.load(Ordering::Relaxed) == 0 || jpeg_bytes.is_empty() {
        return;
    }

    let mut packet = Vec::with_capacity(8 + jpeg_bytes.len());
    packet.extend_from_slice(&tile_id.to_le_bytes());
    packet.extend_from_slice(&width.to_le_bytes());
    packet.extend_from_slice(&height.to_le_bytes());
    packet.extend_from_slice(jpeg_bytes);

    if let Ok(mut clients) = b.clients.lock() {
        clients.retain(|c| c.alive.load(Ordering::Relaxed));
        b.client_count.store(clients.len(), Ordering::Relaxed);
        for client in clients.iter() {
            client.push_packet(tile_id, &packet);
        }
    }
}

fn write_ws_binary_frame(stream: &mut TcpStream, payload: &[u8]) -> std::io::Result<()> {
    let len = payload.len();
    let mut header = [0u8; 10];
    let header_len = if len < 126 {
        header[0] = 0x82;
        header[1] = len as u8;
        2
    } else if len <= 65535 {
        header[0] = 0x82;
        header[1] = 126;
        header[2..4].copy_from_slice(&(len as u16).to_be_bytes());
        4
    } else {
        header[0] = 0x82;
        header[1] = 127;
        header[2..10].copy_from_slice(&(len as u64).to_be_bytes());
        10
    };
    stream.write_all(&header[..header_len])?;
    stream.write_all(payload)?;
    Ok(())
}

fn serve_websocket_client(mut stream: TcpStream, key: &str, filter_tile_id: Option<u32>) {
    let mut accept_input = String::with_capacity(key.len() + 36);
    accept_input.push_str(key.trim());
    accept_input.push_str("258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let accept_key = base64_encode(&sha1_digest(accept_input.as_bytes()));

    let handshake = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept_key
    );
    if stream.write_all(handshake.as_bytes()).is_err() {
        return;
    }
    let _ = stream.set_nodelay(true);

    let slot = WsClientSlot::new(filter_tile_id);
    let b = broadcaster();
    if let Ok(mut clients) = b.clients.lock() {
        clients.push(Arc::clone(&slot));
        b.client_count.store(clients.len(), Ordering::Relaxed);
    }

    if let Ok(mut reader_stream) = stream.try_clone() {
        let slot_reader = Arc::clone(&slot);
        thread::spawn(move || {
            let mut buf = [0u8; 512];
            while slot_reader.alive.load(Ordering::Relaxed) {
                match reader_stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if n >= 2 && (buf[0] & 0x0F) == 0x8 {
                            break;
                        }
                    }
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(_) => break,
                }
            }
            slot_reader.alive.store(false, Ordering::Relaxed);
            slot_reader.wakeup.notify_all();
        });
    }

    while slot.alive.load(Ordering::Relaxed) {
        let pending: Vec<Vec<u8>> = {
            let mut guard = match slot.slots.lock() {
                Ok(g) => g,
                Err(_) => break,
            };
            if guard.is_empty() {
                guard = match slot.wakeup.wait_timeout(guard, Duration::from_millis(250)) {
                    Ok((g, _)) => g,
                    Err(_) => break,
                };
            }
            guard.drain().map(|(_, v)| v).collect()
        };

        for pkt in pending {
            if write_ws_binary_frame(&mut stream, &pkt).is_err() {
                slot.alive.store(false, Ordering::Relaxed);
                break;
            }
        }
    }

    slot.alive.store(false, Ordering::Relaxed);
    if let Ok(mut clients) = b.clients.lock() {
        clients.retain(|c| c.alive.load(Ordering::Relaxed));
        b.client_count.store(clients.len(), Ordering::Relaxed);
    }
}

// ============================================================================
// Static Asset Resolver for SZweb/dist (Production Single-Process Serving)
// ============================================================================

fn find_szweb_dist_dir() -> Option<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("SZweb/dist"),
        PathBuf::from("../SZweb/dist"),
        PathBuf::from("../../SZweb/dist"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("SZweb/dist"));
            candidates.push(dir.join("../../SZweb/dist"));
            candidates.push(dir.join("../../../SZweb/dist"));
        }
    }
    candidates.into_iter().find(|p| p.join("index.html").is_file())
}

fn content_type_for_path(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "json" => "application/json; charset=utf-8",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn try_serve_static(stream: &mut TcpStream, req_path: &str, dist_dir: Option<&Path>) {
    let clean_path = req_path.split('?').next().unwrap_or("/");
    let Some(root) = dist_dir else {
        let html = "<html><body style='background:#111214;color:#f2f3f5;font-family:sans-serif;padding:32px'><h2>SZdisc Backend Ativo (:47839)</h2><p>Rode <code>cd SZweb && npm run build</code> para servir o frontend estático aqui, ou acesse <code>http://localhost:5173</code> em modo dev.</p></body></html>";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            html.len(),
            html
        );
        let _ = stream.write_all(resp.as_bytes());
        return;
    };

    let rel = clean_path.trim_start_matches('/');
    let mut target = if rel.is_empty() {
        root.join("index.html")
    } else {
        let candidate = root.join(rel);
        if candidate.is_file() && candidate.starts_with(root) {
            candidate
        } else {
            root.join("index.html")
        }
    };

    if !target.is_file() {
        target = root.join("index.html");
    }

    match fs::read(&target) {
        Ok(bytes) => {
            let ct = content_type_for_path(&target);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                ct,
                bytes.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&bytes);
        }
        Err(_) => {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    }
}

// ============================================================================
// Bridge HTTP RPC & Snapshot Server (Headless AppCore)
// ============================================================================

pub struct BridgeServer {
    app: Arc<Mutex<AppCore>>,
    latest_snapshot_json: Mutex<String>,
    dist_dir: Option<PathBuf>,
}

impl BridgeServer {
    pub fn start(app: Arc<Mutex<AppCore>>) -> Arc<Self> {
        let initial_json = app
            .lock()
            .map(|a| a.build_bridge_snapshot_json())
            .unwrap_or_else(|_| "{}".to_string());

        let server = Arc::new(Self {
            app: Arc::clone(&app),
            latest_snapshot_json: Mutex::new(initial_json),
            dist_dir: find_szweb_dist_dir(),
        });

        let srv_listener = Arc::clone(&server);
        thread::spawn(move || {
            let listener = match TcpListener::bind(("127.0.0.1", BRIDGE_PORT)) {
                Ok(l) => l,
                Err(_) => return,
            };
            for incoming in listener.incoming().flatten() {
                let srv = Arc::clone(&srv_listener);
                thread::spawn(move || handle_http_connection(incoming, srv));
            }
        });

        // Tick periódico (200ms): sincroniza RoomHub, limpa pop-outs órfãos e calcula telemetria 1s
        let srv_ticker = Arc::clone(&server);
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(200));
            if let Ok(mut guard) = srv_ticker.app.lock() {
                guard.tick();
                let json = guard.build_bridge_snapshot_json();
                srv_ticker.update_snapshot(json);
            }
        });

        server
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

fn handle_http_connection(mut stream: TcpStream, bridge: Arc<BridgeServer>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 2048];

    let header_end = loop {
        match stream.read(&mut tmp) {
            Ok(0) => return,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos;
                }
                if buf.len() > 65536 {
                    return;
                }
            }
            Err(_) => return,
        }
    };

    let header_str = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = header_str.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    let mut content_len: usize = 0;
    let mut ws_key: Option<String> = None;
    let mut is_upgrade = false;

    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let kl = k.trim().to_ascii_lowercase();
            let vt = v.trim();
            if kl == "content-length" {
                content_len = vt.parse().unwrap_or(0);
            } else if kl == "sec-websocket-key" {
                ws_key = Some(vt.to_string());
            } else if kl == "upgrade" && vt.eq_ignore_ascii_case("websocket") {
                is_upgrade = true;
            }
        }
    }

    if is_upgrade && path.starts_with("/ws") {
        if let Some(key) = ws_key {
            let filter_tile_id = path
                .split_once("tileId=")
                .and_then(|(_, rest)| rest.split('&').next())
                .and_then(|num| num.parse::<u32>().ok());
            serve_websocket_client(stream, &key, filter_tile_id);
        }
        return;
    }

    if method == "OPTIONS" {
        let resp = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: 0\r\n\r\n";
        let _ = stream.write_all(resp.as_bytes());
        return;
    }

    if method == "GET" && !path.starts_with("/api/") {
        try_serve_static(&mut stream, path, bridge.dist_dir.as_deref());
        return;
    }

    let body_start = header_end + 4;
    while buf.len() < body_start + content_len {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => break,
        }
    }
    let body_end = (body_start + content_len).min(buf.len());
    let body_str = String::from_utf8_lossy(&buf[body_start..body_end]).to_string();

    let (status, json_body) = if method == "GET" && path == "/api/state" {
        ("200 OK", bridge.get_snapshot_json())
    } else if method == "POST" && path == "/api/rpc" {
        match handle_rpc_request(&bridge, &body_str) {
            Ok(res_json) => ("200 OK", format!(r#"{{"ok":true,"data":{}}}"#, res_json)),
            Err(err_msg) => (
                "400 Bad Request",
                format!(r#"{{"ok":false,"error":{}}}"#, json_str(&err_msg)),
            ),
        }
    } else {
        ("404 Not Found", r#"{"ok":false,"error":"Not found"}"#.to_string())
    };

    let http_resp = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        json_body.len(),
        json_body
    );
    let _ = stream.write_all(http_resp.as_bytes());
}

fn handle_rpc_request(bridge: &BridgeServer, body: &str) -> Result<String, String> {
    let val = parse_json(body)?;
    let method = val
        .get_str("method")
        .ok_or_else(|| "Campo 'method' obrigatório".to_string())?;
    let params = val.get("params").cloned().unwrap_or(JsonVal::Null);

    match method.as_str() {
        "get_state" => Ok(bridge.get_snapshot_json()),
        "list_capture_sources" => Ok(capture_sources_json()),
        "get_source_thumbnails" => Ok(capture_thumbnails_json()),
        _ => {
            let mut app = bridge
                .app
                .lock()
                .map_err(|_| "Falha ao travar estado do AppCore".to_string())?;
            let res = app.execute_bridge_command(&method, &params)?;
            let snap = app.build_bridge_snapshot_json();
            bridge.update_snapshot(snap);
            Ok(res)
        }
    }
}

fn category_slug(cat: SourceCategory) -> &'static str {
    match cat {
        SourceCategory::Application => "Application",
        SourceCategory::Browser => "Browser",
        SourceCategory::Screen => "Screen",
    }
}

fn capture_sources_json() -> String {
    let sources = list_capture_sources();
    let items: Vec<String> = sources
        .iter()
        .map(|s| {
            format!(
                r#"{{"id":{},"name":{},"subtitle":{},"exeName":{},"pid":{},"category":{}}}"#,
                json_str(&s.id),
                json_str(&s.name),
                json_str(&s.subtitle),
                json_str(&s.exe_name),
                s.pid,
                json_str(category_slug(s.category)),
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn capture_thumbnails_json() -> String {
    let sources = list_capture_sources();
    let mut entries = Vec::new();
    let mut rgb_buf = Vec::new();
    let mut jpeg_buf = Vec::new();

    for src in &sources {
        if let Some((w, h, rgba)) = capture_thumbnail_rgba(src, 272, 153) {
            rgb_buf.clear();
            rgb_buf.reserve((w * h * 3) as usize);
            for px in rgba.chunks_exact(4) {
                rgb_buf.push(px[0]);
                rgb_buf.push(px[1]);
                rgb_buf.push(px[2]);
            }
            jpeg_buf.clear();
            let mut enc = Encoder::new(&mut jpeg_buf, 70);
            enc.set_sampling_factor(SamplingFactor::F_2_2);
            if enc.encode(&rgb_buf, w as u16, h as u16, ColorType::Rgb).is_ok() {
                let data_url = format!("data:image/jpeg;base64,{}", base64_encode(&jpeg_buf));
                entries.push(format!("{}:{}", json_str(&src.id), json_str(&data_url)));
            }
        }
    }
    format!("{{{}}}", entries.join(","))
}

// ============================================================================
// AppCore RPC Command Execution & Snapshot Serialization
// ============================================================================

impl AppCore {
    pub(crate) fn execute_bridge_command(
        &mut self,
        method: &str,
        params: &JsonVal,
    ) -> Result<String, String> {
        match method {
            "complete_onboarding" => {
                let display_name = params
                    .get_str("displayName")
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if display_name.is_empty() {
                    return Err("Nome de exibição é obrigatório".into());
                }
                self.custom.display_name = display_name.clone();
                let login = params
                    .get_str("usernameLogin")
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                self.custom.username_login = if login.is_empty() {
                    display_name.to_lowercase().replace(' ', "_")
                } else {
                    login
                };
                if let Some(tag) = params.get_str("userTag") {
                    if !tag.trim().is_empty() {
                        self.custom.user_tag = tag.trim().to_string();
                    }
                }
                if let Some(status) = params.get_str("customStatus") {
                    self.custom.custom_status = status;
                }
                if let Some(rgb) = params.get_rgb("nameColorRgb") {
                    self.custom.name_color_rgb = rgb;
                }
                if let Some(rgb) = params.get_rgb("avatarBg") {
                    self.custom.avatar_bg = rgb;
                }
                self.custom.account_created = true;
                self.custom.save_to_disk();
                self.sync_live_config();
                self.show_toast(format!("Bem-vindo(a), {}!", self.custom.display_name));
                Ok(self.build_bridge_snapshot_json())
            }
            "pick_profile_image" => {
                let kind = params.get_str("kind").unwrap_or_else(|| "avatar".to_string());
                if let Some(path) = pick_image_file_dialog() {
                    if kind == "banner" {
                        if let Some(data_url) = load_image_as_jpeg_data_url(&path, 480, 120) {
                            self.custom.banner_image_path = path;
                            self.banner_data_url = data_url;
                            self.custom.save_to_disk();
                        }
                    } else if let Some(data_url) = load_image_as_jpeg_data_url(&path, 128, 128) {
                        self.custom.avatar_image_path = path;
                        self.avatar_data_url = data_url;
                        self.custom.save_to_disk();
                    }
                }
                Ok(self.build_bridge_snapshot_json())
            }
            "clear_profile_image" => {
                let kind = params.get_str("kind").unwrap_or_else(|| "avatar".to_string());
                if kind == "banner" {
                    self.custom.banner_image_path.clear();
                    self.banner_data_url.clear();
                } else {
                    self.custom.avatar_image_path.clear();
                    self.avatar_data_url.clear();
                }
                self.custom.save_to_disk();
                Ok(self.build_bridge_snapshot_json())
            }
            "save_profile" => {
                if let Some(v) = params.get_str("displayName") {
                    self.custom.display_name = v;
                }
                if let Some(v) = params.get_str("userTag") {
                    self.custom.user_tag = v;
                }
                if let Some(v) = params.get_str("customStatus") {
                    self.custom.custom_status = v;
                }
                if let Some(v) = params.get_str("bio") {
                    self.custom.bio = v;
                }
                if let Some(code) = params.get_u8("presenceCode") {
                    self.custom.presence = PresenceStatus::from_code(code);
                }
                if let Some(rgb) = params.get_rgb("nameColorRgb") {
                    self.custom.name_color_rgb = rgb;
                }
                if let Some(rgb) = params.get_rgb("avatarBg") {
                    self.custom.avatar_bg = rgb;
                }
                if let Some(rgb) = params.get_rgb("bannerBg") {
                    self.custom.banner_bg = rgb;
                }
                if let Some(rgb) = params.get_rgb("bannerBg2") {
                    self.custom.banner_bg_2 = rgb;
                }
                if let Some(b) = params.get_bool("avatarRingGlow") {
                    self.custom.avatar_ring_glow = b;
                }
                if let Some(rgb) = params.get_rgb("ringColorRgb") {
                    self.custom.ring_color_rgb = rgb;
                }
                if let Some(t) = params.get_f32("ringThickness") {
                    self.custom.ring_thickness = t.clamp(0.5, 4.5);
                }
                if let Some(b) = params.get_bool("badgeCrown") {
                    self.custom.badge_crown = b;
                }
                if let Some(b) = params.get_bool("badge1080p") {
                    self.custom.badge_1080p = b;
                }
                if let Some(b) = params.get_bool("badgeRust") {
                    self.custom.badge_rust = b;
                }
                if let Some(b) = params.get_bool("badgeGamer") {
                    self.custom.badge_gamer = b;
                }
                if let Some(b) = params.get_bool("customBadgeEnabled") {
                    self.custom.custom_badge_enabled = b;
                }
                if let Some(v) = params.get_str("customBadgeText") {
                    self.custom.custom_badge_text = v;
                }
                if let Some(rgb) = params.get_rgb("customBadgeRgb") {
                    self.custom.custom_badge_rgb = rgb;
                }
                self.custom.save_to_disk();
                self.sync_live_config();
                Ok(self.build_bridge_snapshot_json())
            }
            "logout" => {
                self.leave_room();
                self.custom.account_created = false;
                self.custom.save_to_disk();
                Ok(self.build_bridge_snapshot_json())
            }
            "apply_theme_preset" => {
                let code = params.get_u8("presetCode").unwrap_or(0);
                self.custom.apply_preset_colors(ThemePreset::from_code(code));
                self.custom.save_to_disk();
                Ok(self.build_bridge_snapshot_json())
            }
            "save_appearance" => {
                if let Some(rgb) = params.get_rgb("accentRgb") {
                    self.custom.accent_rgb = rgb;
                    self.custom.theme_preset = ThemePreset::Custom;
                }
                if let Some(rgb) = params.get_rgb("bgSidebarRgb") {
                    self.custom.bg_sidebar_rgb = rgb;
                    self.custom.theme_preset = ThemePreset::Custom;
                }
                if let Some(rgb) = params.get_rgb("bgStageRgb") {
                    self.custom.bg_stage_rgb = rgb;
                    self.custom.theme_preset = ThemePreset::Custom;
                }
                if let Some(rgb) = params.get_rgb("bgCardRgb") {
                    self.custom.bg_card_rgb = rgb;
                    self.custom.theme_preset = ThemePreset::Custom;
                }
                if let Some(rgb) = params.get_rgb("textRgb") {
                    self.custom.text_rgb = rgb;
                    self.custom.theme_preset = ThemePreset::Custom;
                }
                if let Some(r) = params.get_f32("cornerRadius") {
                    self.custom.corner_radius = r.clamp(0.0, 16.0);
                }
                if let Some(w) = params.get_f32("borderStrokeWidth") {
                    self.custom.border_stroke_width = w.clamp(0.5, 3.0);
                }
                if let Some(s) = params.get_f32("uiScale") {
                    self.custom.ui_scale = s.clamp(0.90, 1.15);
                }
                self.custom.save_to_disk();
                Ok(self.build_bridge_snapshot_json())
            }
            "create_server" => {
                let name = params.get_str("name").unwrap_or_default().trim().to_string();
                if name.is_empty() {
                    return Err("Nome do servidor vazio".into());
                }
                let icon_rgb = params.get_rgb("iconRgb").unwrap_or([88, 101, 242]);
                let mut rooms: Vec<String> = params
                    .get_str_vec("rooms")
                    .unwrap_or_default()
                    .into_iter()
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty())
                    .collect();
                if rooms.is_empty() {
                    rooms.push("Sala Geral".to_string());
                }
                let srv_id = generate_server_id(&name);
                let new_srv = ServerProfile {
                    server_id: srv_id.clone(),
                    name: name.clone(),
                    icon_rgb,
                    rooms,
                };
                if let Some(hub) = &self.room_hub {
                    hub.register_and_select_server(&new_srv);
                }
                self.custom.selected_server_id = srv_id;
                self.custom.saved_servers.push(new_srv);
                self.custom.save_to_disk();
                self.show_toast(format!("Servidor '{}' criado!", name));
                Ok(self.build_bridge_snapshot_json())
            }
            "join_server" => {
                let raw = params.get_str("invite").unwrap_or_default();
                let new_srv = ServerProfile::from_invite_input(&raw)
                    .ok_or_else(|| "Convite ou código inválido".to_string())?;
                let srv_id = new_srv.server_id.clone();
                let srv_name = new_srv.name.clone();

                if let Some(existing) = self
                    .custom
                    .saved_servers
                    .iter_mut()
                    .find(|s| s.server_id.eq_ignore_ascii_case(&srv_id))
                {
                    for r in &new_srv.rooms {
                        if !existing.rooms.iter().any(|x| x.eq_ignore_ascii_case(r)) {
                            existing.rooms.push(r.clone());
                        }
                    }
                    if !new_srv.name.starts_with("Servidor SZ-") {
                        existing.name = new_srv.name.clone();
                    }
                    if let Some(hub) = &self.room_hub {
                        hub.register_and_select_server(existing);
                    }
                } else {
                    if let Some(hub) = &self.room_hub {
                        hub.register_and_select_server(&new_srv);
                    }
                    self.custom.saved_servers.push(new_srv);
                }
                self.custom.selected_server_id = srv_id;
                self.custom.save_to_disk();
                self.show_toast(format!("Você entrou no servidor '{}'!", srv_name));
                Ok(self.build_bridge_snapshot_json())
            }
            "select_server" => {
                let server_id = params.get_str("serverId").unwrap_or_default();
                if let Some(chosen) = self
                    .custom
                    .saved_servers
                    .iter()
                    .find(|s| s.server_id.eq_ignore_ascii_case(&server_id))
                    .cloned()
                {
                    self.custom.selected_server_id = chosen.server_id.clone();
                    self.custom.save_to_disk();
                    if let Some(hub) = &self.room_hub {
                        hub.register_and_select_server(&chosen);
                    }
                }
                Ok(self.build_bridge_snapshot_json())
            }
            "remove_server" => {
                let server_id = params.get_str("serverId").unwrap_or_default();
                if let Some(hub) = &self.room_hub {
                    hub.remove_server(&server_id);
                }
                self.custom
                    .saved_servers
                    .retain(|s| !s.server_id.eq_ignore_ascii_case(&server_id));
                self.custom.selected_server_id = self
                    .custom
                    .saved_servers
                    .first()
                    .map(|s| s.server_id.clone())
                    .unwrap_or_default();
                self.custom.save_to_disk();
                Ok(self.build_bridge_snapshot_json())
            }
            "create_room" => {
                let server_id = params.get_str("serverId").unwrap_or_default();
                let room_name = params
                    .get_str("roomName")
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if room_name.is_empty() {
                    return Err("Nome da sala vazio".into());
                }
                if let Some(hub) = &self.room_hub {
                    let updated = hub.create_and_enter_room(&server_id, &room_name);
                    if let Some(srv_mut) = self
                        .custom
                        .saved_servers
                        .iter_mut()
                        .find(|s| s.server_id.eq_ignore_ascii_case(&server_id))
                    {
                        srv_mut.rooms = updated;
                    }
                    self.custom.save_to_disk();
                }
                self.show_toast(format!("Sala '{}' criada!", room_name));
                Ok(self.build_bridge_snapshot_json())
            }
            "delete_room" => {
                let server_id = params.get_str("serverId").unwrap_or_default();
                let room_name = params.get_str("roomName").unwrap_or_default();
                if let Some(srv_mut) = self
                    .custom
                    .saved_servers
                    .iter_mut()
                    .find(|s| s.server_id.eq_ignore_ascii_case(&server_id))
                {
                    if srv_mut.rooms.len() > 1 {
                        srv_mut.rooms.retain(|r| !r.eq_ignore_ascii_case(&room_name));
                        if let Some(hub) = &self.room_hub {
                            if let Ok(mut map) = hub.server_rooms_map.lock() {
                                map.insert(srv_mut.server_id.to_uppercase(), srv_mut.rooms.clone());
                            }
                        }
                        self.custom.save_to_disk();
                    }
                }
                Ok(self.build_bridge_snapshot_json())
            }
            "enter_room" => {
                let server_id = params.get_str("serverId").unwrap_or_default();
                let room_name = params.get_str("roomName").unwrap_or_default();
                if let Some(hub) = &self.room_hub {
                    hub.enter_server_room(&server_id, &room_name);
                }
                self.show_toast(format!("Conectado em '{}'", room_name));
                Ok(self.build_bridge_snapshot_json())
            }
            "leave_room" => {
                self.leave_room();
                Ok(self.build_bridge_snapshot_json())
            }
            "start_sharing" => {
                let source_id = params.get_str("sourceId").unwrap_or_default();
                self.sources = list_capture_sources();
                if let Some(idx) = self.sources.iter().position(|s| s.id == source_id) {
                    self.selected_source_idx = idx;
                }
                self.start_or_switch_sharing();
                Ok(self.build_bridge_snapshot_json())
            }
            "stop_sharing" => {
                self.stop_sharing();
                Ok(self.build_bridge_snapshot_json())
            }
            "update_stream_settings" => {
                if let Some(res) = params.get_str("resolution") {
                    self.res_preset = match res.as_str() {
                        "1080p" => ResolutionPreset::P1080,
                        "900p" => ResolutionPreset::P900,
                        "720p" => ResolutionPreset::P720,
                        "480p" => ResolutionPreset::P480,
                        "360p" => ResolutionPreset::P360,
                        _ => self.res_preset,
                    };
                }
                if let Some(fps) = params.get_u8("fps") {
                    if matches!(fps, 15 | 30 | 60) {
                        self.fps_preset = fps;
                    }
                }
                if let Some(q) = params.get_u8("jpegQuality") {
                    self.jpeg_quality = q.clamp(40, 95);
                }
                if let Some(b) = params.get_bool("shareAudio") {
                    self.share_audio_enabled = b;
                }
                if let Some(b) = params.get_bool("ecoHidePreview") {
                    self.eco_hide_preview = b;
                }
                if let Some(b) = params.get_bool("autoGameEco") {
                    self.auto_game_eco = b;
                }
                if let Some(b) = params.get_bool("showCursor") {
                    self.custom.show_cursor_in_stream = b;
                    self.custom.save_to_disk();
                }
                if let Some(b) = params.get_bool("showOwnPreviewInGrid") {
                    self.show_own_preview_in_grid = b;
                }
                self.sync_live_config();
                Ok(self.build_bridge_snapshot_json())
            }
            "activate_turbo_mode" => {
                self.res_preset = ResolutionPreset::P720;
                self.fps_preset = 60;
                self.jpeg_quality = 65;
                self.auto_game_eco = true;
                self.sync_live_config();
                self.show_toast("⚡ Modo Turbo Jogos ativado!");
                Ok(self.build_bridge_snapshot_json())
            }
            "set_peer_audio" => {
                let peer_id = params
                    .get_u32("peerId")
                    .ok_or_else(|| "peerId obrigatório".to_string())?;
                if let Some(hub) = &self.room_hub {
                    if let Ok(streams) = hub.remote_streams.lock() {
                        if let Some(stream) = streams.get(&peer_id) {
                            if let Some(vol) = params.get_u32("volumePercent") {
                                stream.volume_percent.store(vol.min(150), Ordering::Relaxed);
                            }
                            if let Some(muted) = params.get_bool("muted") {
                                stream.muted.store(muted, Ordering::Relaxed);
                            }
                        }
                    }
                }
                Ok(self.build_bridge_snapshot_json())
            }
            "set_popout" => {
                if params.get_bool("clearAll") == Some(true) {
                    self.popout_tiles.clear();
                } else if let Some(tile_id) = params.get_u32("tileId") {
                    let popout = params.get_bool("popout").unwrap_or(true);
                    if popout {
                        if self.focused_tile_id == Some(tile_id) {
                            self.focused_tile_id = None;
                        }
                        self.popout_tiles.insert(tile_id);
                    } else {
                        self.popout_tiles.remove(&tile_id);
                    }
                }
                Ok(self.build_bridge_snapshot_json())
            }
            "show_toast" => {
                let msg = params
                    .get_str("message")
                    .unwrap_or_else(|| "Notificação".to_string());
                self.show_toast(msg);
                Ok(self.build_bridge_snapshot_json())
            }
            "check_updates" => {
                check_for_updates_async(Arc::clone(&self.update_status));
                Ok(self.build_bridge_snapshot_json())
            }
            "apply_update" => {
                download_and_apply_update_async(Arc::clone(&self.update_status));
                Ok(self.build_bridge_snapshot_json())
            }
            _ => Err(format!("Método RPC desconhecido: {}", method)),
        }
    }

    pub(crate) fn build_bridge_snapshot_json(&self) -> String {
        let in_room = self
            .room_hub
            .as_ref()
            .map(|h| h.in_room.load(Ordering::Relaxed))
            .unwrap_or(false);
        let connected_srv_id = self
            .room_hub
            .as_ref()
            .and_then(|h| h.connected_server_id.lock().ok().map(|g| g.clone()))
            .unwrap_or_default();
        let active_room_name = self
            .room_hub
            .as_ref()
            .and_then(|h| h.active_room_name.lock().ok().map(|g| g.clone()))
            .unwrap_or_default();

        let servers_json: Vec<String> = self
            .custom
            .saved_servers
            .iter()
            .map(|srv| {
                let rooms_json: Vec<String> = srv.rooms.iter().map(|r| json_str(r)).collect();
                format!(
                    r#"{{"serverId":{},"name":{},"iconRgb":{},"rooms":[{}],"inviteToken":{}}}"#,
                    json_str(&srv.server_id),
                    json_str(&srv.name),
                    json_rgb(srv.icon_rgb),
                    rooms_json.join(","),
                    json_str(&srv.invite_token()),
                )
            })
            .collect();

        let custom_json = format!(
            concat!(
                r#"{{"accountCreated":{},"usernameLogin":{},"displayName":{},"userTag":{},"#,
                r#""customStatus":{},"bio":{},"presenceCode":{},"#,
                r#""avatarDataUrl":{},"bannerDataUrl":{},"#,
                r#""avatarBg":{},"bannerBg":{},"bannerBg2":{},"nameColorRgb":{},"ringColorRgb":{},"#,
                r#""avatarRingGlow":{},"ringThickness":{:.2},"#,
                r#""badge1080p":{},"badgeRust":{},"badgeGamer":{},"badgeCrown":{},"#,
                r#""customBadgeEnabled":{},"customBadgeText":{},"customBadgeRgb":{},"#,
                r#""themePresetCode":{},"accentRgb":{},"bgSidebarRgb":{},"bgStageRgb":{},"bgCardRgb":{},"textRgb":{},"#,
                r#""cornerRadius":{:.1},"borderStrokeWidth":{:.2},"uiScale":{:.2},"#,
                r#""showCursorInStream":{},"selectedServerId":{},"savedServers":[{}]}}"#
            ),
            self.custom.account_created,
            json_str(&self.custom.username_login),
            json_str(&self.custom.display_name),
            json_str(&self.custom.user_tag),
            json_str(&self.custom.custom_status),
            json_str(&self.custom.bio),
            self.custom.presence.code(),
            json_str(&self.avatar_data_url),
            json_str(&self.banner_data_url),
            json_rgb(self.custom.avatar_bg),
            json_rgb(self.custom.banner_bg),
            json_rgb(self.custom.banner_bg_2),
            json_rgb(self.custom.name_color_rgb),
            json_rgb(self.custom.ring_color_rgb),
            self.custom.avatar_ring_glow,
            self.custom.ring_thickness,
            self.custom.badge_1080p,
            self.custom.badge_rust,
            self.custom.badge_gamer,
            self.custom.badge_crown,
            self.custom.custom_badge_enabled,
            json_str(&self.custom.custom_badge_text),
            json_rgb(self.custom.custom_badge_rgb),
            self.custom.theme_preset.code(),
            json_rgb(self.custom.accent_rgb),
            json_rgb(self.custom.bg_sidebar_rgb),
            json_rgb(self.custom.bg_stage_rgb),
            json_rgb(self.custom.bg_card_rgb),
            json_rgb(self.custom.text_rgb),
            self.custom.corner_radius,
            self.custom.border_stroke_width,
            self.custom.ui_scale,
            self.custom.show_cursor_in_stream,
            json_str(&self.custom.selected_server_id),
            servers_json.join(","),
        );

        let peers_json: Vec<String> = self
            .room_hub
            .as_ref()
            .and_then(|h| h.peers.lock().ok())
            .map(|map| {
                map.values()
                    .map(|p| {
                        let (vol, muted, vw, vh, fps, mbps) = self
                            .room_hub
                            .as_ref()
                            .and_then(|h| {
                                h.remote_streams.lock().ok().and_then(|sm| {
                                    sm.get(&p.peer_id).map(|s| {
                                        (
                                            s.volume_percent.load(Ordering::Relaxed),
                                            s.muted.load(Ordering::Relaxed),
                                            s.video_width.load(Ordering::Relaxed),
                                            s.video_height.load(Ordering::Relaxed),
                                            s.measured_fps.load(Ordering::Relaxed),
                                            s.measured_mbps_x100.load(Ordering::Relaxed) as f32
                                                / 100.0,
                                        )
                                    })
                                })
                            })
                            .unwrap_or((100, false, 0, 0, 0, 0.0));
                        format!(
                            r#"{{"peerId":{},"name":{},"isSharing":{},"resLabel":{},"videoSize":[{},{}],"fps":{},"mbps":{:.2},"volumePercent":{},"muted":{}}}"#,
                            p.peer_id,
                            json_str(&p.name),
                            p.is_sharing,
                            json_str(&p.res_label),
                            vw,
                            vh,
                            fps,
                            mbps,
                            vol,
                            muted,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        let members_json: Vec<String> = self
            .room_hub
            .as_ref()
            .and_then(|h| h.server_members.lock().ok())
            .map(|map| {
                map.values()
                    .map(|m| {
                        format!(
                            r#"{{"peerId":{},"name":{},"serverId":{},"currentRoom":{},"isSharing":{},"resLabel":{}}}"#,
                            m.peer_id,
                            json_str(&m.name),
                            json_str(&m.server_id),
                            json_str(&m.current_room),
                            m.is_sharing,
                            json_str(&m.res_label),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        let room_json = format!(
            r#"{{"inRoom":{},"connectedServerId":{},"activeRoomName":{},"peers":[{}],"serverMembers":[{}]}}"#,
            in_room,
            json_str(&connected_srv_id),
            json_str(&active_room_name),
            peers_json.join(","),
            members_json.join(","),
        );

        let active_source_id = self
            .sources
            .get(self.selected_source_idx)
            .map(|s| s.id.as_str())
            .unwrap_or("");

        let stream_json = format!(
            r#"{{"isSharing":{},"resolution":{},"fps":{},"jpegQuality":{},"shareAudio":{},"ecoHidePreview":{},"autoGameEco":{},"showOwnPreviewInGrid":{},"activeSourceId":{}}}"#,
            self.is_sharing,
            json_str(self.res_preset.short_label()),
            self.fps_preset,
            self.jpeg_quality,
            self.share_audio_enabled,
            self.eco_hide_preview,
            self.auto_game_eco,
            self.show_own_preview_in_grid,
            json_str(active_source_id),
        );

        let lw = self.stream_config.local_video_w.load(Ordering::Relaxed);
        let lh = self.stream_config.local_video_h.load(Ordering::Relaxed);
        let telemetry_json = format!(
            r#"{{"ramMb":{:.1},"localFps":{},"localMbps":{:.2},"localVideoSize":[{},{}],"wsClients":{}}}"#,
            self.ram_mb,
            self.local_measured_fps,
            self.local_measured_mbps,
            lw,
            lh,
            ws_video_client_count(),
        );

        let popouts: Vec<String> = self.popout_tiles.iter().map(|id| id.to_string()).collect();

        let upd_str = match self.update_status.lock().ok().map(|g| g.clone()) {
            Some(UpdateStatus::Checking) => "Checking".to_string(),
            Some(UpdateStatus::UpToDate) => "UpToDate".to_string(),
            Some(UpdateStatus::UpdateAvailable(v)) => format!("UpdateAvailable:{}", v),
            Some(UpdateStatus::Downloading) => "Downloading".to_string(),
            Some(UpdateStatus::Failed) => "Failed".to_string(),
            _ => "Idle".to_string(),
        };

        let toast_str = self
            .status_toast
            .as_ref()
            .filter(|(_, when)| when.elapsed() < Duration::from_secs(4))
            .map(|(msg, _)| msg.as_str())
            .unwrap_or("");

        format!(
            r#"{{"version":{},"bridgePort":{},"custom":{},"room":{},"stream":{},"telemetry":{},"popoutTiles":[{}],"updateStatus":{},"activeToast":{}}}"#,
            json_str(CURRENT_VERSION),
            BRIDGE_PORT,
            custom_json,
            room_json,
            stream_json,
            telemetry_json,
            popouts.join(","),
            json_str(&upd_str),
            json_str(toast_str),
        )
    }
}
