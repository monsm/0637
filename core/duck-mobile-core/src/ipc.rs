//! The official `robot.*` JSON-RPC/NDJSON protocol face, served over TCP loopback.
//!
//! Gap #9 of `docs/AUDIT_OFFICIAL_PARITY_2026-11-16.md`: the digital duck could only be
//! driven by the local HTTP intent API, so a client written against the official robot
//! protocol could not drive it. This module serves the wire those clients speak.
//!
//! Official basis, all read at `upstream/microduck@da34a22`:
//!
//! - `duck-ipc-proto/src/lib.rs` — the whole contract, used here **as a crate dependency**
//!   rather than copied or `#[path]`-included: `duck-mobile-core/Cargo.toml` already
//!   carries `duck-ipc-proto = { path = "../../upstream/microduck/duck-ipc-proto" }` and
//!   the checkout is byte-identical to the audit baseline (`git diff da34a22 --
//!   duck-ipc-proto` is empty), so every envelope, params and result type below is the
//!   official one — `Request`/`Response`/`Error`/`Id`, `MoveParams{vx,vy,vyaw}`,
//!   `HeadParams{neck_pitch,head_pitch,head_yaw,head_roll}`, `DoParams{skill}`,
//!   `SubscribeParams{hz}`, `SubscribeResult`, `HealthResult`, `RobotState`,
//!   `API_VERSION = 51`. `Call::parse` gives the official refusals: unknown method →
//!   `METHOD_NOT_FOUND` naming it, a params member the type does not know →
//!   `INVALID_PARAMS` naming the member (every params type is `deny_unknown_fields`).
//!   No field name in this file is invented; nothing is spelled twice.
//! - `robotd/src/main.rs` `handle()`/`dispatch()` — the serving behaviour mirrored
//!   here: NDJSON one object per line, `MAX_LINE = 64 * 1024` over-cap answers
//!   `INVALID_REQUEST` "request too large", unparsable JSON answers `PARSE_ERROR` with
//!   id `None`, notifications (no `id`) get no reply and continuous intents arrive that
//!   way at 50 Hz, `robot.subscribe` acknowledges with `SubscribeResult` and decimates
//!   per subscriber server-side (`hz.filter(|hz| *hz > 0)`, a `1/hz` period, absent or
//!   zero `hz` means every tick, re-subscribing replaces the rate rather than opening a
//!   second stream), a subscriber that cannot keep up loses frames and never applies
//!   backpressure (`STATE_BUFFER = 256`, "Five seconds at 50 Hz"). `hello` is answered
//!   with `api_version: API_VERSION` and, per the crate's own rule ("No daemon refuses a
//!   call because this number differs"), a peer reporting a different version is still
//!   served — the difference is reported, not refused.
//! - `docs/design/mobile-app.md` (drive screen: `robot.move`, `robot.subscribe`) and
//!   `CONTRIBUTING.md` ("JSON-RPC 2.0 one object per line") — the protocol's place.
//!
//! One deliberate deviation, and it is transport-only: officially these methods ride a
//! **unix socket** (`socket::ROBOT`, `/run/robotd.sock`). A phone process has no `/run`,
//! so the same bytes flow over a TCP socket bound to `127.0.0.1` on a fixed port
//! ([`ROBOT_IPC_PORT`]) — framing (single newline) and payloads are unchanged. Like
//! `intent.rs`'s loopback trust, no authentication: the loopback address is the phone
//! talking to itself, and the socket is never exposed on the LAN.
//!
//! Scope: the `robot.*` methods a phone can honestly serve — `hello`,
//! `robot.health`, `robot.safeToRestart`, `robot.move`, `robot.head`,
//! `robot.look`, `robot.stop`, `robot.enable`, `robot.init`, `robot.relax`,
//! `robot.rest`, `robot.shutdown`, `robot.do`, `robot.pose`, `robot.mouth`,
//! `robot.sound`, `robot.flashlight`, `robot.mode`, `robot.setMode`,
//! `robot.policies`, `robot.model`, `robot.vetbook`, `robot.skills`,
//! `robot.loadPolicy`, `robot.reloadPolicies` and `robot.subscribe`.
//!
//! Deliberately not served, and refused with `METHOD_NOT_FOUND` and the
//! official "is not served by robotd" sentence: `robot.theremin` and
//! `robot.chorale` (no ToF depth source, no chorale score player),
//! `robot.setSkill`/`robot.removeSkill` (the phone's skill set is the
//! manifest's; there is no `robotd.toml` edit surface), and the updaterd /
//! configd namespaces. That is the same refusal a `robotd` missing a route
//! gives, which is the designed skew behaviour rather than a parse error.
//! Extending the served set is a `dispatch` arm plus a [`RobotHandlers`]
//! method, not a protocol change.
//!
//! What this module does **not** do: it owns no runtime. Where the health verdict, the
//! twist and the per-tick state frames come from is [`RobotHandlers`] and
//! [`RobotIpcServer::publish_state`], injected by the wiring ticket — the same seam the
//! HTTP intent API's `IntentQueue` leaves to `ffi.rs`.
//!
//! `robot.state` carries no sequence number: the official frame correlates samples with
//! its monotonic `t` (seconds since daemon start), and `seq` exists only on the pad
//! streams (`PadFrame::seq`, `PadImuSample::seq`). Matching the official shape means no
//! seq here either.

pub use duck_ipc_proto as proto;

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The robot protocol's fixed loopback port.
///
/// Distinct from everything the core already binds: the HTTP intent API defaults to
/// 8765, `lan.rs` announces on UDP 8766 (and its tests source from 8767), `pad.rs`
/// receives DKPD on UDP 4210. Officially the robot face is a unix socket with no port
/// at all; 8768 is the loopback TCP stand-in, never bound on a LAN interface.
pub const ROBOT_IPC_PORT: u16 = 8768;

/// How large one request line may be before it is refused unread.
///
/// `robotd/src/main.rs`: `const MAX_LINE: usize = 64 * 1024;` — the same ceiling and
/// the same refusal, so a client's too-large request behaves identically against the
/// robot and the digital duck.
const MAX_LINE: usize = 64 * 1024;

/// How far a subscriber may fall behind before it starts losing frames.
///
/// `robotd/src/main.rs`: `const STATE_BUFFER: usize = 256;` — "Five seconds at 50 Hz.
/// State is advisory: a client that cannot keep up gets a gap, never backpressure onto
/// the control loop." The same bounded channel stands between `publish_state` and each
/// connection's writer here, and an overflow drops that subscriber's frame silently,
/// exactly like the official broadcast-lagged path.
const STATE_BUFFER: usize = 256;

