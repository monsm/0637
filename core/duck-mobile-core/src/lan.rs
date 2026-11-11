//! Other ducks on the same LAN.
//!
//! The physical robot has no such feature; this is the phone's social layer.
//! Each app announces itself on UDP 8766 every two seconds, keeps the peers
//! it has heard from, and can send a greet. A greet is answered with a hello
//! back and both ducks react with their own official voices.
//!
//! The protocol is deliberately tiny and cannot command anything: the only
//! verbs are announce, greet and hello. A greeting reaches the duck as one of
//! its own reactions, never as a control input or an observation.

use serde_json::{json, Value};
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const PORT: u16 = 8766;
const VERSION: u64 = 1;
const ANNOUNCE_EVERY: Duration = Duration::from_secs(2);
const PEER_TTL_MS: u128 = 8_000;
const MAX_PEERS: usize = 16;
const MAX_MESSAGE: usize = 2_048;

#[derive(Debug, Clone, PartialEq)]
pub enum PeerEvent {
    /// Somebody said hello to this duck.
    Greeted { from: String, name: String },
    /// A duck we greeted answered.
    Replied { from: String, name: String },
}

struct Peer {
    id: String,
    name: String,
    addr: SocketAddr,
    last_seen_ms: u128,
    replied: bool,
}

struct Inner {
    id: String,
    name: String,
    peers: Vec<Peer>,
    events: Vec<PeerEvent>,
}

/// The shared half: the listener thread and the shell both use it.
#[derive(Clone)]
pub struct Lan {
    inner: Arc<Mutex<Inner>>,
    socket: Arc<UdpSocket>,
}

impl Lan {
    /// Bind the peer port and start announcing. Returns None when the port is
    /// taken (another duck on this device) — the rest of the duck still runs.
    pub fn start(name: &str) -> Option<Lan> {
        let socket = UdpSocket::bind(("0.0.0.0", PORT)).ok()?;
        socket.set_broadcast(true).ok()?;
        socket
            .set_read_timeout(Some(Duration::from_millis(200)))
            .ok()?;
        let inner = Arc::new(Mutex::new(Inner {
            id: fresh_id(),
            name: clean_name(name),
            peers: Vec::new(),
            events: Vec::new(),
        }));
        let socket = Arc::new(socket);
        let listener_socket = socket.clone();
        let listener_inner = inner.clone();
        std::thread::Builder::new()
            .name("duck-lan".to_owned())
            .spawn(move || listen(listener_socket, listener_inner))
            .ok()?;
        Some(Lan { inner, socket })
    }

    pub fn set_name(&self, name: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.name = clean_name(name);
        }
        let _ = self.announce();
    }

    /// Say hello to one peer, or to every duck when `target` is None.
    pub fn greet(&self, target: Option<&str>) {
        let (from, name, peer) = match self.inner.lock() {
            Ok(inner) => (
                inner.id.clone(),
                inner.name.clone(),
                target.and_then(|id| inner.peers.iter().find(|peer| peer.id == id).map(|peer| peer.addr)),
            ),
            Err(_) => return,
        };
        let message = json!({
            "v": VERSION,
            "type": "greet",
            "from": from,
            "name": name,
            "to": target.unwrap_or("all"),
        });
        let bytes = message.to_string().into_bytes();
        match peer {
            Some(addr) => {
                let _ = self.socket.send_to(&bytes, addr);
            }
            None => {
                let _ = self
                    .socket
                    .send_to(&bytes, ("255.255.255.255", PORT));
            }
        }
    }

    /// What `/state.peers` shows. Expired ducks are pruned here, so the list
    /// is what is on the LAN now, not everyone ever heard.
    pub fn snapshot(&self) -> Value {
        let now = now_ms();
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(_) => return json!({ "peers": [] }),
        };
        inner
            .peers
            .retain(|peer| now.saturating_sub(peer.last_seen_ms) <= PEER_TTL_MS);
        let peers: Vec<Value> = inner
            .peers
            .iter()
            .map(|peer| {
                json!({
                    "id": peer.id,
                    "name": peer.name,
                    "addr": peer.addr.to_string(),
                    "replied": peer.replied,
                    "last_seen_ms": peer.last_seen_ms as u64,
                })
            })
            .collect();
        json!({ "id": inner.id, "name": inner.name, "port": PORT, "peers": peers })
    }

    pub fn take_events(&self) -> Vec<PeerEvent> {
        match self.inner.lock() {
            Ok(mut inner) => std::mem::take(&mut inner.events),
            Err(_) => Vec::new(),
        }
    }

    fn announce(&self) -> std::io::Result<usize> {
        let (id, name) = match self.inner.lock() {
            Ok(inner) => (inner.id.clone(), inner.name.clone()),
            Err(_) => return Ok(0),
        };
        let message = json!({
            "v": VERSION,
            "type": "announce",
            "from": id,
            "name": name,
        });
        self.socket
            .send_to(message.to_string().as_bytes(), ("255.255.255.255", PORT))
    }
}

fn listen(socket: Arc<UdpSocket>, inner: Arc<Mutex<Inner>>) {
    let mut last_announce: Option<Instant> = None;
    let mut buffer = [0u8; MAX_MESSAGE];
    loop {
        if last_announce.is_none_or(|at| at.elapsed() >= ANNOUNCE_EVERY) {
            let (id, name) = match inner.lock() {
                Ok(inner) => (inner.id.clone(), inner.name.clone()),
                Err(_) => return,
            };
            let message = json!({
                "v": VERSION,
                "type": "announce",
                "from": id,
                "name": name,
            });
            let _ = socket.send_to(
                message.to_string().as_bytes(),
                ("255.255.255.255", PORT),
            );
            last_announce = Some(Instant::now());
        }
        let Ok((size, source)) = socket.recv_from(&mut buffer) else {
            continue;
        };
        let Ok(message) = serde_json::from_slice::<Value>(&buffer[..size]) else {
            continue;
        };
        let reply = match inner.lock() {
            Ok(mut inner) => ingest(&mut inner, &message, source, now_ms()),
            Err(_) => None,
        };
        if let Some(reply) = reply {
            let _ = socket.send_to(reply.as_bytes(), source);
        }
    }
}

