use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::Context;
use zune_jpeg::JpegDecoder;

use crate::audio::StreamAudioPlayer;

pub const DEFAULT_STREAM_PORT: u16 = 47830;
pub const LAN_DISCOVERY_PORT: u16 = 47831;
const BORE_RELAY_HOST: &str = "bore.pub:7835";
const FRAME_MAGIC: &[u8; 4] = b"DSC2";
const AUDIO_MAGIC: &[u8; 4] = b"AUD2";

#[derive(Clone, Debug)]
pub struct DiscoveredRoom {
    pub host_name: String,
    pub addr: String,
    pub res_label: String,
    pub last_seen: Instant,
}

pub struct DecodedRemoteFrame {
    pub width: u32,
    pub height: u32,
    pub byte_len: usize,
    pub rgba: Vec<u8>,
}

/// Slot inteligente por espectador: sempre sobrescreve o quadro de vídeo pendente pelo mais recente
/// para NUNCA acumular atraso nem travar o túnel de internet!
struct ViewerSlot {
    latest_video: Mutex<Option<Arc<Vec<u8>>>>,
    audio_queue: Mutex<VecDeque<Arc<Vec<u8>>>>,
    wakeup: Condvar,
    alive: AtomicBool,
}

impl ViewerSlot {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            latest_video: Mutex::new(None),
            audio_queue: Mutex::new(VecDeque::with_capacity(16)),
            wakeup: Condvar::new(),
            alive: AtomicBool::new(true),
        })
    }

    fn push_video(&self, pkt: Arc<Vec<u8>>) {
        if let Ok(mut guard) = self.latest_video.lock() {
            *guard = Some(pkt);
            self.wakeup.notify_one();
        }
    }

    fn push_audio(&self, pkt: Arc<Vec<u8>>) {
        if let Ok(mut q) = self.audio_queue.lock() {
            if q.len() >= 14 {
                q.pop_front();
            }
            q.push_back(pkt);
            self.wakeup.notify_one();
        }
    }
}

pub fn get_local_ip() -> String {
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

pub fn start_lan_discovery_listener() -> Arc<Mutex<HashMap<String, DiscoveredRoom>>> {
    let rooms: Arc<Mutex<HashMap<String, DiscoveredRoom>>> = Arc::new(Mutex::new(HashMap::new()));
    let rooms_clone = Arc::clone(&rooms);

    thread::spawn(move || {
        let sock = match UdpSocket::bind(("0.0.0.0", LAN_DISCOVERY_PORT)) {
            Ok(s) => s,
            Err(_) => return,
        };
        let _ = sock.set_read_timeout(Some(Duration::from_millis(1000)));

        let mut buf = [0u8; 512];
        loop {
            if let Ok((len, src)) = sock.recv_from(&mut buf) {
                if let Ok(msg) = std::str::from_utf8(&buf[..len]) {
                    let parts: Vec<&str> = msg.split('|').collect();
                    if parts.len() >= 4 && parts[0] == "DISC2" {
                        let port = parts[1];
                        let host_name = parts[2].to_string();
                        let res_label = parts[3].to_string();
                        let addr = format!("{}:{}", src.ip(), port);

                        if let Ok(mut map) = rooms_clone.lock() {
                            map.insert(
                                addr.clone(),
                                DiscoveredRoom {
                                    host_name,
                                    addr,
                                    res_label,
                                    last_seen: Instant::now(),
                                },
                            );
                        }
                    }
                }
            }

            if let Ok(mut map) = rooms_clone.lock() {
                map.retain(|_, v| v.last_seen.elapsed() < Duration::from_secs(6));
            }
        }
    });

    rooms
}

fn attach_viewer_stream(
    mut stream: TcpStream,
    viewers: &Arc<Mutex<Vec<Arc<ViewerSlot>>>>,
    viewer_count: &Arc<AtomicUsize>,
) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(Duration::from_secs(12)));

    let slot = ViewerSlot::new();
    if let Ok(mut list) = viewers.lock() {
        list.push(Arc::clone(&slot));
        viewer_count.store(list.len(), Ordering::Relaxed);
    }

    thread::spawn(move || {
        while slot.alive.load(Ordering::Relaxed) {
            // 1. Prioriza enviar todos os pacotes de áudio pendentes para o som nunca cortar
            loop {
                let next_aud = slot.audio_queue.lock().ok().and_then(|mut q| q.pop_front());
                if let Some(aud_pkt) = next_aud {
                    if let Err(e) = stream.write_all(&aud_pkt) {
                        if e.kind() != std::io::ErrorKind::WouldBlock
                            && e.kind() != std::io::ErrorKind::TimedOut
                        {
                            slot.alive.store(false, Ordering::Relaxed);
                            return;
                        }
                    }
                } else {
                    break;
                }
            }

            // 2. Pega o quadro de vídeo mais recente (descartando intermediários se a internet do amigo oscilar)
            let vid_pkt = {
                let Ok(mut guard) = slot.latest_video.lock() else {
                    break;
                };
                if guard.is_none() {
                    let res = slot
                        .wakeup
                        .wait_timeout(guard, Duration::from_millis(50));
                    match res {
                        Ok((mut g, _)) => g.take(),
                        Err(_) => None,
                    }
                } else {
                    guard.take()
                }
            };

            if let Some(pkt) = vid_pkt {
                if let Err(e) = stream.write_all(&pkt) {
                    if e.kind() != std::io::ErrorKind::WouldBlock
                        && e.kind() != std::io::ErrorKind::TimedOut
                    {
                        slot.alive.store(false, Ordering::Relaxed);
                        return;
                    }
                }
                let _ = stream.flush();
            }
        }
    });
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