/// Per-connection cap. The official daemon caps nothing (tokio tasks are cheap); a
/// phone process bounds its threads instead. Loopback-only and the shells hold one or
/// two connections, so 16 is headroom, not a design point. An over-cap connection is
/// answered with the protocol's own `BUSY` code ("retry on BUSY", per `code`'s doc)
/// and closed.
const MAX_CONNECTIONS: usize = 16;

/// How long one accepted connection may keep the accept loop from noticing shutdown.
const ACCEPT_POLL_MS: u64 = 20;

/// The robot-side behaviours, injected by the wiring ticket.
///
/// Everything returns official wire types, so the answer's shape is guaranteed by
/// construction and cannot drift a field name. The wiring ticket implements this
/// against the same execution path the HTTP intent API drives (`ffi.rs`), which is why
/// the traits carry no business logic here.
pub trait RobotHandlers: Send + Sync + 'static {
    /// `robot.health`'s answer — the verdict and its numbers, official shape.
    fn health(&self) -> proto::HealthResult;

    /// `robot.move` — a velocity twist (m/s, m/s, rad/s; trunk frame, positive `vyaw`
    /// turns left). Continuous intent; also arrives as a notification at 50 Hz.
    fn move_twist(&self, params: proto::MoveParams) -> proto::IntentResult;

    /// `robot.head` — head joint targets in radians. Continuous intent; also arrives
    /// as a notification.
    fn head(&self, params: proto::HeadParams) -> proto::IntentResult;

    /// `robot.do` — a one-shot skill by name (or the sit↔stand toggle). The refusal
    /// reasons are the handler's: the official ones name what the robot *can* do.
    fn do_skill(&self, params: proto::DoParams) -> proto::IntentResult;

    /// `robot.look` — solve the head IK for a trunk-frame point and hand the joints to
    /// the same path `robot.head` feeds. The official dispatch does the IK on the IPC
    /// side (`robotd/src/main.rs:5395-5410`), so this answers with `LookResult`
    /// immediately rather than waiting for a control tick.
    fn look(&self, _params: proto::LookParams) -> proto::LookResult {
        proto::LookResult {
            head: proto::HeadParams::default(),
            clamped: true,
        }
    }

    /// `robot.pose` — a standing body pose. Continuous intent.
    fn pose(&self, _params: proto::PoseParams) -> proto::IntentResult {
        proto::IntentResult::accepted()
    }

    /// `robot.mouth` — beak opening, 0..1. Continuous intent.
    fn mouth(&self, _params: proto::MouthParams) -> proto::IntentResult {
        proto::IntentResult::accepted()
    }

    /// `robot.sound` — a voice-bank tag. The wheee tag is a hold, not a one-shot.
    fn sound(&self, _params: proto::SoundParams) -> proto::IntentResult {
        proto::IntentResult::accepted()
    }

    /// `robot.flashlight` — the virtual face LED.
    fn flashlight(&self, _params: proto::FlashlightParams) -> proto::IntentResult {
        proto::IntentResult::accepted()
    }

    /// `robot.stop` — zero the twist. Official `Intents::stop` does exactly that and
    /// leaves the policy enabled (`intents.rs:328-330`).
    fn stop(&self) -> proto::IntentResult {
        proto::IntentResult::accepted()
    }

    /// `robot.enable` — ask the policy to drive (`on:false` stops the policy and keeps
    /// torque on). `toggle` flips the robot's own state.
    fn enable(&self, _params: proto::EnableParams) -> proto::IntentResult {
        proto::IntentResult::accepted()
    }

    /// `robot.init` — torque on and ramp to the home pose, no policy needed.
    fn init(&self) -> proto::IntentResult {
        proto::IntentResult::accepted()
    }

    /// `robot.relax` — cut torque and stay powered/limp.
    fn relax(&self) -> proto::IntentResult {
        proto::IntentResult::accepted()
    }

    /// `robot.rest` — sit and ease into the rest pose without powering off.
    fn rest(&self) -> proto::IntentResult {
        proto::IntentResult::accepted()
    }

    /// `robot.shutdown` — sit, rest, torque off, power-off latch.
    fn shutdown(&self) -> proto::IntentResult {
        proto::IntentResult::accepted()
    }

    /// `robot.mode` — which drive bundle is loaded.
    fn mode(&self) -> proto::ModeResult {
        proto::ModeResult::default()
    }

    /// `robot.setMode` — switch the whole drive bundle.
    fn set_mode(&self, _params: proto::SetModeParams) -> proto::IntentResult {
        proto::IntentResult::refused("mode switching is not wired")
    }

    /// `robot.policies` — what each slot is actually running.
    fn policies(&self) -> proto::PoliciesResult {
        proto::PoliciesResult::default()
    }

    /// `robot.model` — the static geometry the per-tick poses are stated in.
    fn model(&self) -> proto::ModelResult {
        proto::ModelResult::default()
    }

    /// `robot.vetbook` — the lifetime record. A JSON value rather than the official
    /// `Vetbook` struct: the phone's book publishes the same counters in the flat shape
    /// `book().summary()` has always served (`/life`, the shells).
    fn vetbook(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// `robot.skills` — every skill, with its parameters, and the built-ins.
    fn skills(&self) -> proto::SkillsResult {
        proto::SkillsResult::default()
    }

    /// `robot.loadPolicy` — point one slot at a file (or reset it), then reload.
    fn load_policy(&self, _params: proto::LoadPolicyParams) -> proto::IntentResult {
        proto::IntentResult::refused("policy loading is not wired")
    }

    /// `robot.reloadPolicies` — re-read every slot from disk.
    fn reload_policies(&self) -> proto::IntentResult {
        proto::IntentResult::accepted()
    }

    /// `robot.safeToRestart` — whether restarting motor control now would drop the duck.
    fn safe_to_restart(&self) -> proto::SafeToRestartResult {
        proto::SafeToRestartResult {
            safe: true,
            reason: None,
        }
    }

    /// The `robot.subscribe` acknowledgement: the policy identity that is constant for
    /// the life of the process. Defaults to a bare acceptance; the wiring ticket fills
    /// `walk`/`stand`/`skills` from what is actually loaded.
    fn subscribe_info(&self) -> proto::SubscribeResult {
        proto::SubscribeResult {
            accepted: true,
            ..Default::default()
        }
    }

    /// The source revision of the running binary, for `hello`'s `revision` field.
    /// `None` is the official shape for "not from CI"; the wiring ticket may hand over
    /// the provenance fingerprint.
    fn revision(&self) -> Option<String> {
        None
    }
}

