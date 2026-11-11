//! The app's intent API: a small HTTP server on the local network.
//!
//! The duck's control loop is the only thing that touches the runtime; this
//! server only enqueues intents and publishes a snapshot. A user's LLM, script
//! or agent can drive the duck with plain JSON:
//!
//! ```text
//! POST /twist       {"vx":0.4,"vy":0,"wz":0}
//! POST /skill       {"name":"roulade"}
//! POST /sit         {}
//! POST /ground_pick {}
//! POST /push        {"vx":0.6,"vy":0,"vz":0}
//! POST /reload      {"dir":"/path/to/policies"}
//! GET  /state
//! GET  /skills
//! ```
//!
//! No authentication: it binds to the local network for a phone or a desktop
//! agent. Treat it as a development API, not an internet-facing service.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// How long a one-shot route waits for the tick to decide. The loop runs at
/// 50 Hz, so a live duck answers in 20 ms; the margin is for a slow frame, and
/// the timeout is what a backgrounded app looks like.
const ACTION_TIMEOUT: Duration = Duration::from_millis(300);

/// A mode switch loads a second MJCF and a whole policy set, then enables the
/// body: the same work as core creation, and worth waiting for.
const MODE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub enum Intent {
    Twist([f64; 3]),
    Head([f64; 4]),
    Body(f64, f64, f64),
    BodyRelease,
    Skill(String),
    Sit,
    GroundPick,
    Push([f64; 3]),
    /// Beak opening, 0..1. The mouth is not part of any policy: on the robot
    /// it is a client's intent (or the idle yawn), written by the loop.
    Mouth(f64),
    Reload(String),
    /// A hand on the duck: hold the trunk at `offset` from where it was picked
    /// up (`active: false` lets go).
    Carry([f64; 3], bool),
    /// Time machine: freeze the duck.
    Pause(bool),
    /// Time machine: advance exactly this many 20 ms ticks while paused.
    Step(u32),
    /// Time machine: 0.05x .. 4x.
    Speed(f64),
    /// Time machine: save the whole duck at this tick.
    Checkpoint,
    /// Time machine: resume from a saved tick.
    Restore(u64),
    /// Time machine: step back `ticks` and resume from there.
    Seek(u64),
    /// Fork the duck at this instant; `label` names the branch.
    Fork(String),
    /// Drop a branch.
    DropBranch(String),
    /// Drive one branch, or the original when empty.
    SelectBranch(String),
    /// Play one of the duck's sounds: greet, chirp, coo, peck, inquire, alarm.
    Sound(String),
    /// Switch drive bundles: "walk" or "roller". A whole-policy-set switch,
    /// exactly as `robotd`'s mode line, not a per-command filter.
    Mode(String),
    /// `robot.shutdown`: sit, ease into the rest pose, cut torque.
    Shutdown,
    /// Power back on after a shutdown.
    PowerOn,
    /// A phone sensor reading (camera, microphone, depth, IMU).
    Sensor(crate::sensors::Reading),
    /// `robot.flashlight`: the virtual face LED, one of robotd's seven colours.
    Flashlight { toggle: bool, on: bool, color: String },
    /// The held wheee ride: `hold` starts it, releasing ends it.
    Wheee(bool),
    /// A stroke of the hand on the duck.
    Pet,
    Feed,
    /// The lifetime record a shell saved across launches, handed back so the
    /// duck's history survives an app restart.
    RestoreLife(serde_json::Value),
    /// The care state a shell saved across launches (hunger, mood, bond).
    RestoreCare(serde_json::Value),
    /// How fast care ages against real time; 1.0 is normal.
    CareScale(f64),
    /// A child's block program: triggers and actions in the duck's own verbs.
    SetRules(serde_json::Value),
    GetRules,
    /// Sit down (`true`) or stand up (`false`), idempotently. `/sit` is the
    /// button's toggle; a model must not invert its own request.
    Posture(bool),
    /// Say hello to another duck on the LAN (None = every duck).
    GreetPeer(Option<String>),
    /// The name this duck announces.
    PeerName(String),
    /// Open the LAN socket and announce as `name`. On-demand because iOS
    /// asks for local-network permission the first time one is opened.
    StartLan(String),
    /// How long the duck waits before doing something by itself, in seconds.
    IdleRitual(f64),
    /// `robot.look`: aim the head at a trunk-frame point with a neck pitch.
    Look { target: [f64; 3], neck_pitch: f64 },
    /// Recognized speech, as text. The core maps it to one of its own verbs,
    /// so a phone's recognizer, a typed word and an AI client all take the
    /// same path.
    Heard(String),
}

