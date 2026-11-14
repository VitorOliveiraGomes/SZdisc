use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::Context;
use zune_jpeg::JpegDecoder;

use crate::audio::StreamAudioPlayer;

pub const DEFAULT_STREAM_PORT: u16 = 47830;
pub const LAN_DISCOVERY_PORT: u16 = 47831;
const BORE_RELAY_HOST: &str = "bore.pub:7835";

const MAGIC_HELLO: &[u8; 4] = b"HLO4";
const MAGIC_INFO: &[u8; 4] = b"INF4";
const MAGIC_VIDEO: &[u8; 4] = b"VID4";
const MAGIC_AUDIO: &[u8; 4] = b"AUD4";
const MAGIC_STOP_STREAM: &[u8; 4] = b"STP4";
const MAGIC_LEFT_ROOM: &[u8; 4] = b"BYE4";

#[derive(Clone, Debug)]
pub struct DiscoveredRoom {
    pub host_peer_id: u32,
    pub host_name: String,
    pub addr: String,
    pub res_label: String,
    pub peer_count: usize,
    pub is_radmin: bool,
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

/// Fila de envio por conexão TCP (Full-Duplex).
/// Sobrescreve sempre o quadro de vídeo pendente por `sender_id` para nunca acumular atraso!
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

/// Gera um ID fixo por máquina + conta para NUNCA duplicar o mesmo usuário ao reconectar ou reiniciar o app!
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

    // FNV-1a 32-bit hash
    let mut hash: u32 = 2166136261;
    for b in seed.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16777619);
    }
    if hash == 0 { 1 } else { hash }
}

/// Detecta especificamente o IP do Radmin VPN (faixa 26.x.x.x) se o Radmin estiver aberto
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