/// The connection's write side: responses (reader thread) and `robot.state`
/// notifications (writer thread) both go through one mutex, so lines interleave
/// atomically — each is a complete NDJSON line, and JSON-RPC says nothing about their
/// relative order.
type WriteHalf = Arc<Mutex<TcpStream>>;

/// One subscriber in the publish registry: its channel's write end and an identity the
/// connection's guard uses to remove itself.
struct SubEntry {
    id: u64,
    tx: SyncSender<Vec<u8>>,
}

/// Per-subscriber decimation, exactly `handle()`'s pair: the period `1/hz` (or `ZERO`
/// for every tick) and the instant of the last frame actually written.
struct StreamState {
    period: Duration,
    last_sent: Option<Instant>,
}

impl Default for StreamState {
    fn default() -> Self {
        Self {
            period: Duration::ZERO,
            last_sent: None,
        }
    }
}

/// Removes this connection's registry entry when the connection ends.
struct SubGuard {
    id: u64,
    registry: Arc<Mutex<Vec<SubEntry>>>,
}

impl Drop for SubGuard {
    fn drop(&mut self) {
        if let Ok(mut subs) = self.registry.lock() {
            subs.retain(|entry| entry.id != self.id);
        }
    }
}

/// The per-connection subscription bookkeeping the reader thread owns. Dropping it —
/// which happens when the connection ends for any reason — removes the registry entry,
/// which drops the last sender and ends the writer thread.
struct ConnState {
    registry: Arc<Mutex<Vec<SubEntry>>>,
    write: WriteHalf,
    stream_state: Arc<Mutex<StreamState>>,
    guard: Option<SubGuard>,
}

impl ConnState {
    fn new(registry: Arc<Mutex<Vec<SubEntry>>>, write: WriteHalf) -> Self {
        Self {
            registry,
            write,
            stream_state: Arc::new(Mutex::new(StreamState::default())),
            guard: None,
        }
    }

    /// `robot.subscribe` — official `handle()`:
    ///
    /// ```text
    /// decimate = params.hz.filter(|hz| *hz > 0)
    ///     .map(|hz| Duration::from_secs_f64(1.0 / hz as f64))
    ///     .unwrap_or(Duration::ZERO);
    /// // Subscribing again replaces the rate rather than opening a second stream.
    /// states = Some(state.state_tx.subscribe());
    /// last_sent = None;
    /// ```
    ///
    /// The stream is a bounded channel per connection rather than a broadcast
    /// receiver; decimation still happens at write time, per subscriber, server-side.
    fn subscribe(&mut self, hz: Option<u32>) -> std::io::Result<()> {
        let period = hz
            .filter(|hz| *hz > 0)
            .map(|hz| Duration::from_secs_f64(1.0 / hz as f64))
            .unwrap_or(Duration::ZERO);

        if self.guard.is_none() {
            let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(STATE_BUFFER);
            let id = NEXT_SUB_ID.fetch_add(1, Ordering::Relaxed);
            self.registry.lock().unwrap().push(SubEntry { id, tx });

            let write = self.write.clone();
            let stream_state = self.stream_state.clone();
            let spawned = std::thread::Builder::new()
                .name("duck-robot-ipc-state".to_owned())
                .spawn(move || state_writer(rx, write, stream_state));
            match spawned {
                Ok(_) => {
                    self.guard = Some(SubGuard {
                        id,
                        registry: self.registry.clone(),
                    });
                }
                // Practically unreachable; the honest answer is an error, not an
                // "accepted" that never streams.
                Err(_) => {
                    self.registry.lock().unwrap().retain(|entry| entry.id != id);
                    return Err(std::io::Error::other("cannot start the state writer"));
                }
            }
        }

        *self.stream_state.lock().unwrap() = StreamState {
            period,
            last_sent: None,
        };
        Ok(())
    }
}

static NEXT_SUB_ID: AtomicU64 = AtomicU64::new(1);

/// One frame on one subscriber's channel, written only when the subscriber's own rate
/// says it is due. `recv()` hands over every frame the publisher enqueued — a
/// subscriber at 10 Hz fed by a faster source reads one and discards the rest, which
/// is the official decimation shape (`rx.recv()` then the `due` check in `handle()`).
fn state_writer(rx: Receiver<Vec<u8>>, write: WriteHalf, stream_state: Arc<Mutex<StreamState>>) {
    loop {
        let Ok(line) = rx.recv() else {
            // All senders gone: the connection ended and the guard removed the
            // registry entry. Nothing left to stream.
            break;
        };
        let due = {
            let mut st = stream_state.lock().unwrap();
            let due = st
                .last_sent
                .map(|at| at.elapsed() >= st.period)
                .unwrap_or(true);
            if due {
                st.last_sent = Some(Instant::now());
            }
            due
        };
        if !due {
            continue;
        }
        let mut out = write.lock().unwrap();
        if out.write_all(&line).and_then(|_| out.flush()).is_err() {
            break;
        }
    }
}

/// A running `robot.*` server. Dropping it (or calling [`stop`](Self::stop)) ends the
/// accept loop and closes the port; connections already serving run until their client
/// goes away, the same deal `intent.rs`'s server offers.
pub struct RobotIpcServer {
    shutdown: Arc<AtomicBool>,
    registry: Arc<Mutex<Vec<SubEntry>>>,
}

impl RobotIpcServer {
    /// Stop accepting. Existing connections are not force-closed.
    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// How many connections are currently subscribed to `robot.state`. Also how the
    /// tests assert disconnect cleanup, and how the wiring ticket can tell whether
    /// `publish_state` has anyone to feed.
    pub fn subscriber_count(&self) -> usize {
        self.registry.lock().map(|subs| subs.len()).unwrap_or(0)
    }

    /// Feed one tick's state to every subscriber, server-side decimated.
    ///
    /// The wiring ticket calls this once per control-loop tick with the same
    /// `proto::RobotState` the official loop publishes. With nobody subscribed this is
    /// a registry peek — the official loop skips frame assembly on
    /// `receiver_count() == 0` for the same reason ("a per-tick allocation on the
    /// thread that should not be visiting the allocator without a reason").
    ///
    /// The frame is serialised once and handed to each subscriber's bounded channel
    /// with `try_send`: a subscriber that is behind loses the frame rather than
    /// blocking the control loop, which is the official contract verbatim.
    ///
    /// One inherited caveat: a frame carrying a NaN serialises to `"params":null` —
    /// the official `Request::notify_state` has the same `unwrap_or(Value::Null)`.
    pub fn publish_state(&self, state: &proto::RobotState) {
        let subs = match self.registry.lock() {
            Ok(subs) => subs,
            Err(_) => return,
        };
        if subs.is_empty() {
            return;
        }
        let mut line = match serde_json::to_vec(&proto::Request::notify_state(state)) {
            Ok(bytes) => bytes,
            Err(_) => return,
        };
        line.push(b'\n');
        for entry in subs.iter() {
            match entry.tx.try_send(line.clone()) {
                Ok(()) => {}
                // Full: this subscriber fell behind; it gets a gap, not backpressure.
                // Disconnected: the connection is ending and its guard is on the way.
                Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
            }
        }
    }
}