/// An intent with the ticket its client is waiting on.
#[derive(Debug, Clone)]
pub struct QueuedIntent {
    pub id: u64,
    pub intent: Intent,
}

struct Commands {
    items: VecDeque<QueuedIntent>,
    /// The core's answers, by ticket. Bounded: a route that gave up waiting
    /// must not grow this map without end.
    answers: HashMap<u64, Value>,
    /// Tickets somebody is actually waiting on. A fire-and-forget stream
    /// (`/twist` at 50 Hz) must not fill the answer map with results nobody
    /// reads — and, worse, evict the result a long mode-switch is waiting for.
    awaited: HashSet<u64>,
    next_id: u64,
}

pub struct IntentQueue {
    commands: Mutex<Commands>,
    answered: Condvar,
    state: Mutex<Value>,
    skills: Mutex<Vec<String>>,
    fingerprint: Mutex<String>,
    /// The LAN peers as the loop last published them.
    peers: Mutex<Value>,
    /// The time machine's answers, filled by the core on the next tick.
    time: Mutex<Value>,
    /// The forks: one entry per branch, filled by the core.
    branches: Mutex<Value>,
    /// The latest phone-sensor readings.
    sensors: Mutex<Value>,
    /// The duck's lifetime record, shared with the loop that counts it.
    life: Mutex<crate::vetbook::Book>,
}

impl Default for IntentQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl IntentQueue {
    pub fn new() -> Self {
        Self {
            commands: Mutex::new(Commands {
                items: VecDeque::new(),
                answers: HashMap::new(),
                awaited: HashSet::new(),
                next_id: 0,
            }),
            answered: Condvar::new(),
            state: Mutex::new(json!({
                "label": "idle",
                "bringup": 0,
                "fallen": false,
                "gain": 200,
                "trunk": [0.0, 0.0, 0.0],
                "gravity": [0.0, 0.0, -1.0],
            })),
            skills: Mutex::new(Vec::new()),
            fingerprint: Mutex::new("{}".to_owned()),
            peers: Mutex::new(json!({ "peers": [] })),
            branches: Mutex::new(json!({ "branches": [], "selected": "" })),
            sensors: Mutex::new(json!({ "mode": "twin" })),
            life: Mutex::new(crate::vetbook::Book::default()),
            time: Mutex::new(json!({
                "tick": 0,
                "paused": false,
                "speed": 1.0,
                "checkpoints": [],
            })),
        }
    }

    /// Enqueue an intent; the id is the ticket the core answers. Nobody waits
    /// on a plain push — streams use this — so its answer is dropped.
    pub fn push(&self, intent: Intent) -> u64 {
        self.enqueue(intent, false)
    }

    fn enqueue(&self, intent: Intent, awaited: bool) -> u64 {
        let mut commands = self.commands.lock().expect("intent queue");
        commands.next_id += 1;
        let id = commands.next_id;
        if awaited {
            commands.awaited.insert(id);
        }
        commands.items.push_back(QueuedIntent { id, intent });
        id
    }

    pub fn drain(&self) -> Vec<QueuedIntent> {
        self.commands
            .lock()
            .expect("intent queue")
            .items
            .drain(..)
            .collect()
    }

    /// The core's answer to one queued intent.
    pub fn answer(&self, id: u64, result: Value) {
        let mut commands = self.commands.lock().expect("intent answers");
        // Only keep an answer somebody asked for. Otherwise 50 Hz streams
        // would grow the map (and its eviction would discard a real waiter's
        // result).
        if commands.awaited.remove(&id) {
            commands.answers.insert(id, result);
        }
        drop(commands);
        self.answered.notify_all();
    }

