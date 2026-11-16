use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::Context;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use crate::audio::StreamAudioPlayer;
use crate::profile_theme::ServerProfile;

pub const DEFAULT_STREAM_PORT: u16 = 47830;
const BORE_RELAY_HOST: &str = "bore.pub:7835";

const MAGIC_HELLO: &[u8; 4] = b"HLO6";
const MAGIC_INFO: &[u8; 4] = b"INF6";
const MAGIC_VIDEO: &[u8; 4] = b"VID6";
const MAGIC_AUDIO: &[u8; 4] = b"AUD6";
const MAGIC_STOP_STREAM: &[u8; 4] = b"STP6";
const MAGIC_LEFT_ROOM: &[u8; 4] = b"BYE6";

/// Representa um membro online dentro de um Servidor específico (mesmo em outra sala daquele servidor)
#[derive(Clone, Debug)]
pub struct ServerMemberPresence {
    pub peer_id: u32,
    pub name: String,
    pub server_id: String,
    pub current_room: String,
    pub is_sharing: bool,
    pub res_label: String,
    pub last_seen: Instant,
}

#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub peer_id: u32,
    pub name: String,
    pub is_sharing: bool,
    pub res_label: String,
    pub last_seen: Instant,
}

pub struct DecodedRemoteFrame {
    pub width: u32,
    pub height: u32,
    pub byte_len: usize,
    pub rgba: Vec<u8>,
}

struct RawCompressedVideo {
    width: u32,
    height: u32,
    jpeg_bytes: Vec<u8>,
}

/// Estado de recepção de vídeo e áudio de um participante específico na sala.
/// Possui uma thread decodificadora dedicada (Zero-Backpressure) para que a leitura do socket TCP
/// nunca trave esperando decodificação JPEG e sempre processe o quadro mais recente em tempo real!
pub struct RemotePeerStream {
    pub peer_id: u32,
    pub latest_frame: Mutex<Option<DecodedRemoteFrame>>,
    raw_video_slot: Mutex<Option<RawCompressedVideo>>,
    raw_video_wakeup: Condvar,
    stop_decoder: AtomicBool,
    pub volume_percent: Arc<AtomicU32>,
    pub muted: Arc<AtomicBool>,
    pub audio_player: StreamAudioPlayer,
}

impl RemotePeerStream {
    fn new(peer_id: u32, ctx: Context) -> Arc<Self> {
        let volume_percent = Arc::new(AtomicU32::new(100));
        let muted = Arc::new(AtomicBool::new(false));
        let audio_player =
            StreamAudioPlayer::start(Arc::clone(&volume_percent), Arc::clone(&muted));
        let stream = Arc::new(Self {
            peer_id,
            latest_frame: Mutex::new(None),
            raw_video_slot: Mutex::new(None),
            raw_video_wakeup: Condvar::new(),
            stop_decoder: AtomicBool::new(false),
            volume_percent,
            muted,
            audio_player,
        });

        let stream_worker = Arc::clone(&stream);
        thread::spawn(move || {
            while !stream_worker.stop_decoder.load(Ordering::Relaxed) {
                let raw_opt = {
                    let mut guard = match stream_worker.raw_video_slot.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    if guard.is_none() {
                        guard = match stream_worker
                            .raw_video_wakeup
                            .wait_timeout(guard, Duration::from_millis(40))
                        {
                            Ok((g, _)) => g,
                            Err(_) => break,
                        };
                    }
                    guard.take()
                };

                if let Some(raw) = raw_opt {
                    let byte_len = raw.jpeg_bytes.len();
                    let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
                    let mut decoder = JpegDecoder::new_with_options(&raw.jpeg_bytes, opts);
                    if let Ok(rgba) = decoder.decode() {
                        let w = raw.width;
                        let h = raw.height;
                        let expected_len = (w * h * 4) as usize;
                        if rgba.len() >= expected_len {
                            if let Ok(mut latest) = stream_worker.latest_frame.lock() {
                                *latest = Some(DecodedRemoteFrame {
                                    width: w,
                                    height: h,
                                    byte_len,
                                    rgba,
                                });
                            }
                            ctx.request_repaint();
                        }
                    }
                }
            }
        });

        stream
    }

    fn push_compressed_video(&self, width: u32, height: u32, jpeg_bytes: Vec<u8>) {
        crate::bridge::publish_video_frame(self.peer_id, width as u16, height as u16, &jpeg_bytes);
        if let Ok(mut slot) = self.raw_video_slot.lock() {
            *slot = Some(RawCompressedVideo {
                width,
                height,
                jpeg_bytes,
            });
            self.raw_video_wakeup.notify_one();
        }
    }
}

impl Drop for RemotePeerStream {
    fn drop(&mut self) {
        self.stop_decoder.store(true, Ordering::Relaxed);
        self.raw_video_wakeup.notify_all();
        self.audio_player.stop();
    }
}

struct SlotQueues {
    control: VecDeque<Arc<Vec<u8>>>,
    audio: VecDeque<Arc<Vec<u8>>>,
    latest_video_by_sender: HashMap<u32, Arc<Vec<u8>>>,
}

struct PeerConnectionSlot {
    conn_id: u64,
    is_outbound: bool,
    remote_peer_id: AtomicU32,
    queues: Mutex<SlotQueues>,
    wakeup: Condvar,
    alive: AtomicBool,
}

impl PeerConnectionSlot {
    fn new(conn_id: u64, is_outbound: bool) -> Arc<Self> {
        Arc::new(Self {
            conn_id,
            is_outbound,
            remote_peer_id: AtomicU32::new(0),
            queues: Mutex::new(SlotQueues {
                control: VecDeque::with_capacity(24),
                audio: VecDeque::with_capacity(24),
                latest_video_by_sender: HashMap::new(),
            }),
            wakeup: Condvar::new(),
            alive: AtomicBool::new(true),
        })
    }

    fn push_control(&self, pkt: Arc<Vec<u8>>) {
        if let Ok(mut q) = self.queues.lock() {
            if q.control.len() >= 32 {
                q.control.pop_front();
            }
            q.control.push_back(pkt);
            self.wakeup.notify_one();
        }
    }

    fn push_audio(&self, pkt: Arc<Vec<u8>>) {
        if let Ok(mut q) = self.queues.lock() {
            if q.audio.len() >= 18 {
                q.audio.pop_front();
            }
            q.audio.push_back(pkt);
            self.wakeup.notify_one();
        }
    }

    fn push_video(&self, sender_id: u32, pkt: Arc<Vec<u8>>) {
        if let Ok(mut q) = self.queues.lock() {
            q.latest_video_by_sender.insert(sender_id, pkt);
            self.wakeup.notify_one();
        }
    }
}

/// Gera um ID fixo por máquina + conta para nunca duplicar o mesmo usuário
pub fn derive_persistent_peer_id(display_name: &str, user_tag: &str) -> u32 {
    let comp = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".to_string());
    let user = std::env::var("USERNAME").unwrap_or_else(|_| "USER".to_string());
    let seed = format!(
        "{}|{}|{}|{}",
        comp.to_uppercase(),
        user.to_uppercase(),
        display_name.trim().to_uppercase(),
        user_tag.trim().to_uppercase()
    );

    let mut hash: u32 = 2166136261;
    for b in seed.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16777619);
    }
    if hash == 0 { 1 } else { hash }
}

