//! QA round 3 · boundary probes on the intent server's HTTP surface.
//!
//! These are not product tests: they are the checks a release gate needs and
//! the smoke scripts never made. Every case sends a hostile body to a route
//! and asserts the *documented* answer — refused (`ok:false`) and, crucially,
//! nothing queued on the duck's side. A case that fails is a live defect, not
//! a flaky test: the harness starts a real listener on a real socket.
//!
//! Run: `cargo test -p duck-mobile-core --test qa3_boundaries`
//! (the core needs `--no-default-features --features onnx-dynamic`; the
//! feature set only matters for the policy tests, not for these probes).

use duck_mobile_core::intent::{start_listener, IntentQueue};
use serde_json::json;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

struct Harness {
    port: u16,
    queue: Arc<IntentQueue>,
    // The server owns the accept thread; it has to outlive the probes.
    _server: duck_mobile_core::intent::IntentServer,
}

impl Harness {
    fn start() -> Harness {
        let queue = Arc::new(IntentQueue::new());
        // The official emotion names the core loads, so the chorus probes
        // measure the shape of the request and not the mood table.
        queue.set_emotions(vec!["laugh".to_owned(), "yes_fast".to_owned()]);
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = start_listener(listener, queue.clone());
        Self::start_applier(&queue);
        Harness {
            port,
            queue,
            _server: server,
        }
    }

    /// The apply half: drains the queue in a background thread so a GET
    /// probe's synchronous request gets a real answer instead of a pending.
    fn start_applier(queue: &Arc<IntentQueue>) {
        let queue = queue.clone();
        std::thread::spawn(move || loop {
            for item in queue.drain() {
                let answer = match &item.intent {
                    duck_mobile_core::intent::Intent::Backup => json!({
                        "format": "microduck-backup/1",
                        "day": "2026-11-14",
                        "care": {},
                        "quests": {},
                        "rules": {"rules": []},
                        "fingerprint": "test",
                    }),
                    duck_mobile_core::intent::Intent::RestoreBackup(_) => {
                        serde_json::json!({"ok": true})
                    }
                    _ => serde_json::json!({"ok": true}),
                };
                queue.answer(item.id, answer);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        });
    }

    fn post(&self, path: &str, body: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).expect("write");
        let mut response = String::new();
        // The server closes when it is done; a reset after the body arrived
        // still carries the answer, so it is not a failure.
        let _ = stream.read_to_string(&mut response);
        response
    }

    /// A refusal has to say so *and* leave the duck alone.
    fn refuses(&self, path: &str, body: &str) {
        let response = self.post(path, body);
        assert!(
            response.contains(r#""ok":false"#),
            "{path} {body} was not refused: {response}"
        );
        assert!(
            self.queue.drain().is_empty(),
            "{path} {body} was refused but still reached the duck"
        );
    }
}

#[test]
fn a_chorus_needs_a_list_of_moves() {
    let h = Harness::start();
    // `moves` is not a list. The duck must not sing a song nobody asked for.
    h.refuses("/chorus", r#"{"moves":"laugh"}"#);
    h.refuses("/chorus", r#"{"moves":{"0":"laugh"}}"#);
    h.refuses("/chorus", r#"{"moves":7}"#);
}

#[test]
fn a_chorus_refuses_a_move_that_is_not_a_name() {
    let h = Harness::start();
    // A number inside the list is not a move. Silently dropping it would
    // play a different song than the one that was asked for.
    h.refuses("/chorus", r#"{"moves":["laugh",7]}"#);
    h.refuses("/chorus", r#"{"moves":["laugh",null]}"#);
    h.refuses("/chorus", r#"{"moves":[null]}"#);
}

#[test]
fn a_chorus_needs_at_least_one_and_at_most_three_moves() {
    let h = Harness::start();
    h.refuses("/chorus", r#"{"moves":[]}"#);
    h.refuses("/chorus", r#"{"moves":["laugh","yes_fast","laugh","yes_fast"]}"#);
}

#[test]
fn a_chorus_refuses_a_move_that_is_not_an_official_emotion() {
    let h = Harness::start();
    h.refuses("/chorus", r#"{"moves":["moonwalk"]}"#);
}

#[test]
fn a_chorus_without_moves_sings_the_documented_pair() {
    let h = Harness::start();
    let response = h.post("/chorus", r#"{}"#);
    assert!(response.contains(r#""ok":true"#), "{response}");
    let queued = h.queue.drain();
    assert_eq!(queued.len(), 1, "the chorus was not queued");
}

#[test]
fn a_night_needs_a_boolean() {
    let h = Harness::start();
    h.refuses("/night", r#"{"on":"true"}"#);
    h.refuses("/night", r#"{"on":1}"#);
    h.refuses("/night", r#"{"on":null}"#);
    h.refuses("/night", r#"{}"#);
}

#[test]
fn a_painted_route_stays_inside_its_caps() {
    let h = Harness::start();
    // 64 points is the cap; 65 must be refused before the duck queues it.
    let mut points = Vec::new();
    for index in 0..65 {
        points.push(format!("[{index},0]"));
    }
    h.refuses("/path", &format!(r#"{{"points":[{}]}}"#, points.join(",")));
    // 20 m is the length cap.
    h.refuses("/path", r#"{"points":[[0,0],[25,0]]}"#);
    // A route needs at least two points to be a route.
    h.refuses("/path", r#"{"points":[[0,0]]}"#);
    // A point that is not a point.
    h.refuses("/path", r#"{"points":[[0,0],["x",0]]}"#);
    h.refuses("/path", r#"{"points":"0,0"}"#);
}

#[test]
fn a_gift_needs_a_known_peer_and_a_known_gift() {
    let h = Harness::start();
    h.refuses("/peers/gift", r#"{"peer":"ghost","gift":"treat"}"#);
    h.refuses("/peers/gift", r#"{"peer":"ghost","gift":"emotion","value":"laugh"}"#);
    h.refuses("/peers/gift", r#"{"peer":"ghost","gift":"drone"}"#);
}

#[test]
fn a_backup_names_its_format_and_a_strange_one_is_refused() {
    let h = Harness::start();
    // GET /backup answers with the versioned document; every part a client
    // could carry back is in it.
    let response = h.post("/backup", "{}");
    for needle in ["microduck-backup/1", "\"care\"", "\"quests\"", "\"rules\"", "\"fingerprint\""] {
        assert!(
            response.contains(needle.trim_matches('"')),
            "backup is missing {needle}: {response}"
        );
    }
    // A document without the format field, or with a strange one, is
    // refused as a whole - no part of a strange backup is applied.
    h.refuses("/backup/restore", r#"{"care":{"hunger":0.5}}"#);
    h.refuses(
        "/backup/restore",
        r#"{"format":"some-other-backup/9","care":{"hunger":0.5}}"#,
    );
}