    /// Wait for the tick that applies `id`. `None` means no tick ran in time —
    /// the duck may be backgrounded, not broken.
    pub fn await_result(&self, id: u64, timeout: Duration) -> Option<Value> {
        let deadline = Instant::now() + timeout;
        let mut commands = self.commands.lock().expect("intent answers");
        loop {
            if let Some(result) = commands.answers.remove(&id) {
                return Some(result);
            }
            let now = Instant::now();
            if now >= deadline {
                // Give up cleanly: the core may still answer later, and the
                // answer would have nowhere to go.
                commands.awaited.remove(&id);
                return None;
            }
            let (guard, _) = self
                .answered
                .wait_timeout(commands, deadline - now)
                .expect("intent answers");
            commands = guard;
        }
    }

    /// Enqueue, then answer with what the duck actually did.
    ///
    /// One-shot actions are answered after the next tick has decided, so a
    /// refusal ("a scripted move is already running") reaches the client
    /// instead of an `ok` that meant nothing more than "the queue took it".
    /// Control streams (`/twist`, `/head`, `/body`) stay fire-and-forget: a
    /// 50 Hz stream cannot wait half a tick for every sample, and a twist has
    /// no decision to report.
    pub fn request(&self, intent: Intent) -> Value {
        self.request_within(intent, ACTION_TIMEOUT)
    }

    /// [`Self::request`] with an explicit budget. A mode switch loads a second
    /// MJCF and a policy set on the tick thread, which is seconds, not ticks.
    pub fn request_within(&self, intent: Intent, timeout: Duration) -> Value {
        let id = self.enqueue(intent, true);
        self.await_result(id, timeout).unwrap_or_else(|| {
            json!({
                "ok": false,
                "pending": true,
                "error": "the duck did not answer within 300 ms; is the app running?",
            })
        })
    }

    pub fn set_state(&self, state: Value) {
        *self.state.lock().expect("intent state") = state;
    }

    pub fn state_json(&self) -> String {
        self.state.lock().expect("intent state").to_string()
    }

    pub fn set_skills(&self, skills: Vec<String>) {
        *self.skills.lock().expect("intent skills") = skills;
    }

    pub fn skills_json(&self) -> String {
        json!({ "skills": *self.skills.lock().expect("intent skills") }).to_string()
    }

    pub fn set_fingerprint(&self, document: String) {
        *self.fingerprint.lock().expect("intent fingerprint") = document;
    }

    pub fn fingerprint_json(&self) -> String {
        self.fingerprint.lock().expect("intent fingerprint").clone()
    }

    pub fn set_time(&self, value: Value) {
        *self.time.lock().expect("intent time") = value;
    }

    pub fn time_json(&self) -> String {
        self.time.lock().expect("intent time").to_string()
    }

    pub fn set_branches(&self, value: Value) {
        *self.branches.lock().expect("intent branches") = value;
    }

    pub fn branches_json(&self) -> String {
        self.branches.lock().expect("intent branches").to_string()
    }

    pub fn set_sensors(&self, value: Value) {
        *self.sensors.lock().expect("intent sensors") = value;
    }

    /// The lifetime record as JSON. The queue owns the book so the shell can
    /// read it without going through the core's FFI.
    pub fn life_json(&self) -> String {
        self.life.lock().expect("intent life").summary().to_string()
    }

    /// Hand the loop's book over, so a client sees the same numbers the
    /// duck has been counting.
    pub fn set_life(&self, book: &crate::vetbook::Book) {
        if let Ok(mut slot) = self.life.lock() {
            *slot = book.clone();
        }
    }

    pub fn sensors_json(&self) -> String {
        self.sensors.lock().expect("intent sensors").to_string()
    }

    pub fn set_peers(&self, value: Value) {
        *self.peers.lock().expect("intent peers") = value;
    }

    pub fn peers_json(&self) -> String {
        self.peers.lock().expect("intent peers").to_string()
    }

    pub fn peers_value(&self) -> Value {
        self.peers.lock().expect("intent peers").clone()
    }

    /// The care slice of the published state: one GET for a shell that wants
    /// to save the duck's tummy and mood without parsing the whole snapshot.
    pub fn care_json(&self) -> String {
        let state = self.state.lock().expect("intent state");
        json!({
            "care": state.get("care").cloned().unwrap_or(Value::Null),
            "care_time_scale": state.get("care_time_scale").cloned().unwrap_or(json!(1.0)),
        })
        .to_string()
    }

