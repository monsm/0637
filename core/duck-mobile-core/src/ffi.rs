//! C ABI for the iOS app (SwiftUI/Metal) and any other shell.
//!
//! One `DuckCore` owns the runtime; the shell calls `duck_core_tick` at 50 Hz,
//! reads `duck_core_status` for the UI and the body/geom accessors for the
//! renderer. All pointers are borrowed for the call only.
#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_float, c_int};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;

use duck_control::policy::Policy;
use duck_control::safety::SafetyConfig;

use crate::control::Tuning;
use crate::intent::{Intent, IntentQueue};
use crate::mujoco::MujocoIo;
use crate::runtime::{BringupState, DuckRuntime, TickReport};

#[repr(C)]
pub struct DuckStatus {
    /// 0 limp, 1 homing, 2 ready.
    pub bringup: c_int,
    pub fallen: c_int,
    pub gain: c_int,
    pub label: [c_char; 64],
    pub trunk: [c_double; 3],
    pub gravity: [c_double; 3],
}

pub struct DuckCore {
    runtime: DuckRuntime<MujocoIo>,
    skills: Vec<CString>,
    meshes: Vec<CString>,
    last: Option<TickReport>,
    label: String,
    intents: Arc<IntentQueue>,
    server_started: bool,
    /// What this build is: a hash over every asset that decides behaviour.
    provenance: crate::provenance::Provenance,
    fingerprint: CString,
    /// The time machine: the last minute of checkpoints, newest last.
    timeline: VecDeque<crate::runtime::Checkpoint>,
    /// A restore that must land before the next tick (the tick it replaces).
    pending_restore: Option<crate::runtime::Checkpoint>,
    /// Forks of this duck, each running on its own body. The original is the
    /// one `runtime` points at; the rest live here so every branch can be
    /// stepped independently.
    branches: Vec<Branch>,
    /// Which branch the tick drives: 0 is the original.
    selected: usize,
    /// The latest phone-sensor readings, and the mode they were taken in.
    sensors: crate::sensors::Sensors,
}

struct Branch {
    label: String,
    runtime: crate::runtime::DuckRuntime<MujocoIo>,
    tick: u64,
}

unsafe fn write_error(buffer: *mut c_char, size: c_int, message: &str) {
    if buffer.is_null() || size <= 0 {
        return;
    }
    let bytes = message.as_bytes();
    let count = bytes.len().min(size as usize - 1);
    for (index, byte) in bytes.iter().take(count).enumerate() {
        *buffer.add(index) = *byte as c_char;
    }
    *buffer.add(count) = 0;
}

unsafe fn c_string(pointer: *const c_char) -> Result<String, String> {
    if pointer.is_null() {
        return Err("null string".to_owned());
    }
    CStr::from_ptr(pointer)
        .to_str()
        .map(str::to_owned)
        .map_err(|error| error.to_string())
}