/// Calcula a porta exclusiva do Lobby daquele Servidor (`server_id`)!
/// Assim cada Servidor tem sua própria lista de salas e membros, 100% isolada de outros servidores!
pub fn derive_server_lobby_port(server_id: &str) -> u16 {
    let norm = server_id.trim().to_uppercase();
    let seed = format!("SZDISC_V6_SERVER_LOBBY::{}", norm);
    let mut hash: u32 = 2166136261;
    for b in seed.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16777619);
    }
    let mut port = 20000 + (hash % 37000) as u16;
    if (47820..=47850).contains(&port) {
        port += 120;
    }
    port
}

/// Calcula a porta dedicada de uma Sala dentro de um Servidor específico (`server_id` + `room_name`)!
/// Assim, duas salas com o mesmo nome ("Sala Geral") em servidores diferentes NUNCA se misturam!
pub fn derive_room_port(server_id: &str, room_name: &str) -> u16 {
    let norm_srv = server_id.trim().to_uppercase();
    let norm_room = room_name.trim().to_lowercase();
    let seed = format!("SZDISC_V6_SRV_ROOM::{}::{}", norm_srv, norm_room);
    let mut hash: u32 = 2166136261;
    for b in seed.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16777619);
    }
    let mut port = 21000 + (hash % 36000) as u16;
    if (47820..=47850).contains(&port) {
        port += 100;
    }
    port
}

fn build_packet(
    magic: &[u8; 4],
    sender_id: u32,
    width: u16,
    height: u16,
    fps: u8,
    payload: &[u8],
) -> Arc<Vec<u8>> {
    let payload_len = payload.len() as u32;
    let mut pkt = Vec::with_capacity(17 + payload.len());
    pkt.extend_from_slice(magic);
    pkt.extend_from_slice(&sender_id.to_le_bytes());
    pkt.extend_from_slice(&width.to_le_bytes());
    pkt.extend_from_slice(&height.to_le_bytes());
    pkt.push(fps);
    pkt.extend_from_slice(&payload_len.to_le_bytes());
    pkt.extend_from_slice(payload);
    Arc::new(pkt)
}

fn build_info_or_hello_packet(
    magic: &[u8; 4],
    info: &PeerInfo,
    server_id: &str,
    server_name: &str,
    room_name: &str,
    known_rooms: &[String],
) -> Arc<Vec<u8>> {
    let share_flag = if info.is_sharing { "1" } else { "0" };
    let clean_name = info.name.replace('\t', " ").replace('\n', " ");
    let clean_res = info.res_label.replace('\t', " ").replace('\n', " ");
    let clean_srv_id = server_id.replace('\t', " ").replace('\n', " ");
    let clean_srv_name = server_name.replace('\t', " ").replace('\n', " ");
    let clean_room = room_name.replace('\t', " ").replace('\n', " ");
    let rooms_csv = known_rooms
        .iter()
        .map(|r| r.replace('\t', " ").replace(';', " ").trim().to_string())
        .filter(|r| !r.is_empty())
        .collect::<Vec<_>>()
        .join(";");
    let text = format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        clean_name, share_flag, clean_res, clean_srv_id, clean_srv_name, clean_room, rooms_csv
    );
    build_packet(magic, info.peer_id, 0, 0, 0, text.as_bytes())
}