    /// What the duck's ear last heard, for GET /heard.
    pub fn heard_json(&self) -> String {
        let state = self.state.lock().expect("intent state");
        json!({
            "last_heard": state.get("last_heard").cloned().unwrap_or(Value::Null),
        })
        .to_string()
    }

    /// The duck's care numbers as the core's own value, for an export.
    fn care_snapshot(&self) -> crate::care::Care {
        let state = self.state.lock().expect("intent state");
        let mut care = crate::care::Care::default();
        if let Some(value) = state.get("care") {
            care.restore(value);
        }
        care
    }

    fn build_fingerprint(&self) -> String {
        let document = self.fingerprint.lock().expect("intent fingerprint");
        serde_json::from_str::<Value>(&document)
            .ok()
            .and_then(|value| value.get("fingerprint").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_default()
    }

    /// A shareable document, built from the same counters /life serves.
    pub fn vetbook_markdown(&self) -> String {
        let book = self.life.lock().expect("intent life").clone();
        crate::export::markdown(&book.life, &self.care_snapshot(), &self.build_fingerprint())
    }

    pub fn vetbook_json(&self) -> String {
        let book = self.life.lock().expect("intent life").clone();
        crate::export::json(&book.life, &self.care_snapshot(), &self.build_fingerprint()).to_string()
    }
}

/// Start the server on `0.0.0.0:port`.
pub fn start(port: u16, queue: Arc<IntentQueue>) -> std::io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    start_listener(listener, queue);
    Ok(())
}

/// Start the server on an already-bound listener (port 0 in tests).
pub fn start_listener(listener: TcpListener, queue: Arc<IntentQueue>) {
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let queue = queue.clone();
            std::thread::spawn(move || {
                let _ = handle(stream, queue);
            });
        }
    });
}