/// Create the core. `scene_path` is the bundled `scene_backlash.xml`;
/// `policies_dir` holds the official `.onnx` files and `manifest.json`.
/// One line per load stage in logcat, so a slow or stuck startup is visible.
fn stage(name: &str) {
    #[cfg(target_os = "android")]
    {
        unsafe extern "C" {
            fn __android_log_print(
                prio: c_int,
                tag: *const c_char,
                text: *const c_char,
                ...
            ) -> c_int;
        }
        const ANDROID_LOG_INFO: c_int = 4;
        if let (Ok(tag), Ok(text)) = (CString::new("duck-rust"), CString::new(format!("stage {name}"))) {
            unsafe {
                __android_log_print(
                    ANDROID_LOG_INFO,
                    tag.as_ptr(),
                    text.as_ptr(),
                    std::ptr::null::<c_char>(),
                )
            };
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = name;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_new(
    scene_path: *const c_char,
    policies_dir: *const c_char,
    error: *mut c_char,
    error_size: c_int,
) -> *mut DuckCore {
    // A panic inside this extern "C" function would abort the process
    // (panic_cannot_unwind). Catch it and surface the message through the
    // error buffer instead; on Android also mirror it to logcat.
    #[cfg(target_os = "android")]
    {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // __android_log_print lives in liblog, which rustc links for
            // Android targets; the device exports it from /system/lib64.
            unsafe extern "C" {
                fn __android_log_print(
                    prio: c_int,
                    tag: *const c_char,
                    text: *const c_char,
                    ...
                ) -> c_int;
            }
            let payload = if let Some(text) = info.payload().downcast_ref::<&str>() {
                (*text).to_owned()
            } else if let Some(text) = info.payload().downcast_ref::<String>() {
                text.clone()
            } else {
                "unknown panic".to_owned()
            };
            let location = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_default();
            let message =
                CString::new(format!("rust panic: {payload} at {location}"))
                    .unwrap_or_else(|_| CString::new("rust panic").expect("static"));
            const ANDROID_LOG_ERROR: c_int = 4;
            let tag = CString::new("duck-rust").expect("static");
            unsafe {
                __android_log_print(
                    ANDROID_LOG_ERROR,
                    tag.as_ptr(),
                    message.as_ptr(),
                    std::ptr::null::<c_char>(),
                )
            };
            default_hook(info);
        }));
    }

    let result = std::panic::catch_unwind(|| -> Result<DuckCore, String> {
        stage("start");
        let scene = c_string(scene_path)?;
        let policies = PathBuf::from(c_string(policies_dir)?);
        stage("manifest");
        let bundle = crate::manifest::load(&policies)?;

        stage("policy");
        let policy = Policy::load(&bundle.paths, 0.05).map_err(|error| error.to_string())?;
        stage("mujoco");
        let body = MujocoIo::load(&scene)?;
        stage("runtime");
        let tuning = bundle.tuning();
        let mut runtime = DuckRuntime::new(
            body,
            policy,
            tuning,
            bundle.skill_tuning,
            SafetyConfig::default(),
        );
        stage("enable");
        runtime.enable().map_err(|error| error.to_string())?;
        stage("provenance");
        let provenance = crate::provenance::collect(Path::new(&scene), &policies);
        let fingerprint = CString::new(provenance.fingerprint.clone()).unwrap_or_default();
        stage(&format!("fingerprint {}", provenance.fingerprint));
        stage("ready");

        let skills = runtime
            .skill_names()
            .into_iter()
            .map(|name| CString::new(name).unwrap_or_default())
            .collect();
        let meshes = (0..runtime.io().mesh_count())
            .map(|mesh| {
                CString::new(runtime.io().mesh_name(mesh).unwrap_or_default()).unwrap_or_default()
            })
            .collect();
        let intents = Arc::new(IntentQueue::new());
        intents.set_skills(runtime.skill_names());
        intents.set_fingerprint(provenance.to_json());

        Ok(DuckCore {
            runtime,
            skills,
            meshes,
            last: None,
            label: "idle".to_owned(),
            intents,
            server_started: false,
            provenance,
            fingerprint,
            timeline: VecDeque::new(),
            pending_restore: None,
            branches: Vec::new(),
            selected: 0,
            sensors: crate::sensors::Sensors::default(),
        })
    });

    let result = match result {
        Ok(result) => result,
        Err(payload) => Err(format!(
            "panic during core creation: {payload:?}"
        )),
    };

    match result {
        Ok(core) => Box::into_raw(Box::new(core)),
        Err(message) => {
            write_error(error, error_size, &message);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_free(core: *mut DuckCore) {
    if !core.is_null() {
        drop(Box::from_raw(core));
    }
}

/// Reload the policy set from `policies_dir` (manifest + ONNX). The body and
/// bring-up state stay; call it while the duck is standing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_reload_policies(
    core: *mut DuckCore,
    policies_dir: *const c_char,
) -> c_int {
    if core.is_null() {
        return -1;
    }
    let Ok(dir) = c_string(policies_dir) else {
        return -1;
    };
    let Ok(bundle) = crate::manifest::load(Path::new(&dir)) else {
        return -1;
    };
    let Ok(policy) = Policy::load(&bundle.paths, 0.05) else {
        return -1;
    };
    let tuning = bundle.tuning();
    let core = &mut *core;
    core.runtime
        .replace_policy(policy, tuning, bundle.skill_tuning);
    let names = core.runtime.skill_names();
    core.skills = names
        .iter()
        .map(|name| CString::new(name.clone()).unwrap_or_default())
        .collect();
    core.intents.set_skills(names);
    core.label = "policy reloaded".to_owned();
    0
}

/// Start the local intent API on `0.0.0.0:port`. Idempotent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_start_intent_server(
    core: *mut DuckCore,
    port: c_int,
) -> c_int {
    if core.is_null() || port <= 0 || port > 65535 {
        return -1;
    }
    let core = &mut *core;
    if core.server_started {
        return 0;
    }
    match crate::intent::start(port as u16, core.intents.clone()) {
        Ok(()) => {
            core.server_started = true;
            publish_state(core);
            stage("intent server listening");
            0
        }
        Err(error) => {
            stage(&format!("intent server failed: {error}"));
            -1
        }
    }
}

fn reload_policies(core: &mut DuckCore, dir: &str) -> Result<(), String> {
    let bundle = crate::manifest::load(Path::new(dir))?;
    let policy = Policy::load(&bundle.paths, 0.05).map_err(|error| error.to_string())?;
    let tuning = bundle.tuning();
    core.runtime
        .replace_policy(policy, tuning, bundle.skill_tuning);
    let names = core.runtime.skill_names();
    core.skills = names
        .iter()
        .map(|name| CString::new(name.clone()).unwrap_or_default())
        .collect();
    core.intents.set_skills(names);
    Ok(())
}

/// Keep the last `CAPACITY` ticks of checkpoints. One checkpoint is a few
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

fn apply_intent(core: &mut DuckCore, intent: Intent) {
    match intent {
        Intent::Twist(twist) => core.runtime.set_api_twist(twist),
        Intent::Head(head) => core.runtime.set_head(head),
        Intent::Body(z, roll, pitch) => core.runtime.set_body(z, roll, pitch),
        Intent::BodyRelease => core.runtime.release_body(),
        Intent::Skill(name) => {
            if let Err(error) = core.runtime.trigger_skill(&name) {
                core.label = format!("skill refused: {error}");
            }
        }
        Intent::Sit => {
            if let Err(error) = core.runtime.sit_toggle() {
                core.label = format!("sit refused: {error}");
            }
        }
        Intent::GroundPick => {
            if let Err(error) = core.runtime.start_ground_pick() {
                core.label = format!("ground pick refused: {error}");
            }
        }
        Intent::Push(velocity) => {
            let _ = core.runtime.push(velocity);
        }
        Intent::Reload(dir) => {
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
                core.last = Some(report);
            }
            publish_state(core);
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
        }
        Intent::Fork(label) => {
            let Some(fork) = core.runtime.fork() else {
                core.label = "fork refused: the body cannot be cloned".to_owned();
                return;
            };
            core.branches.push(Branch {
                label,
                tick: fork.clock().tick,
                runtime: fork,
            });
        }
        Intent::DropBranch(label) => {
            core.branches.retain(|branch| branch.label != label);
            if core.selected > core.branches.len() {
                core.selected = 0;
            }
        }
        Intent::SelectBranch(label) => {
            let index = core
                .branches
                .iter()
                .position(|branch| branch.label == label)
                .map_or(0, |index| index + 1);
            core.selected = index;
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
        }
    }
}