impl Drop for RobotIpcServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Start the server on `127.0.0.1:port`, normally [`ROBOT_IPC_PORT`].
pub fn start(port: u16, handlers: Arc<dyn RobotHandlers>) -> std::io::Result<RobotIpcServer> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    Ok(start_listener(listener, handlers))
}

/// Start the server on an already-bound listener (port 0 in tests).
///
/// Same skeleton as `intent.rs`'s accept loop: non-blocking accept polled against a
/// shutdown flag, streams restored to blocking for their handler (BSD and macOS hand
/// back a stream that inherited the listener's non-blocking mode), a refused accept
/// sleeps instead of hot-spinning, and a thread spawn failure costs one connection,
/// never the listener.
pub fn start_listener(listener: TcpListener, handlers: Arc<dyn RobotHandlers>) -> RobotIpcServer {
    let shutdown = Arc::new(AtomicBool::new(false));
    let registry: Arc<Mutex<Vec<SubEntry>>> = Arc::new(Mutex::new(Vec::new()));
    let flag = shutdown.clone();
    let registry_for_loop = registry.clone();
    listener.set_nonblocking(true).ok();
    let _ = std::thread::Builder::new()
        .name("duck-robot-ipc".to_owned())
        .spawn(move || {
            let registry = registry_for_loop;
            let live = Arc::new(AtomicUsize::new(0));
            loop {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                let mut stream = match listener.accept() {
                    Ok((stream, _peer)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(ACCEPT_POLL_MS));
                        continue;
                    }
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                };
                stream.set_nonblocking(false).ok();
                if live.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    // Over capacity: say so in the protocol's own vocabulary and let
                    // the client retry. The line is written best-effort; the
                    // connection closes either way.
                    let response = proto::Response::err(
                        None,
                        proto::Error::new(proto::code::BUSY, "too many connections"),
                    );
                    if let Ok(mut line) = serde_json::to_vec(&response) {
                        line.push(b'\n');
                        let _ = stream.write_all(&line);
                    }
                    continue;
                }
                let handlers = handlers.clone();
                let registry = registry.clone();
                let live = live.clone();
                // A refused thread drops only this connection, never the listener.
                let spawned = std::thread::Builder::new()
                    .name("duck-robot-ipc-conn".to_owned())
                    .spawn(move || {
                        live.fetch_add(1, Ordering::SeqCst);
                        let _ = serve_connection(stream, handlers, registry);
                        live.fetch_sub(1, Ordering::SeqCst);
                    });
                if spawned.is_err() {
                    let _ = std::thread::sleep(Duration::from_millis(10));
                }
            }
        });
    RobotIpcServer { shutdown, registry }
}

/// What came off the socket: a complete line, a line past [`MAX_LINE`], or the end of
/// the connection.
enum Incoming {
    Line(Vec<u8>),
    TooLong,
    Eof,
}

/// Read one newline-terminated line without ever holding more than `cap` bytes.
///
/// The official server reads a line and checks its length afterwards (`handle()`:
/// `if line.len() > MAX_LINE`), which lets one giant line buffer unboundedly first.
/// The observable behaviour — refuse with `INVALID_REQUEST` "request too large", id
/// `None`, connection continues — is reproduced here, with the memory bounded: the
/// over-cap tail is drained in chunks through the next newline and discarded. A final
/// chunk without a newline is a line, as the official `lines()` yields it.
fn read_line_capped(reader: &mut BufReader<TcpStream>, cap: usize) -> std::io::Result<Incoming> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(if buf.is_empty() {
                Incoming::Eof
            } else {
                Incoming::Line(buf)
            });
        }
        if let Some(i) = available.iter().position(|&b| b == b'\n') {
            buf.extend_from_slice(&available[..=i]);
            reader.consume(i + 1);
            return Ok(if line_len(&buf) > cap {
                Incoming::TooLong
            } else {
                Incoming::Line(buf)
            });
        }
        let n = available.len();
        buf.extend_from_slice(available);
        reader.consume(n);
        if buf.len() > cap {
            loop {
                let chunk = reader.fill_buf()?;
                if chunk.is_empty() {
                    return Ok(Incoming::TooLong);
                }
                if let Some(i) = chunk.iter().position(|&b| b == b'\n') {
                    reader.consume(i + 1);
                    return Ok(Incoming::TooLong);
                }
                let n = chunk.len();
                reader.consume(n);
            }
        }
    }
}

