import io

p = r'D:\AI\Duck\core\duck-mobile-core\src\ffi.rs'
s = io.open(p, encoding='utf-8').read()
start = s.index('fn apply_intent(core: &mut DuckCore, intent: Intent) {')
end = s.index('fn bringup_code(state: BringupState) -> c_int {')

new = r'''/// Record a refusal where the shell and the next `/state` can see it, and
/// answer the client that asked. A refusal that lives only for the tick that
/// produced it is invisible to both.
fn refuse(core: &mut DuckCore, message: String) -> serde_json::Value {
    core.label = message.clone();
    core.notice = Some((core.runtime.clock().tick, message.clone()));
    json!({ "ok": false, "error": message })
}

fn apply_intent(core: &mut DuckCore, intent: Intent) -> serde_json::Value {
    match intent {
        Intent::Twist(twist) => {
            core.runtime.set_api_twist(twist);
            json!({ "ok": true, "twist": twist })
        }
        Intent::Head(head) => {
            core.runtime.set_head(head);
            json!({ "ok": true })
        }
        Intent::Body(z, roll, pitch) => {
            core.runtime.set_body(z, roll, pitch);
            json!({ "ok": true })
        }
        Intent::BodyRelease => {
            core.runtime.release_body();
            json!({ "ok": true })
        }
        Intent::Skill(name) => {
            // The ring and `/skills` name the same verbs. `sit`/`stand` are
            // idempotent here even though the pad's A button toggles: a client
            // asking for `stand` while standing has already succeeded.
            let result = match name.as_str() {
                "sit" | "stand" => {
                    let sitting = core.runtime.is_sitting();
                    if (name == "sit") == sitting {
                        Ok(json!({ "ok": true, "skill": name, "already": true }))
                    } else {
                        core.runtime
                            .sit_toggle()
                            .map(|did| json!({ "ok": true, "skill": name, "did": did }))
                            .map_err(str::to_owned)
                    }
                }
                "ground_pick" => core
                    .runtime
                    .start_ground_pick()
                    .map(|()| json!({ "ok": true, "skill": name }))
                    .map_err(str::to_owned),
                _ => core
                    .runtime
                    .trigger_skill(&name)
                    .map(|started| {
                        json!({ "ok": true, "skill": name, "chained": !started })
                    })
                    .map_err(str::to_owned),
            };
            match result {
                Ok(value) => value,
                Err(error) => refuse(core, format!("skill refused: {error}")),
            }
        }
        Intent::Sit => match core.runtime.sit_toggle() {
            Ok(did) => json!({ "ok": true, "did": did }),
            Err(error) => refuse(core, format!("sit refused: {error}")),
        },
        Intent::GroundPick => match core.runtime.start_ground_pick() {
            Ok(()) => json!({ "ok": true }),
            Err(error) => refuse(core, format!("ground pick refused: {error}")),
        },
        Intent::Push(velocity) => {
            let _ = core.runtime.push(velocity);
            json!({ "ok": true })
        }
        Intent::Carry(offset, active) => {
            core.runtime.set_carry(offset, active);
            json!({ "ok": true, "carrying": active })
        }
        Intent::Reload(dir) => match reload_policies(core, &dir) {
            Ok(()) => json!({ "ok": true }),
            Err(error) => refuse(core, format!("reload failed: {error}")),
        },
        Intent::Pause(paused) => {
            core.runtime.set_paused(paused);
            json!({ "ok": true, "paused": paused })
        }
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
                core.last = Some(report);
            }
            publish_state(core);
            json!({ "ok": true, "ticks": ticks, "tick": core.runtime.clock().tick })
        }
        Intent::Speed(speed) => {
            core.runtime.set_speed(speed);
            json!({ "ok": true, "speed": core.runtime.clock().speed })
        }
        Intent::Checkpoint => {
            checkpoint_now(core);
            json!({ "ok": true, "tick": core.runtime.clock().tick })
        }
        Intent::Restore(tick) => {
            let found = core
                .timeline
                .iter()
                .rev()
                .find(|checkpoint| checkpoint.tick == tick)
                .cloned();
            match found {
                Some(checkpoint) => {
                    core.pending_restore = Some(checkpoint);
                    // The restore lands on the next tick, which replaces this
                    // one; the tick number is the answer, not a guess.
                    json!({ "ok": true, "pending": true, "tick": tick })
                }
                None => refuse(core, format!("no checkpoint at tick {tick}")),
            }
        }
        Intent::Sensor(reading) => {
            use crate::sensors::Mode;
            let mode = match reading.mode.as_str() {
                "ar" => Some(Mode::Ar),
                "possession" => Some(Mode::Possession),
                "eval-only" => Some(Mode::Eval),
                _ => Some(Mode::Twin),
            };
            core.sensors.mode = mode;
            match reading.sensor.as_str() {
                "imu" => core.sensors.imu = Some(reading),
                "camera" => core.sensors.camera = Some(reading),
                "microphone" => core.sensors.microphone = Some(reading),
                "depth" => core.sensors.depth = Some(reading),
                _ => {}
            }
            json!({ "ok": true })
        }
        Intent::Fork(label) => match core.runtime.fork() {
            Some(fork) => {
                let tick = fork.clock().tick;
                core.branches.push(Branch {
                    label: label.clone(),
                    tick,
                    runtime: fork,
                });
                json!({ "ok": true, "label": label, "tick": tick })
            }
            None => refuse(core, "fork refused: the body cannot be cloned".to_owned()),
        },
        Intent::DropBranch(label) => {
            core.branches.retain(|branch| branch.label != label);
            if core.selected > core.branches.len() {
                core.selected = 0;
            }
            json!({ "ok": true, "label": label })
        }
        Intent::SelectBranch(label) => {
            let index = core
                .branches
                .iter()
                .position(|branch| branch.label == label)
                .map_or(0, |index| index + 1);
            core.selected = index;
            json!({ "ok": true, "selected": if index == 0 { String::new() } else { label } })
        }
        Intent::Seek(ticks) => {
            let current = core.runtime.clock().tick;
            let target = current.saturating_sub(ticks);
            let found = core
                .timeline
                .iter()
                .rev()
                .find(|checkpoint| checkpoint.tick <= target)
                .cloned();
            match found {
                Some(checkpoint) => {
                    let at = checkpoint.tick;
                    core.pending_restore = Some(checkpoint);
                    json!({ "ok": true, "pending": true, "back": ticks, "tick": at })
                }
                None => refuse(core, format!("nothing back to tick {target}")),
            }
        }
    }
}

'''

s = s[:start] + new + s[end:]
io.open(p, 'w', encoding='utf-8').write(s)
print('apply_intent rewritten')
