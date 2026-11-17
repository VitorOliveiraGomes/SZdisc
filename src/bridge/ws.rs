use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use crate::config::encode_base64;

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

    for chunk in msg.as_chunks::<64>().0 {
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

struct WsClientSlot {
    filter_tile_id: Option<u32>,
    slots: Mutex<HashMap<u32, Arc<Vec<u8>>>>,
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

    fn push_packet(&self, tile_id: u32, packet: &Arc<Vec<u8>>) {
        if let Some(filter) = self.filter_tile_id {
            if filter != tile_id {
                return;
            }
        }
        if let Ok(mut guard) = self.slots.lock() {
            guard.insert(tile_id, Arc::clone(packet));
            self.wakeup.notify_one();
        }
    }
}

pub struct VideoBroadcaster {
    clients: Mutex<Vec<Arc<WsClientSlot>>>,
    client_count: AtomicUsize,
}

static GLOBAL_BROADCASTER: OnceLock<Arc<VideoBroadcaster>> = OnceLock::new();
static WS_CONNECT_SEQ: AtomicU64 = AtomicU64::new(0);

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

pub fn ws_connect_seq() -> u64 {
    WS_CONNECT_SEQ.load(Ordering::Relaxed)
}

/// Formato binário: `[tile_id: u32 LE][width: u16 LE][height: u16 LE][jpeg_bytes...]`.
pub fn publish_video_frame(tile_id: u32, width: u16, height: u16, jpeg_bytes: &[u8]) {
    let b = broadcaster();
    if b.client_count.load(Ordering::Relaxed) == 0 || jpeg_bytes.is_empty() {
        return;
    }

    let payload_len = 8 + jpeg_bytes.len();
    let mut packet = Vec::with_capacity(10 + payload_len);
    push_ws_binary_header(&mut packet, payload_len);
    packet.extend_from_slice(&tile_id.to_le_bytes());
    packet.extend_from_slice(&width.to_le_bytes());
    packet.extend_from_slice(&height.to_le_bytes());
    packet.extend_from_slice(jpeg_bytes);
    let packet = Arc::new(packet);

    if let Ok(mut clients) = b.clients.lock() {
        clients.retain(|c| c.alive.load(Ordering::Relaxed));
        b.client_count.store(clients.len(), Ordering::Relaxed);
        for client in clients.iter() {
            client.push_packet(tile_id, &packet);
        }
    }
}

fn push_ws_binary_header(out: &mut Vec<u8>, len: usize) {
    out.push(0x82);
    if len < 126 {
        out.push(len as u8);
    } else if len <= 65535 {
        out.push(126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
}

/// Lê um cabeçalho RFC 6455 completo e descarta o payload, retornando o opcode real.
fn read_ws_client_frame(stream: &mut TcpStream, alive: &AtomicBool) -> Option<u8> {
    let mut hdr = [0u8; 2];
    if !read_exact_ws(stream, &mut hdr, alive) {
        return None;
    }
    let opcode = hdr[0] & 0x0F;
    let masked = (hdr[1] & 0x80) != 0;
    let mut len = (hdr[1] & 0x7F) as u64;

    if len == 126 {
        let mut ext = [0u8; 2];
        if !read_exact_ws(stream, &mut ext, alive) {
            return None;
        }
        len = u16::from_be_bytes(ext) as u64;
    } else if len == 127 {
        let mut ext = [0u8; 8];
        if !read_exact_ws(stream, &mut ext, alive) {
            return None;
        }
        len = u64::from_be_bytes(ext);
    }

    if len > 1_048_576 {
        return None;
    }

    if masked {
        let mut mask = [0u8; 4];
        if !read_exact_ws(stream, &mut mask, alive) {
            return None;
        }
    }

    let mut remaining = len as usize;
    let mut sink = [0u8; 512];
    while remaining > 0 {
        let chunk = remaining.min(sink.len());
        if !read_exact_ws(stream, &mut sink[..chunk], alive) {
            return None;
        }
        remaining -= chunk;
    }

    Some(opcode)
}

fn read_exact_ws(stream: &mut TcpStream, mut buf: &mut [u8], alive: &AtomicBool) -> bool {
    while !buf.is_empty() {
        if !alive.load(Ordering::Relaxed) {
            return false;
        }
        match stream.read(buf) {
            Ok(0) => return false,
            Ok(n) => buf = &mut buf[n..],
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::Interrupted =>
            {
                continue;
            }
            Err(_) => return false,
        }
    }
    true
}

pub(crate) fn serve_websocket_client(
    mut stream: TcpStream,
    key: &str,
    filter_tile_id: Option<u32>,
) {
    let mut accept_input = String::with_capacity(key.len() + 36);
    accept_input.push_str(key.trim());
    accept_input.push_str("258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let accept_key = encode_base64(&sha1_digest(accept_input.as_bytes()));

    let handshake = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept_key
    );
    if stream.write_all(handshake.as_bytes()).is_err() {
        return;
    }
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

    let slot = WsClientSlot::new(filter_tile_id);
    let b = broadcaster();
    if let Ok(mut clients) = b.clients.lock() {
        clients.push(Arc::clone(&slot));
        b.client_count.store(clients.len(), Ordering::Relaxed);
    }
    WS_CONNECT_SEQ.fetch_add(1, Ordering::Relaxed);

    if let Ok(mut reader_stream) = stream.try_clone() {
        let _ = reader_stream.set_read_timeout(Some(Duration::from_secs(2)));
        let slot_reader = Arc::clone(&slot);
        thread::spawn(move || {
            while slot_reader.alive.load(Ordering::Relaxed) {
                match read_ws_client_frame(&mut reader_stream, &slot_reader.alive) {
                    None | Some(0x8) => break,
                    Some(_) => {}
                }
            }
            slot_reader.alive.store(false, Ordering::Relaxed);
            slot_reader.wakeup.notify_all();
        });
    }

    while slot.alive.load(Ordering::Relaxed) {
        let pending: Vec<Arc<Vec<u8>>> = {
            let Ok(mut guard) = slot.slots.lock() else {
                break;
            };
            if guard.is_empty() {
                let Ok((g, _)) = slot.wakeup.wait_timeout(guard, Duration::from_millis(250)) else {
                    break;
                };
                guard = g;
            }
            guard.drain().map(|(_, v)| v).collect()
        };

        for pkt in pending {
            if stream.write_all(&pkt).is_err() {
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