fn handle(mut stream: TcpStream, queue: Arc<IntentQueue>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end;
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = find_headers_end(&buffer) {
            header_end = position;
            break;
        }
        if buffer.len() > 64 * 1024 {
            return respond(&mut stream, 413, "Payload Too Large", "{}");
        }
    }

    let headers = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = headers.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default().split('?').next().unwrap_or_default();

    let content_length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0)
        .min(1024 * 1024);

    while buffer.len() < header_end + 4 + content_length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let body_start = header_end + 4;
    let body = String::from_utf8_lossy(
        &buffer[body_start..buffer.len().min(body_start + content_length)],
    )
    .into_owned();

    if method == "OPTIONS" {
        return respond(&mut stream, 204, "No Content", "");
    }

    let value: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let result = match (method, path) {
        ("GET", "/state") => Ok(queue.state_json()),
        ("GET", "/skills") => Ok(queue.skills_json()),
        ("GET", "/fingerprint") => Ok(queue.fingerprint_json()),
        ("GET", "/time") => Ok(queue.time_json()),
        ("GET", "/timeline") => Ok(queue.time_json()),
        ("GET", "/branches") => Ok(queue.branches_json()),
        ("GET", "/sensors") => Ok(queue.sensors_json()),
        // The lifetime record, the same counters the shell keeps in storage.
        ("GET", "/life") => Ok(queue.life_json()),
        // The same record as a file a person can share, in the two shapes a
        // share sheet wants. No new counting path: both call vetbook/export.
        ("GET", "/export/vetbook.md") => Ok(queue.vetbook_markdown()),
        ("GET", "/export/vetbook.json") => Ok(queue.vetbook_json()),
        ("POST", "/life") => Ok(queue
            .request(Intent::RestoreLife(value.clone()))
            .to_string()),
        ("POST", "/mcp") => Ok(crate::mcp::handle(&body, &queue)),
        ("GET", "/mcp") => Ok(crate::mcp::handle(
            r#"{"jsonrpc":"2.0","id":0,"method":"tools/list"}"#,
            &queue,
        )),
        ("POST", "/sensor") => match value["sensor"].as_str() {
            Some(sensor) => {
                let reading = crate::sensors::Reading {
                    sensor: sensor.to_owned(),
                    mode: value["mode"].as_str().unwrap_or("twin").to_owned(),
                    value: value["value"].clone(),
                    tick_ms: value["tick_ms"].as_u64().unwrap_or(0),
                    in_observation: false,
                };
                queue.push(Intent::Sensor(reading));
                Ok(json!({ "ok": true }).to_string())
            }
            None => Err("missing sensor"),
        },
        ("POST", "/fork") => {
            let label = value["label"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| "branch".to_owned());
            // A fork deep-copies a MuJoCo model and reloads the policy set.
            Ok(queue
                .request_within(Intent::Fork(label), Duration::from_secs(15))
                .to_string())
        }
        ("POST", "/branch/drop") => match value["label"].as_str() {
            Some(label) => Ok(queue
                .request(Intent::DropBranch(label.to_owned()))
                .to_string()),
            None => Err("missing label"),
        },
        ("POST", "/branch/select") => {
            let label = value["label"].as_str().unwrap_or("").to_owned();
            Ok(queue.request(Intent::SelectBranch(label)).to_string())
        }
        ("POST", "/time") => {
            let paused = value["paused"].as_bool();
            let speed = value["speed"].as_f64();
            match (paused, speed) {
                (Some(paused), _) => {
                    Ok(queue.request(Intent::Pause(paused)).to_string())
                }
                (None, Some(speed)) => {
                    Ok(queue.request(Intent::Speed(speed)).to_string())
                }
                _ => Err("send {paused:true|false} or {speed:0.05..4}"),
            }
        }
        ("POST", "/step") => {
            let ticks = value["ticks"].as_u64().unwrap_or(1).clamp(1, 500) as u32;
            // Each tick is 20 ms of work plus whatever the policy costs;
            // 500 ticks is far past the one-shot budget.
            Ok(queue
                .request_within(
                    Intent::Step(ticks),
                    Duration::from_millis(500 + u64::from(ticks) * 20),
                )
                .to_string())
        }
        ("POST", "/checkpoint") => Ok(queue.request(Intent::Checkpoint).to_string()),
        ("POST", "/restore") => match value["tick"].as_u64() {
            Some(tick) => Ok(queue.request(Intent::Restore(tick)).to_string()),
            None => Err("missing tick"),
        },
        ("POST", "/seek") => match value["ticks"].as_u64() {
            Some(ticks) => Ok(queue.request(Intent::Seek(ticks)).to_string()),
            None => Err("missing ticks"),
        },
        ("POST", "/twist") => {
            queue.push(Intent::Twist([
                value["vx"].as_f64().unwrap_or(0.0),
                value["vy"].as_f64().unwrap_or(0.0),
                value["wz"].as_f64().unwrap_or(0.0),
            ]));
            Ok(json!({ "ok": true }).to_string())
        }
        ("POST", "/head") => {
            queue.push(Intent::Head([
                value["neck"].as_f64().unwrap_or(0.0),
                value["head"].as_f64().unwrap_or(0.0),
                value["yaw"].as_f64().unwrap_or(0.0),
                value["roll"].as_f64().unwrap_or(0.0),
            ]));
            Ok(json!({ "ok": true }).to_string())
        }
        ("POST", "/body") => {
            // `active: false` releases the pose (robotd's B-button exit).
            if value["active"].as_bool() == Some(false) {
                queue.push(Intent::BodyRelease);
            } else {
                queue.push(Intent::Body(
                    value["z"].as_f64().unwrap_or(0.0),
                    value["roll"].as_f64().unwrap_or(0.0),
                    value["pitch"].as_f64().unwrap_or(0.0),
                ));
            }
            Ok(json!({ "ok": true }).to_string())
        }
        ("POST", "/skill") => match value["name"].as_str() {
            Some(name) => Ok(queue.request(Intent::Skill(name.to_owned())).to_string()),
            None => Err("missing name"),
        },
        ("POST", "/sit") => Ok(queue.request(Intent::Sit).to_string()),
        ("POST", "/posture") => Ok(queue
            .request(Intent::Posture(value["sit"].as_bool().unwrap_or(true)))
            .to_string()),
        ("POST", "/ground_pick") => Ok(queue.request(Intent::GroundPick).to_string()),
        ("POST", "/shutdown") => Ok(queue.request_within(
            Intent::Shutdown,
            Duration::from_secs(15),
        ).to_string()),
        ("POST", "/power") => Ok(queue.request(Intent::PowerOn).to_string()),
        ("POST", "/mode") => match value["mode"].as_str() {
            // The switch rebuilds MuJoCo and the policy set: seconds, not the
            // 300 ms a one-shot gets.
            Some(mode) => Ok(queue
                .request_within(Intent::Mode(mode.to_owned()), MODE_TIMEOUT)
                .to_string()),
            None => Err("missing mode"),
        },
        // The voice, by tag: `robotctl quack` is a chirp, and the others are
        // the duck's own vocabulary.
        ("POST", "/sound") => match value["tag"].as_str() {
            Some(tag) => Ok(queue.request(Intent::Sound(tag.to_owned())).to_string()),
            None => Err("missing tag"),
        },
        ("POST", "/pet") => Ok(queue.request(Intent::Pet).to_string()),
        ("POST", "/feed") => Ok(queue.request(Intent::Feed).to_string()),
        // The companion state is public read / restore-only write: a client
        // can hand back what it saved, but cannot claim the duck loves it.
        ("GET", "/care") => Ok(queue.care_json()),
        ("GET", "/heard") => Ok(queue.heard_json()),
        ("GET", "/rules") => Ok(queue.request(Intent::GetRules).to_string()),
        ("POST", "/rules") => Ok(queue
            .request(Intent::SetRules(value.clone()))
            .to_string()),
        ("GET", "/peers") => Ok(queue.peers_json()),
        ("POST", "/peers/start") => Ok(queue
            .request(Intent::StartLan(
                value["name"].as_str().unwrap_or("MicroDuck").to_owned(),
            ))
            .to_string()),
        ("POST", "/peers/greet") => Ok(queue
            .request(Intent::GreetPeer(
                value["id"].as_str().filter(|id| !id.is_empty()).map(str::to_owned),
            ))
            .to_string()),
        ("POST", "/peers/name") => match value["name"].as_str() {
            Some(name) => Ok(queue
                .request(Intent::PeerName(name.to_owned()))
                .to_string()),
            None => Err("missing name"),
        },        ("POST", "/care") => {
            let saved = value.get("care").cloned().unwrap_or(value);
            Ok(queue.request(Intent::RestoreCare(saved)).to_string())
        }
        ("POST", "/config") => {
            let idle = value.get("idle_ritual_s").and_then(Value::as_f64);
            let care = value.get("care_time_scale").and_then(Value::as_f64);
            // Keep the old shape working: /config with only idle_ritual_s.
            // A care scale travels as its own fire-and-forget intent so one
            // response still means "the configuration was taken".
            if let Some(scale) = care {
                queue.push(Intent::CareScale(scale));
            }
            Ok(queue
                .request(Intent::IdleRitual(idle.unwrap_or(30.0)))
                .to_string())
        }
        ("POST", "/wheee") => Ok(queue
            .request(Intent::Wheee(value["hold"].as_bool().unwrap_or(false)))
            .to_string()),
        ("POST", "/look") => Ok(queue.request(Intent::Look {
            target: [
                value["x"].as_f64().unwrap_or(1.0),
                value["y"].as_f64().unwrap_or(0.0),
                value["z"].as_f64().unwrap_or(0.15),
            ],
            neck_pitch: value["neck"].as_f64().unwrap_or(0.0),
        }).to_string()),
        ("POST", "/heard") => match value["text"].as_str() {
            Some(text) => Ok(queue.request(Intent::Heard(text.to_owned())).to_string()),
            None => Err("missing text"),
        },
        ("POST", "/flashlight") => Ok(queue
            .request(Intent::Flashlight {
                toggle: value["toggle"].as_bool().unwrap_or(true),
                on: value["on"].as_bool().unwrap_or(false),
                color: value["color"].as_str().unwrap_or("white").to_owned(),
            })
            .to_string()),
        ("POST", "/mouth") => {
            let open = value["open"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0);
            Ok(queue.request(Intent::Mouth(open)).to_string())
        }
        ("POST", "/push") => {
            queue.push(Intent::Push([
                value["vx"].as_f64().unwrap_or(0.0),
                value["vy"].as_f64().unwrap_or(0.0),
                value["vz"].as_f64().unwrap_or(0.0),
            ]));
            Ok(json!({ "ok": true }).to_string())
        }
        ("POST", "/reload") => match value["dir"].as_str() {
            // Loading a policy set is seconds of ONNX work.
            Some(dir) => Ok(queue
                .request_within(Intent::Reload(dir.to_owned()), Duration::from_secs(30))
                .to_string()),
            None => Err("missing dir"),
        },
        // A hand on the duck, for the shell's grab gesture and for an agent
        // that wants to pick the robot up. `dx/dy/dz` are metres from where it
        // was grabbed; `active:false` sets it down.
        ("POST", "/carry") => {
            let offset = [
                value["dx"].as_f64().unwrap_or(0.0),
                value["dy"].as_f64().unwrap_or(0.0),
                value["dz"].as_f64().unwrap_or(0.0),
            ];
            let active = value["active"].as_bool().unwrap_or(true);
            Ok(queue.request(Intent::Carry(offset, active)).to_string())
        }
        _ => return respond(&mut stream, 404, "Not Found", "{\"ok\":false,\"error\":\"not found\"}"),
    };

    match result {
        Ok(body) => respond(&mut stream, 200, "OK", &body),
        Err(error) => respond(
            &mut stream,
            400,
            "Bad Request",
            &json!({ "ok": false, "error": error }).to_string(),
        ),
    }
}