/// One datagram, pure enough to test. Returns a unicast reply when the message
/// asks for one.
fn ingest(inner: &mut Inner, message: &Value, source: SocketAddr, now: u128) -> Option<String> {
    if message.get("v").and_then(Value::as_u64) != Some(VERSION) {
        return None;
    }
    let kind = message.get("type").and_then(Value::as_str)?;
    let from = message
        .get("from")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if from.is_empty() || from == inner.id {
        return None;
    }
    let name = clean_name(message.get("name").and_then(Value::as_str).unwrap_or("Duck"));
    upsert(inner, from, &name, source, now);
    match kind {
        "announce" => None,
        "greet" => {
            let to = message.get("to").and_then(Value::as_str).unwrap_or("all");
            if to != "all" && to != inner.id {
                return None;
            }
            inner.events.push(PeerEvent::Greeted {
                from: from.to_owned(),
                name: name.clone(),
            });
            if let Some(peer) = inner.peers.iter_mut().find(|peer| peer.id == from) {
                peer.replied = true;
            }
            Some(
                json!({
                    "v": VERSION,
                    "type": "hello",
                    "from": inner.id,
                    "name": inner.name,
                    "to": from,
                })
                .to_string(),
            )
        }
        "hello" => {
            let to = message.get("to").and_then(Value::as_str).unwrap_or("all");
            if to != "all" && to != inner.id {
                return None;
            }
            inner.events.push(PeerEvent::Replied {
                from: from.to_owned(),
                name,
            });
            if let Some(peer) = inner.peers.iter_mut().find(|peer| peer.id == from) {
                peer.replied = true;
            }
            None
        }
        _ => None,
    }
}

fn upsert(inner: &mut Inner, id: &str, name: &str, source: SocketAddr, now: u128) {
    if let Some(peer) = inner.peers.iter_mut().find(|peer| peer.id == id) {
        peer.name = name.to_owned();
        peer.addr = source;
        peer.last_seen_ms = now;
        return;
    }
    if inner.peers.len() >= MAX_PEERS {
        // Drop the stalest rather than refusing every new duck.
        if let Some((index, _)) = inner
            .peers
            .iter()
            .enumerate()
            .min_by_key(|(_, peer)| peer.last_seen_ms)
        {
            inner.peers.remove(index);
        }
    }
    inner.peers.push(Peer {
        id: id.to_owned(),
        name: name.to_owned(),
        addr: source,
        last_seen_ms: now,
        replied: false,
    });
}

fn clean_name(name: &str) -> String {
    let trimmed: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(24)
        .collect();
    if trimmed.trim().is_empty() {
        "Duck".to_owned()
    } else {
        trimmed.trim().to_owned()
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0)
}

/// Eight hex digits, unique enough for a room: the boot time and the pid.
fn fresh_id() -> String {
    let nanos = now_ms();
    let pid = std::process::id() as u128;
    format!("{:08x}", ((nanos ^ (pid << 32)) & 0xffff_ffff) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inner() -> Inner {
        Inner {
            id: "self0001".to_owned(),
            name: "Mine".to_owned(),
            peers: Vec::new(),
            events: Vec::new(),
        }
    }

    fn source(port: u16) -> SocketAddr {
        format!("192.168.1.9:{port}").parse().unwrap()
    }

    #[test]
    fn an_announce_is_discovered() {
        let mut duck = inner();
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "announce", "from": "peer01", "name": "Sunny" }),
            source(8767),
            1000,
        );
        assert_eq!(duck.peers.len(), 1);
        assert_eq!(duck.peers[0].name, "Sunny");
        assert!(duck.events.is_empty());
    }

    #[test]
    fn a_greet_is_answered_and_announced() {
        let mut duck = inner();
        let reply = ingest(
            &mut duck,
            &json!({ "v": 1, "type": "greet", "from": "peer01", "name": "Sunny", "to": "self0001" }),
            source(8767),
            1000,
        );
        let reply: Value = serde_json::from_str(&reply.expect("a greet is answered")).unwrap();
        assert_eq!(reply["type"], "hello");
        assert_eq!(reply["to"], "peer01");
        assert!(matches!(
            duck.events.first(),
            Some(PeerEvent::Greeted { name, .. }) if name == "Sunny"
        ));
    }

    #[test]
    fn the_hello_marks_the_duck_as_replied() {
        let mut duck = inner();
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "announce", "from": "peer01", "name": "Sunny" }),
            source(8767),
            1000,
        );
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "hello", "from": "peer01", "name": "Sunny", "to": "self0001" }),
            source(8767),
            1200,
        );
        assert!(duck.peers[0].replied);
        assert!(matches!(duck.events.first(), Some(PeerEvent::Replied { .. })));
    }

    #[test]
    fn our_own_messages_are_ignored() {
        let mut duck = inner();
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "greet", "from": "self0001", "name": "Mine", "to": "all" }),
            source(8767),
            1000,
        );
        assert!(duck.peers.is_empty());
        assert!(duck.events.is_empty());
    }
}