fn read_exact_interruptible(
    stream: &mut TcpStream,
    buf: &mut [u8],
    alive: &AtomicBool,
    stop: &AtomicBool,
) -> bool {
    let mut filled = 0;
    let mut last_progress = Instant::now();
    while filled < buf.len() {
        if !alive.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed) {
            return false;
        }
        match stream.read(&mut buf[filled..]) {
            Ok(0) => return false,
            Ok(n) => {
                filled += n;
                last_progress = Instant::now();
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::Interrupted =>
            {
                if last_progress.elapsed() >= Duration::from_secs(12) {
                    return false;
                }
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(_) => return false,
        }
    }
    true
}

fn write_all_interruptible(stream: &mut TcpStream, buf: &[u8], alive: &AtomicBool) -> bool {
    let mut written = 0;
    let mut retries = 0;
    while written < buf.len() {
        if !alive.load(Ordering::Relaxed) {
            return false;
        }
        match stream.write(&buf[written..]) {
            Ok(0) => return false,
            Ok(n) => {
                written += n;
                retries = 0;
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::Interrupted =>
            {
                retries += 1;
                if retries > 300 {
                    return false;
                }
                thread::sleep(Duration::from_millis(2));
                continue;
            }
            Err(_) => return false,
        }
    }
    true
}

fn parse_bore_hello_port(msg: &str) -> Option<u16> {
    let idx = msg.find("\"Hello\":")?;
    let rest = &msg[idx + 8..];
    let digits: String = rest
        .chars()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u16>().ok()
}

fn parse_bore_connection_uuid(msg: &str) -> Option<String> {
    let idx = msg.find("\"Connection\":\"")?;
    let rest = &msg[idx + 14..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Hub Multi-Servidores Estilo Discord:
/// - Cada Servidor (`server_id`) é totalmente isolado: possui suas próprias salas e seus próprios membros!
/// - Quando você entra em um Servidor (pelo código de convite) 1 vez, ele fica salvo para sempre na sua barra esquerda.
/// - Dentro de cada Servidor, basta clicar em qualquer sala para entrar/sair sem precisar de código!
pub struct RoomHub {
    pub my_peer_id: Arc<AtomicU32>,
    pub in_room: Arc<AtomicBool>,
    pub viewed_server_id: Arc<Mutex<String>>,
    pub connected_server_id: Arc<Mutex<String>>,
    pub active_room_name: Arc<Mutex<String>>,
    pub desired_bore_port: Arc<AtomicU16>,
    pub holding_bore_port: Arc<AtomicU16>,
    pub server_rooms_map: Arc<Mutex<HashMap<String, Vec<String>>>>,
    pub server_names_map: Arc<Mutex<HashMap<String, String>>>,
    pub server_members: Arc<Mutex<HashMap<u32, ServerMemberPresence>>>,
    pub is_connecting_outbound: Arc<AtomicBool>,
    pub status_notice: Arc<Mutex<Option<String>>>,
    pub my_info: Arc<Mutex<PeerInfo>>,
    pub peers: Arc<Mutex<HashMap<u32, PeerInfo>>>,
    pub remote_streams: Arc<Mutex<HashMap<u32, Arc<RemotePeerStream>>>>,
    pub bytes_sent_per_sec: Arc<AtomicU32>,
    connections: Arc<Mutex<Vec<Arc<PeerConnectionSlot>>>>,
    next_conn_id: Arc<AtomicU64>,
    stop_flag: Arc<AtomicBool>,
    ctx: Context,
}

impl RoomHub {
    pub fn start(
        initial_peer_id: u32,
        initial_name: String,
        initial_servers: &[ServerProfile],
        selected_server_id: &str,
        ctx: Context,
    ) -> Result<Arc<Self>, String> {
        let mut bound_listener = None;

        for p in DEFAULT_STREAM_PORT..=DEFAULT_STREAM_PORT + 15 {
            if let Ok(l) = TcpListener::bind(("0.0.0.0", p)) {
                bound_listener = Some(l);
                break;
            }
        }

        let listener =
            bound_listener.ok_or_else(|| "Não foi possível abrir porta TCP local".to_string())?;
        let _ = listener.set_nonblocking(true);

        let mut rooms_map = HashMap::new();
        let mut names_map = HashMap::new();
        for srv in initial_servers {
            rooms_map.insert(srv.server_id.to_uppercase(), srv.rooms.clone());
            names_map.insert(srv.server_id.to_uppercase(), srv.name.clone());
        }

        let initial_viewed = if !selected_server_id.is_empty() {
            selected_server_id.to_uppercase()
        } else {
            initial_servers
                .first()
                .map(|s| s.server_id.to_uppercase())
                .unwrap_or_default()
        };

        let my_peer_id = Arc::new(AtomicU32::new(initial_peer_id));
        let my_info = Arc::new(Mutex::new(PeerInfo {
            peer_id: initial_peer_id,
            name: initial_name,
            is_sharing: false,
            res_label: "Na sala".to_string(),
            last_seen: Instant::now(),
        }));

        let hub = Arc::new(Self {
            my_peer_id,
            in_room: Arc::new(AtomicBool::new(false)),
            viewed_server_id: Arc::new(Mutex::new(initial_viewed)),
            connected_server_id: Arc::new(Mutex::new(String::new())),
            active_room_name: Arc::new(Mutex::new(String::new())),
            desired_bore_port: Arc::new(AtomicU16::new(0)),
            holding_bore_port: Arc::new(AtomicU16::new(0)),
            server_rooms_map: Arc::new(Mutex::new(rooms_map)),
            server_names_map: Arc::new(Mutex::new(names_map)),
            server_members: Arc::new(Mutex::new(HashMap::new())),
            is_connecting_outbound: Arc::new(AtomicBool::new(false)),
            status_notice: Arc::new(Mutex::new(None)),
            my_info,
            peers: Arc::new(Mutex::new(HashMap::new())),
            remote_streams: Arc::new(Mutex::new(HashMap::new())),
            bytes_sent_per_sec: Arc::new(AtomicU32::new(0)),
            connections: Arc::new(Mutex::new(Vec::new())),
            next_conn_id: Arc::new(AtomicU64::new(1)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            ctx,
        });

        // 1. Thread que aceita conexões TCP de entrada
        let hub_accept = Arc::clone(&hub);
        thread::spawn(move || {
            while !hub_accept.stop_flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _peer)) => {
                        if hub_accept.in_room.load(Ordering::Relaxed) {
                            hub_accept.attach_bidirectional_stream(stream, false);
                        } else {
                            drop(stream);
                        }
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(40));
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        });

        // 2. Thread Automática da Sala via Internet (`bore.pub` com porta determinística de `server_id + room_name`)
        let hub_tunnel = Arc::clone(&hub);
        thread::spawn(move || {
            hub_tunnel.room_internet_auto_mesh_loop();
        });

        // 4. Thread do Lobby Dedicado do Servidor Selecionado (`derive_server_lobby_port(server_id)`)
        let hub_lobby = Arc::clone(&hub);
        thread::spawn(move || {
            hub_lobby.server_lobby_sync_loop();
        });

        // 4. Thread de Heartbeat (1.2s) e limpeza de fantasmas
        let hub_beacon = Arc::clone(&hub);
        thread::spawn(move || {
            while !hub_beacon.stop_flag.load(Ordering::Relaxed) {

                // Expira participantes inativos na sala (> 6s)
                let mut expired_ids = Vec::new();
                if let Ok(mut peers_map) = hub_beacon.peers.lock() {
                    peers_map.retain(|&pid, p| {
                        let ok = p.last_seen.elapsed() < Duration::from_secs(6);
                        if !ok {
                            expired_ids.push(pid);
                        }
                        ok
                    });
                }
                if !expired_ids.is_empty() {
                    if let Ok(mut streams) = hub_beacon.remote_streams.lock() {
                        for pid in expired_ids {
                            streams.remove(&pid);
                        }
                    }
                    hub_beacon.ctx.request_repaint();
                }

                // Expira membros do servidor (> 8s)
                if let Ok(mut members) = hub_beacon.server_members.lock() {
                    members.retain(|_, m| m.last_seen.elapsed() < Duration::from_secs(8));
                }

                let in_room = hub_beacon.in_room.load(Ordering::Relaxed);
                let conn_srv = hub_beacon
                    .connected_server_id
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                let viewed_srv = hub_beacon
                    .viewed_server_id
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                let active_srv = if in_room && !conn_srv.is_empty() {
                    conn_srv.clone()
                } else {
                    viewed_srv.clone()
                };

                let curr_room = if in_room {
                    hub_beacon
                        .active_room_name
                        .lock()
                        .map(|g| g.clone())
                        .unwrap_or_default()
                } else {
                    String::new()
                };

                let srv_name = hub_beacon
                    .server_names_map
                    .lock()
                    .ok()
                    .and_then(|m| m.get(&active_srv).cloned())
                    .unwrap_or_default();

                let rooms_snapshot = hub_beacon
                    .server_rooms_map
                    .lock()
                    .ok()
                    .and_then(|m| m.get(&active_srv).cloned())
                    .unwrap_or_default();

                if let Ok(info) = hub_beacon.my_info.lock() {
                    if in_room && !conn_srv.is_empty() {
                        let pkt = build_info_or_hello_packet(
                            MAGIC_INFO,
                            &info,
                            &conn_srv,
                            &srv_name,
                            &curr_room,
                            &rooms_snapshot,
                        );
                        if let Ok(mut conns) = hub_beacon.connections.lock() {
                            conns.retain(|c| c.alive.load(Ordering::Relaxed));
                            for c in conns.iter() {
                                c.push_control(Arc::clone(&pkt));
                            }
                        }
                    }
                }

                thread::sleep(Duration::from_millis(1200));
            }
        });

        Ok(hub)
    }

    /// Registra ou atualiza um servidor local no Hub e seleciona ele como servidor visível
    pub fn register_and_select_server(&self, srv: &ServerProfile) {
        let key = srv.server_id.trim().to_uppercase();
        if let Ok(mut map) = self.server_rooms_map.lock() {
            let entry = map.entry(key.clone()).or_insert_with(|| srv.rooms.clone());
            for r in &srv.rooms {
                if !entry.iter().any(|x| x.eq_ignore_ascii_case(r)) {
                    entry.push(r.clone());
                }
            }
        }
        if let Ok(mut nmap) = self.server_names_map.lock() {
            if !srv.name.starts_with("Servidor SZ-") || !nmap.contains_key(&key) {
                nmap.insert(key.clone(), srv.name.clone());
            }
        }
        if let Ok(mut v) = self.viewed_server_id.lock() {
            *v = key;
        }
        self.ctx.request_repaint();
    }

    pub fn remove_server(&self, server_id: &str) {
        let key = server_id.trim().to_uppercase();
        let is_connected_here = self
            .connected_server_id
            .lock()
            .map(|g| g.eq_ignore_ascii_case(&key))
            .unwrap_or(false);
        if is_connected_here {
            self.leave_room_completely();
        }
        if let Ok(mut map) = self.server_rooms_map.lock() {
            map.remove(&key);
        }
        if let Ok(mut nmap) = self.server_names_map.lock() {
            nmap.remove(&key);
        }
    }

    fn is_member_of_server(&self, server_id: &str) -> bool {
        let key = server_id.trim().to_uppercase();
        self.server_rooms_map
            .lock()
            .map(|m| m.contains_key(&key))
            .unwrap_or(false)
    }

    fn merge_server_metadata(&self, server_id: &str, server_name: &str, rooms_csv: &str) {
        let key = server_id.trim().to_uppercase();
        if key.is_empty() || !self.is_member_of_server(&key) {
            return;
        }

        if !server_name.trim().is_empty() && !server_name.starts_with("Servidor SZ-") {
            if let Ok(mut nmap) = self.server_names_map.lock() {
                nmap.insert(key.clone(), server_name.trim().to_string());
            }
        }

        if !rooms_csv.trim().is_empty() {
            if let Ok(mut map) = self.server_rooms_map.lock() {
                if let Some(rooms) = map.get_mut(&key) {
                    let mut existing_lower: HashSet<String> =
                        rooms.iter().map(|r| r.to_lowercase()).collect();
                    for item in rooms_csv.split(';') {
                        let clean = item.trim();
                        if !clean.is_empty() && clean.len() <= 40 {
                            let lower = clean.to_lowercase();
                            if !existing_lower.contains(&lower) {
                                existing_lower.insert(lower);
                                rooms.push(clean.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    /// Loop automático da Sala via Internet (`bore.pub` usando porta exclusiva `derive_room_port(server_id, room_name)`)
    fn room_internet_auto_mesh_loop(self: Arc<Self>) {
        let mut port_offset: u16 = 0;
        while !self.stop_flag.load(Ordering::Relaxed) {
            if !self.in_room.load(Ordering::Relaxed) {
                self.holding_bore_port.store(0, Ordering::Relaxed);
                port_offset = 0;
                thread::sleep(Duration::from_millis(400));
                continue;
            }

            let base_port = self.desired_bore_port.load(Ordering::Relaxed);
            if base_port == 0 {
                port_offset = 0;
                thread::sleep(Duration::from_millis(400));
                continue;
            }

            let target_port = 21000 + ((base_port - 21000 + port_offset * 137) % 36000);

            let has_alive_outbound = self
                .connections
                .lock()
                .map(|cs| {
                    cs.iter()
                        .any(|c| c.is_outbound && c.alive.load(Ordering::Relaxed))
                })
                .unwrap_or(false);

            if has_alive_outbound {
                thread::sleep(Duration::from_millis(900));
                continue;
            }

            let relay_addr = match BORE_RELAY_HOST
                .to_socket_addrs()
                .ok()
                .and_then(|mut it| it.next())
            {
                Some(a) => a,
                None => {
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            let mut ctrl = match TcpStream::connect_timeout(&relay_addr, Duration::from_secs(5)) {
                Ok(s) => s,
                Err(_) => {
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            let _ = ctrl.set_nodelay(true);
            let _ = ctrl.set_read_timeout(Some(Duration::from_secs(18)));

            let hello_cmd = format!("{{\"Hello\":{}}}\0", target_port);
            if ctrl.write_all(hello_cmd.as_bytes()).is_err() {
                thread::sleep(Duration::from_secs(1));
                continue;
            }

            let mut pending = Vec::with_capacity(512);
            let mut temp = [0u8; 256];
            let mut port_already_in_use = false;
            let mut last_ctrl_activity = Instant::now();

            while !self.stop_flag.load(Ordering::Relaxed)
                && self.in_room.load(Ordering::Relaxed)
                && self.desired_bore_port.load(Ordering::Relaxed) == base_port
            {
                match ctrl.read(&mut temp) {
                    Ok(0) => break,
                    Ok(n) => {
                        last_ctrl_activity = Instant::now();
                        pending.extend_from_slice(&temp[..n]);
                        while let Some(pos) = pending.iter().position(|&b| b == 0) {
                            let msg_bytes: Vec<u8> = pending.drain(..=pos).collect();
                            let s = String::from_utf8_lossy(&msg_bytes[..msg_bytes.len() - 1]);

                            if s.contains("\"Error\"") {
                                port_already_in_use = true;
                                break;
                            } else if let Some(port) = parse_bore_hello_port(&s) {
                                if port > 0 {
                                    self.holding_bore_port.store(port, Ordering::Relaxed);
                                    self.ctx.request_repaint();
                                }
                            } else if let Some(uuid) = parse_bore_connection_uuid(&s) {
                                if !self.in_room.load(Ordering::Relaxed) {
                                    continue;
                                }
                                let hub_clone = Arc::clone(&self);
                                thread::spawn(move || {
                                    if let Ok(mut data_stream) =
                                        TcpStream::connect_timeout(&relay_addr, Duration::from_secs(6))
                                    {
                                        let _ = data_stream.set_nodelay(true);
                                        let accept_msg = format!("{{\"Accept\":\"{}\"}}\0", uuid);
                                        if data_stream.write_all(accept_msg.as_bytes()).is_ok() {
                                            let _ = data_stream.flush();
                                            hub_clone
                                                .attach_bidirectional_stream(data_stream, false);
                                        }
                                    }
                                });
                            }
                        }
                        if port_already_in_use {
                            break;
                        }
                    }
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        if last_ctrl_activity.elapsed() >= Duration::from_secs(40) {
                            break;
                        }
                        continue;
                    }
                    Err(_) => break,
                }
            }

            self.holding_bore_port.store(0, Ordering::Relaxed);
            drop(ctrl);

            if port_already_in_use
                && self.in_room.load(Ordering::Relaxed)
                && self.desired_bore_port.load(Ordering::Relaxed) == base_port
            {
                let bore_room_target = format!("bore.pub:{}", target_port);
                self.spawn_outbound_connect(bore_room_target, true);
                thread::sleep(Duration::from_millis(2400));
                let has_valid_peer = self
                    .peers
                    .lock()
                    .map(|p| !p.is_empty())
                    .unwrap_or(false);
                if !has_valid_peer {
                    // Porta ocupada por outro serviço no bore.pub (sem handshake SZdisc): avança para próxima porta
                    port_offset = (port_offset + 1) % 6;
                } else {
                    port_offset = 0;
                }
            } else {
                thread::sleep(Duration::from_millis(800));
            }
        }
    }

    /// Sincroniza as salas e os membros online APENAS do Servidor atualmente selecionado (`viewed_server_id`)!
    fn server_lobby_sync_loop(self: Arc<Self>) {
        while !self.stop_flag.load(Ordering::Relaxed) {
            let lobby_clients: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));
            let current_srv = self
                .viewed_server_id
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default();

            if current_srv.is_empty() {
                thread::sleep(Duration::from_millis(600));
                continue;
            }

            let lobby_port = derive_server_lobby_port(&current_srv);

            let relay_addr = match BORE_RELAY_HOST
                .to_socket_addrs()
                .ok()
                .and_then(|mut it| it.next())
            {
                Some(a) => a,
                None => {
                    thread::sleep(Duration::from_secs(3));
                    continue;
                }
            };

            let mut ctrl = match TcpStream::connect_timeout(&relay_addr, Duration::from_secs(5)) {
                Ok(s) => s,
                Err(_) => {
                    thread::sleep(Duration::from_secs(3));
                    continue;
                }
            };

            let _ = ctrl.set_nodelay(true);
            let _ = ctrl.set_read_timeout(Some(Duration::from_secs(3)));

            let hello_cmd = format!("{{\"Hello\":{}}}\0", lobby_port);
            if ctrl.write_all(hello_cmd.as_bytes()).is_err() {
                thread::sleep(Duration::from_secs(2));
                continue;
            }

            let mut pending = Vec::with_capacity(512);
            let mut temp = [0u8; 256];
            let mut lobby_already_hosted = false;

            while !self.stop_flag.load(Ordering::Relaxed) {
                let latest_srv = self
                    .viewed_server_id
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                if latest_srv != current_srv {
                    break;
                }

                match ctrl.read(&mut temp) {
                    Ok(0) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&temp[..n]);
                        while let Some(pos) = pending.iter().position(|&b| b == 0) {
                            let msg_bytes: Vec<u8> = pending.drain(..=pos).collect();
                            let s = String::from_utf8_lossy(&msg_bytes[..msg_bytes.len() - 1]);

                            if s.contains("\"Error\"") {
                                lobby_already_hosted = true;
                                break;
                            } else if let Some(uuid) = parse_bore_connection_uuid(&s) {
                                let hub_clone = Arc::clone(&self);
                                let clients_clone = Arc::clone(&lobby_clients);
                                let srv_clone = current_srv.clone();
                                thread::spawn(move || {
                                    if let Ok(mut data_stream) =
                                        TcpStream::connect_timeout(&relay_addr, Duration::from_secs(5))
                                    {
                                        let _ = data_stream.set_nonblocking(false);
                                        let _ = data_stream.set_nodelay(true);
                                        let _ = data_stream
                                            .set_read_timeout(Some(Duration::from_secs(10)));
                                        let accept_msg = format!("{{\"Accept\":\"{}\"}}\0", uuid);
                                        if data_stream.write_all(accept_msg.as_bytes()).is_ok() {
                                            let _ = data_stream.flush();
                                            if let Ok(cloned) = data_stream.try_clone() {
                                                if let Ok(mut list) = clients_clone.lock() {
                                                    list.push(cloned);
                                                }
                                            }
                                            hub_clone.handle_lobby_stream(
                                                data_stream,
                                                &srv_clone,
                                                Some(clients_clone),
                                            );
                                        }
                                    }
                                });
                            }
                        }
                        if lobby_already_hosted {
                            break;
                        }
                    }
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        let line = self.build_lobby_line(&current_srv);
                        if let Ok(mut list) = lobby_clients.lock() {
                            list.retain_mut(|c| c.write_all(line.as_bytes()).is_ok());
                        }
                        continue;
                    }
                    Err(_) => break,
                }
            }

            drop(ctrl);

            if lobby_already_hosted {
                if let Ok(lobby_addr) = format!("bore.pub:{}", lobby_port)
                    .to_socket_addrs()
                    .and_then(|mut it| {
                        it.next()
                            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "no addr"))
                    })
                {
                    if let Ok(stream) = TcpStream::connect_timeout(&lobby_addr, Duration::from_secs(5))
                    {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_nodelay(true);
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
                        self.handle_lobby_stream(stream, &current_srv, None);
                    }
                }
                thread::sleep(Duration::from_secs(2));
            } else {
                thread::sleep(Duration::from_millis(800));
            }
        }
    }

    fn build_lobby_line(&self, server_id: &str) -> String {
        let key = server_id.trim().to_uppercase();
        let my_id = self.my_peer_id.load(Ordering::Relaxed);
        let in_room = self.in_room.load(Ordering::Relaxed);
        let conn_srv = self
            .connected_server_id
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        let curr_room = if in_room && conn_srv.eq_ignore_ascii_case(&key) {
            self.active_room_name
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };

        let srv_name = self
            .server_names_map
            .lock()
            .ok()
            .and_then(|m| m.get(&key).cloned())
            .unwrap_or_default();

        let (name, is_sharing, res_label) = self
            .my_info
            .lock()
            .map(|i| (i.name.clone(), i.is_sharing, i.res_label.clone()))
            .unwrap_or_else(|_| ("Jogador".to_string(), false, "Na sala".to_string()));

        let rooms_csv = self
            .server_rooms_map
            .lock()
            .ok()
            .and_then(|m| m.get(&key).cloned())
            .map(|rs| {
                rs.iter()
                    .map(|r| r.replace('|', " ").replace(';', " ").trim().to_string())
                    .filter(|r| !r.is_empty())
                    .collect::<Vec<_>>()
                    .join(";")
            })
            .unwrap_or_default();

        // LBY6|peer_id|name|server_id|server_name|curr_room|is_sharing|res_label|rooms_csv
        format!(
            "LBY6|{}|{}|{}|{}|{}|{}|{}|{}\n",
            my_id,
            name.replace('|', " ").replace('\n', " "),
            key,
            srv_name.replace('|', " ").replace('\n', " "),
            curr_room.replace('|', " ").replace('\n', " "),
            if is_sharing && !curr_room.is_empty() {
                "1"
            } else {
                "0"
            },
            res_label.replace('|', " ").replace('\n', " "),
            rooms_csv.replace('\n', " ")
        )
    }

    fn handle_lobby_stream(
        &self,
        mut stream: TcpStream,
        expected_server_id: &str,
        relay_clients: Option<Arc<Mutex<Vec<TcpStream>>>>,
    ) {
        let mut pending = Vec::with_capacity(1024);
        let mut buf = [0u8; 512];
        let mut last_send = Instant::now() - Duration::from_secs(5);

        while !self.stop_flag.load(Ordering::Relaxed) {
            let latest_srv = self
                .viewed_server_id
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default();
            if !latest_srv.eq_ignore_ascii_case(expected_server_id) {
                break;
            }

            if last_send.elapsed() >= Duration::from_millis(1600) {
                let line = self.build_lobby_line(expected_server_id);
                if stream.write_all(line.as_bytes()).is_err() {
                    break;
                }
                let _ = stream.flush();
                last_send = Instant::now();
            }

            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    while let Some(pos) = pending.iter().position(|&b| b == b'\n') {
                        let line_bytes: Vec<u8> = pending.drain(..=pos).collect();
                        if let Ok(line_str) = std::str::from_utf8(&line_bytes) {
                            let trimmed = line_str.trim();
                            let parts: Vec<&str> = trimmed.split('|').collect();
                            // LBY6|peer_id|name|server_id|server_name|curr_room|is_sharing|res_label|rooms_csv
                            if parts.len() >= 9 && parts[0] == "LBY6" {
                                let pid = parts[1].parse::<u32>().unwrap_or(0);
                                let my_id = self.my_peer_id.load(Ordering::Relaxed);
                                let srv_id = parts[3].trim().to_uppercase();
                                if pid != 0
                                    && pid != my_id
                                    && srv_id.eq_ignore_ascii_case(expected_server_id)
                                {
                                    let uname = parts[2].trim().to_string();
                                    let srv_name = parts[4].trim();
                                    let uroom = parts[5].trim().to_string();
                                    let usharing = parts[6] == "1";
                                    let ures = parts[7].trim().to_string();
                                    let ucsv = parts[8];

                                    self.merge_server_metadata(&srv_id, srv_name, ucsv);
                                    if !uroom.is_empty() {
                                        self.merge_server_metadata(&srv_id, srv_name, &uroom);
                                    }

                                    if let Ok(mut members) = self.server_members.lock() {
                                        members.insert(
                                            pid,
                                            ServerMemberPresence {
                                                peer_id: pid,
                                                name: uname,
                                                server_id: srv_id,
                                                current_room: uroom,
                                                is_sharing: usharing,
                                                res_label: ures,
                                                last_seen: Instant::now(),
                                            },
                                        );
                                    }

                                    if let Some(ref clients_arc) = relay_clients {
                                        let forward = format!("{}\n", trimmed);
                                        if let Ok(mut list) = clients_arc.lock() {
                                            list.retain_mut(|c| {
                                                c.write_all(forward.as_bytes()).is_ok()
                                            });
                                        }
                                    }
                                    self.ctx.request_repaint();
                                }
                            }
                        }
                    }
                }
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue;
                }
                Err(_) => break,
            }
        }
    }

    /// Cria uma nova sala exclusivamente dentro do Servidor informado e já entra nela!
    pub fn create_and_enter_room(
        self: &Arc<Self>,
        server_id: &str,
        room_name: &str,
    ) -> Vec<String> {
        let key = server_id.trim().to_uppercase();
        let clean = room_name.trim().to_string();
        if key.is_empty() || clean.is_empty() {
            return self
                .server_rooms_map
                .lock()
                .ok()
                .and_then(|m| m.get(&key).cloned())
                .unwrap_or_default();
        }

        let updated_list = {
            let mut map = self.server_rooms_map.lock().unwrap();
            let rooms = map.entry(key.clone()).or_default();
            if !rooms.iter().any(|r| r.eq_ignore_ascii_case(&clean)) {
                rooms.push(clean.clone());
            }
            rooms.clone()
        };

        self.enter_server_room(&key, &clean);
        updated_list
    }

    /// Entra em uma sala específica dentro de um Servidor específico (`server_id` + `room_name`)
    pub fn enter_server_room(self: &Arc<Self>, server_id: &str, room_name: &str) {
        let key = server_id.trim().to_uppercase();
        let clean = room_name.trim().to_string();
        if key.is_empty() || clean.is_empty() {
            return;
        }

        let curr_srv = self
            .connected_server_id
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let curr_room = self
            .active_room_name
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        if self.in_room.load(Ordering::Relaxed)
            && curr_srv.eq_ignore_ascii_case(&key)
            && curr_room.eq_ignore_ascii_case(&clean)
        {
            return;
        }

        self.disconnect_all_connections();
        if let Ok(mut cs) = self.connected_server_id.lock() {
            *cs = key.clone();
        }
        if let Ok(mut vs) = self.viewed_server_id.lock() {
            *vs = key.clone();
        }
        if let Ok(mut active) = self.active_room_name.lock() {
            *active = clean.clone();
        }
        let port = derive_room_port(&key, &clean);
        self.desired_bore_port.store(port, Ordering::Relaxed);
        self.in_room.store(true, Ordering::Relaxed);
        self.ctx.request_repaint();
    }

    fn spawn_outbound_connect(self: &Arc<Self>, addr_str: String, is_silent_auto: bool) {
        if self
            .is_connecting_outbound
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let hub = Arc::clone(self);
        thread::spawn(move || {
            let sock_addr = match addr_str
                .to_socket_addrs()
                .ok()
                .and_then(|mut it| it.next())
            {
                Some(a) => a,
                None => {
                    hub.is_connecting_outbound.store(false, Ordering::SeqCst);
                    return;
                }
            };

            match TcpStream::connect_timeout(&sock_addr, Duration::from_secs(5)) {
                Ok(stream) => {
                    hub.is_connecting_outbound.store(false, Ordering::SeqCst);
                    hub.ctx.request_repaint();
                    hub.attach_bidirectional_stream(stream, true);
                }
                Err(e) => {
                    if !is_silent_auto {
                        if let Ok(mut n) = hub.status_notice.lock() {
                            *n = Some(format!("Não foi possível conectar: {}", e));
                        }
                    }
                    hub.is_connecting_outbound.store(false, Ordering::SeqCst);
                    hub.ctx.request_repaint();
                }
            }
        });
    }

    fn disconnect_all_connections(&self) {
        let my_id = self.my_peer_id.load(Ordering::Relaxed);
        let bye_pkt = build_packet(MAGIC_LEFT_ROOM, my_id, 0, 0, 0, &[0]);
        if let Ok(conns) = self.connections.lock() {
            for c in conns.iter() {
                c.push_control(Arc::clone(&bye_pkt));
            }
        }
        thread::sleep(Duration::from_millis(25));
        if let Ok(mut conns) = self.connections.lock() {
            for c in conns.iter() {
                c.alive.store(false, Ordering::Relaxed);
                c.wakeup.notify_all();
            }
            conns.clear();
        }
        if let Ok(mut peers) = self.peers.lock() {
            peers.clear();
        }
        if let Ok(mut streams) = self.remote_streams.lock() {
            streams.clear();
        }
    }

    pub fn leave_room_completely(&self) {
        self.in_room.store(false, Ordering::Relaxed);
        self.holding_bore_port.store(0, Ordering::Relaxed);
        self.desired_bore_port.store(0, Ordering::Relaxed);
        if let Ok(mut cs) = self.connected_server_id.lock() {
            cs.clear();
        }
        if let Ok(mut ar) = self.active_room_name.lock() {
            ar.clear();
        }
        self.disconnect_all_connections();
        self.ctx.request_repaint();
    }

    pub fn update_my_status(
        &self,
        name: String,
        _user_tag: &str,
        is_sharing: bool,
        res_label: String,
    ) {
        let stable_id = self.my_peer_id.load(Ordering::Relaxed);

        let conn_srv = self
            .connected_server_id
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let curr_room = self
            .active_room_name
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let srv_name = self
            .server_names_map
            .lock()
            .ok()
            .and_then(|m| m.get(&conn_srv).cloned())
            .unwrap_or_default();
        let rooms_snapshot = self
            .server_rooms_map
            .lock()
            .ok()
            .and_then(|m| m.get(&conn_srv).cloned())
            .unwrap_or_default();

        let mut stopped_sharing = false;
        let pkt = {
            let Ok(mut info) = self.my_info.lock() else {
                return;
            };
            if info.is_sharing && !is_sharing {
                stopped_sharing = true;
            }
            info.peer_id = stable_id;
            info.name = name;
            info.is_sharing = is_sharing;
            info.res_label = res_label;
            info.last_seen = Instant::now();
            build_info_or_hello_packet(
                MAGIC_INFO,
                &info,
                &conn_srv,
                &srv_name,
                &curr_room,
                &rooms_snapshot,
            )
        };

        let stop_pkt = if stopped_sharing {
            Some(build_packet(MAGIC_STOP_STREAM, stable_id, 0, 0, 0, &[0]))
        } else {
            None
        };

        if let Ok(mut conns) = self.connections.lock() {
            conns.retain(|c| c.alive.load(Ordering::Relaxed));
            for c in conns.iter() {
                if let Some(ref sp) = stop_pkt {
                    c.push_control(Arc::clone(sp));
                }
                c.push_control(Arc::clone(&pkt));
            }
        }
    }

    pub fn broadcast_my_video(&self, width: u16, height: u16, fps: u8, jpeg_bytes: &[u8]) {
        if !self.in_room.load(Ordering::Relaxed) {
            return;
        }
        let mut conns = match self.connections.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        conns.retain(|c| c.alive.load(Ordering::Relaxed));
        if conns.is_empty() {
            return;
        }

        let my_id = self.my_peer_id.load(Ordering::Relaxed);
        let pkt = build_packet(MAGIC_VIDEO, my_id, width, height, fps, jpeg_bytes);

        for c in conns.iter() {
            c.push_video(my_id, Arc::clone(&pkt));
        }
    }

    pub fn broadcast_my_audio(&self, pcm_bytes: &[u8]) {
        if !self.in_room.load(Ordering::Relaxed) {
            return;
        }
        let mut conns = match self.connections.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        conns.retain(|c| c.alive.load(Ordering::Relaxed));
        if conns.is_empty() {
            return;
        }

        let my_id = self.my_peer_id.load(Ordering::Relaxed);
        let pkt = build_packet(MAGIC_AUDIO, my_id, 0, 0, 0, pcm_bytes);

        for c in conns.iter() {
            c.push_audio(Arc::clone(&pkt));
        }
    }

    pub fn has_connected_peers(&self) -> bool {
        self.connections
            .lock()
            .map(|c| c.iter().any(|s| s.alive.load(Ordering::Relaxed)))
            .unwrap_or(false)
    }

    fn attach_bidirectional_stream(
        self: &Arc<Self>,
        mut write_stream: TcpStream,
        is_outbound: bool,
    ) {
        let _ = write_stream.set_nonblocking(false);
        let _ = write_stream.set_nodelay(true);
        let _ = write_stream.set_read_timeout(Some(Duration::from_secs(25)));
        let _ = write_stream.set_write_timeout(Some(Duration::from_secs(20)));

        let mut read_stream = match write_stream.try_clone() {
            Ok(s) => s,
            Err(_) => return,
        };
        let _ = read_stream.set_nonblocking(false);

        let conn_id = self.next_conn_id.fetch_add(1, Ordering::Relaxed);
        let slot = PeerConnectionSlot::new(conn_id, is_outbound);

        let conn_srv = self
            .connected_server_id
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let curr_room = self
            .active_room_name
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let srv_name = self
            .server_names_map
            .lock()
            .ok()
            .and_then(|m| m.get(&conn_srv).cloned())
            .unwrap_or_default();
        let rooms_snapshot = self
            .server_rooms_map
            .lock()
            .ok()
            .and_then(|m| m.get(&conn_srv).cloned())
            .unwrap_or_default();

        if let Ok(info) = self.my_info.lock() {
            slot.push_control(build_info_or_hello_packet(
                MAGIC_HELLO,
                &info,
                &conn_srv,
                &srv_name,
                &curr_room,
                &rooms_snapshot,
            ));
        }
        if let Ok(existing_peers) = self.peers.lock() {
            for p in existing_peers.values() {
                if p.last_seen.elapsed() < Duration::from_secs(5) {
                    slot.push_control(build_info_or_hello_packet(
                        MAGIC_INFO,
                        p,
                        &conn_srv,
                        &srv_name,
                        &curr_room,
                        &rooms_snapshot,
                    ));
                }
            }
        }

        if let Ok(mut conns) = self.connections.lock() {
            conns.push(Arc::clone(&slot));
        }

        // Thread de ESCRITA (fila única sob 1 Mutex + 1 Condvar: zero missed wakeups)
        let slot_writer = Arc::clone(&slot);
        thread::spawn(move || {
            while slot_writer.alive.load(Ordering::Relaxed) {
                let (ctrl_pkts, aud_pkts, video_pkts) = {
                    let mut guard = match slot_writer.queues.lock() {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    if guard.control.is_empty()
                        && guard.audio.is_empty()
                        && guard.latest_video_by_sender.is_empty()
                    {
                        let (g, _) = match slot_writer
                            .wakeup
                            .wait_timeout(guard, Duration::from_millis(45))
                        {
                            Ok(res) => res,
                            Err(_) => return,
                        };
                        guard = g;
                    }
                    let c: Vec<Arc<Vec<u8>>> = guard.control.drain(..).collect();
                    let a: Vec<Arc<Vec<u8>>> = guard.audio.drain(..).collect();
                    let v: Vec<Arc<Vec<u8>>> =
                        guard.latest_video_by_sender.drain().map(|(_, p)| p).collect();
                    (c, a, v)
                };

                for pkt in ctrl_pkts.into_iter().chain(aud_pkts.into_iter()) {
                    if !write_all_interruptible(&mut write_stream, &pkt, &slot_writer.alive) {
                        slot_writer.alive.store(false, Ordering::Relaxed);
                        return;
                    }
                }

                for pkt in video_pkts {
                    if !write_all_interruptible(&mut write_stream, &pkt, &slot_writer.alive) {
                        slot_writer.alive.store(false, Ordering::Relaxed);
                        return;
                    }
                    let _ = write_stream.flush();
                }
            }
        });

        // Thread de LEITURA
        let hub_reader = Arc::clone(self);
        let slot_reader = Arc::clone(&slot);
        thread::spawn(move || {
            let mut header = [0u8; 17];
            while slot_reader.alive.load(Ordering::Relaxed)
                && !hub_reader.stop_flag.load(Ordering::Relaxed)
            {
                if !read_exact_interruptible(
                    &mut read_stream,
                    &mut header,
                    &slot_reader.alive,
                    &hub_reader.stop_flag,
                ) {
                    break;
                }

                let magic: [u8; 4] = [header[0], header[1], header[2], header[3]];
                let sender_id =
                    u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
                let width = u16::from_le_bytes([header[8], header[9]]);
                let height = u16::from_le_bytes([header[10], header[11]]);
                let fps = header[12];
                let payload_len =
                    u32::from_le_bytes([header[13], header[14], header[15], header[16]]) as usize;

                if payload_len == 0 || payload_len > 15_000_000 {
                    break;
                }

                let mut payload = vec![0u8; payload_len];
                if !read_exact_interruptible(
                    &mut read_stream,
                    &mut payload,
                    &slot_reader.alive,
                    &hub_reader.stop_flag,
                ) {
                    break;
                }

                let my_id = hub_reader.my_peer_id.load(Ordering::Relaxed);
                if sender_id == 0 || sender_id == my_id {
                    continue;
                }

                if &magic == MAGIC_HELLO
                    || slot_reader.remote_peer_id.load(Ordering::Relaxed) == 0
                {
                    slot_reader.remote_peer_id.store(sender_id, Ordering::Relaxed);
                    let mut should_drop_self = false;
                    if let Ok(mut conns) = hub_reader.connections.lock() {
                        let prefer_outbound = my_id < sender_id;
                        for other in conns.iter() {
                            if other.conn_id != slot_reader.conn_id
                                && other.alive.load(Ordering::Relaxed)
                                && other.remote_peer_id.load(Ordering::Relaxed) == sender_id
                            {
                                // Desempate determinístico: ambos os lados escolhem a mesma conexão
                                if other.is_outbound == prefer_outbound
                                    && slot_reader.is_outbound != prefer_outbound
                                {
                                    should_drop_self = true;
                                } else {
                                    other.alive.store(false, Ordering::Relaxed);
                                    other.wakeup.notify_all();
                                }
                            }
                        }
                        conns.retain(|c| c.alive.load(Ordering::Relaxed));
                    }
                    if should_drop_self {
                        break;
                    }
                }

                let direct_peer_id = slot_reader.remote_peer_id.load(Ordering::Relaxed);

                // Apenas o host do túnel (conexões de entrada: !is_outbound) repassa pacotes para outros membros
                if sender_id == direct_peer_id && !slot_reader.is_outbound {
                    let relay_magic = if &magic == MAGIC_HELLO {
                        MAGIC_INFO
                    } else {
                        &magic
                    };
                    let relay_pkt =
                        build_packet(relay_magic, sender_id, width, height, fps, &payload);
                    if let Ok(conns) = hub_reader.connections.lock() {
                        for other in conns.iter() {
                            if other.conn_id != slot_reader.conn_id
                                && other.alive.load(Ordering::Relaxed)
                                && other.remote_peer_id.load(Ordering::Relaxed) != sender_id
                            {
                                if &magic == MAGIC_VIDEO {
                                    other.push_video(sender_id, Arc::clone(&relay_pkt));
                                } else if &magic == MAGIC_AUDIO {
                                    other.push_audio(Arc::clone(&relay_pkt));
                                } else {
                                    other.push_control(Arc::clone(&relay_pkt));
                                }
                            }
                        }
                    }
                }

                if &magic == MAGIC_HELLO || &magic == MAGIC_INFO {
                    if let Ok(text) = std::str::from_utf8(&payload) {
                        let parts: Vec<&str> = text.split('\t').collect();
                        // name \t share_flag \t res_label \t server_id \t server_name \t room_name \t rooms_csv
                        if parts.len() >= 3 {
                            let peer_name = parts[0].trim().to_string();
                            let is_sharing = parts[1] == "1";
                            let res_label = parts[2].to_string();
                            if parts.len() >= 7 {
                                let srv_id = parts[3].trim();
                                let srv_name = parts[4].trim();
                                hub_reader.merge_server_metadata(srv_id, srv_name, parts[6]);
                            }

                            let is_same_as_me = hub_reader
                                .my_info
                                .lock()
                                .map(|me| {
                                    me.peer_id == sender_id
                                        || (me.name.eq_ignore_ascii_case(&peer_name)
                                            && sender_id != direct_peer_id)
                                })
                                .unwrap_or(false);

                            if !is_same_as_me {
                                let mut old_duplicate_ids = Vec::new();
                                if let Ok(mut peers_map) = hub_reader.peers.lock() {
                                    peers_map.retain(|&k, v| {
                                        if k != sender_id && v.name.eq_ignore_ascii_case(&peer_name)
                                        {
                                            old_duplicate_ids.push(k);
                                            false
                                        } else {
                                            true
                                        }
                                    });

                                    peers_map.insert(
                                        sender_id,
                                        PeerInfo {
                                            peer_id: sender_id,
                                            name: peer_name.clone(),
                                            is_sharing,
                                            res_label: res_label.clone(),
                                            last_seen: Instant::now(),
                                        },
                                    );
                                }

                                let conn_srv = hub_reader
                                    .connected_server_id
                                    .lock()
                                    .map(|g| g.clone())
                                    .unwrap_or_default();
                                let curr_room = hub_reader
                                    .active_room_name
                                    .lock()
                                    .map(|g| g.clone())
                                    .unwrap_or_default();
                                if let Ok(mut members) = hub_reader.server_members.lock() {
                                    members.insert(
                                        sender_id,
                                        ServerMemberPresence {
                                            peer_id: sender_id,
                                            name: peer_name,
                                            server_id: conn_srv,
                                            current_room: curr_room,
                                            is_sharing,
                                            res_label,
                                            last_seen: Instant::now(),
                                        },
                                    );
                                }

                                if !old_duplicate_ids.is_empty() || !is_sharing {
                                    if let Ok(mut streams) = hub_reader.remote_streams.lock() {
                                        for old_id in old_duplicate_ids {
                                            streams.remove(&old_id);
                                        }
                                        if !is_sharing {
                                            streams.remove(&sender_id);
                                        }
                                    }
                                }
                                hub_reader.ctx.request_repaint();
                            }
                        }
                    }
                } else if &magic == MAGIC_STOP_STREAM {
                    if let Ok(mut peers_map) = hub_reader.peers.lock() {
                        if let Some(p) = peers_map.get_mut(&sender_id) {
                            p.is_sharing = false;
                            p.last_seen = Instant::now();
                        }
                    }
                    if let Ok(mut streams) = hub_reader.remote_streams.lock() {
                        streams.remove(&sender_id);
                    }
                    hub_reader.ctx.request_repaint();
                } else if &magic == MAGIC_LEFT_ROOM {
                    if let Ok(mut peers_map) = hub_reader.peers.lock() {
                        peers_map.remove(&sender_id);
                    }
                    if let Ok(mut streams) = hub_reader.remote_streams.lock() {
                        streams.remove(&sender_id);
                    }
                    hub_reader.ctx.request_repaint();
                } else if &magic == MAGIC_VIDEO {
                    let w = width as u32;
                    let h = height as u32;
                    if w > 0 && h > 0 {
                        if let Ok(mut peers_map) = hub_reader.peers.lock() {
                            let entry =
                                peers_map.entry(sender_id).or_insert_with(|| PeerInfo {
                                    peer_id: sender_id,
                                    name: format!("Amigo #{}", sender_id % 1000),
                                    is_sharing: true,
                                    res_label: format!("{}p {}fps", h, fps),
                                    last_seen: Instant::now(),
                                });
                            entry.is_sharing = true;
                            entry.last_seen = Instant::now();
                        }

                        let stream_arc = {
                            let mut streams = hub_reader.remote_streams.lock().unwrap();
                            let ctx_clone = hub_reader.ctx.clone();
                            Arc::clone(
                                streams
                                    .entry(sender_id)
                                    .or_insert_with(|| RemotePeerStream::new(sender_id, ctx_clone)),
                            )
                        };

                        stream_arc.push_compressed_video(w, h, payload);
                    }
                } else if &magic == MAGIC_AUDIO {
                    let stream_arc = {
                        let mut streams = hub_reader.remote_streams.lock().unwrap();
                        let ctx_clone = hub_reader.ctx.clone();
                        Arc::clone(
                            streams
                                .entry(sender_id)
                                .or_insert_with(|| RemotePeerStream::new(sender_id, ctx_clone)),
                        )
                    };
                    let _ = stream_arc.audio_player.clone_sender().try_send(payload);
                }
            }

            slot_reader.alive.store(false, Ordering::Relaxed);
            slot_reader.wakeup.notify_all();

            let disconnected_peer = slot_reader.remote_peer_id.load(Ordering::Relaxed);
            if disconnected_peer != 0 {
                let still_connected_elsewhere = hub_reader
                    .connections
                    .lock()
                    .map(|conns| {
                        conns.iter().any(|c| {
                            c.conn_id != slot_reader.conn_id
                                && c.alive.load(Ordering::Relaxed)
                                && c.remote_peer_id.load(Ordering::Relaxed) == disconnected_peer
                        })
                    })
                    .unwrap_or(false);

                if !still_connected_elsewhere {
                    if let Ok(mut peers_map) = hub_reader.peers.lock() {
                        peers_map.remove(&disconnected_peer);
                    }
                    if let Ok(mut streams) = hub_reader.remote_streams.lock() {
                        streams.remove(&disconnected_peer);
                    }

                    let bye_pkt = build_packet(MAGIC_LEFT_ROOM, disconnected_peer, 0, 0, 0, &[0]);
                    if let Ok(mut conns) = hub_reader.connections.lock() {
                        conns.retain(|c| c.alive.load(Ordering::Relaxed));
                        for c in conns.iter() {
                            c.push_control(Arc::clone(&bye_pkt));
                        }
                    }
                }
            }

            hub_reader.ctx.request_repaint();
        });
    }
}

impl StreamAudioPlayer {
    pub fn clone_sender(&self) -> std::sync::mpsc::SyncSender<Vec<u8>> {
        self.tx.clone()
    }
}
