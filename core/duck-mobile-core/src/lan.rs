//! Other ducks on the same LAN.
//!
//! The physical robot has no such feature; this is the phone's social layer.
//! Each app announces itself on UDP 8766 every two seconds, keeps the peers
//! it has heard from, and can send a greet or a small gift. A greet is
//! answered with a hello back and both ducks react with their own official
//! voices. A gift is one of the duck's own reactions too: a treat becomes the
//! ground-pick peck, an emotion becomes that official emotion.
//!
//! The protocol is deliberately tiny and cannot command anything: the only
//! verbs are announce, greet, hello and gift. A greeting or a gift reaches
//! the duck as one of its own reactions, never as a control input or an
//! observation.

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
/// One gift per peer in each direction per three seconds; a flood is one
/// gift. Public because the HTTP sender keeps the same window.
pub const GIFT_EVERY_MS: u128 = 3_000;

/// The two things a duck can gift. A treat is food; an emotion is a mood by
/// its official name (`value`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiftKind {
    Treat,
    Emotion,
}

impl GiftKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            GiftKind::Treat => "treat",
            GiftKind::Emotion => "emotion",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PeerEvent {
    /// Somebody said hello to this duck.
    Greeted { from: String, name: String },
    /// A duck we greeted answered.
    Replied { from: String, name: String },
    /// A known peer sent a gift, addressed to us or to every duck. The
    /// emotion name is carried through unvalidated: lan.rs has no runtime
    /// and cannot know which official emotions this build loaded, so the
    /// apply layer (ffi.rs) checks the name and drops an unknown gift.
    Gift {
        from: String,
        name: String,
        gift: GiftKind,
        value: String,
    },
}

struct Peer {
    id: String,
    name: String,
    addr: SocketAddr,
    last_seen_ms: u128,
    replied: bool,
    /// Set when we greeted this duck, consumed by their hello: only a duck
    /// we actually greeted can be marked replied.
    awaiting: bool,
    /// The last greet we answered, so a flood of datagrams is one greeting.
    last_greet_ms: u128,
    /// The last gift we accepted from this duck: one per three seconds.
    last_gift_ms: u128,
    /// The last gift we sent this duck: the sender's own window.
    last_sent_gift_ms: u128,
}

struct Inner {
    id: String,
    name: String,
    peers: Vec<Peer>,
    events: Vec<PeerEvent>,
    /// The official emotion names this build loaded: an emotion gift for a
    /// name that is not on the list is refused at the wire, so a junk packet
    /// spends neither the window nor a place in the event queue.
    emotions: Vec<String>,
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
            // Empty until duck_core_load_emotions publishes the official
            // list; an emotion gift before then is simply refused.
            emotions: Vec::new(),
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

