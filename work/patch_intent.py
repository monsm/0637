import io

# ---------- intent.rs: the time-machine intents and routes ----------
p = 'core/duck-mobile-core/src/intent.rs'
s = io.open(p, encoding='utf-8').read()

s = s.replace('''    Push([f64; 3]),
    Reload(String),
}''', '''    Push([f64; 3]),
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
}''')

s = s.replace('''    skills: Mutex<Vec<String>>,
    fingerprint: Mutex<String>,
}''', '''    skills: Mutex<Vec<String>>,
    fingerprint: Mutex<String>,
    /// The time machine's answers, filled by the core on the next tick.
    time: Mutex<Value>,
}''')
s = s.replace('''            fingerprint: Mutex::new("{}".to_owned()),
        }''', '''            fingerprint: Mutex::new("{}".to_owned()),
            time: Mutex::new(json!({
                "tick": 0,
                "paused": false,
                "speed": 1.0,
                "checkpoints": [],
            })),
        }''')
s = s.replace('''    pub fn fingerprint_json(&self) -> String {
        self.fingerprint.lock().expect("intent fingerprint").clone()
    }''', '''    pub fn fingerprint_json(&self) -> String {
        self.fingerprint.lock().expect("intent fingerprint").clone()
    }

    pub fn set_time(&self, value: Value) {
        *self.time.lock().expect("intent time") = value;
    }

    pub fn time_json(&self) -> String {
        self.time.lock().expect("intent time").to_string()
    }''')

