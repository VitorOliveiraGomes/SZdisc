use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::Context;
use zune_jpeg::JpegDecoder;

use crate::audio::StreamAudioPlayer;

pub const DEFAULT_STREAM_PORT: u16 = 47830;
pub const LAN_DISCOVERY_PORT: u16 = 47831;
pub const GLOBAL_LOBBY_BORE_PORT: u16 = 47829;
const BORE_RELAY_HOST: &str = "bore.pub:7835";

const MAGIC_HELLO: &[u8; 4] = b"HLO5";
const MAGIC_INFO: &[u8; 4] = b"INF5";
const MAGIC_VIDEO: &[u8; 4] = b"VID5";
const MAGIC_AUDIO: &[u8; 4] = b"AUD5";
const MAGIC_STOP_STREAM: &[u8; 4] = b"STP5";
const MAGIC_LEFT_ROOM: &[u8; 4] = b"BYE5";

#[derive(Clone, Debug)]
pub struct DiscoveredRoom {
    pub host_peer_id: u32,
    pub current_room: String,
    pub addr: String,
    pub res_label: String,
    pub is_sharing: bool,
    pub is_radmin: bool,
    pub last_seen: Instant,
}

/// Representa qualquer amigo online no Servidor (mesmo que esteja em outra sala ou no lobby)
#[derive(Clone, Debug)]
pub struct ServerMemberPresence {
    pub peer_id: u32,
    pub name: String,
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

/// Estado de recepção de vídeo e áudio de um participante específico na sala
pub struct RemotePeerStream {
    pub latest_frame: Mutex<Option<DecodedRemoteFrame>>,
    pub volume_percent: Arc<AtomicU32>,
    pub muted: Arc<AtomicBool>,
    pub audio_player: StreamAudioPlayer,
}

impl RemotePeerStream {
    fn new() -> Arc<Self> {
        let volume_percent = Arc::new(AtomicU32::new(100));
        let muted = Arc::new(AtomicBool::new(false));
        let audio_player =
            StreamAudioPlayer::start(Arc::clone(&volume_percent), Arc::clone(&muted));
        Arc::new(Self {
            latest_frame: Mutex::new(None),
            volume_percent,
            muted,
            audio_player,
        })
    }
}

impl Drop for RemotePeerStream {
    fn drop(&mut self) {
        self.audio_player.stop();
    }
}

struct PeerConnectionSlot {
    conn_id: u64,
    is_outbound: bool,
    remote_peer_id: AtomicU32,
    control_queue: Mutex<VecDeque<Arc<Vec<u8>>>>,
    audio_queue: Mutex<VecDeque<Arc<Vec<u8>>>>,
    latest_video_by_sender: Mutex<HashMap<u32, Arc<Vec<u8>>>>,
    wakeup: Condvar,
    alive: AtomicBool,
}

impl PeerConnectionSlot {
    fn new(conn_id: u64, is_outbound: bool) -> Arc<Self> {
        Arc::new(Self {
            conn_id,
            is_outbound,
            remote_peer_id: AtomicU32::new(0),
            control_queue: Mutex::new(VecDeque::with_capacity(24)),
            audio_queue: Mutex::new(VecDeque::with_capacity(24)),
            latest_video_by_sender: Mutex::new(HashMap::new()),
            wakeup: Condvar::new(),
            alive: AtomicBool::new(true),
        })
    }

    fn push_control(&self, pkt: Arc<Vec<u8>>) {
        if let Ok(mut q) = self.control_queue.lock() {
            if q.len() >= 32 {
                q.pop_front();
            }
            q.push_back(pkt);
            self.wakeup.notify_one();
        }
    }

    fn push_audio(&self, pkt: Arc<Vec<u8>>) {
        if let Ok(mut q) = self.audio_queue.lock() {
            if q.len() >= 18 {
                q.pop_front();
            }
            q.push_back(pkt);
            self.wakeup.notify_one();
        }
    }

