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

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

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
    Reload(String),
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
    /// A phone sensor reading (camera, microphone, depth, IMU).
    Sensor(crate::sensors::Reading),
}

pub struct IntentQueue {
    commands: Mutex<VecDeque<Intent>>,
    state: Mutex<Value>,
    skills: Mutex<Vec<String>>,
    fingerprint: Mutex<String>,
    /// The time machine's answers, filled by the core on the next tick.
    time: Mutex<Value>,
    /// The forks: one entry per branch, filled by the core.
    branches: Mutex<Value>,
    /// The latest phone-sensor readings.
    sensors: Mutex<Value>,
}

impl Default for IntentQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl IntentQueue {
    pub fn new() -> Self {
        Self {
            commands: Mutex::new(VecDeque::new()),
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
            branches: Mutex::new(json!({ "branches": [], "selected": "" })),
            sensors: Mutex::new(json!({ "mode": "twin" })),
            time: Mutex::new(json!({
                "tick": 0,
                "paused": false,
                "speed": 1.0,
                "checkpoints": [],
            })),
        }
    }

    pub fn push(&self, intent: Intent) {
        self.commands.lock().expect("intent queue").push_back(intent);
    }

    pub fn drain(&self) -> Vec<Intent> {
        self.commands
            .lock()
            .expect("intent queue")
            .drain(..)
            .collect()
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

    pub fn sensors_json(&self) -> String {
        self.sensors.lock().expect("intent sensors").to_string()
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
            queue.push(Intent::Fork(label.clone()));
            Ok(json!({ "ok": true, "label": label }).to_string())
        }
        ("POST", "/branch/drop") => match value["label"].as_str() {
            Some(label) => {
                queue.push(Intent::DropBranch(label.to_owned()));
                Ok(json!({ "ok": true }).to_string())
            }
            None => Err("missing label"),
        },
        ("POST", "/branch/select") => {
            let label = value["label"].as_str().unwrap_or("").to_owned();
            queue.push(Intent::SelectBranch(label));
            Ok(json!({ "ok": true }).to_string())
        }
        ("POST", "/time") => {
            let paused = value["paused"].as_bool();
            let speed = value["speed"].as_f64();
            match (paused, speed) {
                (Some(paused), _) => {
                    queue.push(Intent::Pause(paused));
                    Ok(json!({ "ok": true, "paused": paused }).to_string())
                }
                (None, Some(speed)) => {
                    queue.push(Intent::Speed(speed));
                    Ok(json!({ "ok": true, "speed": speed }).to_string())
                }
                _ => Err("send {paused:true|false} or {speed:0.05..4}"),
            }
        }
        ("POST", "/step") => {
            let ticks = value["ticks"].as_u64().unwrap_or(1).clamp(1, 500) as u32;
            queue.push(Intent::Step(ticks));
            Ok(json!({ "ok": true, "ticks": ticks }).to_string())
        }
        ("POST", "/checkpoint") => {
            queue.push(Intent::Checkpoint);
            Ok(json!({ "ok": true }).to_string())
        }
        ("POST", "/restore") => match value["tick"].as_u64() {
            Some(tick) => {
                queue.push(Intent::Restore(tick));
                Ok(json!({ "ok": true, "tick": tick }).to_string())
            }
            None => Err("missing tick"),
        },
        ("POST", "/seek") => match value["ticks"].as_u64() {
            Some(ticks) => {
                queue.push(Intent::Seek(ticks));
                Ok(json!({ "ok": true, "back": ticks }).to_string())
            }
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
            Some(name) => {
                queue.push(Intent::Skill(name.to_owned()));
                Ok(json!({ "ok": true }).to_string())
            }
            None => Err("missing name"),
        },
        ("POST", "/sit") => {
            queue.push(Intent::Sit);
            Ok(json!({ "ok": true }).to_string())
        }
        ("POST", "/ground_pick") => {
            queue.push(Intent::GroundPick);
            Ok(json!({ "ok": true }).to_string())
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
            Some(dir) => {
                queue.push(Intent::Reload(dir.to_owned()));
                Ok(json!({ "ok": true }).to_string())
            }
            None => Err("missing dir"),
        },
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
        match &intents[0] {
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
}