pub fn start_lan_discovery_listener(
    my_peer_id: Arc<AtomicU32>,
) -> Arc<Mutex<HashMap<u32, DiscoveredRoom>>> {
    let rooms: Arc<Mutex<HashMap<u32, DiscoveredRoom>>> = Arc::new(Mutex::new(HashMap::new()));
    let rooms_clone = Arc::clone(&rooms);

    thread::spawn(move || {
        let sock = match UdpSocket::bind(("0.0.0.0", LAN_DISCOVERY_PORT)) {
            Ok(s) => s,
            Err(_) => return,
        };
        let _ = sock.set_read_timeout(Some(Duration::from_millis(800)));

        let mut buf = [0u8; 512];
        loop {
            if let Ok((len, src)) = sock.recv_from(&mut buf) {
                if let Ok(msg) = std::str::from_utf8(&buf[..len]) {
                    let parts: Vec<&str> = msg.split('|').collect();
                    // Formato: DISC4|peer_id|port|host_name|res_label|peer_count
                    if parts.len() >= 6 && parts[0] == "DISC4" {
                        let host_peer_id = parts[1].parse::<u32>().unwrap_or(0);
                        let current_my_id = my_peer_id.load(Ordering::Relaxed);
                        if host_peer_id != 0 && host_peer_id != current_my_id {
                            let port = parts[2];
                            let host_name = parts[3].to_string();
                            let res_label = parts[4].to_string();
                            let peer_count = parts[5].parse::<usize>().unwrap_or(1);
                            let ip_str = src.ip().to_string();
                            let is_radmin = ip_str.starts_with("26.");
                            let addr = format!("{}:{}", ip_str, port);

                            if let Ok(mut map) = rooms_clone.lock() {
                                // Prefere manter o endereço Radmin (26.x.x.x) se recebermos de duas interfaces
                                let should_update = match map.get(&host_peer_id) {
                                    Some(existing) if existing.is_radmin && !is_radmin => false,
                                    _ => true,
                                };
                                if should_update {
                                    map.insert(
                                        host_peer_id,
                                        DiscoveredRoom {
                                            host_peer_id,
                                            host_name,
                                            addr,
                                            res_label,
                                            peer_count,
                                            is_radmin,
                                            last_seen: Instant::now(),
                                        },
                                    );
                                } else if let Some(existing) = map.get_mut(&host_peer_id) {
                                    existing.last_seen = Instant::now();
                                    existing.peer_count = peer_count;
                                    existing.res_label = res_label;
                                }
                            }
                        }
                    }
                }
            }

            if let Ok(mut map) = rooms_clone.lock() {
                map.retain(|_, v| v.last_seen.elapsed() < Duration::from_secs(4));
            }
        }
    });

    rooms
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

fn build_info_or_hello_packet(magic: &[u8; 4], info: &PeerInfo) -> Arc<Vec<u8>> {
    let share_flag = if info.is_sharing { "1" } else { "0" };
    let clean_name = info.name.replace('\t', " ").replace('\n', " ");
    let clean_res = info.res_label.replace('\t', " ").replace('\n', " ");
    let text = format!("{}\t{}\t{}", clean_name, share_flag, clean_res);
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

/// Hub Central da Sala (Bidirecional / Estilo Discord):
/// - Garante que Sockets aceitos pelo `TcpListener` fiquem em modo BLOCKING (`set_nonblocking(false)`),
///   eliminando quedas repentinas de transmissão (`WouldBlock`)!
/// - Evita loops de relay e contas duplicadas usando ID persistente + limpeza automática de participantes inativos.
/// - Suporta botão "Sair da Sala" tanto para quem entrou quanto para o dono da sala!
pub struct RoomHub {
    pub my_peer_id: Arc<AtomicU32>,
    pub bound_port: u16,
    pub listen_addr: String,
    pub radmin_addr: Mutex<Option<String>>,
    pub internet_code: Arc<Mutex<Option<String>>>,
    pub in_room: Arc<AtomicBool>,
    pub joined_room_label: Arc<Mutex<Option<String>>>,
    pub target_outbound_addr: Arc<Mutex<Option<String>>>,
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
            internet_code: Arc::new(Mutex::new(None)),
            in_room: Arc::new(AtomicBool::new(true)),
            joined_room_label: Arc::new(Mutex::new(None)),
            target_outbound_addr: Arc::new(Mutex::new(None)),
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
                            // Se o usuário saiu da sala ("Desconectado"), recusa novas entradas até ele voltar
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

        // 2. Thread do túnel de Internet (Código de 5 dígitos sem Radmin via bore.pub)
        let hub_tunnel = Arc::clone(&hub);
        thread::spawn(move || {
            hub_tunnel.internet_tunnel_loop();
        });

        // 3. Thread de Heartbeat (1.2s), limpeza de fantasmas e anúncio no Radmin VPN + LAN
        let hub_beacon = Arc::clone(&hub);
        thread::spawn(move || {
            let udp_any = UdpSocket::bind("0.0.0.0:0").ok();
            if let Some(ref s) = udp_any {
                let _ = s.set_broadcast(true);
            }

            while !hub_beacon.stop_flag.load(Ordering::Relaxed) {
                // Atualiza IP do Radmin caso o usuário tenha aberto o Radmin VPN depois
                if let Some(rip) = get_radmin_ip() {
                    if let Ok(mut g) = hub_beacon.radmin_addr.lock() {
                        *g = Some(format!("{}:{}", rip, hub_beacon.bound_port));
                    }
                }

                // Remove qualquer participante fantasma que não enviou sinal nos últimos 6 segundos
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

                // Se estamos em uma sala externa e a conexão caiu por oscilação de rede, reconecta automaticamente!
                let need_reconnect = if hub_beacon.in_room.load(Ordering::Relaxed)
                    && !hub_beacon.is_connecting_outbound.load(Ordering::Relaxed)
                {
                    let has_outbound_target = hub_beacon
                        .target_outbound_addr
                        .lock()
                        .ok()
                        .and_then(|g| g.clone());
                    let has_alive_outbound = hub_beacon
                        .connections
                        .lock()
                        .map(|c| c.iter().any(|s| s.is_outbound && s.alive.load(Ordering::Relaxed)))
                        .unwrap_or(false);
                    if !has_alive_outbound {
                        has_outbound_target
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(addr_to_reconnect) = need_reconnect {
                    hub_beacon.spawn_outbound_connect(addr_to_reconnect, None, true);
                }

                // Envia nosso status para todos na sala e anuncia no Radmin VPN + LAN
                if hub_beacon.in_room.load(Ordering::Relaxed) {
                    if let Ok(info) = hub_beacon.my_info.lock() {
                        let pkt = build_info_or_hello_packet(MAGIC_INFO, &info);
                        if let Ok(mut conns) = hub_beacon.connections.lock() {
                            conns.retain(|c| c.alive.load(Ordering::Relaxed));
                            for c in conns.iter() {
                                c.push_control(Arc::clone(&pkt));
                            }
                        }

                        // Só anuncia nossa própria sala na rede se não estivermos como convidado na sala de outra pessoa
                        let is_guest = hub_beacon
                            .joined_room_label
                            .lock()
                            .map(|g| g.is_some())
                            .unwrap_or(false);

                        if !is_guest {
                            let peer_total = hub_beacon
                                .peers
                                .lock()
                                .map(|m| m.len() + 1)
                                .unwrap_or(1);
                            let clean_name = info.name.replace('|', " ");
                            let clean_res = if info.is_sharing {
                                info.res_label.replace('|', " ")
                            } else {
                                "Sala Aberta".to_string()
                            };
                            let my_id = hub_beacon.my_peer_id.load(Ordering::Relaxed);
                            let payload = format!(
                                "DISC4|{}|{}|{}|{}|{}",
                                my_id, hub_beacon.bound_port, clean_name, clean_res, peer_total
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

                            // Envia também diretamente pela interface Radmin (26.x.x.x) para garantir entrega no Radmin VPN!
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
                    }
                }

                thread::sleep(Duration::from_millis(1200));
            }
        });

        Ok(hub)
    }

    fn internet_tunnel_loop(self: Arc<Self>) {
        while !self.stop_flag.load(Ordering::Relaxed) {
            let addr = match BORE_RELAY_HOST
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

            let mut ctrl = match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
                Ok(s) => s,
                Err(_) => {
                    thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };

            let _ = ctrl.set_nodelay(true);
            let _ = ctrl.set_read_timeout(Some(Duration::from_secs(20)));

            if ctrl.write_all(b"{\"Hello\":0}\0").is_err() {
                thread::sleep(Duration::from_secs(2));
                continue;
            }

            let mut pending = Vec::with_capacity(512);
            let mut temp = [0u8; 256];

            while !self.stop_flag.load(Ordering::Relaxed) {
                match ctrl.read(&mut temp) {
                    Ok(0) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&temp[..n]);
                        while let Some(pos) = pending.iter().position(|&b| b == 0) {
                            let msg_bytes: Vec<u8> = pending.drain(..=pos).collect();
                            let s = String::from_utf8_lossy(&msg_bytes[..msg_bytes.len() - 1]);

                            if let Some(port) = parse_bore_hello_port(&s) {
                                if port > 0 {
                                    if let Ok(mut code_guard) = self.internet_code.lock() {
                                        *code_guard = Some(port.to_string());
                                    }
                                    self.ctx.request_repaint();
                                }
                            } else if let Some(uuid) = parse_bore_connection_uuid(&s) {
                                if !self.in_room.load(Ordering::Relaxed) {
                                    continue;
                                }
                                let hub_clone = Arc::clone(&self);
                                thread::spawn(move || {
                                    if let Ok(mut data_stream) =
                                        TcpStream::connect_timeout(&addr, Duration::from_secs(6))
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

            if !self.stop_flag.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_secs(1));
            }
        }
    }

    /// Entra na sala de um amigo (por Radmin, IP ou Código de 5 dígitos),
    /// fechando antes qualquer conexão anterior para nunca misturar ou duplicar contas!
    pub fn join_friend_room(self: &Arc<Self>, raw_target: &str, custom_host_name: Option<String>) {
        let trimmed = raw_target.trim().trim_start_matches('#');
        if trimmed.is_empty() {
            return;
        }

        if self.is_connecting_outbound.load(Ordering::Relaxed) {
            return;
        }

        if let Ok(guard) = self.internet_code.lock() {
            if let Some(my_code) = guard.as_ref() {
                if my_code == trimmed {
                    if let Ok(mut n) = self.status_notice.lock() {
                        *n = Some("Esse já é o código da sua própria sala!".to_string());
                    }
                    return;
                }
            }
        }

        let (addr_str, default_label) = if !trimmed.contains('.') && !trimmed.contains(':') {
            (
                format!("bore.pub:{}", trimmed),
                format!("Sala #{}", trimmed),
            )
        } else if trimmed.contains(':') {
            (trimmed.to_string(), format!("Sala {}", trimmed))
        } else {
            (
                format!("{}:{}", trimmed, DEFAULT_STREAM_PORT),
                format!("Sala {}", trimmed),
            )
        };

        let label = custom_host_name
            .map(|n| format!("Sala de {}", n))
            .unwrap_or(default_label);

        // Limpa conexões antigas antes de entrar na nova sala
        self.disconnect_all_connections();
        self.in_room.store(true, Ordering::Relaxed);

        if let Ok(mut t) = self.target_outbound_addr.lock() {
            *t = Some(addr_str.clone());
        }

        self.spawn_outbound_connect(addr_str, Some(label), false);
    }

    fn spawn_outbound_connect(
        self: &Arc<Self>,
        addr_str: String,
        label_opt: Option<String>,
        is_silent_reconnect: bool,
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
                    if !is_silent_reconnect {
                        if let Ok(mut n) = hub.status_notice.lock() {
                            *n = Some(format!("Endereço inválido: {}", addr_str));
                        }
                        if let Ok(mut t) = hub.target_outbound_addr.lock() {
                            *t = None;
                        }
                    }
                    hub.is_connecting_outbound.store(false, Ordering::SeqCst);
                    hub.ctx.request_repaint();
                    return;
                }
            };

            match TcpStream::connect_timeout(&sock_addr, Duration::from_secs(5)) {
                Ok(stream) => {
                    if let Some(ref lbl) = label_opt {
                        if let Ok(mut joined) = hub.joined_room_label.lock() {
                            *joined = Some(lbl.clone());
                        }
                        if !is_silent_reconnect {
                            if let Ok(mut n) = hub.status_notice.lock() {
                                *n = Some(format!("Conectado em {}!", lbl));
                            }
                        }
                    }
                    hub.is_connecting_outbound.store(false, Ordering::SeqCst);
                    hub.ctx.request_repaint();
                    hub.attach_bidirectional_stream(stream, true);
                }
                Err(e) => {
                    if !is_silent_reconnect {
                        if let Ok(mut t) = hub.target_outbound_addr.lock() {
                            *t = None;
                        }
                        if let Ok(mut joined) = hub.joined_room_label.lock() {
                            *joined = None;
                        }
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
        if let Ok(mut joined) = self.joined_room_label.lock() {
            *joined = None;
        }
        if let Ok(mut peers) = self.peers.lock() {
            peers.clear();
        }
        if let Ok(mut streams) = self.remote_streams.lock() {
            streams.clear();
        }
    }

    /// Botão "📞 Sair da Sala" estilo Discord: desconecta de tudo (seja você Host ou Convidado)
    pub fn leave_room_completely(&self) {
        if let Ok(mut t) = self.target_outbound_addr.lock() {
            *t = None;
        }
        self.in_room.store(false, Ordering::Relaxed);
        self.disconnect_all_connections();
        self.ctx.request_repaint();
    }

    /// Reabre nossa própria sala quando clicamos em "🔊 Entrar na Minha Sala"
    pub fn reopen_my_room(&self) {
        if let Ok(mut t) = self.target_outbound_addr.lock() {
            *t = None;
        }
        if let Ok(mut joined) = self.joined_room_label.lock() {
            *joined = None;
        }
        self.in_room.store(true, Ordering::Relaxed);
        self.ctx.request_repaint();
    }

    pub fn update_my_status(&self, name: String, user_tag: &str, is_sharing: bool, res_label: String) {
        let new_id = derive_persistent_peer_id(&name, user_tag);
        self.my_peer_id.store(new_id, Ordering::Relaxed);

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
            build_info_or_hello_packet(MAGIC_INFO, &info)
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
        // CRÍTICO NO WINDOWS: Sockets vindos de `listener.accept()` herdam `nonblocking = true`!
        // Forçamos `set_nonblocking(false)` para a transmissão NUNCA cair com erro `WouldBlock`!
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

        // Envia nosso pacote HELLO direto para identificar esta conexão sem ambiguidade
        if let Ok(info) = self.my_info.lock() {
            slot.push_control(build_info_or_hello_packet(MAGIC_HELLO, &info));
        }
        if let Ok(existing_peers) = self.peers.lock() {
            for p in existing_peers.values() {
                if p.last_seen.elapsed() < Duration::from_secs(5) {
                    slot.push_control(build_info_or_hello_packet(MAGIC_INFO, p));
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

                // Se for HELLO direto (ou primeiro pacote direto), registra o dono desta conexão
                // e derruba qualquer conexão duplicada anterior do mesmo amigo!
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

                // REGRA ANTI-LOOP: Só repassa pacotes que vieram DIRETAMENTE do dono deste socket (`sender_id == direct_peer_id`),
                // nunca pacotes já repassados!
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

                            // Ignora fantasmas com o nosso próprio nome caso tenhamos mudado de tag
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
                                    // Remove entradas antigas com o mesmo nome mas ID antigo
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

            // Conexão encerrada
            slot_reader.alive.store(false, Ordering::Relaxed);
            slot_reader.wakeup.notify_all();

            let disconnected_peer = slot_reader.remote_peer_id.load(Ordering::Relaxed);
            if disconnected_peer != 0 {
                // Só remove o participante se NÃO houver outra conexão ativa para o mesmo `disconnected_peer`!
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