fn bringup_code(state: BringupState) -> c_int {
    match state {
        BringupState::Limp => 0,
        BringupState::Homing => 1,
        BringupState::Ready => 2,
    }
}

fn publish_state(core: &DuckCore) {
    let trunk = core.runtime.io().trunk();
    let gravity = core.runtime.io().gravity();
    let clock = core.runtime.clock();
    let checkpoints: Vec<u64> = core.timeline.iter().map(|saved| saved.tick).collect();
    let branches: Vec<serde_json::Value> = core
        .branches
        .iter()
        .map(|branch| {
            let clock = branch.runtime.clock();
            json!({
                "label": branch.label,
                "tick": clock.tick,
                "paused": clock.paused,
                "trunk": branch.runtime.io().trunk(),
                "gravity": branch.runtime.io().gravity(),
                "label_state": branch.runtime.bringup().label(),
            })
        })
        .collect();
    let selected = if core.selected == 0 {
        String::new()
    } else {
        core.branches
            .get(core.selected - 1)
            .map(|branch| branch.label.clone())
            .unwrap_or_default()
    };
    core.intents.set_branches(json!({
        "branches": branches,
        "selected": selected,
    }));
    core.intents.set_sensors(core.sensors.to_json());
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
    }));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_enable(core: *mut DuckCore) -> c_int {
    if core.is_null() {
        return -1;
    }
    match (*core).runtime.enable() {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_disable(core: *mut DuckCore) -> c_int {
    if core.is_null() {
        return -1;
    }
    match (*core).runtime.disable() {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// One 50 Hz tick. Returns 0 on success, -1 if the body read failed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_tick(core: *mut DuckCore) -> c_int {
    if core.is_null() {
        return -1;
    }
    let core = &mut *core;
    for intent in core.intents.drain() {
        apply_intent(core, intent);
    }
    // The phone's IMU is an external disturbance, not a sense: a shake is a
    // shove, exactly as a hand on the real duck would be. It is one impulse —
    // the reading is consumed here, or a single shake would shove the duck
    // every tick until the next sensor post.
    if let Some(push) = core.sensors.take_imu_push() {
        let _ = core.runtime.push(push);
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
    // Every branch steps too, on its own body and its own state. A branch is
    // a separate duck from here on: nothing that happens to one reaches the
    // other.
    for branch in &mut core.branches {
        if !branch.runtime.clock().paused {
            let _ = branch.runtime.tick();
            branch.tick = branch.runtime.clock().tick;
        }
    }

    let report = if core.selected == 0 {
        core.runtime.tick()
    } else {
        match core.branches.get_mut(core.selected - 1) {
            Some(branch) => branch.runtime.tick(),
            None => {
                core.selected = 0;
                core.runtime.tick()
            }
        }
    };
    match report {
        Ok(report) => {
            core.label = report.label.clone();
            core.last = Some(report);
            publish_state(core);
            0
        }
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_set_twist(
    core: *mut DuckCore,
    vx: c_double,
    vy: c_double,
    wz: c_double,
) {
    if !core.is_null() {
        (*core).runtime.set_twist([vx, vy, wz]);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_set_head(
    core: *mut DuckCore,
    neck: c_double,
    head: c_double,
    yaw: c_double,
    roll: c_double,
) {
    if !core.is_null() {
        (*core).runtime.set_head([neck, head, yaw, roll]);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_set_body(
    core: *mut DuckCore,
    z: c_double,
    roll: c_double,
    pitch: c_double,
) {
    if !core.is_null() {
        (*core).runtime.set_body(z, roll, pitch);
    }
}

/// A phone shake or shove: an external trunk velocity impulse.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_push(
    core: *mut DuckCore,
    vx: c_double,
    vy: c_double,
    vz: c_double,
) -> c_int {
    if core.is_null() {
        return -1;
    }
    match (*core).runtime.push([vx, vy, vz]) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// 1 if a skill started or a running chain was refreshed, 0 if refused.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_trigger_skill(
    core: *mut DuckCore,
    name: *const c_char,
) -> c_int {
    if core.is_null() {
        return 0;
    }
    let Ok(name) = c_string(name) else {
        return 0;
    };
    match (*core).runtime.trigger_skill(&name) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_sit_toggle(core: *mut DuckCore) -> c_int {
    if core.is_null() {
        return -1;
    }
    match (*core).runtime.sit_toggle() {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_ground_pick(core: *mut DuckCore) -> c_int {
    if core.is_null() {
        return -1;
    }
    match (*core).runtime.start_ground_pick() {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_skill_count(core: *mut DuckCore) -> c_int {
    if core.is_null() {
        0
    } else {
        (*core).skills.len() as c_int
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_skill_name(core: *mut DuckCore, index: c_int) -> *const c_char {
    if core.is_null() || index < 0 {
        return std::ptr::null();
    }
    let core = &*core;
    core.skills
        .get(index as usize)
        .map_or(std::ptr::null(), |name| name.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_status(core: *mut DuckCore, out: *mut DuckStatus) -> c_int {
    if core.is_null() || out.is_null() {
        return -1;
    }
    let core = &*core;
    let out = &mut *out;
    out.bringup = match core.runtime.bringup() {
        BringupState::Limp => 0,
        BringupState::Homing => 1,
        BringupState::Ready => 2,
    };
    out.fallen = core.runtime.fallen() as c_int;
    out.gain = core.last.as_ref().map_or(200, |report| report.gain as c_int);
    out.label = [0; 64];
    let label = core.label.as_bytes();
    let count = label.len().min(63);
    for (index, byte) in label.iter().take(count).enumerate() {
        out.label[index] = *byte as c_char;
    }
    out.trunk = core.runtime.io().trunk();
    out.gravity = core.runtime.io().gravity();
    0
}

/// Copy the renderer qpos (7 trunk + 14 joints) into `out`. Returns the count,
/// or the required count when `capacity` is too small.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_render_qpos(
    core: *mut DuckCore,
    out: *mut c_double,
    capacity: c_int,
) -> c_int {
    if core.is_null() {
        return 0;
    }
    let qpos = (*core).runtime.io().render_qpos();
    let count = qpos.len();
    if !out.is_null() && capacity >= 0 {
        let copy = count.min(capacity as usize);
        for (index, value) in qpos.iter().take(copy).enumerate() {
            *out.add(index) = *value;
        }
    }
    count as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_body_count(core: *mut DuckCore) -> c_int {
    if core.is_null() {
        0
    } else {
        (*core).runtime.io().body_count() as c_int
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_body_pose(
    core: *mut DuckCore,
    body: c_int,
    pos: *mut c_double,
    quat: *mut c_double,
) -> c_int {
    if core.is_null() || body < 0 || pos.is_null() || quat.is_null() {
        return -1;
    }
    let (position, rotation) = (*core).runtime.io().body_pose(body as usize);
    for (index, value) in position.iter().enumerate() {
        *pos.add(index) = *value;
    }
    for (index, value) in rotation.iter().enumerate() {
        *quat.add(index) = *value;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_geom_count(core: *mut DuckCore) -> c_int {
    if core.is_null() {
        0
    } else {
        (*core).runtime.io().geom_count() as c_int
    }
}

/// One geom, for the renderer. `mesh` is -1 for non-mesh geoms.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_geom_info(
    core: *mut DuckCore,
    geom: c_int,
    body: *mut c_int,
    geom_type: *mut c_int,
    mesh: *mut c_int,
    group: *mut c_int,
    pos: *mut c_double,
    quat: *mut c_double,
    rgba: *mut c_float,
) -> c_int {
    if core.is_null() || geom < 0 {
        return -1;
    }
    let Some(info) = (*core).runtime.io().geom_info(geom as usize) else {
        return -1;
    };
    if !body.is_null() {
        *body = info.body as c_int;
    }
    if !geom_type.is_null() {
        *geom_type = info.geom_type;
    }
    if !mesh.is_null() {
        *mesh = info.mesh;
    }
    if !group.is_null() {
        *group = info.group;
    }
    if !pos.is_null() {
        for (index, value) in info.pos.iter().enumerate() {
            *pos.add(index) = *value;
        }
    }
    if !quat.is_null() {
        for (index, value) in info.quat.iter().enumerate() {
            *quat.add(index) = *value;
        }
    }
    if !rgba.is_null() {
        for (index, value) in info.rgba.iter().enumerate() {
            *rgba.add(index) = *value;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_mesh_name(
    core: *mut DuckCore,
    mesh: c_int,
) -> *const c_char {
    if core.is_null() || mesh < 0 {
        return std::ptr::null();
    }
    let core = &*core;
    core.meshes
        .get(mesh as usize)
        .map_or(std::ptr::null(), |name| name.as_ptr())
}

/// Time-machine controls for the shell. They take effect on the next tick.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_set_paused(core: *mut DuckCore, paused: c_int) {
    if !core.is_null() {
        (*core).runtime.set_paused(paused != 0);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_set_speed(core: *mut DuckCore, speed: c_double) {
    if !core.is_null() {
        (*core).runtime.set_speed(speed);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_step(core: *mut DuckCore, ticks: c_int) {
    if core.is_null() {
        return;
    }
    let core = &mut *core;
    let was_paused = core.runtime.clock().paused;
    core.runtime.set_paused(false);
    for _ in 0..ticks.clamp(1, 500) {
        let _ = core.runtime.tick();
    }
    core.runtime.set_paused(was_paused);
}

/// Step back to a saved tick. Returns 0 when the rewind landed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_seek(core: *mut DuckCore, back_ticks: c_int) -> c_int {
    if core.is_null() {
        return -1;
    }
    let core = &mut *core;
    let target = core
        .runtime
        .clock()
        .tick
        .saturating_sub(back_ticks.max(0) as u64);
    let Some(checkpoint) = core
        .timeline
        .iter()
        .rev()
        .find(|checkpoint| checkpoint.tick <= target)
        .cloned()
    else {
        return -1;
    };
    if !core.runtime.restore(&checkpoint) {
        return -1;
    }
    core.timeline.retain(|saved| saved.tick <= checkpoint.tick);
    0
}

/// Fork the duck at this instant. Returns 0 on success.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_fork(core: *mut DuckCore, label: *const c_char) -> c_int {
    if core.is_null() {
        return -1;
    }
    let core = &mut *core;
    let Some(fork) = core.runtime.fork() else {
        return -1;
    };
    let label = if label.is_null() {
        format!("branch-{}", core.branches.len() + 1)
    } else {
        CStr::from_ptr(label).to_string_lossy().into_owned()
    };
    core.branches.push(Branch {
        label,
        tick: fork.clock().tick,
        runtime: fork,
    });
    0
}

/// The time machine, for a shell that drives the loop itself: whether the
/// duck is frozen, and how many ticks one wall-clock frame should advance.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_paused(core: *mut DuckCore) -> c_int {
    if core.is_null() {
        return 0;
    }
    (*core).runtime.clock().paused as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_speed(core: *mut DuckCore) -> c_double {
    if core.is_null() {
        return 1.0;
    }
    (*core).runtime.clock().speed
}

/// The oldest checkpoint the core still holds, for the scrubber's span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_oldest_tick(core: *mut DuckCore) -> u64 {
    if core.is_null() {
        return 0;
    }
    (*core)
        .timeline
        .front()
        .map_or(0, |checkpoint| checkpoint.tick)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_tick_index(core: *mut DuckCore) -> u64 {
    if core.is_null() {
        return 0;
    }
    (*core).runtime.clock().tick
}

/// What this build is: one hash over the scene, meshes, policies and
/// manifest. Two builds with the same fingerprint run the same duck.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_fingerprint(core: *mut DuckCore) -> *const c_char {
    if core.is_null() {
        return std::ptr::null();
    }
    (*core).fingerprint.as_ptr()
}

/// Triangle soup for a mesh: (px, py, pz, nx, ny, nz) per vertex, three per
/// face, in MuJoCo's own mesh frame. Returns the float count and, when `out`
/// is non-null and `capacity` is large enough, writes the soup.
///
/// The renderer must use this rather than parsing the STL: MuJoCo re-centres
/// and re-orients every mesh when it compiles the model, so the raw file and
/// `mesh_vert` are not the same coordinates.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_mesh_soup(
    core: *mut DuckCore,
    mesh: c_int,
    out: *mut c_float,
    capacity: c_int,
) -> c_int {
    if core.is_null() || mesh < 0 {
        return -1;
    }
    let Some(soup) = (*core).runtime.io().mesh_soup(mesh as usize) else {
        return -1;
    };
    let count = soup.len() as c_int;
    if !out.is_null() && capacity >= count {
        std::ptr::copy_nonoverlapping(soup.as_ptr(), out, soup.len());
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    fn new_core() -> *mut DuckCore {
        let scene = CString::new(
            r"D:\AI\Duck\upstream\microduck_rl\src\mjlab_microduck\robot\microduck\scene_backlash.xml",
        )
        .unwrap();
        let policies = CString::new(r"D:\AI\Duck\policies").unwrap();
        let mut error = [0 as c_char; 1024];
        let core = unsafe {
            duck_core_new(
                scene.as_ptr(),
                policies.as_ptr(),
                error.as_mut_ptr(),
                error.len() as c_int,
            )
        };
        if core.is_null() {
            let message = unsafe { CStr::from_ptr(error.as_ptr()) }.to_string_lossy();
            panic!("duck_core_new failed: {message}");
        }
        core
    }

    #[test]
    fn ffi_creates_ticks_and_exposes_render_data() {
        let core = new_core();

        assert_eq!(unsafe { duck_core_tick(core) }, 0);

        let mut status = DuckStatus {
            bringup: 0,
            fallen: 0,
            gain: 0,
            label: [0; 64],
            trunk: [0.0; 3],
            gravity: [0.0; 3],
        };
        assert_eq!(unsafe { duck_core_status(core, &mut status) }, 0);
        assert_eq!(status.bringup, 2, "already at home -> Ready");
        assert!(unsafe { duck_core_body_count(core) } > 0);
        assert!(unsafe { duck_core_geom_count(core) } > 0);
        assert!(unsafe { duck_core_skill_count(core) } >= 3);

        let count = unsafe { duck_core_render_qpos(core, std::ptr::null_mut(), 0) };
        assert_eq!(count, 22, "7 trunk + 15 wire joints");
        let mut qpos = vec![0.0f64; count as usize];
        assert_eq!(
            unsafe { duck_core_render_qpos(core, qpos.as_mut_ptr(), count) },
            count
        );

        unsafe { duck_core_free(core) };
    }

    #[test]
    fn intent_server_drives_the_core() {
        let core = new_core();
        let port = 19000 + (std::process::id() % 1000) as i32;
        assert_eq!(unsafe { duck_core_start_intent_server(core, port) }, 0);

        let body = r#"{"vx":0.4,"vy":0.0,"wz":0.0}"#;
        let request = format!(
            "POST /twist HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let mut stream = TcpStream::connect(("127.0.0.1", port as u16)).expect("connect");
        stream.write_all(request.as_bytes()).expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read");
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains(r#""ok":true"#), "{response}");

        assert_eq!(unsafe { duck_core_tick(core) }, 0);

        let mut stream = TcpStream::connect(("127.0.0.1", port as u16)).expect("connect");
        stream
            .write_all(b"GET /state HTTP/1.1\r\nHost: x\r\n\r\n")
            .expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read");
        assert!(response.contains(r#""bringup":2"#), "{response}");
        assert!(response.contains("\"trunk\""), "{response}");

        unsafe { duck_core_free(core) };
    }
}
