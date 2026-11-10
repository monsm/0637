//! A JSON-RPC 2.0 endpoint, so an MCP client can drive the duck.
//!
//! MCP over HTTP is JSON-RPC: `initialize`, `tools/list`, `tools/call`.
//! The tool table is built from the same intent set the HTTP routes dispatch
//! to, so a tool cannot drift from the command it names — and an unknown tool
//! fails closed rather than being guessed at.
//!
//! Everything the duck can do here is something the real duck can do through
//! its own intent protocol. The one addition is the *observation* surface: a
//! model can read the duck's state, its provenance and its sensor readings,
//! which is what makes it useful as a tool rather than a remote control.

use serde_json::{json, Value};

use crate::intent::IntentQueue;

pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// The tools, in the order `tools/list` reports them.
pub fn tools() -> Value {
    json!([
        {
            "name": "duck_state",
            "description": "What the duck is doing right now: label, bring-up state, whether it has fallen, the trunk position and the gravity vector.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "duck_move",
            "description": "Drive the duck. vx is forward (m/s, up to 0.6), vy is sideways, wz is yaw rate (rad/s). The command holds for one second unless repeated, and the safety layer can always preempt it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "vx": { "type": "number", "description": "forward, m/s" },
                    "vy": { "type": "number", "description": "sideways, m/s" },
                    "wz": { "type": "number", "description": "yaw rate, rad/s" }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "duck_stop",
            "description": "Stop driving and stand still.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "duck_skill",
            "description": "Run one of the duck's own one-shot skills: roulade, kick_left, kick_right, or one of the 13 official emotions (curious, excited, sad, laugh, yes, no, ...). Get the full list from GET /skills.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": { "type": "string" } },
                "required": ["name"],
                "additionalProperties": false
            }
        },
        {
            "name": "duck_look",
            "description": "Aim the head at a point in the duck's own frame (metres: x forward, y left, z up; the duck's position is /state.pose). Returns the joint angles and whether the aim was clamped by the servos' travel.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "x": { "type": "number" }, "y": { "type": "number" },
                    "z": { "type": "number" }, "neck": { "type": "number" }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "duck_hear",
            "description": "Say something to the duck in Chinese or English and it does the matching verb: 坐下/sit, 翻滚/roll, 开心/excited, 摇头/no, 捡起来/pick, ... Returns what it heard and what it did.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"],
                "additionalProperties": false
            }
        },
        {
            "name": "duck_flashlight",
            "description": "Toggle the duck's virtual face flashlight, or set it to one of robotd's seven colours: white, red, green, blue, yellow, cyan, magenta.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "on": { "type": "boolean" },
                    "color": { "type": "string" }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "duck_sit",
            "description": "Sit down, or stand back up if already sitting.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "duck_ground_pick",
            "description": "Run the ground-pick motion: reach down with the beak.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "duck_head",
            "description": "Aim the head. Angles in radians; the real duck's neck and head axes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "neck": { "type": "number" },
                    "head": { "type": "number" },
                    "yaw": { "type": "number" },
                    "roll": { "type": "number" }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "duck_mode",
            "description": "Switch drive bundles: walk (legs) or roller (wheels). The whole policy set changes, exactly as the robot's mode line. The duck must be standing still.",
            "inputSchema": {
                "type": "object",
                "properties": { "mode": { "type": "string" } },
                "required": ["mode"],
                "additionalProperties": false
            }
        },
        {
            "name": "duck_shutdown",
            "description": "Power the duck off the way the robot does: sit down, ease into the rest pose, cut torque. The battery EMAs to empty on its own and runs the same sequence.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "duck_power",
            "description": "Power the duck back on after a shutdown.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "duck_sound",
            "description": "Make the duck's voice: greet, chirp, coo, peck, inquire or alarm. Rendered by the robot's own synth, so it sounds like the duck and not like a sample.",
            "inputSchema": {
                "type": "object",
                "properties": { "tag": { "type": "string" } },
                "required": ["tag"],
                "additionalProperties": false
            }
        },
        {
            "name": "duck_mouth",
            "description": "Open or close the beak, 0..1. The mouth is not part of any policy; the idle yawn only ever opens it further.",
            "inputSchema": {
                "type": "object",
                "properties": { "open": { "type": "number" } },
                "required": ["open"],
                "additionalProperties": false
            }
        },
        {
            "name": "duck_sensors",
            "description": "The phone's latest camera, microphone, depth and IMU readings, tagged with the mode they were taken in. None of them is part of the duck's own observation.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "duck_provenance",
            "description": "Which scene, meshes and policies this build is running, as one SHA-256 fingerprint.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "duck_time",
            "description": "The simulation clock: tick, paused, speed, and the checkpoints available to rewind to.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "duck_pause",
            "description": "Freeze the duck, or resume it.",
            "inputSchema": {
                "type": "object",
                "properties": { "paused": { "type": "boolean" } },
                "required": ["paused"],
                "additionalProperties": false
            }
        },
        {
            "name": "duck_fork",
            "description": "Copy the duck at this instant into an independent branch.",
            "inputSchema": {
                "type": "object",
                "properties": { "label": { "type": "string" } },
                "required": ["label"],
                "additionalProperties": false
            }
        }
    ])
}

