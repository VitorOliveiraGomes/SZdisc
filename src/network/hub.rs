use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::audio::StreamAudioPlayer;
use crate::config::ServerProfile;

use super::bore;
use super::protocol::{
    build_info_or_hello_packet, build_packet, read_exact_interruptible, write_all_interruptible,
    DEFAULT_STREAM_PORT, MAGIC_AUDIO, MAGIC_HELLO, MAGIC_INFO, MAGIC_LEFT_ROOM, MAGIC_STOP_STREAM,
    MAGIC_VIDEO,
};

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

pub struct RemotePeerStream {
    pub peer_id: u32,
    pub video_width: AtomicU32,
    pub video_height: AtomicU32,
    pub frame_counter: AtomicU32,
    pub bytes_counter: AtomicU32,
    pub measured_fps: AtomicU32,
    pub measured_mbps_x100: AtomicU32,
    pub volume_percent: Arc<AtomicU32>,
    pub muted: Arc<AtomicBool>,
    pub audio_player: StreamAudioPlayer,
}

impl RemotePeerStream {
    fn new(peer_id: u32) -> Arc<Self> {
        let volume_percent = Arc::new(AtomicU32::new(100));
        let muted = Arc::new(AtomicBool::new(false));
        let audio_player =
            StreamAudioPlayer::start(Arc::clone(&volume_percent), Arc::clone(&muted));
        Arc::new(Self {
            peer_id,
            video_width: AtomicU32::new(0),
            video_height: AtomicU32::new(0),
            frame_counter: AtomicU32::new(0),
            bytes_counter: AtomicU32::new(0),
            measured_fps: AtomicU32::new(0),
            measured_mbps_x100: AtomicU32::new(0),
            volume_percent,
            muted,
            audio_player,
        })
    }

    fn push_compressed_video(&self, width: u32, height: u32, jpeg_bytes: Vec<u8>) {
        self.video_width.store(width, Ordering::Relaxed);
        self.video_height.store(height, Ordering::Relaxed);
        self.frame_counter.fetch_add(1, Ordering::Relaxed);
        self.bytes_counter
            .fetch_add(jpeg_bytes.len() as u32, Ordering::Relaxed);
        crate::bridge::publish_video_frame(self.peer_id, width as u16, height as u16, &jpeg_bytes);
    }
}

impl Drop for RemotePeerStream {
    fn drop(&mut self) {
        self.audio_player.stop();
    }
}

struct SlotQueues {
    control: VecDeque<Arc<Vec<u8>>>,
    audio: VecDeque<Arc<Vec<u8>>>,
    latest_video_by_sender: HashMap<u32, Arc<Vec<u8>>>,
}

pub(crate) struct PeerConnectionSlot {
    pub(crate) conn_id: u64,
    pub(crate) is_outbound: bool,
    pub(crate) remote_peer_id: AtomicU32,
    queues: Mutex<SlotQueues>,
    wakeup: Condvar,
    pub(crate) alive: AtomicBool,
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

pub struct RoomHub {
    pub my_peer_id: Arc<AtomicU32>,
    pub account_active: Arc<AtomicBool>,
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
    pub(crate) connections: Arc<Mutex<Vec<Arc<PeerConnectionSlot>>>>,
    next_conn_id: Arc<AtomicU64>,
    pub(crate) stop_flag: Arc<AtomicBool>,
}

impl RoomHub {
    pub fn start(
        initial_peer_id: u32,
        initial_name: String,
        account_active: bool,
        initial_servers: &[ServerProfile],
        selected_server_id: &str,
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

        let initial_viewed = if !account_active {
            String::new()
        } else if !selected_server_id.is_empty() {
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
            account_active: Arc::new(AtomicBool::new(account_active)),
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
        });

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

        let hub_tunnel = Arc::clone(&hub);
        thread::spawn(move || bore::run_room_mesh_loop(hub_tunnel));

        let hub_lobby = Arc::clone(&hub);
        thread::spawn(move || bore::run_server_lobby_loop(hub_lobby));

        let hub_beacon = Arc::clone(&hub);
        thread::spawn(move || Self::beacon_loop(hub_beacon));

        Ok(hub)
    }

    fn beacon_loop(hub_beacon: Arc<Self>) {
        while !hub_beacon.stop_flag.load(Ordering::Relaxed) {
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
            }

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
                viewed_srv
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
    }

