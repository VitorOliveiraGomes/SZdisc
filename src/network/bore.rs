use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::hub::{RoomHub, ServerMemberPresence};
use super::protocol::{derive_room_port, derive_server_lobby_port};

const BORE_RELAY_ADDR: &str = "bore.pub:7835";
const BORE_RELAY_NAME: &str = "bore.pub";

#[derive(Deserialize)]
#[serde(untagged)]
enum BoreServerMessage {
    Hello {
        #[serde(rename = "Hello")]
        hello: u16,
    },
    Connection {
        #[serde(rename = "Connection")]
        connection: String,
    },
    Error {
        #[serde(rename = "Error")]
        _error: serde_json::Value,
    },
    Other(#[allow(dead_code)] serde_json::Value),
}

fn parse_bore_message(raw: &str) -> Option<BoreServerMessage> {
    serde_json::from_str::<BoreServerMessage>(raw).ok()
}

pub fn run_room_mesh_loop(hub: Arc<RoomHub>) {
    let mut port_offset: u16 = 0;
    while !hub.stop_flag.load(Ordering::Relaxed) {
        if !hub.in_room.load(Ordering::Relaxed) || !hub.account_active.load(Ordering::Relaxed) {
            hub.holding_bore_port.store(0, Ordering::Relaxed);
            port_offset = 0;
            thread::sleep(Duration::from_millis(400));
            continue;
        }

        let base_port = hub.desired_bore_port.load(Ordering::Relaxed);
        if base_port == 0 {
            port_offset = 0;
            thread::sleep(Duration::from_millis(400));
            continue;
        }

        let target_port = 21000 + ((base_port - 21000 + port_offset * 137) % 36000);

        let has_alive_outbound = hub
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

        let Some(relay_addr) = BORE_RELAY_ADDR
            .to_socket_addrs()
            .ok()
            .and_then(|mut it| it.next())
        else {
            thread::sleep(Duration::from_secs(2));
            continue;
        };

        let Ok(mut ctrl) = TcpStream::connect_timeout(&relay_addr, Duration::from_secs(5)) else {
            thread::sleep(Duration::from_secs(2));
            continue;
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

        while !hub.stop_flag.load(Ordering::Relaxed)
            && hub.in_room.load(Ordering::Relaxed)
            && hub.desired_bore_port.load(Ordering::Relaxed) == base_port
        {
            match ctrl.read(&mut temp) {
                Ok(0) => break,
                Ok(n) => {
                    last_ctrl_activity = Instant::now();
                    pending.extend_from_slice(&temp[..n]);
                    while let Some(pos) = pending.iter().position(|&b| b == 0) {
                        let msg_bytes: Vec<u8> = pending.drain(..=pos).collect();
                        let s = String::from_utf8_lossy(&msg_bytes[..msg_bytes.len() - 1]);

                        match parse_bore_message(&s) {
                            Some(BoreServerMessage::Error { .. }) => {
                                port_already_in_use = true;
                                break;
                            }
                            Some(BoreServerMessage::Hello { hello }) => {
                                if hello > 0 {
                                    hub.holding_bore_port.store(hello, Ordering::Relaxed);
                                }
                            }
                            Some(BoreServerMessage::Connection { connection: uuid }) => {
                                if !hub.in_room.load(Ordering::Relaxed) {
                                    continue;
                                }
                                let hub_clone = Arc::clone(&hub);
                                thread::spawn(move || {
                                    if let Ok(mut data_stream) = TcpStream::connect_timeout(
                                        &relay_addr,
                                        Duration::from_secs(6),
                                    ) {
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
                            _ => {}
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

        hub.holding_bore_port.store(0, Ordering::Relaxed);
        drop(ctrl);

        if port_already_in_use
            && hub.in_room.load(Ordering::Relaxed)
            && hub.desired_bore_port.load(Ordering::Relaxed) == base_port
        {
            let bore_room_target = format!("{}:{}", BORE_RELAY_NAME, target_port);
            hub.spawn_outbound_connect(bore_room_target, true);
            thread::sleep(Duration::from_millis(2400));
            let has_valid_peer = hub.peers.lock().map(|p| !p.is_empty()).unwrap_or(false);
            if !has_valid_peer {
                port_offset = (port_offset + 1) % 6;
            } else {
                port_offset = 0;
            }
        } else {
            thread::sleep(Duration::from_millis(800));
        }
    }
}

pub fn run_server_lobby_loop(hub: Arc<RoomHub>) {
    while !hub.stop_flag.load(Ordering::Relaxed) {
        if !hub.account_active.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(600));
            continue;
        }

        let lobby_clients: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));
        let current_srv = hub
            .viewed_server_id
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        if current_srv.is_empty() {
            thread::sleep(Duration::from_millis(600));
            continue;
        }

        let lobby_port = derive_server_lobby_port(&current_srv);

        let Some(relay_addr) = BORE_RELAY_ADDR
            .to_socket_addrs()
            .ok()
            .and_then(|mut it| it.next())
        else {
            thread::sleep(Duration::from_secs(3));
            continue;
        };

        let Ok(mut ctrl) = TcpStream::connect_timeout(&relay_addr, Duration::from_secs(5)) else {
            thread::sleep(Duration::from_secs(3));
            continue;
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

        while !hub.stop_flag.load(Ordering::Relaxed) && hub.account_active.load(Ordering::Relaxed) {
            let latest_srv = hub
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

                        match parse_bore_message(&s) {
                            Some(BoreServerMessage::Error { .. }) => {
                                lobby_already_hosted = true;
                                break;
                            }
                            Some(BoreServerMessage::Connection { connection: uuid }) => {
                                let hub_clone = Arc::clone(&hub);
                                let clients_clone = Arc::clone(&lobby_clients);
                                let srv_clone = current_srv.clone();
                                thread::spawn(move || {
                                    if let Ok(mut data_stream) = TcpStream::connect_timeout(
                                        &relay_addr,
                                        Duration::from_secs(5),
                                    ) {
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
                                            handle_lobby_stream(
                                                &hub_clone,
                                                data_stream,
                                                &srv_clone,
                                                Some(clients_clone),
                                            );
                                        }
                                    }
                                });
                            }
                            _ => {}
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
                    let line = build_lobby_line(&hub, &current_srv);
                    if let Ok(mut list) = lobby_clients.lock() {
                        list.retain_mut(|c| c.write_all(line.as_bytes()).is_ok());
                    }
                    continue;
                }
                Err(_) => break,
            }
        }

        drop(ctrl);

        if lobby_already_hosted && hub.account_active.load(Ordering::Relaxed) {
            if let Ok(lobby_addr) = format!("{}:{}", BORE_RELAY_NAME, lobby_port)
                .to_socket_addrs()
                .and_then(|mut it| it.next().ok_or_else(|| std::io::Error::other("no addr")))
            {
                if let Ok(stream) = TcpStream::connect_timeout(&lobby_addr, Duration::from_secs(5))
                {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_nodelay(true);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
                    handle_lobby_stream(&hub, stream, &current_srv, None);
                }
            }
            thread::sleep(Duration::from_secs(2));
        } else {
            thread::sleep(Duration::from_millis(800));
        }
    }
}

fn build_lobby_line(hub: &RoomHub, server_id: &str) -> String {
    let key = server_id.trim().to_uppercase();
    let my_id = hub.my_peer_id.load(Ordering::Relaxed);
    let in_room = hub.in_room.load(Ordering::Relaxed);
    let conn_srv = hub
        .connected_server_id
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();

    let curr_room = if in_room && conn_srv.eq_ignore_ascii_case(&key) {
        hub.active_room_name
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    } else {
        String::new()
    };

    let srv_name = hub
        .server_names_map
        .lock()
        .ok()
        .and_then(|m| m.get(&key).cloned())
        .unwrap_or_default();

    let (name, is_sharing, res_label) = hub
        .my_info
        .lock()
        .map(|i| (i.name.clone(), i.is_sharing, i.res_label.clone()))
        .unwrap_or_else(|_| ("Jogador".to_string(), false, "Na sala".to_string()));

    let rooms_csv = hub
        .server_rooms_map
        .lock()
        .ok()
        .and_then(|m| m.get(&key).cloned())
        .map(|rs| {
            rs.iter()
                .map(|r| r.replace(['|', ';'], " ").trim().to_string())
                .filter(|r| !r.is_empty())
                .collect::<Vec<_>>()
                .join(";")
        })
        .unwrap_or_default();

    format!(
        "LBY6|{}|{}|{}|{}|{}|{}|{}|{}\n",
        my_id,
        name.replace(['|', '\n'], " "),
        key,
        srv_name.replace(['|', '\n'], " "),
        curr_room.replace(['|', '\n'], " "),
        if is_sharing && !curr_room.is_empty() {
            "1"
        } else {
            "0"
        },
        res_label.replace(['|', '\n'], " "),
        rooms_csv.replace('\n', " ")
    )
}

fn handle_lobby_stream(
    hub: &RoomHub,
    mut stream: TcpStream,
    expected_server_id: &str,
    relay_clients: Option<Arc<Mutex<Vec<TcpStream>>>>,
) {
    let mut pending = Vec::with_capacity(1024);
    let mut buf = [0u8; 512];
    let mut last_send = Instant::now() - Duration::from_secs(5);

    while !hub.stop_flag.load(Ordering::Relaxed) && hub.account_active.load(Ordering::Relaxed) {
        let latest_srv = hub
            .viewed_server_id
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        if !latest_srv.eq_ignore_ascii_case(expected_server_id) {
            break;
        }

        if last_send.elapsed() >= Duration::from_millis(1600) {
            let line = build_lobby_line(hub, expected_server_id);
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
                        if parts.len() >= 9 && parts[0] == "LBY6" {
                            let pid = parts[1].parse::<u32>().unwrap_or(0);
                            let my_id = hub.my_peer_id.load(Ordering::Relaxed);
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

                                hub.merge_server_metadata(&srv_id, srv_name, ucsv);
                                if !uroom.is_empty() {
                                    hub.merge_server_metadata(&srv_id, srv_name, &uroom);
                                }

                                if let Ok(mut members) = hub.server_members.lock() {
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

pub fn derive_room_port_for_hub(server_id: &str, room_name: &str) -> u16 {
    derive_room_port(server_id, room_name)
}