fn result(id: Value, value: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": value }).to_string()
}

fn error(id: Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
    .to_string()
}

/// A tool call's answer, in MCP's content shape.
fn text(value: Value) -> Value {
    json!({ "content": [ { "type": "text", "text": value.to_string() } ] })
}

/// Handle one JSON-RPC message. Unknown methods and unknown tools fail closed.
pub fn handle(message: &str, queue: &IntentQueue) -> String {
    let request: Value = match serde_json::from_str(message) {
        Ok(value) => value,
        Err(_) => return error(Value::Null, -32700, "parse error"),
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => result(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "microduck", "version": "0.1.0" },
            }),
        ),
        "notifications/initialized" => String::new(),
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({ "tools": tools() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            match call(name, &arguments, queue) {
                Ok(value) => result(id, text(value)),
                Err(reason) => result(
                    id,
                    json!({
                        "content": [ { "type": "text", "text": reason } ],
                        "isError": true,
                    }),
                ),
            }
        }
        _ => error(id, -32601, "method not found"),
    }
}

fn number(arguments: &Value, key: &str) -> f64 {
    arguments.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn call(name: &str, arguments: &Value, queue: &IntentQueue) -> Result<Value, String> {
    match name {
        "duck_state" => Ok(serde_json::from_str(&queue.state_json()).unwrap_or(json!({}))),
        "duck_move" => {
            queue.push(crate::intent::Intent::Twist([
                number(arguments, "vx").clamp(-0.6, 0.6),
                number(arguments, "vy").clamp(-0.4, 0.4),
                number(arguments, "wz").clamp(-1.5, 1.5),
            ]));
            Ok(json!({ "ok": true, "stream": true }))
        }
        "duck_stop" => {
            queue.push(crate::intent::Intent::Twist([0.0; 3]));
            Ok(json!({ "ok": true, "stream": true }))
        }
        "duck_skill" => {
            let skill = arguments
                .get("name")
                .and_then(Value::as_str)
                .ok_or("missing name")?;
            // The tool answers what the duck did, not what the queue took: a
            // refusal is the useful half of a skill call.
            Ok(queue.request(crate::intent::Intent::Skill(skill.to_owned())))
        }
        "duck_look" => Ok(queue.request(crate::intent::Intent::Look {
            target: [
                number(arguments, "x"),
                number(arguments, "y"),
                number(arguments, "z"),
            ],
            neck_pitch: number(arguments, "neck"),
        })),
        "duck_hear" => {
            let text = arguments
                .get("text")
                .and_then(Value::as_str)
                .ok_or("missing text")?;
            Ok(queue.request(crate::intent::Intent::Heard(text.to_owned())))
        }
        "duck_flashlight" => {
            let on = arguments.get("on").and_then(Value::as_bool);
            let color = arguments
                .get("color")
                .and_then(Value::as_str)
                .unwrap_or("white")
                .to_owned();
            Ok(queue.request(crate::intent::Intent::Flashlight {
                toggle: on.is_none(),
                on: on.unwrap_or(false),
                color,
            }))
        }
        "duck_sit" => Ok(queue.request(crate::intent::Intent::Sit)),
        "duck_ground_pick" => Ok(queue.request(crate::intent::Intent::GroundPick)),
        "duck_head" => {
            queue.push(crate::intent::Intent::Head([
                number(arguments, "neck"),
                number(arguments, "head"),
                number(arguments, "yaw"),
                number(arguments, "roll"),
            ]));
            Ok(json!({ "ok": true, "stream": true }))
        }
        "duck_shutdown" => Ok(queue.request_within(
            crate::intent::Intent::Shutdown,
            std::time::Duration::from_secs(15),
        )),
        "duck_power" => Ok(queue.request(crate::intent::Intent::PowerOn)),
        "duck_mode" => {
            let mode = arguments
                .get("mode")
                .and_then(Value::as_str)
                .ok_or("missing mode")?;
            // A mode switch loads a second MJCF and a policy set: seconds,
            // not the 300 ms a one-shot gets. The same budget as POST /mode.
            Ok(queue.request_within(
                crate::intent::Intent::Mode(mode.to_owned()),
                std::time::Duration::from_secs(30),
            ))
        }
        "duck_sound" => {
            let tag = arguments
                .get("tag")
                .and_then(Value::as_str)
                .ok_or("missing tag")?;
            Ok(queue.request(crate::intent::Intent::Sound(tag.to_owned())))
        }
        "duck_mouth" => {
            let open = number(arguments, "open").clamp(0.0, 1.0);
            Ok(queue.request(crate::intent::Intent::Mouth(open)))
        }
        "duck_sensors" => Ok(serde_json::from_str(&queue.sensors_json()).unwrap_or(json!({}))),
        "duck_provenance" => {
            Ok(serde_json::from_str(&queue.fingerprint_json()).unwrap_or(json!({})))
        }
        "duck_time" => Ok(serde_json::from_str(&queue.time_json()).unwrap_or(json!({}))),
        "duck_pause" => {
            let paused = arguments
                .get("paused")
                .and_then(Value::as_bool)
                .ok_or("missing paused")?;
            Ok(queue.request(crate::intent::Intent::Pause(paused)))
        }
        "duck_fork" => {
            let label = arguments
                .get("label")
                .and_then(Value::as_str)
                .ok_or("missing label")?;
            Ok(queue.request(crate::intent::Intent::Fork(label.to_owned())))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_and_tools_list_answer() {
        let queue = IntentQueue::new();
        let response = handle(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#, &queue);
        assert!(response.contains("\"protocolVersion\""), "{response}");
        let response = handle(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, &queue);
        assert!(response.contains("duck_move"), "{response}");
        assert!(response.contains("duck_fork"), "{response}");
    }

    #[test]
    fn an_unknown_tool_fails_closed() {
        let queue = IntentQueue::new();
        let response = handle(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"duck_fly"}}"#,
            &queue,
        );
        assert!(response.contains("unknown tool"), "{response}");
    }

    #[test]
    fn a_move_enqueues_a_clamped_twist() {
        let queue = IntentQueue::new();
        let response = handle(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"duck_move","arguments":{"vx":9.0}}}"#,
            &queue,
        );
        assert!(response.contains("ok\\\":true"), "{response}");
        let intents = queue.drain();
        assert_eq!(intents.len(), 1);
        match &intents[0].intent {
            crate::intent::Intent::Twist(twist) => assert_eq!(twist[0], 0.6),
            other => panic!("unexpected intent: {other:?}"),
        }
    }
}