    /// The official emotion names, for refusing an unknown mood at the wire.
    pub fn set_emotions(&self, names: Vec<String>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.emotions = names;
        }
    }

    /// Say hello to one peer, or to every duck when `target` is None.
    pub fn greet(&self, target: Option<&str>) {
        let (from, name, peer) = match self.inner.lock() {
            Ok(mut inner) => {
                // Mark who we asked, so their hello is the only thing that
                // can flip the replied flag.
                match target {
                    Some(id) => {
                        if let Some(peer) = inner.peers.iter_mut().find(|peer| peer.id == id) {
                            peer.awaiting = true;
                        }
                    }
                    None => {
                        for peer in inner.peers.iter_mut() {
                            peer.awaiting = true;
                        }
                    }
                }
                (
                    inner.id.clone(),
                    inner.name.clone(),
                    target.and_then(|id| inner.peers.iter().find(|peer| peer.id == id).map(|peer| peer.addr)),
                )
            }
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

    /// Send one gift to a known peer: a treat, or an emotion by its official
    /// name. The packet is unicast to the address greet uses, so a gift never
    /// becomes a broadcast to ducks that were never chosen. Validation (an
    /// unknown peer) is checked before anything leaves the socket; the one
    /// per-peer window is spent only when the send succeeds. `claim` says the
    /// HTTP layer has already promised this send the window, so an in-flight
    /// queued send cannot be refused as too soon at the last moment.
    pub fn gift(&self, target: &str, gift: GiftKind, value: &str) -> Result<(), String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "the peer table is busy".to_owned())?;
        let now = now_ms();
        let (from, name) = (inner.id.clone(), inner.name.clone());
        let Some(peer) = inner.peers.iter_mut().find(|peer| peer.id == target) else {
            return Err("no such peer".to_owned());
        };
        let message = json!({
            "v": VERSION,
            "type": "gift",
            "from": from,
            "name": name,
            "to": target,
            "gift": gift.as_str(),
            // A treat carries no mood; an emotion carries its name.
            "value": match gift {
                GiftKind::Emotion => value,
                GiftKind::Treat => "",
            },
        });
        // Only a packet that really left spends the window: a failed send
        // must not silence the next try.
        self.socket
            .send_to(message.to_string().as_bytes(), peer.addr)
            .map_err(|error| error.to_string())?;
        peer.last_sent_gift_ms = now;
        Ok(())
    }

    /// Whether a gift to `target` may leave now under the one-per-peer
    /// window, without sending. The HTTP route calls this synchronously so
    /// "too soon" is answered while the client still waits, and the queued
    /// send then rides on the promise. False does not spend the window.
    pub fn gift_window_open(&self, target: &str) -> bool {
        let inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(_) => return false,
        };
        let now = now_ms();
        inner
            .peers
            .iter()
            .find(|peer| peer.id == target)
            .is_some_and(|peer| now.saturating_sub(peer.last_sent_gift_ms) >= GIFT_EVERY_MS)
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
    // Only the four real verbs may touch the peer table: any other type is
    // noise, and noise must not create peers or evict the ducks we know.
    if !matches!(kind, "announce" | "greet" | "hello" | "gift") {
        return None;
    }
    let from = message
        .get("from")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if from.is_empty() || from == inner.id {
        return None;
    }
    let name = clean_name(message.get("name").and_then(Value::as_str).unwrap_or("Duck"));
    match kind {
        "announce" => {
            upsert(inner, from, &name, source, now);
            None
        }
        "greet" => {
            upsert(inner, from, &name, source, now);
            let to = message.get("to").and_then(Value::as_str).unwrap_or("all");
            if to != "all" && to != inner.id {
                return None;
            }
            // A LAN flood must not become a chorus of emotions. One greet
            // per peer per second is both a greeting and a reply.
            if let Some(peer) = inner.peers.iter_mut().find(|peer| peer.id == from) {
                if now.saturating_sub(peer.last_greet_ms) < 1_000 {
                    return None;
                }
                peer.last_greet_ms = now;
            }
            inner.events.push(PeerEvent::Greeted {
                from: from.to_owned(),
                name: name.clone(),
            });
            if inner.events.len() > 32 {
                inner.events.remove(0);
            }
            // Replying hello does not make them "answered": the flag is only
            // set when we receive their hello, below.
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
            // A hello only counts from a duck we greeted: the awaiting flag
            // is set by greet() and consumed here. A forged or flooded hello
            // can neither forge "replied" nor crowd the emotion queue.
            let to = message.get("to").and_then(Value::as_str).unwrap_or("all");
            if to != "all" && to != inner.id {
                return None;
            }
            let Some(peer) = inner.peers.iter_mut().find(|peer| peer.id == from) else {
                return None;
            };
            if !peer.awaiting {
                return None;
            }
            peer.awaiting = false;
            peer.last_seen_ms = now;
            peer.replied = true;
            inner.events.push(PeerEvent::Replied {
                from: from.to_owned(),
                name,
            });
            if inner.events.len() > 32 {
                inner.events.remove(0);
            }
            None
        }
        "gift" => {
            // A gift is not a discovery: only a duck already in the table may
            // send one, so announce/greet remain the only verbs that create
            // peers. A gift may refresh that peer's last_seen.
            let to = message.get("to").and_then(Value::as_str).unwrap_or_default();
            if to != "all" && to != inner.id {
                return None;
            }
            let gift = match message.get("gift").and_then(Value::as_str) {
                Some("treat") => GiftKind::Treat,
                Some("emotion") => GiftKind::Emotion,
                _ => return None,
            };
            let value = message
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            // An emotion gift must name a mood this build actually has: the
            // wire refuses it here, so a junk packet spends neither the
            // window nor a place in the event queue. The runtime stays the
            // authority at apply time (a mood could have been swapped since
            // the snapshot).
            if gift == GiftKind::Emotion && !inner.emotions.iter().any(|n| *n == value) {
                return None;
            }
            let Some(peer) = inner.peers.iter_mut().find(|peer| peer.id == from) else {
                return None;
            };
            // One gift per peer per three seconds; extra packets inside the
            // window are dropped silently, exactly like a greet flood.
            if now.saturating_sub(peer.last_gift_ms) < GIFT_EVERY_MS {
                return None;
            }
            peer.last_gift_ms = now;
            peer.last_seen_ms = now;
            inner.events.push(PeerEvent::Gift {
                from: from.to_owned(),
                name,
                gift,
                value,
            });
            if inner.events.len() > 32 {
                inner.events.remove(0);
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
        awaiting: false,
        last_greet_ms: 0,
        last_gift_ms: 0,
        last_sent_gift_ms: 0,
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
            emotions: vec!["laugh".to_owned(), "excited".to_owned()],
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
        // We greeted them (Lan::greet marks the peer): only then can their
        // hello be an answer.
        duck.peers[0].awaiting = true;
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
    fn a_stranger_hello_forges_nothing() {
        let mut duck = inner();
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "announce", "from": "peer01", "name": "Sunny" }),
            source(8767),
            1000,
        );
        // Nobody greeted them: a hello cannot claim to be an answer.
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "hello", "from": "peer01", "name": "Sunny", "to": "self0001" }),
            source(8767),
            1200,
        );
        assert!(!duck.peers[0].replied);
        assert!(duck.events.is_empty());
    }

    #[test]
    fn a_flood_of_junk_creates_no_peers() {
        let mut duck = inner();
        for index in 0..50 {
            ingest(
                &mut duck,
                &json!({ "v": 1, "type": format!("junk{index}"), "from": format!("f{index:02}"), "name": "spoof" }),
                source(9000 + index as u16),
                1000 + index,
            );
        }
        assert!(duck.peers.is_empty(), "noise must not crowd the table");
        assert!(duck.events.is_empty());
    }

    #[test]
    fn a_hello_flood_becomes_one_answer() {
        let mut duck = inner();
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "announce", "from": "peer01", "name": "Sunny" }),
            source(8767),
            1000,
        );
        duck.peers[0].awaiting = true;
        for index in 0..50 {
            ingest(
                &mut duck,
                &json!({ "v": 1, "type": "hello", "from": "peer01", "name": "Sunny", "to": "all" }),
                source(8767),
                1100 + index,
            );
        }
        let replies = duck
            .events
            .iter()
            .filter(|event| matches!(event, PeerEvent::Replied { .. }))
            .count();
        assert_eq!(replies, 1, "one greet, one answer, however many hellos");
    }

    #[test]
    fn a_gift_from_a_known_peer_becomes_an_event() {
        let mut duck = inner();
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "announce", "from": "peer01", "name": "Sunny" }),
            source(8767),
            1000,
        );
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "gift", "from": "peer01", "name": "Sunny", "to": "self0001", "gift": "treat", "value": "" }),
            source(8767),
            10_000,
        );
        assert_eq!(duck.peers.len(), 1);
        // The gift is not a discovery, but it keeps the sender alive.
        assert_eq!(duck.peers[0].last_seen_ms, 10_000);
        assert_eq!(duck.peers[0].last_gift_ms, 10_000);
        match duck.events.first() {
            Some(PeerEvent::Gift {
                from,
                name,
                gift,
                value,
            }) => {
                assert_eq!(from, "peer01");
                assert_eq!(name, "Sunny");
                assert_eq!(*gift, GiftKind::Treat);
                assert!(value.is_empty());
            }
            other => panic!("expected a gift, got {other:?}"),
        }
    }

    #[test]
    fn a_gift_from_a_stranger_creates_no_peer_and_no_event() {
        let mut duck = inner();
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "gift", "from": "ghost01", "name": "Ghost", "to": "all", "gift": "treat", "value": "" }),
            source(8767),
            10_000,
        );
        assert!(duck.peers.is_empty(), "a gift must not create a peer");
        assert!(duck.events.is_empty());
    }

    #[test]
    fn a_gift_for_another_duck_is_ignored() {
        let mut duck = inner();
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "announce", "from": "peer01", "name": "Sunny" }),
            source(8767),
            1000,
        );
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "gift", "from": "peer01", "name": "Sunny", "to": "peer02", "gift": "treat", "value": "" }),
            source(8767),
            10_000,
        );
        assert!(duck.events.is_empty());
    }

    #[test]
    fn a_gift_flood_is_one_gift() {
        let mut duck = inner();
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "announce", "from": "peer01", "name": "Sunny" }),
            source(8767),
            1000,
        );
        for index in 0..20 {
            ingest(
                &mut duck,
                &json!({ "v": 1, "type": "gift", "from": "peer01", "name": "Sunny", "to": "all", "gift": "treat", "value": "" }),
                source(8767),
                10_000 + index * 100,
            );
        }
        let gifts = duck
            .events
            .iter()
            .filter(|event| matches!(event, PeerEvent::Gift { .. }))
            .count();
        assert_eq!(gifts, 1, "one gift per peer per three seconds");
        // One millisecond past the window it is a new gift.
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "gift", "from": "peer01", "name": "Sunny", "to": "all", "gift": "emotion", "value": "excited" }),
            source(8767),
            13_000,
        );
        assert_eq!(duck.events.len(), 2);
    }

    #[test]
    fn an_unknown_emotion_is_refused_at_the_wire() {
        // The official list lives here now: an emotion gift for a mood this
        // build does not have is refused before it can spend the window or
        // take a place in the event queue. The runtime stays the authority
        // at apply time (a mood could have been swapped since the snapshot).
        let mut duck = inner();
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "announce", "from": "peer01", "name": "Sunny" }),
            source(8767),
            1000,
        );
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "gift", "from": "peer01", "name": "Sunny", "to": "all", "gift": "emotion", "value": "moonwalk" }),
            source(8767),
            10_000,
        );
        assert!(duck.events.is_empty());
        // The window is not spent by a refused gift: a real one right after
        // still lands.
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "gift", "from": "peer01", "name": "Sunny", "to": "all", "gift": "emotion", "value": "laugh" }),
            source(8767),
            10_000,
        );
        assert!(matches!(
            duck.events.first(),
            Some(PeerEvent::Gift { gift: GiftKind::Emotion, value, .. }) if value == "laugh"
        ));
    }

    #[test]
    fn an_unknown_gift_is_ignored() {
        let mut duck = inner();
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "announce", "from": "peer01", "name": "Sunny" }),
            source(8767),
            1000,
        );
        ingest(
            &mut duck,
            &json!({ "v": 1, "type": "gift", "from": "peer01", "name": "Sunny", "to": "all", "gift": "rocket", "value": "" }),
            source(8767),
            10_000,
        );
        assert!(duck.events.is_empty());
    }

    #[test]
    fn the_sender_keeps_one_gift_per_peer() {
        let socket = UdpSocket::bind(("127.0.0.1", 0)).expect("bind a test socket");
        let peer = Peer {
            id: "peer01".to_owned(),
            name: "Sunny".to_owned(),
            addr: "127.0.0.1:9".parse().unwrap(),
            last_seen_ms: 1,
            replied: false,
            awaiting: false,
            last_greet_ms: 0,
            last_gift_ms: 0,
            last_sent_gift_ms: 0,
        };
        let lan = Lan {
            inner: Arc::new(Mutex::new(Inner {
                id: "self0001".to_owned(),
                name: "Mine".to_owned(),
                peers: vec![peer],
                events: Vec::new(),
                emotions: vec!["laugh".to_owned()],
            })),
            socket: Arc::new(socket),
        };
        assert_eq!(lan.gift("peer01", GiftKind::Treat, ""), Ok(()));
        // One gift per peer per three seconds, across kinds: the route asks
        // gift_window_open before queueing, and the send itself trusts the
        // promise the route made.
        assert!(!lan.gift_window_open("peer01"));
        assert_eq!(lan.gift("peer01", GiftKind::Emotion, "excited"), Ok(()));
        // An unknown peer has no window at all.
        assert!(!lan.gift_window_open("ghost01"));
        assert_eq!(
            lan.gift("ghost01", GiftKind::Treat, ""),
            Err("no such peer".to_owned())
        );
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
