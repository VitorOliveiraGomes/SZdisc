use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use zune_jpeg::JpegDecoder;

use crate::audio::StreamAudioPlayer;

pub const DEFAULT_STREAM_PORT: u16 = 47830;
pub const LAN_DISCOVERY_PORT: u16 = 47831;
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

pub struct StreamHostServer {
    pub listen_addr: String,
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

        let stop_accept = Arc::clone(&stop_flag);
        let viewers_accept = Arc::clone(&viewers);
        let count_accept = Arc::clone(&viewer_count);

        thread::spawn(move || {
            while !stop_accept.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _peer)) => {
                        let _ = stream.set_nodelay(true);
                        let _ = stream.set_write_timeout(Some(Duration::from_millis(800)));

                        // Fila curta (vídeo + áudio de baixa latência)
                        let (tx, rx) = sync_channel::<Arc<Vec<u8>>>(8);
                        if let Ok(mut list) = viewers_accept.lock() {
                            list.push(tx);
                            count_accept.store(list.len(), Ordering::Relaxed);
                        }

                        thread::spawn(move || {
                            while let Ok(packet) = rx.recv() {
                                if stream.write_all(&packet).is_err() {
                                    break;
                                }
                            }
                        });
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
        let addr_str = if raw_target.contains(':') {
            raw_target.trim().to_string()
        } else {
            format!("{}:{}", raw_target.trim(), DEFAULT_STREAM_PORT)
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
        let audio_tx = audio_player_sender_clone(&audio_player);

        thread::spawn(move || {
            let sock_addr: SocketAddr = match target_copy.parse() {
                Ok(a) => a,
                Err(_) => {
                    if let Ok(mut e) = err_clone.lock() {
                        *e = Some(format!("Endereço inválido: {}", target_copy));
                    }
                    return;
                }
            };

            let mut stream = match TcpStream::connect_timeout(&sock_addr, Duration::from_secs(4)) {
                Ok(s) => s,
                Err(err) => {
                    if let Ok(mut e) = err_clone.lock() {
                        *e = Some(format!("Falha ao conectar: {}", err));
                    }
                    return;
                }
            };

            let _ = stream.set_nodelay(true);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
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

fn audio_player_sender_clone(player: &StreamAudioPlayer) -> SyncSender<Vec<u8>> {
    player.clone_sender()
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
