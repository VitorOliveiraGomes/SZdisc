use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::hub::PeerInfo;

pub const DEFAULT_STREAM_PORT: u16 = 47830;

pub const MAGIC_HELLO: &[u8; 4] = b"HLO6";
pub const MAGIC_INFO: &[u8; 4] = b"INF6";
pub const MAGIC_VIDEO: &[u8; 4] = b"VID6";
pub const MAGIC_AUDIO: &[u8; 4] = b"AUD6";
pub const MAGIC_STOP_STREAM: &[u8; 4] = b"STP6";
pub const MAGIC_LEFT_ROOM: &[u8; 4] = b"BYE6";

/// ID numérico estável por máquina/usuário para evitar colisão entre participantes.
pub fn derive_persistent_peer_id(display_name: &str, user_tag: &str) -> u32 {
    let comp = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "LOCAL".to_string());
    let seed = format!(
        "{}|{}|{}",
        comp.trim().to_uppercase(),
        display_name.trim(),
        user_tag.trim()
    );
    let mut hash: u32 = 2_166_136_261;
    for b in seed.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16_777_619);
    }
    if hash == 0 {
        1
    } else {
        hash
    }
}

/// Porta determinística do Lobby de um Servidor (`server_id`).
// Os namespaces V6 fazem parte do protocolo existente: mantê-los preserva os convites.
pub fn derive_server_lobby_port(server_id: &str) -> u16 {
    let norm = server_id.trim().to_uppercase();
    let seed = format!("SZDISC_V6_SERVER_LOBBY::{}", norm);
    let mut hash: u32 = 2_166_136_261;
    for b in seed.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16_777_619);
    }
    let mut port = 20000 + (hash % 37000) as u16;
    if (47820..=47850).contains(&port) {
        port += 120;
    }
    port
}

/// Porta determinística de uma Sala dentro de um Servidor (`server_id` + `room_name`).
pub fn derive_room_port(server_id: &str, room_name: &str) -> u16 {
    let norm_srv = server_id.trim().to_uppercase();
    let norm_room = room_name.trim().to_lowercase();
    let seed = format!("SZDISC_V6_SRV_ROOM::{}::{}", norm_srv, norm_room);
    let mut hash: u32 = 2_166_136_261;
    for b in seed.as_bytes() {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(16_777_619);
    }
    let mut port = 21000 + (hash % 36000) as u16;
    if (47820..=47850).contains(&port) {
        port += 100;
    }
    port
}

pub fn build_packet(
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

pub fn build_info_or_hello_packet(
    magic: &[u8; 4],
    info: &PeerInfo,
    server_id: &str,
    server_name: &str,
    room_name: &str,
    known_rooms: &[String],
) -> Arc<Vec<u8>> {
    let share_flag = if info.is_sharing { "1" } else { "0" };
    let clean_name = info.name.replace(['\t', '\n'], " ");
    let clean_res = info.res_label.replace(['\t', '\n'], " ");
    let clean_srv_id = server_id.replace(['\t', '\n'], " ");
    let clean_srv_name = server_name.replace(['\t', '\n'], " ");
    let clean_room = room_name.replace(['\t', '\n'], " ");
    let rooms_csv = known_rooms
        .iter()
        .map(|r| r.replace(['\t', ';'], " ").trim().to_string())
        .filter(|r| !r.is_empty())
        .collect::<Vec<_>>()
        .join(";");
    let text = format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        clean_name, share_flag, clean_res, clean_srv_id, clean_srv_name, clean_room, rooms_csv
    );
    build_packet(magic, info.peer_id, 0, 0, 0, text.as_bytes())
}

pub fn read_exact_interruptible(
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

pub fn write_all_interruptible(stream: &mut TcpStream, buf: &[u8], alive: &AtomicBool) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portas_deterministicas_fora_da_faixa_local() {
        let lp = derive_server_lobby_port("SZ-48291");
        let rp = derive_room_port("SZ-48291", "Sala Geral");
        assert!(!(47820..=47850).contains(&lp));
        assert!(!(47820..=47850).contains(&rp));
        assert_eq!(rp, derive_room_port("sz-48291", "sala geral"));
    }
}