fn find_headers_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn respond(stream: &mut TcpStream, status: u16, reason: &str, body: &str) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
         Connection: close\r\n\
         \r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(port: u16, method: &str, path: &str, body: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read");
        response
    }

    #[test]
    fn http_enqueues_intents_and_serves_state() {
        let queue = Arc::new(IntentQueue::new());
        queue.set_state(json!({ "label": "walk", "gain": 200 }));
        queue.set_skills(vec!["roulade".to_owned()]);
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        start_listener(listener, queue.clone());

        let response = request(port, "POST", "/twist", r#"{"vx":0.4,"vy":0.0,"wz":0.2}"#);
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains(r#""ok":true"#), "{response}");

        let intents = queue.drain();
        assert_eq!(intents.len(), 1);
        match &intents[0].intent {
            Intent::Twist(twist) => assert_eq!(*twist, [0.4, 0.0, 0.2]),
            other => panic!("unexpected intent: {other:?}"),
        }

        let response = request(port, "GET", "/state", "");
        assert!(response.contains(r#""label":"walk""#), "{response}");
        let response = request(port, "GET", "/skills", "");
        assert!(response.contains("roulade"), "{response}");
    }

    #[test]
    fn http_rejects_unknown_routes() {
        let queue = Arc::new(IntentQueue::new());
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        start_listener(listener, queue);
        let response = request(port, "GET", "/nope", "");
        assert!(response.starts_with("HTTP/1.1 404"), "{response}");
    }

    /// A one-shot route answers with the tick's verdict, not "the queue took
    /// it": a client that asks for a skill while another is running must learn
    /// that it was refused.
    #[test]
    fn a_one_shot_route_answers_with_what_the_tick_decided() {
        let queue = Arc::new(IntentQueue::new());
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        start_listener(listener, queue.clone());

        // The duck's side of the conversation: take the request, answer it.
        let worker = {
            let queue = queue.clone();
            std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(2);
                while Instant::now() < deadline {
                    let queued = queue.drain();
                    for item in queued {
                        let answer = match item.intent {
                            Intent::Skill(name) => {
                                json!({ "ok": false, "error": format!("no such skill: {name}") })
                            }
                            _ => json!({ "ok": true }),
                        };
                        queue.answer(item.id, answer);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
        };

        let response = request(port, "POST", "/skill", r#"{"name":"fly"}"#);
        assert!(response.contains(r#""ok":false"#), "{response}");
        assert!(response.contains("no such skill"), "{response}");
        let _ = worker.join();
    }

    /// A route whose tick never runs says so instead of pretending success.
    #[test]
    fn a_silent_core_is_reported_as_pending() {
        let queue = IntentQueue::new();
        let result = queue.request(Intent::Sit);
        assert_eq!(result["ok"], json!(false));
        assert_eq!(result["pending"], json!(true));
    }
}