fn spawn_internet_tunnel_worker(
    stop_flag: Arc<AtomicBool>,
    internet_code: Arc<Mutex<Option<String>>>,
    viewers: Arc<Mutex<Vec<Arc<ViewerSlot>>>>,
    viewer_count: Arc<AtomicUsize>,
) {
    thread::spawn(move || {
        while !stop_flag.load(Ordering::Relaxed) {
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

            while !stop_flag.load(Ordering::Relaxed) {
                match ctrl.read(&mut temp) {
                    Ok(0) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&temp[..n]);
                        while let Some(pos) = pending.iter().position(|&b| b == 0) {
                            let msg_bytes: Vec<u8> = pending.drain(..=pos).collect();
                            let s = String::from_utf8_lossy(&msg_bytes[..msg_bytes.len() - 1]);

                            if let Some(port) = parse_bore_hello_port(&s) {
                                if port > 0 {
                                    if let Ok(mut code_guard) = internet_code.lock() {
                                        *code_guard = Some(port.to_string());
                                    }
                                }
                            } else if let Some(uuid) = parse_bore_connection_uuid(&s) {
                                let viewers_clone = Arc::clone(&viewers);
                                let count_clone = Arc::clone(&viewer_count);
                                thread::spawn(move || {
                                    if let Ok(mut data_stream) =
                                        TcpStream::connect_timeout(&addr, Duration::from_secs(6))
                                    {
                                        let _ = data_stream.set_nodelay(true);
                                        let accept_msg = format!("{{\"Accept\":\"{}\"}}\0", uuid);
                                        if data_stream.write_all(accept_msg.as_bytes()).is_ok() {
                                            let _ = data_stream.flush();
                                            attach_viewer_stream(
                                                data_stream,
                                                &viewers_clone,
                                                &count_clone,
                                            );
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

            if !stop_flag.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_secs(1));
            }
        }
    });
}

pub struct StreamHostServer {
    pub listen_addr: String,
    pub internet_code: Arc<Mutex<Option<String>>>,
    stop_flag: Arc<AtomicBool>,
    viewers: Arc<Mutex<Vec<Arc<ViewerSlot>>>>,
    pub viewer_count: Arc<AtomicUsize>,
    pub bytes_sent_per_sec: Arc<AtomicU32>,
}

impl StreamHostServer {
    pub fn start(host_name: String, res_label: Arc<Mutex<String>>) -> Result<Self, String> {
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

        let stop_flag = Arc::new(AtomicBool::new(false));
        let viewers: Arc<Mutex<Vec<Arc<ViewerSlot>>>> = Arc::new(Mutex::new(Vec::new()));
        let viewer_count = Arc::new(AtomicUsize::new(0));
        let bytes_sent_per_sec = Arc::new(AtomicU32::new(0));
        let internet_code: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

        let stop_accept = Arc::clone(&stop_flag);
        let viewers_accept = Arc::clone(&viewers);
        let count_accept = Arc::clone(&viewer_count);

        thread::spawn(move || {
            while !stop_accept.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _peer)) => {
                        attach_viewer_stream(stream, &viewers_accept, &count_accept);
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

        spawn_internet_tunnel_worker(
            Arc::clone(&stop_flag),
            Arc::clone(&internet_code),
            Arc::clone(&viewers),
            Arc::clone(&viewer_count),
        );

        let stop_udp = Arc::clone(&stop_flag);
        let count_udp = Arc::clone(&viewer_count);
        thread::spawn(move || {
            if let Ok(udp) = UdpSocket::bind("0.0.0.0:0") {
                let _ = udp.set_broadcast(true);
                while !stop_udp.load(Ordering::Relaxed) {
                    let label = res_label
                        .lock()
                        .map(|s| s.clone())
                        .unwrap_or_else(|_| "1080p 60fps".to_string());
                    let c = count_udp.load(Ordering::Relaxed);
                    let payload = format!("DISC2|{}|{}|{}|{}", bound_port, host_name, label, c);
                    let _ =
                        udp.send_to(payload.as_bytes(), ("255.255.255.255", LAN_DISCOVERY_PORT));
                    thread::sleep(Duration::from_millis(1500));
                }
            }
        });

        Ok(Self {
            listen_addr,
            internet_code,
            stop_flag,
            viewers,
            viewer_count,
            bytes_sent_per_sec,
        })
    }

    pub fn broadcast_frame(&self, width: u16, height: u16, fps: u8, jpeg_bytes: &[u8]) {
        let mut list = match self.viewers.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if list.is_empty() {
            return;
        }

        let payload_len = jpeg_bytes.len() as u32;
        let mut packet = Vec::with_capacity(13 + jpeg_bytes.len());
        packet.extend_from_slice(FRAME_MAGIC);
        packet.extend_from_slice(&width.to_le_bytes());
        packet.extend_from_slice(&height.to_le_bytes());
        packet.push(fps);
        packet.extend_from_slice(&payload_len.to_le_bytes());
        packet.extend_from_slice(jpeg_bytes);

        let shared_packet = Arc::new(packet);

        list.retain(|slot| {
            if slot.alive.load(Ordering::Relaxed) {
                slot.push_video(Arc::clone(&shared_packet));
                true
            } else {
                false
            }
        });

        self.viewer_count.store(list.len(), Ordering::Relaxed);
    }

    pub fn broadcast_audio(&self, pcm_bytes: &[u8]) {
        let mut list = match self.viewers.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if list.is_empty() {
            return;
        }

        let payload_len = pcm_bytes.len() as u32;
        let mut packet = Vec::with_capacity(13 + pcm_bytes.len());
        packet.extend_from_slice(AUDIO_MAGIC);
        packet.extend_from_slice(&0u16.to_le_bytes());
        packet.extend_from_slice(&0u16.to_le_bytes());
        packet.push(0);
        packet.extend_from_slice(&payload_len.to_le_bytes());
        packet.extend_from_slice(pcm_bytes);

        let shared_packet = Arc::new(packet);

        list.retain(|slot| {
            if slot.alive.load(Ordering::Relaxed) {
                slot.push_audio(Arc::clone(&shared_packet));
                true
            } else {
                false
            }
        });

        self.viewer_count.store(list.len(), Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Ok(mut list) = self.viewers.lock() {
            for slot in list.iter() {
                slot.alive.store(false, Ordering::Relaxed);
                slot.wakeup.notify_all();
            }
            list.clear();
        }
        self.viewer_count.store(0, Ordering::Relaxed);
    }
}

impl Drop for StreamHostServer {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct StreamViewerClient {
    pub display_label: String,
    pub remote_addr: String,
    pub connected: Arc<AtomicBool>,
    pub stop_flag: Arc<AtomicBool>,
    pub frame_rx: Receiver<DecodedRemoteFrame>,
    pub error_msg: Arc<Mutex<Option<String>>>,
    pub volume_percent: Arc<AtomicU32>,
    pub muted: Arc<AtomicBool>,
    pub audio_player: StreamAudioPlayer,
}

impl StreamViewerClient {
    pub fn connect(raw_target: &str, ctx: Context) -> Self {
        let trimmed = raw_target.trim().trim_start_matches('#');

        let (addr_str, display_label) = if !trimmed.contains('.') && !trimmed.contains(':') {
            (
                format!("bore.pub:{}", trimmed),
                format!("Sala #{}", trimmed),
            )
        } else if trimmed.contains(':') {
            (trimmed.to_string(), trimmed.to_string())
        } else {
            (
                format!("{}:{}", trimmed, DEFAULT_STREAM_PORT),
                trimmed.to_string(),
            )
        };

        let volume_percent = Arc::new(AtomicU32::new(100));
        let muted = Arc::new(AtomicBool::new(false));

        let connected = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let error_msg = Arc::new(Mutex::new(None));
        let (tx, rx) = sync_channel::<DecodedRemoteFrame>(2);
        let audio_player =
            StreamAudioPlayer::start(Arc::clone(&volume_percent), Arc::clone(&muted));

        let conn_flag = Arc::clone(&connected);
        let stop_clone = Arc::clone(&stop_flag);
        let err_clone = Arc::clone(&error_msg);
        let target_copy = addr_str.clone();
        let audio_tx = audio_player.clone_sender();

        thread::spawn(move || {
            let mut first_attempt = true;

            while !stop_clone.load(Ordering::Relaxed) {
                let sock_addr = match target_copy
                    .to_socket_addrs()
                    .ok()
                    .and_then(|mut it| it.next())
                {
                    Some(a) => a,
                    None => {
                        if first_attempt {
                            if let Ok(mut e) = err_clone.lock() {
                                *e = Some(format!("Código/endereço inválido: {}", target_copy));
                            }
                        }
                        break;
                    }
                };

                let mut stream = match TcpStream::connect_timeout(&sock_addr, Duration::from_secs(6))
                {
                    Ok(s) => s,
                    Err(err) => {
                        if first_attempt {
                            if let Ok(mut e) = err_clone.lock() {
                                *e = Some(format!("Não foi possível conectar ({})", err));
                            }
                            break;
                        }
                        thread::sleep(Duration::from_millis(800));
                        continue;
                    }
                };

                first_attempt = false;
                let _ = stream.set_nodelay(true);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(12)));
                conn_flag.store(true, Ordering::Relaxed);

                let mut header = [0u8; 13];
                while !stop_clone.load(Ordering::Relaxed) {
                    match stream.read_exact(&mut header) {
                        Ok(()) => {}
                        Err(ref e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut =>
                        {
                            continue;
                        }
                        Err(_) => break,
                    }

                    let magic = &header[0..4];
                    let width = u16::from_le_bytes([header[4], header[5]]) as u32;
                    let height = u16::from_le_bytes([header[6], header[7]]) as u32;
                    let payload_len = u32::from_le_bytes([
                        header[9],
                        header[10],
                        header[11],
                        header[12],
                    ]) as usize;

                    if payload_len == 0 || payload_len > 15_000_000 {
                        break;
                    }

                    let mut payload = vec![0u8; payload_len];
                    match stream.read_exact(&mut payload) {
                        Ok(()) => {}
                        Err(_) => break,
                    }

                    if magic == FRAME_MAGIC {
                        let mut decoder = JpegDecoder::new(&payload);
                        if let Ok(rgb) = decoder.decode() {
                            let pixel_count = (width * height) as usize;
                            if rgb.len() >= pixel_count * 3 {
                                let mut rgba = vec![0u8; pixel_count * 4];
                                for i in 0..pixel_count {
                                    rgba[i * 4] = rgb[i * 3];
                                    rgba[i * 4 + 1] = rgb[i * 3 + 1];
                                    rgba[i * 4 + 2] = rgb[i * 3 + 2];
                                    rgba[i * 4 + 3] = 255;
                                }

                                let _ = tx.try_send(DecodedRemoteFrame {
                                    width,
                                    height,
                                    byte_len: payload_len,
                                    rgba,
                                });
                                // Solicita redesenho imediato da janela a cada quadro recebido!
                                ctx.request_repaint();
                            }
                        }
                    } else if magic == AUDIO_MAGIC {
                        let _ = audio_tx.try_send(payload);
                    } else {
                        break;
                    }
                }

                conn_flag.store(false, Ordering::Relaxed);
                if !stop_clone.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(600));
                }
            }
        });

        Self {
            display_label,
            remote_addr: addr_str,
            connected,
            stop_flag,
            frame_rx: rx,
            error_msg,
            volume_percent,
            muted,
            audio_player,
        }
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        self.connected.store(false, Ordering::Relaxed);
        self.audio_player.stop();
    }
}

impl StreamAudioPlayer {
    pub fn clone_sender(&self) -> std::sync::mpsc::SyncSender<Vec<u8>> {
        self.tx.clone()
    }
}

impl Drop for StreamViewerClient {
    fn drop(&mut self) {
        self.stop();
    }
}