    fn push_video(&self, sender_id: u32, pkt: Arc<Vec<u8>>) {
        if let Ok(mut map) = self.latest_video_by_sender.lock() {
            map.insert(sender_id, pkt);
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

/// Calcula a porta dedicada de uma Sala a partir do nome dela!
/// Assim, quando você ou seu amigo clicam na mesma sala (ex: "Sala Geral #1" ou "Tropa"),
/// ambos se encontram automaticamente sem precisar digitar código!
pub fn derive_room_port(room_name: &str) -> u16 {
    let norm = room_name.trim().to_lowercase();
    let seed = format!("SZDISC_V5_ROOM::{}", norm);
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

pub fn get_radmin_ip() -> Option<String> {
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("26.1.1.1:80").is_ok() {
            if let Ok(local_addr) = sock.local_addr() {
                if let IpAddr::V4(ip) = local_addr.ip() {
                    let s = ip.to_string();
                    if s.starts_with("26.") {
                        return Some(s);
                    }
                }
            }
        }
    }
    None
}

pub fn get_local_ip() -> String {
    if let Some(radmin) = get_radmin_ip() {
        return radmin;
    }
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(local_addr) = sock.local_addr() {
                if let IpAddr::V4(ip) = local_addr.ip() {
                    if !ip.is_loopback() {
                        return ip.to_string();
                    }
                }
            }
        }
    }
    "127.0.0.1".to_string()
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
    room_name: &str,
    known_rooms: &[String],
) -> Arc<Vec<u8>> {
    let share_flag = if info.is_sharing { "1" } else { "0" };
    let clean_name = info.name.replace('\t', " ").replace('\n', " ");
    let clean_res = info.res_label.replace('\t', " ").replace('\n', " ");
    let clean_room = room_name.replace('\t', " ").replace('\n', " ");
    let rooms_csv = known_rooms
        .iter()
        .map(|r| r.replace('\t', " ").replace(';', " ").trim().to_string())
        .filter(|r| !r.is_empty())
        .collect::<Vec<_>>()
        .join(";");
    let text = format!(
        "{}\t{}\t{}\t{}\t{}",
        clean_name, share_flag, clean_res, clean_room, rooms_csv
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
    while filled < buf.len() {
        if !alive.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed) {
            return false;
        }
        match stream.read(&mut buf[filled..]) {
            Ok(0) => return false,
            Ok(n) => filled += n,
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::Interrupted =>
            {
                thread::sleep(Duration::from_millis(5));
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
                if retries > 40 {
                    return false;
                }
                thread::sleep(Duration::from_millis(15));
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

/// Hub de Salas Estilo Discord:
/// - Salas persistentes por nome (sem precisar digitar código!).
/// - Qualquer pessoa pode criar uma sala ou entrar/sair a hora que quiser.
/// - Sincroniza as salas criadas e quem está em cada sala automaticamente via Lobby Global + Radmin/LAN.
pub struct RoomHub {
    pub my_peer_id: Arc<AtomicU32>,
    pub bound_port: u16,
    pub listen_addr: String,
    pub radmin_addr: Mutex<Option<String>>,
    pub in_room: Arc<AtomicBool>,
    pub active_room_name: Arc<Mutex<String>>,
    pub desired_bore_port: Arc<AtomicU16>,
    pub holding_bore_port: Arc<AtomicU16>,
    pub known_rooms: Arc<Mutex<Vec<String>>>,
    pub server_members: Arc<Mutex<HashMap<u32, ServerMemberPresence>>>,
    pub lan_discovered: Arc<Mutex<HashMap<u32, DiscoveredRoom>>>,
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
        initial_rooms: Vec<String>,
        ctx: Context,
    ) -> Result<Arc<Self>, String> {
        let mut bound_listener = None;
        let mut bound_port = DEFAULT_STREAM_PORT;

        for p in DEFAULT_STREAM_PORT..=DEFAULT_STREAM_PORT + 15 {
            if let Ok(l) = TcpListener::bind(("0.0.0.0", p)) {
                bound_listener = Some(l);
                bound_port = p;
                break;
            }
        }

        let listener =
            bound_listener.ok_or_else(|| "Não foi possível abrir porta TCP local".to_string())?;
        let _ = listener.set_nonblocking(true);

        let ip = get_local_ip();
        let listen_addr = format!("{}:{}", ip, bound_port);
        let radmin_addr = get_radmin_ip().map(|rip| format!("{}:{}", rip, bound_port));

        let default_room = initial_rooms
            .first()
            .cloned()
            .unwrap_or_else(|| "Sala Geral #1".to_string());
        let initial_port = derive_room_port(&default_room);

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
            bound_port,
            listen_addr,
            radmin_addr: Mutex::new(radmin_addr),
            in_room: Arc::new(AtomicBool::new(true)),
            active_room_name: Arc::new(Mutex::new(default_room)),
            desired_bore_port: Arc::new(AtomicU16::new(initial_port)),
            holding_bore_port: Arc::new(AtomicU16::new(0)),
            known_rooms: Arc::new(Mutex::new(initial_rooms)),
            server_members: Arc::new(Mutex::new(HashMap::new())),
            lan_discovered: Arc::new(Mutex::new(HashMap::new())),
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

        // 1. Thread que aceita conexões TCP locais / Radmin VPN
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

        // 2. Thread de escuta UDP (Radmin VPN + LAN) para descobrir salas e amigos automaticamente
        let hub_udp_rx = Arc::clone(&hub);
        thread::spawn(move || {
            hub_udp_rx.udp_discovery_listener_loop();
        });

        // 3. Thread Automática da Sala via Internet (bore.pub com porta determinística da sala!)
        // Se ninguém estiver hospedando a sala no momento, assume o túnel da sala;
        // se um amigo já estiver na sala, conecta nele automaticamente sem precisar de código!
        let hub_tunnel = Arc::clone(&hub);
        thread::spawn(move || {
            hub_tunnel.room_internet_auto_mesh_loop();
        });

        // 4. Thread do Lobby Global do Servidor (sincroniza salas criadas e membros online em outras salas)
        let hub_lobby = Arc::clone(&hub);
        thread::spawn(move || {
            hub_lobby.global_lobby_sync_loop();
        });

        // 5. Thread de Heartbeat (1.2s), limpeza de fantasmas e anúncio no Radmin VPN + LAN
        let hub_beacon = Arc::clone(&hub);
        thread::spawn(move || {
            let udp_any = UdpSocket::bind("0.0.0.0:0").ok();
            if let Some(ref s) = udp_any {
                let _ = s.set_broadcast(true);
            }

            while !hub_beacon.stop_flag.load(Ordering::Relaxed) {
                if let Some(rip) = get_radmin_ip() {
                    if let Ok(mut g) = hub_beacon.radmin_addr.lock() {
                        *g = Some(format!("{}:{}", rip, hub_beacon.bound_port));
                    }
                }

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

                let curr_room = hub_beacon
                    .active_room_name
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                let in_room = hub_beacon.in_room.load(Ordering::Relaxed);
                let rooms_snapshot = hub_beacon
                    .known_rooms
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();

                if let Ok(info) = hub_beacon.my_info.lock() {
                    if in_room {
                        let pkt = build_info_or_hello_packet(
                            MAGIC_INFO,
                            &info,
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

                    // Anuncia presença e lista de salas no Radmin VPN + LAN
                    let my_id = hub_beacon.my_peer_id.load(Ordering::Relaxed);
                    let clean_name = info.name.replace('|', " ");
                    let clean_room = if in_room {
                        curr_room.replace('|', " ")
                    } else {
                        String::new()
                    };
                    let clean_res = if info.is_sharing {
                        info.res_label.replace('|', " ")
                    } else {
                        "Na sala".to_string()
                    };
                    let share_bit = if info.is_sharing { "1" } else { "0" };
                    let rooms_csv = rooms_snapshot
                        .iter()
                        .map(|r| r.replace('|', " ").replace(';', " ").trim().to_string())
                        .filter(|r| !r.is_empty())
                        .collect::<Vec<_>>()
                        .join(";");

                    let payload = format!(
                        "DISC5|{}|{}|{}|{}|{}|{}|{}",
                        my_id,
                        hub_beacon.bound_port,
                        clean_name,
                        clean_room,
                        share_bit,
                        clean_res,
                        rooms_csv
                    );

                    if let Some(ref s) = udp_any {
                        let _ = s.send_to(
                            payload.as_bytes(),
                            ("255.255.255.255", LAN_DISCOVERY_PORT),
                        );
                        let _ = s.send_to(
                            payload.as_bytes(),
                            ("26.255.255.255", LAN_DISCOVERY_PORT),
                        );
                    }

                    if let Some(radmin_ip) = get_radmin_ip() {
                        if let Ok(rsock) = UdpSocket::bind((radmin_ip.as_str(), 0)) {
                            let _ = rsock.set_broadcast(true);
                            let _ = rsock.send_to(
                                payload.as_bytes(),
                                ("26.255.255.255", LAN_DISCOVERY_PORT),
                            );
                            let _ = rsock.send_to(
                                payload.as_bytes(),
                                ("255.255.255.255", LAN_DISCOVERY_PORT),
                            );
                        }
                    }
                }

                // Se houver alguém no Radmin/LAN na MESMA sala que nós e ainda não estivermos conectados a ele, conecta automaticamente!
                if in_room && !curr_room.is_empty() {
                    let lan_target = hub_beacon.lan_discovered.lock().ok().and_then(|map| {
                        for d in map.values() {
                            if d.current_room.eq_ignore_ascii_case(&curr_room)
                                && d.last_seen.elapsed() < Duration::from_secs(4)
                            {
                                let already_conn = hub_beacon
                                    .connections
                                    .lock()
                                    .map(|cs| {
                                        cs.iter().any(|c| {
                                            c.alive.load(Ordering::Relaxed)
                                                && c.remote_peer_id.load(Ordering::Relaxed)
                                                    == d.host_peer_id
                                        })
                                    })
                                    .unwrap_or(false);
                                if !already_conn {
                                    return Some(d.addr.clone());
                                }
                            }
                        }
                        None
                    });

                    if let Some(addr) = lan_target {
                        hub_beacon.spawn_outbound_connect(addr, true);
                    }
                }

                thread::sleep(Duration::from_millis(1200));
            }
        });

        Ok(hub)
    }

    fn udp_discovery_listener_loop(self: Arc<Self>) {
        let sock = match UdpSocket::bind(("0.0.0.0", LAN_DISCOVERY_PORT)) {
            Ok(s) => s,
            Err(_) => return,
        };
        let _ = sock.set_read_timeout(Some(Duration::from_millis(800)));

        let mut buf = [0u8; 1024];
        while !self.stop_flag.load(Ordering::Relaxed) {
            if let Ok((len, src)) = sock.recv_from(&mut buf) {
                if let Ok(msg) = std::str::from_utf8(&buf[..len]) {
                    let parts: Vec<&str> = msg.split('|').collect();
                    // DISC5|peer_id|port|host_name|current_room|is_sharing|res_label|rooms_csv
                    if parts.len() >= 8 && parts[0] == "DISC5" {
                        let host_peer_id = parts[1].parse::<u32>().unwrap_or(0);
                        let my_id = self.my_peer_id.load(Ordering::Relaxed);
                        if host_peer_id != 0 && host_peer_id != my_id {
                            let port = parts[2];
                            let host_name = parts[3].trim().to_string();
                            let current_room = parts[4].trim().to_string();
                            let is_sharing = parts[5] == "1";
                            let res_label = parts[6].trim().to_string();
                            let rooms_csv = parts[7];

                            self.merge_known_rooms_from_csv(rooms_csv);
                            if !current_room.is_empty() {
                                self.merge_known_rooms_from_csv(&current_room);
                            }

                            if let Ok(mut members) = self.server_members.lock() {
                                members.insert(
                                    host_peer_id,
                                    ServerMemberPresence {
                                        peer_id: host_peer_id,
                                        name: host_name.clone(),
                                        current_room: current_room.clone(),
                                        is_sharing,
                                        res_label: res_label.clone(),
                                        last_seen: Instant::now(),
                                    },
                                );
                            }

                            let ip_str = src.ip().to_string();
                            let is_radmin = ip_str.starts_with("26.");
                            let addr = format!("{}:{}", ip_str, port);

                            if let Ok(mut map) = self.lan_discovered.lock() {
                                let should_update = match map.get(&host_peer_id) {
                                    Some(existing) if existing.is_radmin && !is_radmin => false,
                                    _ => true,
                                };
                                if should_update {
                                    map.insert(
                                        host_peer_id,
                                        DiscoveredRoom {
                                            host_peer_id,
                                            current_room,
                                            addr,
                                            res_label,
                                            is_sharing,
                                            is_radmin,
                                            last_seen: Instant::now(),
                                        },
                                    );
                                } else if let Some(existing) = map.get_mut(&host_peer_id) {
                                    existing.last_seen = Instant::now();
                                    existing.current_room = current_room;
                                    existing.is_sharing = is_sharing;
                                    existing.res_label = res_label;
                                }
                            }
                            self.ctx.request_repaint();
                        }
                    }
                }
            }

            if let Ok(mut map) = self.lan_discovered.lock() {
                map.retain(|_, v| v.last_seen.elapsed() < Duration::from_secs(4));
            }
        }
    }

    fn merge_known_rooms_from_csv(&self, csv: &str) {
        if csv.trim().is_empty() {
            return;
        }
        if let Ok(mut rooms) = self.known_rooms.lock() {
            let mut existing_lower: HashSet<String> =
                rooms.iter().map(|r| r.to_lowercase()).collect();
            for item in csv.split(';') {
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

    /// Loop automático da Sala via Internet (Sem precisar de código!):
    /// Usa a porta determinística da sala (`desired_bore_port`) no `bore.pub`.
    /// - Se a sala estiver livre na internet, registra a porta no `bore.pub` e aceita amigos que clicarem na sala.
    /// - Se um amigo já estiver na sala segurando a porta no `bore.pub`, conecta automaticamente nele como cliente!
    fn room_internet_auto_mesh_loop(self: Arc<Self>) {
        while !self.stop_flag.load(Ordering::Relaxed) {
            if !self.in_room.load(Ordering::Relaxed) {
                self.holding_bore_port.store(0, Ordering::Relaxed);
                thread::sleep(Duration::from_millis(500));
                continue;
            }

            let target_port = self.desired_bore_port.load(Ordering::Relaxed);
            if target_port == 0 {
                thread::sleep(Duration::from_millis(500));
                continue;
            }

            // Se já temos uma conexão ativa de saída para a sala na internet, aguarda enquanto ela estiver viva
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

            while !self.stop_flag.load(Ordering::Relaxed)
                && self.in_room.load(Ordering::Relaxed)
                && self.desired_bore_port.load(Ordering::Relaxed) == target_port
            {
                match ctrl.read(&mut temp) {
                    Ok(0) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&temp[..n]);
                        while let Some(pos) = pending.iter().position(|&b| b == 0) {
                            let msg_bytes: Vec<u8> = pending.drain(..=pos).collect();
                            let s = String::from_utf8_lossy(&msg_bytes[..msg_bytes.len() - 1]);

                            if s.contains("\"Error\"") {
                                // A porta da sala já está aberta pelo nosso amigo! Conectamos nele automaticamente!
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
                        continue;
                    }
                    Err(_) => break,
                }
            }

            self.holding_bore_port.store(0, Ordering::Relaxed);
            drop(ctrl);

            if port_already_in_use
                && self.in_room.load(Ordering::Relaxed)
                && self.desired_bore_port.load(Ordering::Relaxed) == target_port
            {
                let bore_room_target = format!("bore.pub:{}", target_port);
                self.spawn_outbound_connect(bore_room_target, true);
                thread::sleep(Duration::from_millis(1500));
            } else {
                thread::sleep(Duration::from_millis(800));
            }
        }
    }

    /// Sincroniza a lista de salas e quem está online em cada sala pela internet (`GLOBAL_LOBBY_BORE_PORT`)
    fn global_lobby_sync_loop(self: Arc<Self>) {
        let lobby_clients: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));

        while !self.stop_flag.load(Ordering::Relaxed) {
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
            let _ = ctrl.set_read_timeout(Some(Duration::from_secs(4)));

            let hello_cmd = format!("{{\"Hello\":{}}}\0", GLOBAL_LOBBY_BORE_PORT);
            if ctrl.write_all(hello_cmd.as_bytes()).is_err() {
                thread::sleep(Duration::from_secs(2));
                continue;
            }

            let mut pending = Vec::with_capacity(512);
            let mut temp = [0u8; 256];
            let mut lobby_already_hosted = false;

            while !self.stop_flag.load(Ordering::Relaxed) {
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
                        // Envia nosso status para todos os clientes do Lobby
                        let line = self.build_lobby_line();
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
                // Conecta como cliente ao Lobby Global
                if let Ok( lobby_addr ) = format!("bore.pub:{}", GLOBAL_LOBBY_BORE_PORT)
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
                        self.handle_lobby_stream(stream, None);
                    }
                }
                thread::sleep(Duration::from_secs(2));
            } else {
                thread::sleep(Duration::from_secs(2));
            }
        }
    }

    fn build_lobby_line(&self) -> String {
        let my_id = self.my_peer_id.load(Ordering::Relaxed);
        let in_room = self.in_room.load(Ordering::Relaxed);
        let curr_room = if in_room {
            self.active_room_name
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let (name, is_sharing, res_label) = self
            .my_info
            .lock()
            .map(|i| (i.name.clone(), i.is_sharing, i.res_label.clone()))
            .unwrap_or_else(|_| ("Jogador".to_string(), false, "Na sala".to_string()));
        let rooms_csv = self
            .known_rooms
            .lock()
            .map(|rs| {
                rs.iter()
                    .map(|r| r.replace('|', " ").replace(';', " ").trim().to_string())
                    .filter(|r| !r.is_empty())
                    .collect::<Vec<_>>()
                    .join(";")
            })
            .unwrap_or_default();

        format!(
            "LBY5|{}|{}|{}|{}|{}|{}\n",
            my_id,
            name.replace('|', " ").replace('\n', " "),
            curr_room.replace('|', " ").replace('\n', " "),
            if is_sharing { "1" } else { "0" },
            res_label.replace('|', " ").replace('\n', " "),
            rooms_csv.replace('\n', " ")
        )
    }

    fn handle_lobby_stream(
        &self,
        mut stream: TcpStream,
        relay_clients: Option<Arc<Mutex<Vec<TcpStream>>>>,
    ) {
        let mut pending = Vec::with_capacity(1024);
        let mut buf = [0u8; 512];
        let mut last_send = Instant::now() - Duration::from_secs(5);

        while !self.stop_flag.load(Ordering::Relaxed) {
            if last_send.elapsed() >= Duration::from_millis(1600) {
                let line = self.build_lobby_line();
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
                            if parts.len() >= 7 && parts[0] == "LBY5" {
                                let pid = parts[1].parse::<u32>().unwrap_or(0);
                                let my_id = self.my_peer_id.load(Ordering::Relaxed);
                                if pid != 0 && pid != my_id {
                                    let uname = parts[2].trim().to_string();
                                    let uroom = parts[3].trim().to_string();
                                    let usharing = parts[4] == "1";
                                    let ures = parts[5].trim().to_string();
                                    let ucsv = parts[6];

                                    self.merge_known_rooms_from_csv(ucsv);
                                    if !uroom.is_empty() {
                                        self.merge_known_rooms_from_csv(&uroom);
                                    }

                                    if let Ok(mut members) = self.server_members.lock() {
                                        members.insert(
                                            pid,
                                            ServerMemberPresence {
                                                peer_id: pid,
                                                name: uname,
                                                current_room: uroom,
                                                is_sharing: usharing,
                                                res_label: ures,
                                                last_seen: Instant::now(),
                                            },
                                        );
                                    }

                                    // Se somos o Host do Lobby, repassa a atualização para os demais amigos conectados
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

    /// Cria uma nova sala no servidor e já entra nela automaticamente!
    pub fn create_and_enter_room(self: &Arc<Self>, room_name: &str) -> Vec<String> {
        let clean = room_name.trim().to_string();
        if clean.is_empty() {
            return self
                .known_rooms
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default();
        }

        let updated_list = {
            let mut rooms = self.known_rooms.lock().unwrap();
            if !rooms.iter().any(|r| r.eq_ignore_ascii_case(&clean)) {
                rooms.push(clean.clone());
            }
            rooms.clone()
        };

        self.enter_named_room(&clean);
        updated_list
    }

    /// Entra em qualquer sala do servidor com 1 clique (sem precisar de código!)
    pub fn enter_named_room(self: &Arc<Self>, room_name: &str) {
        let clean = room_name.trim().to_string();
        if clean.is_empty() {
            return;
        }

        let current = self
            .active_room_name
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        if self.in_room.load(Ordering::Relaxed) && current.eq_ignore_ascii_case(&clean) {
            return;
        }

        self.disconnect_all_connections();
        if let Ok(mut active) = self.active_room_name.lock() {
            *active = clean.clone();
        }
        let port = derive_room_port(&clean);
        self.desired_bore_port.store(port, Ordering::Relaxed);
        self.in_room.store(true, Ordering::Relaxed);
        self.ctx.request_repaint();
    }

    /// Também permite conectar manualmente por IP/Código caso queira
    pub fn join_manual_address(self: &Arc<Self>, raw_target: &str) {
        let trimmed = raw_target.trim().trim_start_matches('#');
        if trimmed.is_empty() {
            return;
        }

        let addr_str = if !trimmed.contains('.') && !trimmed.contains(':') {
            format!("bore.pub:{}", trimmed)
        } else if trimmed.contains(':') {
            trimmed.to_string()
        } else {
            format!("{}:{}", trimmed, DEFAULT_STREAM_PORT)
        };

        self.disconnect_all_connections();
        self.in_room.store(true, Ordering::Relaxed);
        self.spawn_outbound_connect(addr_str, false);
    }

    fn spawn_outbound_connect(
        self: &Arc<Self>,
        addr_str: String,
        is_silent_auto: bool,
    ) {
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
        if let Ok(mut conns) = self.connections.lock() {
            for c in conns.iter() {
                c.push_control(Arc::clone(&bye_pkt));
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
        self.disconnect_all_connections();
        self.ctx.request_repaint();
    }

    pub fn update_my_status(
        &self,
        name: String,
        user_tag: &str,
        is_sharing: bool,
        res_label: String,
    ) {
        let new_id = derive_persistent_peer_id(&name, user_tag);
        self.my_peer_id.store(new_id, Ordering::Relaxed);

        let curr_room = self
            .active_room_name
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let rooms_snapshot = self
            .known_rooms
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        let mut stopped_sharing = false;
        let pkt = {
            let Ok(mut info) = self.my_info.lock() else {
                return;
            };
            if info.is_sharing && !is_sharing {
                stopped_sharing = true;
            }
            info.peer_id = new_id;
            info.name = name;
            info.is_sharing = is_sharing;
            info.res_label = res_label;
            info.last_seen = Instant::now();
            build_info_or_hello_packet(MAGIC_INFO, &info, &curr_room, &rooms_snapshot)
        };

        let stop_pkt = if stopped_sharing {
            Some(build_packet(MAGIC_STOP_STREAM, new_id, 0, 0, 0, &[0]))
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

        let curr_room = self
            .active_room_name
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let rooms_snapshot = self
            .known_rooms
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        if let Ok(info) = self.my_info.lock() {
            slot.push_control(build_info_or_hello_packet(
                MAGIC_HELLO,
                &info,
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
                        &curr_room,
                        &rooms_snapshot,
                    ));
                }
            }
        }

        if let Ok(mut conns) = self.connections.lock() {
            conns.push(Arc::clone(&slot));
        }

        // Thread de ESCRITA
        let slot_writer = Arc::clone(&slot);
        thread::spawn(move || {
            while slot_writer.alive.load(Ordering::Relaxed) {
                let mut sent_anything = false;

                loop {
                    let next_ctrl = slot_writer
                        .control_queue
                        .lock()
                        .ok()
                        .and_then(|mut q| q.pop_front());
                    if let Some(pkt) = next_ctrl {
                        sent_anything = true;
                        if !write_all_interruptible(&mut write_stream, &pkt, &slot_writer.alive) {
                            slot_writer.alive.store(false, Ordering::Relaxed);
                            return;
                        }
                    } else {
                        break;
                    }
                }

                loop {
                    let next_aud = slot_writer
                        .audio_queue
                        .lock()
                        .ok()
                        .and_then(|mut q| q.pop_front());
                    if let Some(pkt) = next_aud {
                        sent_anything = true;
                        if !write_all_interruptible(&mut write_stream, &pkt, &slot_writer.alive) {
                            slot_writer.alive.store(false, Ordering::Relaxed);
                            return;
                        }
                    } else {
                        break;
                    }
                }

                let video_pkts: Vec<Arc<Vec<u8>>> = {
                    if let Ok(mut map) = slot_writer.latest_video_by_sender.lock() {
                        map.drain().map(|(_, v)| v).collect()
                    } else {
                        Vec::new()
                    }
                };

                for pkt in video_pkts {
                    sent_anything = true;
                    if !write_all_interruptible(&mut write_stream, &pkt, &slot_writer.alive) {
                        slot_writer.alive.store(false, Ordering::Relaxed);
                        return;
                    }
                    let _ = write_stream.flush();
                }

                if !sent_anything {
                    if let Ok(guard) = slot_writer.latest_video_by_sender.lock() {
                        if guard.is_empty() {
                            let _ = slot_writer
                                .wakeup
                                .wait_timeout(guard, Duration::from_millis(45));
                        }
                    }
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
                    if let Ok(mut conns) = hub_reader.connections.lock() {
                        for other in conns.iter() {
                            if other.conn_id != slot_reader.conn_id
                                && other.remote_peer_id.load(Ordering::Relaxed) == sender_id
                            {
                                other.alive.store(false, Ordering::Relaxed);
                                other.wakeup.notify_all();
                            }
                        }
                        conns.retain(|c| c.alive.load(Ordering::Relaxed));
                    }
                }

                let direct_peer_id = slot_reader.remote_peer_id.load(Ordering::Relaxed);

                // Repassa pacotes diretos para os outros membros da mesma sala
                if sender_id == direct_peer_id {
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
                        if parts.len() >= 3 {
                            let peer_name = parts[0].trim().to_string();
                            let is_sharing = parts[1] == "1";
                            let res_label = parts[2].to_string();
                            if parts.len() >= 5 {
                                hub_reader.merge_known_rooms_from_csv(parts[4]);
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
                    let mut decoder = JpegDecoder::new(&payload);
                    if let Ok(rgb) = decoder.decode() {
                        let w = width as u32;
                        let h = height as u32;
                        let pixel_count = (w * h) as usize;
                        if rgb.len() >= pixel_count * 3 {
                            let mut rgba = vec![0u8; pixel_count * 4];
                            for i in 0..pixel_count {
                                rgba[i * 4] = rgb[i * 3];
                                rgba[i * 4 + 1] = rgb[i * 3 + 1];
                                rgba[i * 4 + 2] = rgb[i * 3 + 2];
                                rgba[i * 4 + 3] = 255;
                            }

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
                                Arc::clone(
                                    streams
                                        .entry(sender_id)
                                        .or_insert_with(RemotePeerStream::new),
                                )
                            };

                            if let Ok(mut latest) = stream_arc.latest_frame.lock() {
                                *latest = Some(DecodedRemoteFrame {
                                    width: w,
                                    height: h,
                                    byte_len: payload_len,
                                    rgba,
                                });
                            }
                            hub_reader.ctx.request_repaint();
                        }
                    }
                } else if &magic == MAGIC_AUDIO {
                    let stream_arc = {
                        let mut streams = hub_reader.remote_streams.lock().unwrap();
                        Arc::clone(
                            streams
                                .entry(sender_id)
                                .or_insert_with(RemotePeerStream::new),
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