/// The line's length the way the official check measures it: newline stripped, as
/// tokio's `lines()` does (it also strips a preceding `\r`).
fn line_len(line: &[u8]) -> usize {
    let mut end = line.len();
    if end > 0 && line[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && line[end - 1] == b'\r' {
        end -= 1;
    }
    end
}

/// Serve one connection until the peer goes away.
///
/// The structure is `robotd`'s `handle()`, on std threads instead of tokio: the reader
/// loop below owns requests and responses; a writer thread per subscription owns the
/// `robot.state` stream; both write through one mutex.
fn serve_connection(
    stream: TcpStream,
    handlers: Arc<dyn RobotHandlers>,
    registry: Arc<Mutex<Vec<SubEntry>>>,
) -> std::io::Result<()> {
    let write: WriteHalf = Arc::new(Mutex::new(stream.try_clone()?));
    let mut reader = BufReader::new(stream);
    let mut conn = ConnState::new(registry, write.clone());

    loop {
        let incoming = read_line_capped(&mut reader, MAX_LINE)?;
        let line = match incoming {
            Incoming::Eof => return Ok(()),
            Incoming::TooLong => {
                write_response(
                    &write,
                    &proto::Response::err(
                        None,
                        proto::Error::new(proto::code::INVALID_REQUEST, "request too large"),
                    ),
                )?;
                continue;
            }
            Incoming::Line(line) => line,
        };

        // Invalid UTF-8 ends the connection, as the official `lines()` reader does
        // (io::InvalidData out of `handle()`).
        let Ok(text) = String::from_utf8(line) else {
            return Ok(());
        };
        if text.trim().is_empty() {
            continue;
        }

        let request: proto::Request = match serde_json::from_str(&text) {
            Ok(request) => request,
            Err(e) => {
                write_response(
                    &write,
                    &proto::Response::err(
                        None,
                        proto::Error::new(proto::code::PARSE_ERROR, e.to_string()),
                    ),
                )?;
                continue;
            }
        };

        let call = request.as_call();

        // Notifications get no reply, per the spec. Continuous intents arrive this
        // way — at 50 Hz a response per message would be pure overhead. A call that
        // fails to parse on a notification is ignored, as in the official handler.
        let Some(id) = request.id.clone() else {
            if let Ok(call) = call {
                apply_notification(&handlers, &call);
            }
            continue;
        };

        // `robot.subscribe` needs the connection's own state, so it is handled before
        // generic dispatch — the official `handle()` does the same, ahead of
        // `dispatch`.
        if let Ok(proto::Call::RobotSubscribe(params)) = &call {
            if let Err(e) = conn.subscribe(params.hz) {
                write_response(
                    &write,
                    &proto::Response::err(
                        Some(id),
                        proto::Error::new(proto::code::INTERNAL_ERROR, e.to_string()),
                    ),
                )?;
                continue;
            }
        }

        let response = match call {
            // The stream is set up; the acknowledgement carries the policy identity.
            Ok(call @ proto::Call::RobotSubscribe(_)) => {
                let _ = &call;
                proto::Response::ok(Some(id), &handlers.subscribe_info())
            }
            Ok(call) => dispatch(&handlers, id, &call),
            Err(e) => proto::Response::err(Some(id), e),
        };
        write_response(&write, &response)?;
    }
}

/// Apply a notification intent. Shared shape with the official `apply_intent`: move,
/// head and do reach their handlers with no reply; nothing else applies.
fn apply_notification(handlers: &Arc<dyn RobotHandlers>, call: &proto::Call) {
    // `apply_intent`'s set: move, head, pose and mouth are continuous, plus a sound
    // (the wheee hold arrives every tick) and a skill (a held button resends it).
    // Nothing else applies as a notification.
    match call {
        proto::Call::RobotMove(p) => {
            handlers.move_twist(*p);
        }
        proto::Call::RobotHead(p) => {
            handlers.head(*p);
        }
        proto::Call::RobotPose(p) => {
            handlers.pose(*p);
        }
        proto::Call::RobotMouth(p) => {
            handlers.mouth(*p);
        }
        proto::Call::RobotDo(p) => {
            handlers.do_skill(p.clone());
        }
        proto::Call::RobotSound(p) => {
            handlers.sound(*p);
        }
        _ => {}
    }
}

/// Answer one request.
///
/// Synchronous and allocation-light on purpose: these answers must be available even
/// when everything else is broken (the official `dispatch`'s own words). The served
/// set is ticket E's; anything else that parsed as an official `Call` is refused the
/// way a `robotd` without the route refuses it.
fn dispatch(
    handlers: &Arc<dyn RobotHandlers>,
    id: proto::Id,
    call: &proto::Call,
) -> proto::Response {
    match call {
        proto::Call::Hello(_) => proto::Response::ok(
            Some(id),
            &proto::HelloResult {
                api_version: proto::API_VERSION,
                daemon_version: proto::semver::Version::parse(env!("CARGO_PKG_VERSION")).ok(),
                revision: handlers.revision(),
            },
        ),
        proto::Call::RobotHealth => proto::Response::ok(Some(id), &handlers.health()),
        proto::Call::RobotSafeToRestart => {
            proto::Response::ok(Some(id), &handlers.safe_to_restart())
        }
        // Continuous intents answered on the request path too: "a client that sends
        // `robot.move` with an `id` is not silently ignored — the spec permits either,
        // and refusing one because of a framing choice would be a surprise."
        proto::Call::RobotMove(p) => proto::Response::ok(Some(id), &handlers.move_twist(*p)),
        proto::Call::RobotHead(p) => proto::Response::ok(Some(id), &handlers.head(*p)),
        proto::Call::RobotPose(p) => proto::Response::ok(Some(id), &handlers.pose(*p)),
        proto::Call::RobotMouth(p) => proto::Response::ok(Some(id), &handlers.mouth(*p)),
        proto::Call::RobotDo(p) => proto::Response::ok(Some(id), &handlers.do_skill(p.clone())),
        proto::Call::RobotLook(p) => proto::Response::ok(Some(id), &handlers.look(*p)),
        proto::Call::RobotStop => proto::Response::ok(Some(id), &handlers.stop()),
        proto::Call::RobotEnable(p) => proto::Response::ok(Some(id), &handlers.enable(*p)),
        proto::Call::RobotInit => proto::Response::ok(Some(id), &handlers.init()),
        proto::Call::RobotRelax => proto::Response::ok(Some(id), &handlers.relax()),
        proto::Call::RobotRest => proto::Response::ok(Some(id), &handlers.rest()),
        proto::Call::RobotShutdown => proto::Response::ok(Some(id), &handlers.shutdown()),
        proto::Call::RobotSound(p) => proto::Response::ok(Some(id), &handlers.sound(*p)),
        proto::Call::RobotFlashlight(p) => proto::Response::ok(Some(id), &handlers.flashlight(*p)),
        proto::Call::RobotMode => proto::Response::ok(Some(id), &handlers.mode()),
        proto::Call::RobotSetMode(p) => {
            proto::Response::ok(Some(id), &handlers.set_mode(p.clone()))
        }
        proto::Call::RobotPolicies => proto::Response::ok(Some(id), &handlers.policies()),
        proto::Call::RobotModel => proto::Response::ok(Some(id), &handlers.model()),
        proto::Call::RobotVetbook => proto::Response::ok(Some(id), &handlers.vetbook()),
        proto::Call::RobotSkills => proto::Response::ok(Some(id), &handlers.skills()),
        proto::Call::RobotLoadPolicy(p) => {
            proto::Response::ok(Some(id), &handlers.load_policy(p.clone()))
        }
        proto::Call::RobotReloadPolicies => {
            proto::Response::ok(Some(id), &handlers.reload_policies())
        }
        // Deliberately still `METHOD_NOT_FOUND`:
        // - `robot.theremin` / `robot.chorale` / `chorale.*`: the phone has no ToF
        //   depth source and no chorale score player (docs/COMPARISON… §5.1), so
        //   answering "accepted" would promise an instrument or a song that cannot play.
        // - `robot.setSkill` / `robot.removeSkill`: the phone's skill set is the
        //   manifest's; there is no `robotd.toml` edit surface to persist a named skill
        //   into, so the official write path has no honest mobile twin.
        // - `robot.setSkill` callers get the same official refusal a `robotd` without
        //   the route gives, which is the designed skew behaviour.
        other => proto::Response::err(
            Some(id),
            proto::Error::new(
                proto::code::METHOD_NOT_FOUND,
                format!("{} is not served by robotd", other.method()),
            ),
        ),
    }
}

fn write_response(write: &WriteHalf, response: &proto::Response) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(response)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    line.push(b'\n');
    let mut out = write.lock().unwrap();
    out.write_all(&line)?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// A handler set the tests can observe, with no runtime behind it — the seam the
    /// wiring ticket will fill.
    struct TestHandlers {
        moves: Mutex<Vec<proto::MoveParams>>,
        heads: Mutex<Vec<proto::HeadParams>>,
        poses: Mutex<Vec<proto::PoseParams>>,
        mouths: Mutex<Vec<proto::MouthParams>>,
        sounds: Mutex<Vec<proto::SoundParams>>,
        skills: Mutex<Vec<String>>,
        refuse_do: AtomicBool,
    }

    impl TestHandlers {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                moves: Mutex::new(Vec::new()),
                heads: Mutex::new(Vec::new()),
                poses: Mutex::new(Vec::new()),
                mouths: Mutex::new(Vec::new()),
                sounds: Mutex::new(Vec::new()),
                skills: Mutex::new(Vec::new()),
                refuse_do: AtomicBool::new(false),
            })
        }
    }

    impl RobotHandlers for TestHandlers {
        fn health(&self) -> proto::HealthResult {
            proto::HealthResult {
                healthy: true,
                degraded: false,
                reason: None,
                ..Default::default()
            }
        }

        fn move_twist(&self, params: proto::MoveParams) -> proto::IntentResult {
            self.moves.lock().unwrap().push(params);
            proto::IntentResult::accepted()
        }

        fn head(&self, params: proto::HeadParams) -> proto::IntentResult {
            self.heads.lock().unwrap().push(params);
            proto::IntentResult::accepted()
        }

        fn pose(&self, params: proto::PoseParams) -> proto::IntentResult {
            self.poses.lock().unwrap().push(params);
            proto::IntentResult::accepted()
        }

        fn mouth(&self, params: proto::MouthParams) -> proto::IntentResult {
            self.mouths.lock().unwrap().push(params);
            proto::IntentResult::accepted()
        }

        fn sound(&self, params: proto::SoundParams) -> proto::IntentResult {
            self.sounds.lock().unwrap().push(params);
            proto::IntentResult::accepted()
        }

        fn do_skill(&self, params: proto::DoParams) -> proto::IntentResult {
            if self.refuse_do.load(Ordering::SeqCst) {
                return proto::IntentResult::refused(format!(
                    "no skill named {:?}; this robot has sit_toggle",
                    params.skill
                ));
            }
            self.skills.lock().unwrap().push(params.skill);
            proto::IntentResult::accepted()
        }

        fn subscribe_info(&self) -> proto::SubscribeResult {
            proto::SubscribeResult {
                accepted: true,
                walk: Some("alpha_walking.onnx".to_owned()),
                skills: vec!["ground_pick".to_owned(), "sit_toggle".to_owned()],
                ..Default::default()
            }
        }
    }

    fn start_test_server(handlers: Arc<TestHandlers>) -> (RobotIpcServer, u16) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        (start_listener(listener, handlers), port)
    }

    /// One tick's frame, assembled like the official loop's publish site: every field
    /// present, empty velocity/load blocks standing for "not reported" (`RobotState`
    /// does not derive `Default`, so there is no `..Default::default()` to lean on).
    fn frame(t: f64) -> proto::RobotState {
        proto::RobotState {
            t,
            movement: proto::MoveState {
                requested: [0.0; 3],
                applied: [0.0; 3],
                limited_by: Vec::new(),
            },
            head: [0.0; 4],
            policy: "held".to_owned(),
            safety: proto::SafetyState {
                fallen: false,
                limp: false,
                gravity: [0.0, 0.0, -1.0],
                gain: None,
                picked_up: false,
            },
            control_loop: proto::LoopState {
                hz: 50.0,
                missed: 0,
            },
            joints: vec![0.0; proto::JOINT_NAMES.len()],
            targets: vec![0.0; proto::JOINT_NAMES.len()],
            velocities: Vec::new(),
            currents_ma: Vec::new(),
            odom: proto::OdomState {
                position: [0.0; 3],
                yaw: 0.0,
            },
            theremin: None,
            chorale: None,
            t_ns: 0,
            imu: None,
            frames: None,
            skeleton: Vec::new(),
        }
    }

    /// Publish `ticks` state frames, `period` apart, numbered through `t`.
    fn feed(server: &RobotIpcServer, ticks: usize, period: Duration) {
        for tick in 0..ticks {
            server.publish_state(&frame(tick as f64));
            std::thread::sleep(period);
        }
    }

    /// A connected client with line helpers. Reads are byte-wise so a read timeout
    /// never swallows a partial line the test still wants.
    struct Client {
        stream: TcpStream,
    }

    impl Client {
        fn connect(port: u16) -> Self {
            let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
            Self { stream }
        }

        fn send(&mut self, line: &str) {
            self.stream.write_all(line.as_bytes()).expect("send");
            self.stream.flush().expect("flush");
        }

        fn request(&mut self, id: u64, method: &str, params: &str) {
            self.send(&format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"{method}\",\"params\":{params}}}\n"
            ));
        }

        /// One response line, or `None` when nothing arrived within `timeout`.
        fn read_line(&mut self, timeout: Duration) -> Option<String> {
            self.stream.set_read_timeout(Some(timeout)).ok()?;
            let mut line = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                match self.stream.read(&mut byte) {
                    Ok(0) => return None,
                    Ok(_) => {
                        if byte[0] == b'\n' {
                            return Some(String::from_utf8_lossy(&line).into_owned());
                        }
                        line.push(byte[0]);
                    }
                    Err(_) => return None,
                }
            }
        }

        /// Drain every line available within `per_line`, then wait out one `quiet`
        /// window. Returns what arrived.
        fn drain(&mut self, per_line: Duration, quiet: Duration) -> Vec<String> {
            let mut lines = Vec::new();
            while let Some(line) = self.read_line(per_line) {
                lines.push(line);
            }
            let _ = self.read_line(quiet);
            lines
        }
    }

    /// Parse one response line into the official envelope. A free function so a call
    /// and the `read_line` feeding it never fight over `&mut client`.
    fn parse(line: String) -> proto::Response {
        serde_json::from_str(&line).expect("valid JSON-RPC response")
    }

    /// The handshake is answered with the official shape and the official version
    /// constant; unknown methods and unparsable lines get the official codes; an
    /// empty line costs nothing.
    #[test]
    fn handshake_unknown_method_and_parse_error() {
        let handlers = TestHandlers::new();
        let (server, port) = start_test_server(handlers);
        let mut client = Client::connect(port);

        client.request(
            1,
            "hello",
            &format!("{{\"api_version\":{}}}", proto::API_VERSION),
        );
        let hello = parse(
            client
                .read_line(Duration::from_secs(2))
                .expect("hello reply"),
        );
        assert_eq!(hello.id, Some(proto::Id::Number(1)));
        let hello: proto::HelloResult = hello.result_as().expect("HelloResult");
        assert_eq!(hello.api_version, proto::API_VERSION);
        assert_eq!(proto::API_VERSION, 51);

        // An unknown method is refused by name, with the spec-reserved code.
        client.request(2, "robot.sit_on_ceiling", "{}");
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        let error = reply.error.expect("error");
        assert_eq!(error.code, proto::code::METHOD_NOT_FOUND);
        assert!(
            error.message.contains("robot.sit_on_ceiling"),
            "{}",
            error.message
        );

        // A method the protocol knows but this face does not serve gets the official
        // "not served" refusal, not a parse error.
        client.request(3, "update.status", "{}");
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        let error = reply.error.expect("error");
        assert_eq!(error.code, proto::code::METHOD_NOT_FOUND);
        assert_eq!(error.message, "update.status is not served by robotd");

        // Unparsable JSON: PARSE_ERROR, and the connection lives on.
        client.send("this is not json\n");
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        assert_eq!(reply.id, None);
        assert_eq!(reply.error.expect("error").code, proto::code::PARSE_ERROR);

        // An empty line is skipped silently, and the next request still answers.
        client.send("\n");
        client.request(4, "hello", "{\"api_version\":51}");
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        assert_eq!(reply.id, Some(proto::Id::Number(4)));

        // A line past the official ceiling is refused without being read into memory,
        // and the connection still works afterwards.
        let giant = format!("{{\"junk\":\"{}\"}}\n", "x".repeat(MAX_LINE + 1024));
        client.send(&giant);
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        assert_eq!(reply.id, None);
        let error = reply.error.expect("error");
        assert_eq!(error.code, proto::code::INVALID_REQUEST);
        assert_eq!(error.message, "request too large");
        client.request(5, "hello", "{\"api_version\":51}");
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        assert_eq!(reply.id, Some(proto::Id::Number(5)));

        server.stop();
    }

    /// `robot.move` / `robot.head` / `robot.do` reach the handlers on the request path
    /// with officially-shaped params, refuse bad params with the official code, and
    /// the notification path applies them with no reply — that is how 50 Hz clients
    /// actually send them.
    #[test]
    fn intents_reach_the_handlers_in_both_framings() {
        let handlers = TestHandlers::new();
        let (server, port) = start_test_server(handlers.clone());
        let mut client = Client::connect(port);

        // Request path: answered with the handler's verdict.
        client.request(1, "robot.move", "{\"vx\":0.2,\"vy\":-0.1,\"vyaw\":0.4}");
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        assert_eq!(reply.id, Some(proto::Id::Number(1)));
        let verdict: proto::IntentResult = reply.result_as().expect("IntentResult");
        assert!(verdict.accepted);
        assert_eq!(
            handlers.moves.lock().unwrap().last(),
            Some(&proto::MoveParams {
                vx: 0.2,
                vy: -0.1,
                vyaw: 0.4
            })
        );

        client.request(
            2,
            "robot.head",
            "{\"neck_pitch\":0.35,\"head_pitch\":-0.1,\"head_yaw\":0.2,\"head_roll\":0.0}",
        );
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        let verdict: proto::IntentResult = reply.result_as().expect("IntentResult");
        assert!(verdict.accepted);
        assert_eq!(
            handlers.heads.lock().unwrap().last(),
            Some(&proto::HeadParams {
                neck_pitch: 0.35,
                head_pitch: -0.1,
                head_yaw: 0.2,
                head_roll: 0.0,
            })
        );

        client.request(3, "robot.do", "{\"skill\":\"ground_pick\"}");
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        let verdict: proto::IntentResult = reply.result_as().expect("IntentResult");
        assert!(verdict.accepted);
        assert_eq!(
            handlers.skills.lock().unwrap().last().map(String::as_str),
            Some("ground_pick")
        );

        // Params of the wrong shape: INVALID_PARAMS, with the official passthrough of
        // serde's own complaint — which names the offending value, not the member.
        client.request(4, "robot.move", "{\"vx\":\"fast\"}");
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        let error = reply.error.expect("error");
        assert_eq!(error.code, proto::code::INVALID_PARAMS);
        assert!(
            error.message.contains("fast") && error.message.contains("f64"),
            "{}",
            error.message
        );

        // A member the params type does not know: refused, naming the member — the
        // strictness `deny_unknown_fields` puts on every official params type.
        client.request(5, "robot.move", "{\"vx\":0.1,\"zzz\":1}");
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        let error = reply.error.expect("error");
        assert_eq!(error.code, proto::code::INVALID_PARAMS);
        assert!(error.message.contains("zzz"), "{}", error.message);

        // A missing required member, same code.
        client.request(6, "robot.do", "{}");
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        let error = reply.error.expect("error");
        assert_eq!(error.code, proto::code::INVALID_PARAMS);
        assert!(error.message.contains("skill"), "{}", error.message);

        // Notification path: applied, and nothing comes back — after the next
        // request's reply there is silence.
        client.send("{\"jsonrpc\":\"2.0\",\"method\":\"robot.move\",\"params\":{\"vx\":0.5,\"vy\":0.0,\"vyaw\":0.0}}\n");
        client.send("{\"jsonrpc\":\"2.0\",\"method\":\"robot.head\",\"params\":{\"neck_pitch\":0.1,\"head_pitch\":0.0,\"head_yaw\":0.0,\"head_roll\":0.0}}\n");
        client.send("{\"jsonrpc\":\"2.0\",\"method\":\"robot.pose\",\"params\":{\"z\":0.01,\"roll\":0.1,\"pitch\":0.2,\"active\":true}}\n");
        client.send("{\"jsonrpc\":\"2.0\",\"method\":\"robot.mouth\",\"params\":{\"open\":0.6}}\n");
        client.send(
            "{\"jsonrpc\":\"2.0\",\"method\":\"robot.sound\",\"params\":{\"tag\":\"coo\"}}\n",
        );
        client.request(7, "hello", "{\"api_version\":51}");
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        assert_eq!(reply.id, Some(proto::Id::Number(7)));
        assert!(
            client.read_line(Duration::from_millis(300)).is_none(),
            "a notification got a reply"
        );
        assert_eq!(handlers.moves.lock().unwrap().last().unwrap().vx, 0.5);
        assert_eq!(
            handlers.heads.lock().unwrap().last().unwrap().neck_pitch,
            0.1
        );
        assert_eq!(
            handlers.poses.lock().unwrap().last().unwrap(),
            &proto::PoseParams {
                z: 0.01,
                roll: 0.1,
                pitch: 0.2,
                active: true
            }
        );
        assert_eq!(handlers.mouths.lock().unwrap().last().unwrap().open, 0.6);
        assert_eq!(
            handlers.sounds.lock().unwrap().last().unwrap().tag,
            proto::SoundTag::Coo
        );

        // The handler's refusal travels as a normal answer with a reason, not an error.
        handlers.refuse_do.store(true, Ordering::SeqCst);
        client.request(8, "robot.do", "{\"skill\":\"backflip\"}");
        let reply = parse(client.read_line(Duration::from_secs(2)).expect("reply"));
        let verdict: proto::IntentResult = reply.result_as().expect("IntentResult");
        assert!(!verdict.accepted);
        assert!(verdict.reason.unwrap().contains("backflip"));

        server.stop();
    }

    /// `robot.subscribe` acknowledges with the policy identity and then decimates the
    /// feed per subscriber, server-side: an `hz: 10` subscriber gets roughly a tenth
    /// of what an every-tick subscriber gets from the same feed, and every delivered
    /// line is an official `robot.state` notification.
    #[test]
    fn subscribe_decimates_per_subscriber() {
        let handlers = TestHandlers::new();
        let (server, port) = start_test_server(handlers);
        let mut fast = Client::connect(port);
        let mut slow = Client::connect(port);

        // The every-tick subscriber, then the 10 Hz one. The acknowledgement carries
        // the handler's policy identity.
        fast.request(1, "robot.subscribe", "{}");
        let ack = parse(fast.read_line(Duration::from_secs(2)).expect("ack"));
        let ack: proto::SubscribeResult = ack.result_as().expect("SubscribeResult");
        assert!(ack.accepted);
        assert_eq!(ack.walk.as_deref(), Some("alpha_walking.onnx"));
        assert!(ack.skills.contains(&"ground_pick".to_owned()));

        slow.request(1, "robot.subscribe", "{\"hz\":10}");
        let ack = parse(slow.read_line(Duration::from_secs(2)).expect("ack"));
        let ack: proto::SubscribeResult = ack.result_as().expect("SubscribeResult");
        assert!(ack.accepted);

        // Sixty ticks at 10 ms: one second of a 100 Hz feed, so the 10 Hz subscriber
        // is due roughly every tenth frame.
        feed(&server, 60, Duration::from_millis(10));

        let fast_lines = fast.drain(Duration::from_millis(200), Duration::from_millis(300));
        let slow_lines = slow.drain(Duration::from_millis(200), Duration::from_millis(300));

        assert!(
            (55..=60).contains(&fast_lines.len()),
            "every-tick subscriber got {} of 60 frames",
            fast_lines.len()
        );
        assert!(
            (3..=12).contains(&slow_lines.len()),
            "10 Hz subscriber got {} frames from a 60-tick feed",
            slow_lines.len()
        );
        assert!(slow_lines.len() < fast_lines.len());

        // Each line is the official notification, and the frames arrive in feed order.
        for line in slow_lines.iter().take(2) {
            let request: proto::Request = serde_json::from_str(line).expect("notification");
            assert!(request.is_notification());
            assert_eq!(request.method, "robot.state");
            let state = request.as_state().expect("RobotState");
            assert!(state.t >= 0.0);
        }

        server.stop();
    }

    /// A connection that never subscribes is streamed nothing, whatever the feed does.
    #[test]
    fn a_connection_that_did_not_subscribe_gets_nothing() {
        let handlers = TestHandlers::new();
        let (server, port) = start_test_server(handlers);
        let mut bystander = Client::connect(port);
        let mut subscriber = Client::connect(port);

        subscriber.request(1, "robot.subscribe", "{\"hz\":50}");
        let ack = parse(subscriber.read_line(Duration::from_secs(2)).expect("ack"));
        let ack: proto::SubscribeResult = ack.result_as().expect("SubscribeResult");
        assert!(ack.accepted);

        feed(&server, 20, Duration::from_millis(5));

        assert_eq!(server.subscriber_count(), 1);
        assert!(
            bystander.read_line(Duration::from_millis(300)).is_none(),
            "an unsubscribed connection received state frames"
        );

        server.stop();
    }

    /// A dropped connection is gone from the registry: its frames stop and nothing is
    /// written into the void.
    #[test]
    fn disconnect_cleans_up_the_subscription() {
        let handlers = TestHandlers::new();
        let (server, port) = start_test_server(handlers);
        let mut client = Client::connect(port);
        client.request(1, "robot.subscribe", "{}");
        let ack = parse(client.read_line(Duration::from_secs(2)).expect("ack"));
        let ack: proto::SubscribeResult = ack.result_as().expect("SubscribeResult");
        assert!(ack.accepted);
        assert_eq!(server.subscriber_count(), 1);

        // Feed a little while the connection lives, then drop it.
        feed(&server, 5, Duration::from_millis(5));
        drop(client);

        let deadline = Instant::now() + Duration::from_secs(2);
        while server.subscriber_count() != 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            server.subscriber_count(),
            0,
            "the dropped subscriber was not cleaned up"
        );

        // And the frames stop: nobody is subscribed, publish is a no-op.
        feed(&server, 5, Duration::from_millis(5));
        assert_eq!(server.subscriber_count(), 0);

        server.stop();
    }

    /// Publishing with nobody subscribed costs nothing and cannot fail.
    #[test]
    fn publish_with_no_subscribers_is_a_no_op() {
        let handlers = TestHandlers::new();
        let (server, _port) = start_test_server(handlers);
        assert_eq!(server.subscriber_count(), 0);
        feed(&server, 10, Duration::from_millis(1));
        assert_eq!(server.subscriber_count(), 0);
        server.stop();
    }
}
