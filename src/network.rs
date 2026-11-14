use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
    viewers: &Arc<Mutex<Vec<SyncSender<Arc<Vec<u8>>>>>>,
    viewer_count: &Arc<AtomicUsize>,
) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(Duration::from_millis(900)));

    let (tx, rx) = sync_channel::<Arc<Vec<u8>>>(8);
    if let Ok(mut list) = viewers.lock() {
        list.push(tx);
        viewer_count.store(list.len(), Ordering::Relaxed);
    }

    thread::spawn(move || {
        while let Ok(packet) = rx.recv() {
            if stream.write_all(&packet).is_err() {
                break;
            }
        }
    });
}

/// Extrai o valor de `"Hello":12345`
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

/// Extrai o UUID de `"Connection":"550e8400-..."`
fn parse_bore_connection_uuid(msg: &str) -> Option<String> {
    let idx = msg.find("\"Connection\":\"")?;
    let rest = &msg[idx + 14..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Inicia túnel público na internet (bore.pub) para que amigos conectem com código de 5 dígitos sem Radmin VPN
fn spawn_internet_tunnel_worker(
    stop_flag: Arc<AtomicBool>,
    internet_code: Arc<Mutex<Option<String>>>,
    viewers: Arc<Mutex<Vec<SyncSender<Arc<Vec<u8>>>>>>,
    viewer_count: Arc<AtomicUsize>,
) {
    thread::spawn(move || {
        let addr = match BORE_RELAY_HOST
            .to_socket_addrs()
            .ok()
            .and_then(|mut it| it.next())
        {
            Some(a) => a,
            None => return,
        };

        let mut ctrl = match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
            Ok(s) => s,
            Err(_) => return,
        };

        let _ = ctrl.set_nodelay(true);
        let _ = ctrl.set_read_timeout(Some(Duration::from_secs(15)));

        if ctrl.write_all(b"{\"Hello\":0}\0").is_err() {
            return;
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
                            // Um amigo conectou pelo código da sala na internet!
                            let viewers_clone = Arc::clone(&viewers);
                            let count_clone = Arc::clone(&viewer_count);
                            thread::spawn(move || {
                                if let Ok(mut data_stream) =
                                    TcpStream::connect_timeout(&addr, Duration::from_secs(5))
                                {
                                    let accept_msg = format!("{{\"Accept\":\"{}\"}}\0", uuid);
                                    if data_stream.write_all(accept_msg.as_bytes()).is_ok() {
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
    });
}

pub struct StreamHostServer {
    pub listen_addr: String,
    pub internet_code: Arc<Mutex<Option<String>>>,
    stop_flag: Arc<AtomicBool>,
    viewers: Arc<Mutex<Vec<SyncSender<Arc<Vec<u8>>>>>>,
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
        let viewers: Arc<Mutex<Vec<SyncSender<Arc<Vec<u8>>>>>> = Arc::new(Mutex::new(Vec::new()));
        let viewer_count = Arc::new(AtomicUsize::new(0));
        let bytes_sent_per_sec = Arc::new(AtomicU32::new(0));
        let internet_code: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

        // 1. Thread aceitando conexões diretas (LAN / Radmin VPN)
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

        // 2. Inicia também o Túnel de Internet (código de 5 dígitos sem precisar de Radmin!)
        spawn_internet_tunnel_worker(
            Arc::clone(&stop_flag),
            Arc::clone(&internet_code),
            Arc::clone(&viewers),
            Arc::clone(&viewer_count),
        );

        // 3. Thread anunciando na rede local via UDP Broadcast
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

        list.retain(|tx| match tx.try_send(Arc::clone(&shared_packet)) {
            Ok(_) => true,
            Err(std::sync::mpsc::TrySendError::Full(_)) => true,
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => false,
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

        list.retain(|tx| match tx.try_send(Arc::clone(&shared_packet)) {
            Ok(_) => true,
            Err(std::sync::mpsc::TrySendError::Full(_)) => true,
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => false,
        });

        self.viewer_count.store(list.len(), Ordering::Relaxed);
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Ok(mut list) = self.viewers.lock() {
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
    pub remote_addr: String,
    pub connected: Arc<AtomicBool>,
    pub stop_flag: Arc<AtomicBool>,
    pub frame_rx: Receiver<DecodedRemoteFrame>,
    pub error_msg: Arc<Mutex<Option<String>>>,
    pub audio_player: StreamAudioPlayer,
}

impl StreamViewerClient {
    pub fn connect(
        raw_target: &str,
        volume_percent: Arc<AtomicU32>,
        muted: Arc<AtomicBool>,
    ) -> Self {
        let trimmed = raw_target.trim().trim_start_matches('#');

        // Se o amigo digitou apenas o Código de 4-5 dígitos (ex: "52735"), conecta via túnel de internet (bore.pub:52735)
        let addr_str = if !trimmed.contains('.') && !trimmed.contains(':') {
            format!("bore.pub:{}", trimmed)
        } else if trimmed.contains(':') {
            trimmed.to_string()
        } else {
            format!("{}:{}", trimmed, DEFAULT_STREAM_PORT)
        };

        let connected = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let error_msg = Arc::new(Mutex::new(None));
        let (tx, rx) = sync_channel::<DecodedRemoteFrame>(2);
        let audio_player = StreamAudioPlayer::start(volume_percent, muted);

        let conn_flag = Arc::clone(&connected);
        let stop_clone = Arc::clone(&stop_flag);
        let err_clone = Arc::clone(&error_msg);
        let target_copy = addr_str.clone();
        let audio_tx = audio_player.clone_sender();

        thread::spawn(move || {
            let sock_addr = match target_copy
                .to_socket_addrs()
                .ok()
                .and_then(|mut it| it.next())
            {
                Some(a) => a,
                None => {
                    if let Ok(mut e) = err_clone.lock() {
                        *e = Some(format!("Endereço/código inválido: {}", target_copy));
                    }
                    return;
                }
            };

            let mut stream = match TcpStream::connect_timeout(&sock_addr, Duration::from_secs(5)) {
                Ok(s) => s,
                Err(err) => {
                    if let Ok(mut e) = err_clone.lock() {
                        *e = Some(format!("Não foi possível conectar na sala ({})", err));
                    }
                    return;
                }
            };

            let _ = stream.set_nodelay(true);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(6)));
            conn_flag.store(true, Ordering::Relaxed);

            let mut header = [0u8; 13];
            while !stop_clone.load(Ordering::Relaxed) {
                if stream.read_exact(&mut header).is_err() {
                    break;
                }

                let magic = &header[0..4];
                let width = u16::from_le_bytes([header[4], header[5]]) as u32;
                let height = u16::from_le_bytes([header[6], header[7]]) as u32;
                let payload_len =
                    u32::from_le_bytes([header[9], header[10], header[11], header[12]]) as usize;

                if payload_len == 0 || payload_len > 15_000_000 {
                    break;
                }

                let mut payload = vec![0u8; payload_len];
                if stream.read_exact(&mut payload).is_err() {
                    break;
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
                        }
                    }
                } else if magic == AUDIO_MAGIC {
                    let _ = audio_tx.try_send(payload);
                } else {
                    break;
                }
            }

            conn_flag.store(false, Ordering::Relaxed);
        });

        Self {
            remote_addr: addr_str,
            connected,
            stop_flag,
            frame_rx: rx,
            error_msg,
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
    pub fn clone_sender(&self) -> SyncSender<Vec<u8>> {
        self.tx.clone()
    }
}

impl Drop for StreamViewerClient {
    fn drop(&mut self) {
        self.stop();
    }
}