    /// Atualiza identidade após onboarding ou edição de perfil.
    pub fn update_identity(&self, peer_id: u32, name: String) {
        self.my_peer_id.store(peer_id, Ordering::Relaxed);
        if let Ok(mut info) = self.my_info.lock() {
            info.peer_id = peer_id;
            info.name = name;
            info.last_seen = Instant::now();
        }
    }

    /// Ativa ou pausa presença no lobby (usado em onboarding e logout).
    pub fn set_account_active(&self, active: bool, selected_server_id: &str) {
        self.account_active.store(active, Ordering::Relaxed);
        if active {
            if let Ok(mut vs) = self.viewed_server_id.lock() {
                *vs = selected_server_id.trim().to_uppercase();
            }
        } else {
            self.leave_room_completely();
            if let Ok(mut vs) = self.viewed_server_id.lock() {
                vs.clear();
            }
            if let Ok(mut members) = self.server_members.lock() {
                members.clear();
            }
        }
    }

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

    pub(crate) fn merge_server_metadata(
        &self,
        server_id: &str,
        server_name: &str,
        rooms_csv: &str,
    ) {
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

        let updated_list = if let Ok(mut map) = self.server_rooms_map.lock() {
            let rooms = map.entry(key.clone()).or_default();
            if !rooms.iter().any(|r| r.eq_ignore_ascii_case(&clean)) {
                rooms.push(clean.clone());
            }
            rooms.clone()
        } else {
            Vec::new()
        };

        self.enter_server_room(&key, &clean);
        updated_list
    }

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
        let port = bore::derive_room_port_for_hub(&key, &clean);
        self.desired_bore_port.store(port, Ordering::Relaxed);
        self.in_room.store(true, Ordering::Relaxed);
    }

    pub(crate) fn spawn_outbound_connect(
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
            let Some(sock_addr) = addr_str
                .to_socket_addrs()
                .ok()
                .and_then(|mut it| it.next())
            else {
                hub.is_connecting_outbound.store(false, Ordering::SeqCst);
                return;
            };

            match TcpStream::connect_timeout(&sock_addr, Duration::from_secs(5)) {
                Ok(stream) => {
                    hub.is_connecting_outbound.store(false, Ordering::SeqCst);
                    hub.attach_bidirectional_stream(stream, true);
                }
                Err(e) => {
                    if !is_silent_auto {
                        if let Ok(mut n) = hub.status_notice.lock() {
                            *n = Some(format!("Não foi possível conectar: {}", e));
                        }
                    }
                    hub.is_connecting_outbound.store(false, Ordering::SeqCst);
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
        let Ok(mut conns) = self.connections.lock() else {
            return;
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
        let Ok(mut conns) = self.connections.lock() else {
            return;
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

    pub(crate) fn attach_bidirectional_stream(
        self: &Arc<Self>,
        mut write_stream: TcpStream,
        is_outbound: bool,
    ) {
        let _ = write_stream.set_nonblocking(false);
        let _ = write_stream.set_nodelay(true);
        let _ = write_stream.set_read_timeout(Some(Duration::from_secs(25)));
        let _ = write_stream.set_write_timeout(Some(Duration::from_secs(20)));

        let Ok(mut read_stream) = write_stream.try_clone() else {
            return;
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

        let slot_writer = Arc::clone(&slot);
        thread::spawn(move || {
            while slot_writer.alive.load(Ordering::Relaxed) {
                let (ctrl_pkts, aud_pkts, video_pkts) = {
                    let Ok(mut guard) = slot_writer.queues.lock() else {
                        return;
                    };
                    if guard.control.is_empty()
                        && guard.audio.is_empty()
                        && guard.latest_video_by_sender.is_empty()
                    {
                        let Ok((g, _)) = slot_writer
                            .wakeup
                            .wait_timeout(guard, Duration::from_millis(45))
                        else {
                            return;
                        };
                        guard = g;
                    }
                    let c: Vec<Arc<Vec<u8>>> = guard.control.drain(..).collect();
                    let a: Vec<Arc<Vec<u8>>> = guard.audio.drain(..).collect();
                    let v: Vec<Arc<Vec<u8>>> =
                        guard.latest_video_by_sender.drain().map(|(_, p)| p).collect();
                    (c, a, v)
                };

                for pkt in ctrl_pkts.into_iter().chain(aud_pkts).chain(video_pkts) {
                    if !write_all_interruptible(&mut write_stream, &pkt, &slot_writer.alive) {
                        slot_writer.alive.store(false, Ordering::Relaxed);
                        return;
                    }
                }
            }
        });

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
                let sender_id = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
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

                Self::handle_incoming_packet(
                    &hub_reader,
                    direct_peer_id,
                    &magic,
                    sender_id,
                    width,
                    height,
                    fps,
                    payload,
                );
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

                    let bye_pkt =
                        build_packet(MAGIC_LEFT_ROOM, disconnected_peer, 0, 0, 0, &[0]);
                    if let Ok(mut conns) = hub_reader.connections.lock() {
                        conns.retain(|c| c.alive.load(Ordering::Relaxed));
                        for c in conns.iter() {
                            c.push_control(Arc::clone(&bye_pkt));
                        }
                    }
                }
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_incoming_packet(
        hub: &Arc<Self>,
        direct_peer_id: u32,
        magic: &[u8; 4],
        sender_id: u32,
        width: u16,
        height: u16,
        fps: u8,
        payload: Vec<u8>,
    ) {
        if magic == MAGIC_HELLO || magic == MAGIC_INFO {
            if let Ok(text) = std::str::from_utf8(&payload) {
                let parts: Vec<&str> = text.split('\t').collect();
                if parts.len() >= 3 {
                    let peer_name = parts[0].trim().to_string();
                    let is_sharing = parts[1] == "1";
                    let res_label = parts[2].to_string();
                    if parts.len() >= 7 {
                        hub.merge_server_metadata(parts[3].trim(), parts[4].trim(), parts[6]);
                    }

                    let is_same_as_me = hub
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
                        if let Ok(mut peers_map) = hub.peers.lock() {
                            peers_map.retain(|&k, v| {
                                if k != sender_id && v.name.eq_ignore_ascii_case(&peer_name) {
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

                        let conn_srv = hub
                            .connected_server_id
                            .lock()
                            .map(|g| g.clone())
                            .unwrap_or_default();
                        let curr_room = hub
                            .active_room_name
                            .lock()
                            .map(|g| g.clone())
                            .unwrap_or_default();
                        if let Ok(mut members) = hub.server_members.lock() {
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
                            if let Ok(mut streams) = hub.remote_streams.lock() {
                                for old_id in old_duplicate_ids {
                                    streams.remove(&old_id);
                                }
                                if !is_sharing {
                                    streams.remove(&sender_id);
                                }
                            }
                        }
                    }
                }
            }
        } else if magic == MAGIC_STOP_STREAM {
            if let Ok(mut peers_map) = hub.peers.lock() {
                if let Some(p) = peers_map.get_mut(&sender_id) {
                    p.is_sharing = false;
                    p.last_seen = Instant::now();
                }
            }
            if let Ok(mut streams) = hub.remote_streams.lock() {
                streams.remove(&sender_id);
            }
        } else if magic == MAGIC_LEFT_ROOM {
            if let Ok(mut peers_map) = hub.peers.lock() {
                peers_map.remove(&sender_id);
            }
            if let Ok(mut streams) = hub.remote_streams.lock() {
                streams.remove(&sender_id);
            }
        } else if magic == MAGIC_VIDEO {
            let w = width as u32;
            let h = height as u32;
            if w > 0 && h > 0 {
                if let Ok(mut peers_map) = hub.peers.lock() {
                    let entry = peers_map.entry(sender_id).or_insert_with(|| PeerInfo {
                        peer_id: sender_id,
                        name: format!("Amigo #{}", sender_id % 1000),
                        is_sharing: true,
                        res_label: format!("{}p {}fps", h, fps),
                        last_seen: Instant::now(),
                    });
                    entry.is_sharing = true;
                    entry.last_seen = Instant::now();
                }

                let stream_arc = hub.remote_streams.lock().ok().map(|mut streams| {
                    Arc::clone(
                        streams
                            .entry(sender_id)
                            .or_insert_with(|| RemotePeerStream::new(sender_id)),
                    )
                });
                if let Some(stream_arc) = stream_arc {
                    stream_arc.push_compressed_video(w, h, payload);
                }
            }
        } else if magic == MAGIC_AUDIO {
            let stream_arc = hub.remote_streams.lock().ok().map(|mut streams| {
                Arc::clone(
                    streams
                        .entry(sender_id)
                        .or_insert_with(|| RemotePeerStream::new(sender_id)),
                )
            });
            if let Some(stream_arc) = stream_arc {
                let _ = stream_arc.audio_player.clone_sender().try_send(payload);
            }
        }
    }
}