s = s.replace('''        ("GET", "/fingerprint") => Ok(queue.fingerprint_json()),''', '''        ("GET", "/fingerprint") => Ok(queue.fingerprint_json()),
        ("GET", "/time") => Ok(queue.time_json()),
        ("GET", "/timeline") => Ok(queue.time_json()),
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
        },''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(s)

# ---------- ffi.rs: apply the intents, keep the ring, publish /time ----------
p = 'core/duck-mobile-core/src/ffi.rs'
s = io.open(p, encoding='utf-8').read()

s = s.replace('''    /// What this build is: a hash over every asset that decides behaviour.
    provenance: crate::provenance::Provenance,
    fingerprint: CString,
}''', '''    /// What this build is: a hash over every asset that decides behaviour.
    provenance: crate::provenance::Provenance,
    fingerprint: CString,
    /// The time machine: the last minute of checkpoints, newest last.
    timeline: VecDeque<crate::runtime::Checkpoint>,
    /// A restore that must land before the next tick (the tick it replaces).
    pending_restore: Option<crate::runtime::Checkpoint>,
}''')

s = s.replace('''            provenance,
            fingerprint,
        })''', '''            provenance,
            fingerprint,
            timeline: VecDeque::new(),
            pending_restore: None,
        })''')

s = s.replace('''use std::path::{Path, PathBuf};''', '''use std::collections::VecDeque;
use std::path::{Path, PathBuf};''')

s = s.replace('''fn apply_intent(core: &mut DuckCore, intent: Intent) {''', '''/// Keep the last `CAPACITY` ticks of checkpoints. One checkpoint is a few
/// kilobytes, so a minute of history is a few megabytes — small enough to hold
/// in the app, big enough to rewind anything a person would want to see.
const TIMELINE_CAPACITY: usize = 3000;

fn checkpoint_now(core: &mut DuckCore) {
    let Some(checkpoint) = core.runtime.checkpoint() else {
        return;
    };
    // One entry per tick: a second checkpoint at the same tick replaces the
    // first rather than stacking.
    if core.timeline.back().is_some_and(|last| last.tick == checkpoint.tick) {
        core.timeline.pop_back();
    }
    core.timeline.push_back(checkpoint);
    while core.timeline.len() > TIMELINE_CAPACITY {
        core.timeline.pop_front();
    }
}

fn apply_intent(core: &mut DuckCore, intent: Intent) {''')

s = s.replace('''        Intent::Reload(dir) => {
            if let Err(error) = reload_policies(core, &dir) {
                core.label = format!("reload failed: {error}");
            }
        }''', '''        Intent::Reload(dir) => {
            if let Err(error) = reload_policies(core, &dir) {
                core.label = format!("reload failed: {error}");
            }
        }
        Intent::Pause(paused) => core.runtime.set_paused(paused),
        Intent::Step(ticks) => {
            // Advance exactly the requested ticks while frozen; the shell's
            // timer keeps calling tick(), which respects the pause.
            core.runtime.set_paused(false);
            for _ in 0..ticks {
                let _ = core.runtime.tick();
            }
            core.runtime.set_paused(true);
            if let Some(report) = core.runtime.last_report() {
                core.label = report.label.clone();
            }
        }
        Intent::Speed(speed) => core.runtime.set_speed(speed),
        Intent::Checkpoint => checkpoint_now(core),
        Intent::Restore(tick) => {
            if let Some(checkpoint) = core
                .timeline
                .iter()
                .rev()
                .find(|checkpoint| checkpoint.tick == tick)
                .cloned()
            {
                core.pending_restore = Some(checkpoint);
            } else {
                core.label = format!("no checkpoint at tick {tick}");
            }
        }
        Intent::Seek(ticks) => {
            let current = core.runtime.clock().tick;
            let target = current.saturating_sub(ticks);
            if let Some(checkpoint) = core
                .timeline
                .iter()
                .rev()
                .find(|checkpoint| checkpoint.tick <= target)
                .cloned()
            {
                core.pending_restore = Some(checkpoint);
            } else {
                core.label = format!("nothing back to tick {target}");
            }
        }''')

s = s.replace('''    let core = &mut *core;
    for intent in core.intents.drain() {
        apply_intent(core, intent);
    }
    match core.runtime.tick() {
        Ok(report) => {
            core.label = report.label.clone();
            core.last = Some(report);
            publish_state(core);
            0
        }
        Err(_) => -1,
    }
}''', '''    let core = &mut *core;
    for intent in core.intents.drain() {
        apply_intent(core, intent);
    }
    // A restore replaces the tick that was about to happen: the duck resumes
    // at the saved tick and this tick's physics runs from there.
    if let Some(checkpoint) = core.pending_restore.take() {
        if !core.runtime.restore(&checkpoint) {
            core.label = "restore refused".to_owned();
        }
        core.timeline.retain(|saved| saved.tick <= checkpoint.tick);
    }
    if core.runtime.clock().paused {
        publish_state(core);
        return 0;
    }
    // Record the state the tick is about to run from, so a rewind lands on a
    // tick that really happened.
    if core.timeline.back().is_none_or(|last| last.tick < core.runtime.clock().tick) {
        checkpoint_now(core);
    }
    match core.runtime.tick() {
        Ok(report) => {
            core.label = report.label.clone();
            core.last = Some(report);
            publish_state(core);
            0
        }
        Err(_) => -1,
    }
}''')

s = s.replace('''    core.intents.set_state(json!({
        "label": core.label,
        "bringup": bringup_code(core.runtime.bringup()),
        "fallen": core.runtime.fallen(),
        "gain": core.last.as_ref().map_or(200, |report| report.gain),
        "trunk": trunk,
        "gravity": gravity,
    }));''', '''    let clock = core.runtime.clock();
    let checkpoints: Vec<u64> = core.timeline.iter().map(|saved| saved.tick).collect();
    core.intents.set_time(json!({
        "tick": clock.tick,
        "paused": clock.paused,
        "speed": clock.speed,
        "checkpoints": checkpoints,
        "oldest": checkpoints.first().copied(),
        "newest": checkpoints.last().copied(),
    }));
    core.intents.set_state(json!({
        "label": core.label,
        "bringup": bringup_code(core.runtime.bringup()),
        "fallen": core.runtime.fallen(),
        "gain": core.last.as_ref().map_or(200, |report| report.gain),
        "trunk": trunk,
        "gravity": gravity,
        "tick": clock.tick,
        "paused": clock.paused,
        "speed": clock.speed,
    }));''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('ok')
