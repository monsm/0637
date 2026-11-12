//! C ABI for the iOS app (SwiftUI/Metal) and any other shell.
//!
//! One `DuckCore` owns the runtime; the shell calls `duck_core_tick` at 50 Hz,
//! reads `duck_core_status` for the UI and the body/geom accessors for the
//! renderer. All pointers are borrowed for the call only.
#![allow(unsafe_op_in_unsafe_fn)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_float, c_int, c_ulonglong};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;

use duck_control::policy::Policy;
use duck_control::safety::SafetyConfig;

use crate::control::Tuning;
use crate::intent::{Intent, IntentQueue};
use crate::lan::GiftKind;
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
    /// The loop's own state, as `robotd` publishes it. Mirrors `duck_core.h`;
    /// the two have to move together.
    pub imu_ready: c_int,
    pub picked_up: c_int,
    pub p_held: c_double,
    pub mouth: c_double,
    pub volts: c_double,
    pub has_mouth: c_int,
    /// The platform's own measurements, once a second: display frames per
    /// second and control ticks per second achieved (target: 50).
    pub fps: c_double,
    pub ticks_per_second: c_double,
}

pub struct DuckCore {
    runtime: DuckRuntime<MujocoIo>,
    skills: Vec<CString>,
    meshes: Vec<CString>,
    last: Option<TickReport>,
    label: String,
    intents: Arc<IntentQueue>,
    /// The HTTP API's own handle: dropping the core stops the listener, so a
    /// freed duck cannot leave :8765 bound to a queue nobody drains.
    server: Option<crate::intent::IntentServer>,
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
    /// Clap/whistle detection over the microphone channel, and the sequence
    /// that changes with each new reading so one post is heard once.
    sound_watch: crate::sensors::SoundWatch,
    sound_sequence: u64,
    /// The LAN social layer: other ducks, announces and greetings.
    lan: Option<crate::lan::Lan>,
    /// Daily quests. Kept on DuckCore, not the simulated runtime: a rewind
    /// must not rewind the person's day.
    quests: crate::quests::QuestBoard,
    /// A refusal the client should see for longer than the tick that made it:
    /// (tick, message). Published on `/state` until it goes stale.
    notice: Option<(u64, String)>,
    /// The last sound rendered through the C ABI, by tag. The caller asks for
    /// the length and then for the samples, and rendering twice would pick the
    /// next variant — a different length, and a buffer overrun.
    rendered_sound: Option<(String, Vec<f32>)>,
    /// The tag `duck_core_next_sound` last returned. A static CStr per synth
    /// tag cannot name an emotion loaded at runtime, so the pointer the
    /// platform holds belongs to this owned slot (valid until the next call).
    sound_tag: Option<CString>,
    /// Where the emotions came from, so a mode switch can re-attach them to
    /// the fresh runtime (the same reason `pickup_model` is carried over).
    emotions_dir: Option<std::path::PathBuf>,
    /// Where this duck came from, so a mode switch can load the other bundle
    /// and its scene.
    scene: String,
    policies: std::path::PathBuf,
    /// The platform's own measurements, written once a second: display frames
    /// per second and control ticks per second actually achieved.
    performance: (f64, f64),
    /// Bumped whenever the body model changes (a mode switch). The renderer
    /// caches geometry per model; a new generation means rebuild it.
    model_generation: u64,
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

        // The shell's ring shows the same verbs `/skills` publishes, so a
        // button and an API call cannot disagree about what exists.
        let skills = published_skills(runtime.skill_names())
            .into_iter()
            .map(|name| CString::new(name).unwrap_or_default())
            .collect();
        let meshes = (0..runtime.io().mesh_count())
            .map(|mesh| {
                CString::new(runtime.io().mesh_name(mesh).unwrap_or_default()).unwrap_or_default()
            })
            .collect();
        let intents = Arc::new(IntentQueue::new());
        intents.set_skills(published_skills(runtime.skill_names()));
        intents.set_fingerprint(provenance.to_json());

        Ok(DuckCore {
            runtime,
            skills,
            meshes,
            last: None,
            label: "idle".to_owned(),
            intents,
            server: None,
            provenance,
            fingerprint,
            timeline: VecDeque::new(),
            pending_restore: None,
            branches: Vec::new(),
            selected: 0,
            sensors: crate::sensors::Sensors::default(),
            sound_watch: crate::sensors::SoundWatch::default(),
            sound_sequence: 0,
            // Discovery starts on demand (POST /peers/start): iOS asks for
            // local-network permission the first time an app sends on the
            // LAN, and that question belongs to the moment the user opens
            // Neighbours, not to every launch.
            lan: None,
            quests: crate::quests::QuestBoard::default(),
            notice: None,
            rendered_sound: None,
            sound_tag: None,
            emotions_dir: None,
            scene: scene.clone(),
            policies: policies.clone(),
            performance: (0.0, 0.0),
            model_generation: 0,
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
    let core = &mut *core;
    // A reload keeps the mode the duck is running: a roller set swapped for a
    // walking one under a standing-disabled controller would be a different
    // robot with the same name.
    let Ok(bundle) = crate::manifest::load_mode(Path::new(&dir), core.runtime.mode()) else {
        return -1;
    };
    let Ok(mut policy) = Policy::load(&bundle.paths, 0.05) else {
        return -1;
    };
    if bundle.standing_disabled {
        policy.set_standing_disabled(true);
    }
    let tuning = bundle.tuning();
    core.runtime
        .replace_policy(policy, tuning, bundle.skill_tuning);
    core.policies = PathBuf::from(&dir);
    let names = published_skills(core.runtime.skill_names());
    core.skills = names
        .iter()
        .map(|name| CString::new(name.clone()).unwrap_or_default())
        .collect();
    core.intents.set_skills(names);
    core.label = "policy reloaded".to_owned();
    0
}

/// The platform's own measurements, once a second: frames drawn per second and
/// control ticks achieved per second. Diagnostic, not part of the duck.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_set_performance(core: *mut DuckCore, fps: f64, tps: f64) {
    if !core.is_null() {
        (*core).performance = (fps, tps);
    }
}

/// The held wheee ride: 1 starts it (start + repeating loops), 0 ends it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_wheee(core: *mut DuckCore, hold: c_int) {
    if !core.is_null() {
        (*core).runtime.wheee(hold != 0);
    }
}

/// `robot.flashlight` on a phone: toggle or set the virtual face LED.
/// `color` is one of white/red/green/blue/yellow/cyan/magenta. Returns the
/// new state: 0 off, 1..7 the colour index + 1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_flashlight(
    core: *mut DuckCore,
    toggle: c_int,
    on: c_int,
    color: *const c_char,
) -> c_int {
    if core.is_null() {
        return -1;
    }
    let color = c_string(color).unwrap_or_else(|_| "white".to_owned());
    let face = (*core).runtime.set_flashlight(toggle != 0, on != 0, &color);
    publish_state(&mut *core);
    i32::from(face.flashlight)
}

/// Load robotd's emotion timelines and voices from a directory in the app
/// bundle. Returns the number loaded, or -1. The emotions then appear in
/// `/skills`, `/skill`, MCP and the shell's skill ring.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_load_emotions(
    core: *mut DuckCore,
    dir: *const c_char,
) -> c_int {
    if core.is_null() {
        return -1;
    }
    let Ok(path) = c_string(dir) else {
        return -1;
    };
    let dir = std::path::PathBuf::from(path.as_str());
    match (*core).runtime.load_emotions(dir.as_path()) {
        Ok(count) => {
            (*core).emotions_dir = Some(dir);
            // `/skills` is a snapshot the intent server owns, so publish the
            // enlarged list before a client asks for it. The gift route needs
            // the official emotion names separately from the skills list.
            let names = published_skills((*core).runtime.skill_names());
            (*core).intents.set_skills(names.clone());
            (*core)
                .intents
                .set_emotions((*core).runtime.emotion_names());
            // The wire refuses an emotion gift for a mood this build does not
            // have, so the same list feeds the LAN layer.
            if let Some(lan) = &(*core).lan {
                lan.set_emotions((*core).runtime.emotion_names());
            }
            (*core).skills = names
                .iter()
                .map(|name| CString::new(name.clone()).unwrap_or_default())
                .collect();
            count as c_int
        }
        Err(_) => -1,
    }
}

/// The drive mode currently loaded: "walk" or "roller".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_mode(core: *mut DuckCore) -> *const c_char {
    if core.is_null() {
        return c"walk".as_ptr();
    }
    match (*core).runtime.mode() {
        robotd_params::Mode::Walk => c"walk".as_ptr(),
        robotd_params::Mode::Roller => c"roller".as_ptr(),
    }
}

/// Bumped when the body model changed (a mode switch); a renderer that caches
/// meshes and geoms rebuilds when this moves.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_model_generation(core: *mut DuckCore) -> c_ulonglong {
    if core.is_null() {
        0
    } else {
        (*core).model_generation
    }
}

/// Switch drive bundles: `"walk"` or `"roller"`. 0 on success.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_set_mode(core: *mut DuckCore, mode: *const c_char) -> c_int {
    if core.is_null() {
        return -1;
    }
    let Ok(name) = c_string(mode) else {
        return -1;
    };
    match switch_mode(&mut *core, &name) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_start_intent_server(
    core: *mut DuckCore,
    port: c_int,
) -> c_int {
    if core.is_null() || port <= 0 || port > 65535 {
        return -1;
    }
    let core = &mut *core;
    if core.server.is_some() {
        return 0;
    }
    match crate::intent::start(port as u16, core.intents.clone()) {
        Ok(server) => {
            core.server = Some(server);
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

/// The verbs a client may ask for. `robotd`'s list is the manifest's
/// one-shots; `sit`, `stand` and `ground_pick` are the loop's own verbs, and a
/// client cannot discover what `/skills` never names. `/skill` accepts all
/// three, so the list and the route agree.
fn published_skills(names: Vec<String>) -> Vec<String> {
    // `robotd::queue_skill` resolves built-ins first, then the manifest's
    // one-shots; the emotion timelines come from the release library, last.
    // Keep `/skills` in that order so a client reading the list sees the
    // same priority the scheduler uses.
    let mut out = Vec::with_capacity(names.len() + 3);
    for builtin in ["sit", "stand", "ground_pick"] {
        out.push(builtin.to_owned());
    }
    for name in names {
        if !out.iter().any(|seen| seen == &name) {
            out.push(name);
        }
    }
    out
}

/// The scene for a mode: the roller has its own feet, so it has its own MJCF
/// beside the walking one. The physics ground and the trunk start pose are the
/// same; only the model does.
fn scene_for(core: &DuckCore, mode: robotd_params::Mode) -> Option<String> {
    let path = Path::new(&core.scene);
    match mode {
        robotd_params::Mode::Walk => Some(core.scene.clone()),
        robotd_params::Mode::Roller => {
            let dir = path.parent()?;
            let roller = dir.join("scene_rollers.xml");
            roller.exists().then(|| roller.to_string_lossy().into_owned())
        }
    }
}

/// Switch the whole bundle, the way `robotd`'s mode line does.
///
/// Not a filter: roller loads `roller.onnx` as the locomotion network, drops
/// the standing network, drives the crouch where the walking set has the
/// ground pick, and keeps the same skills and sit/stand. The duck has to be
/// standing still and idle — a mode switch while a move is in flight would
/// rebuild the controller under it.
fn switch_mode(core: &mut DuckCore, name: &str) -> Result<serde_json::Value, String> {
    let mode = match name {
        "walk" => robotd_params::Mode::Walk,
        "roller" => robotd_params::Mode::Roller,
        other => return Err(format!("no such mode: {other}")),
    };
    if core.runtime.mode() == mode {
        return Ok(json!({ "ok": true, "mode": name, "already": true }));
    }
    if core.runtime.busy() || core.runtime.is_sitting() {
        return Err("a move is running; the duck must be standing still".to_owned());
    }
    if core.runtime.shutting_down() {
        // A new bundle would arrive powered on, under a duck that was
        // deliberately switched off (or is mid power-off). Upstream refuses
        // the same way.
        return Err("the duck is powering off; power it on first".to_owned());
    }
    let scene = scene_for(core, mode)
        .ok_or_else(|| format!("no scene for {name} beside {}", core.scene))?;
    // Reload the set the duck is actually running — a /reload may have
    // replaced the startup directory with a user's own policy set.
    let bundle = crate::manifest::load_mode(&core.policies, mode)?;
    let mut policy = Policy::load(&bundle.paths, 0.05).map_err(|error| error.to_string())?;
    if bundle.standing_disabled {
        // No standing network in the roller bundle: command magnitude must
        // not select one.
        policy.set_standing_disabled(true);
    }
    let body = MujocoIo::load(&scene)?;
    let mut runtime = DuckRuntime::new(
        body,
        policy,
        bundle.tuning(),
        bundle.skill_tuning,
        SafetyConfig::default(),
    );
    // Carry the clock: a frozen or slowed duck stays frozen or slowed across
    // the switch, and the cue must not be an unpause.
    let clock = core.runtime.clock();
    runtime.set_paused(clock.paused);
    runtime.set_speed(clock.speed);
    runtime.enable().map_err(|error| error.to_string())?;
    runtime.set_mode(mode);
    // `enable` queues the boot greet; a mode switch is the robot's cue instead:
    // one chirp for walking, two for roller (robotd's own).
    runtime.clear_sounds();
    runtime.queue_sound("chirp");
    if mode == robotd_params::Mode::Roller {
        runtime.queue_sound("chirp");
    }
    // The pickup classifier is part of the duck, not of the body: rebuild its
    // watch on the new runtime, or it silently disappears for the session.
    if let Some(model) = core.runtime.pickup_model().map(Path::to_path_buf) {
        let _ = runtime.load_pickup(&model);
    }

    // The emotion timelines are part of the duck, not of the body: the fresh
    // runtime above was built without them, so re-attach the same official
    // data or `robot.do curious` vanishes after a mode switch.
    if let Some(dir) = &core.emotions_dir {
        let _ = runtime.load_emotions(dir);
    }

    // The companion layer is the same animal across bodies: hunger, memory,
    // the child's rules and the last heard words move to the new runtime.
    runtime.carry_companion(&core.runtime);
    // A painted route belongs to the body it was drawn against: the switch
    // takes the duck somewhere those world waypoints never were.
    core.runtime.cancel_path();
    // A different body is a different duck: the old timeline and branches
    // describe a robot that no longer exists.
    core.runtime = runtime;
    core.model_generation = core.model_generation.wrapping_add(1);
    core.timeline.clear();
    core.branches.clear();
    core.selected = 0;
    core.pending_restore = None;
    core.last = None;
    core.sensors = crate::sensors::Sensors::default();
    let names = published_skills(core.runtime.skill_names());
    core.skills = names
        .iter()
        .map(|skill| CString::new(skill.clone()).unwrap_or_default())
        .collect();
    core.intents.set_skills(names);
    // The gift route's official-name list follows the body: a switch may
    // re-attach the emotions, and a switch without them must clear the old
    // names rather than leave a mood the new duck cannot play.
    core.intents.set_emotions(core.runtime.emotion_names());
    // The roller's meshes are its own; the debug name table has to move too.
    core.meshes = (0..core.runtime.io().mesh_count())
        .map(|mesh| {
            CString::new(core.runtime.io().mesh_name(mesh).unwrap_or_default()).unwrap_or_default()
        })
        .collect();
    core.label = format!("mode: {name}");
    Ok(json!({ "ok": true, "mode": name }))
}

fn reload_policies(core: &mut DuckCore, dir: &str) -> Result<(), String> {
    // Reload the set this duck is actually running, not always the walking one.
    let bundle = crate::manifest::load_mode(Path::new(dir), core.runtime.mode())?;
    let mut policy = Policy::load(&bundle.paths, 0.05).map_err(|error| error.to_string())?;
    if bundle.standing_disabled {
        policy.set_standing_disabled(true);
    }
    let tuning = bundle.tuning();
    core.runtime
        .replace_policy(policy, tuning, bundle.skill_tuning);
    let names = published_skills(core.runtime.skill_names());
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
    let selected = core.selected;
    let checkpoint = if selected > 0 && selected <= core.branches.len() {
        std::mem::swap(
            &mut core.runtime,
            &mut core.branches[selected - 1].runtime,
        );
        let checkpoint = core.runtime.checkpoint();
        std::mem::swap(
            &mut core.runtime,
            &mut core.branches[selected - 1].runtime,
        );
        checkpoint
    } else {
        core.runtime.checkpoint()
    };
    let Some(checkpoint) = checkpoint else {
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

/// Record a refusal where the shell and the next `/state` can see it, and
/// answer the client that asked. A refusal that lives only for the tick that
/// produced it is invisible to both.
fn refuse(core: &mut DuckCore, message: String) -> serde_json::Value {
    core.label = message.clone();
    core.notice = Some((core.runtime.clock().tick, message.clone()));
    json!({ "ok": false, "error": message })
}

/// Dispatch one intent against the duck the client is actually watching.
///
/// A selected branch lives in `core.branches`; the mutating arms all speak
/// `core.runtime` because that was true before forks existed. Rather than
/// duplicating every arm, swap the selected runtime into `core.runtime` for
/// the duration of the dispatch and swap it back afterwards. Branch-management
/// and mode intents act on the collection itself and are exempt.
fn apply_intent(core: &mut DuckCore, intent: Intent) -> serde_json::Value {
    let global = matches!(
        &intent,
        Intent::Mode(_)
            | Intent::Fork(_)
            | Intent::DropBranch(_)
            | Intent::SelectBranch(_)
            | Intent::QuestDay(_, _)
    );
    let selected = core.selected;
    if global || selected == 0 || selected > core.branches.len() {
        return apply_intent_active(core, intent);
    }
    std::mem::swap(
        &mut core.runtime,
        &mut core.branches[selected - 1].runtime,
    );
    let result = apply_intent_active(core, intent);
    std::mem::swap(
        &mut core.runtime,
        &mut core.branches[selected - 1].runtime,
    );
    result
}

fn apply_intent_active(core: &mut DuckCore, intent: Intent) -> serde_json::Value {
    match intent {
        Intent::Twist(twist) => {
            core.runtime.set_api_twist(twist);
            json!({ "ok": true, "twist": twist })
        }
        Intent::Path(points) => {
            // The route layer already enforced the caps and thinned the
            // points; the runtime freezes what is left into the world frame
            // at the pose the walk actually starts from.
            core.runtime.start_path(points);
            json!({ "ok": true, "queued": true })
        }
        Intent::PathCancel => {
            core.runtime.cancel_path();
            json!({ "ok": true })
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
        Intent::Skill(name) if core.runtime.shutting_down() => {
            // A skill during the shutdown sit would stand the duck up out of
            // the seat the power-off is counting on. Upstream refuses the
            // same way (a skill needs a driving robot that is not shutting
            // down).
            refuse(core, "the duck is powering off".to_owned())
        }
        Intent::Sit if core.runtime.shutting_down() => {
            refuse(core, "the duck is powering off".to_owned())
        }
        Intent::GroundPick if core.runtime.shutting_down() => {
            refuse(core, "the duck is powering off".to_owned())
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
        Intent::Shutdown => {
            core.runtime.request_shutdown();
            json!({ "ok": true, "pending": true, "shutdown": true })
        }
        Intent::PowerOn => match core.runtime.power_on() {
            Ok(()) => json!({ "ok": true, "powered_off": false }),
            Err(error) => refuse(core, format!("power-on refused: {error}")),
        },
        Intent::Mode(name) => match switch_mode(core, &name) {
            Ok(value) => value,
            Err(reason) => refuse(core, format!("mode refused: {reason}")),
        },
        Intent::Sound(tag) => {
            // Emotions are official voices too, loaded at runtime, so
            // `queue_sound` is the authority on what is a valid tag.
            let known = crate::voice::TAGS.contains(&tag.as_str())
                || tag.strip_prefix("emotion:").is_some_and(|name| {
                    core.runtime.emotion_names().iter().any(|loaded| loaded == name)
                });
            if known {
                core.runtime.queue_sound(&tag);
                json!({ "ok": true, "tag": tag })
            } else {
                refuse(core, format!("no such sound: {tag}"))
            }
        }
        Intent::Wheee(hold) => {
            core.runtime.wheee(hold);
            json!({ "ok": true, "held": hold })
        }
        Intent::IdleRitual(seconds) => {
            core.runtime.set_idle_ritual(seconds);
            json!({ "ok": true, "idle_ritual_s": seconds })
        }
        Intent::RestoreLife(saved) => {
            // The shell saved the flat /life summary, not the Book's nested
            // shape: rebuild it field by field rather than silently dropping
            // an unmatched deserialization to an empty history.
            let book = core.runtime.book_mut();
            let life = &mut book.life;
            let num = |key: &str| -> Option<f64> { saved.get(key).and_then(|v| v.as_f64()) };
            if let Some(v) = num("powered_s") { life.powered_s = v; }
            if let Some(v) = num("torqued_s") { life.torqued_s = v; }
            if let Some(v) = num("walking_s") { life.walking_s = v; }
            if let Some(v) = num("standing_s") { life.standing_s = v; }
            if let Some(v) = num("sitting_s") { life.sitting_s = v; }
            if let Some(v) = num("held_s") { life.held_s = v; }
            if let Some(v) = num("boots") { life.boots = v as u64 + 1; }
            if let Some(v) = num("falls") { life.falls = v as u64; }
            if let Some(v) = num("pickups") { life.pickups = v as u64; }
            if let Some(v) = num("ground_picks") { life.ground_picks = v as u64; }
            if let Some(v) = num("sit_downs") { life.sit_downs = v as u64; }
            if let Some(v) = num("stand_ups") { life.stand_ups = v as u64; }
            if let Some(v) = num("wake_ups") { life.wake_ups = v as u64; }
            if let Some(v) = num("pets") { life.pets = v as u64; }
            if let Some(v) = num("fed") { life.fed = v as u64; }
            if let Some(v) = num("claps") { life.claps = v as u64; }
            if let Some(v) = num("whistles") { life.whistles = v as u64; }
            if let Some(v) = num("metres") { life.metres = v; }
            if let Some(v) = num("power_offs") { life.power_offs = v as u64; }
            if let Some(m) = saved.get("skills").and_then(|v| v.as_object()) {
                life.skills = m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_u64()?))).collect();
            }
            if let Some(m) = saved.get("emotions").and_then(|v| v.as_object()) {
                life.emotions = m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_u64()?))).collect();
            }
            if let Some(a) = saved.get("events").and_then(|v| v.as_array()) {
                // Only the words the core itself writes. The restore route
                // answers on the network, and the events are what the diary
                // shows and what the model is told — a client could not put
                // its own sentences into the duck's history.
                book.events = a
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter(|event| crate::vetbook::is_known_event(event))
                    .map(str::to_owned)
                    .collect();
            }
            core.intents.set_life(core.runtime.book());
            json!({ "ok": true })
        }
        Intent::Pet => {
            core.runtime.pet();
            json!({
                "ok": true,
                "pets": core.runtime.book().life.pets,
                "care": core.runtime.care_summary(),
            })
        }
        Intent::Feed => match core.runtime.feed() {
            Ok(true) => {
                core.intents.set_life(core.runtime.book());
                json!({
                    "ok": true,
                    "fed": core.runtime.book().life.fed,
                    "care": core.runtime.care_summary(),
                })
            }
            Ok(false) => refuse(core, "the duck is already pecking".to_owned()),
            Err(reason) => refuse(core, reason.to_owned()),
        },
        Intent::RestoreCare(saved) => {
            core.runtime.restore_care(&saved);
            json!({ "ok": true, "care": core.runtime.care_summary() })
        }
        // The night is the care layer's own sleep, not a power state: the
        // loop keeps running and only the doze overlay owns the duck.
        Intent::Night(true) => {
            core.runtime.enter_night();
            json!({ "ok": true, "queued": true })
        }
        Intent::Night(false) => match core.runtime.end_night() {
            Some(quiet_s) => json!({ "ok": true, "queued": true, "quiet_s": quiet_s }),
            // Not in night: waking is a no-op, and the answer says so with
            // the zero span rather than a fake "queued".
            None => json!({ "ok": true, "quiet_s": 0.0 }),
        },
        Intent::CareScale(scale) => {
            core.runtime.set_care_scale(scale);
            json!({ "ok": true, "care_time_scale": core.runtime.care_scale() })
        }
        Intent::SetRules(rules) => match core.runtime.set_rules(&rules) {
            Ok(()) => json!({ "ok": true, "rules": core.runtime.rules_summary() }),
            Err(reason) => refuse(core, format!("rules refused: {reason}")),
        },
        Intent::Posture(sit) => {
            if core.runtime.is_sitting() == sit {
                json!({ "ok": true, "sitting": sit, "unchanged": true })
            } else {
                match core.runtime.sit_toggle() {
                    Ok(_) => json!({ "ok": true, "sitting": sit }),
                    Err(reason) => refuse(core, format!("posture refused: {reason}")),
                }
            }
        }
        Intent::QuestDay(day, board) => {
            if day.len() == 10 {
                // Rehydrating a saved board measures it against the duck the
                // client is watching, which may be a selected branch.
                let values = json!({
                    "care": active_runtime(core).care_summary(),
                    "life": active_runtime(core).book().summary(),
                });
                core.quests.restore(&board, &values);
                core.intents.set_day(day.clone());
                json!({ "ok": true, "day": day })
            } else {
                refuse(core, "quest day must be yyyy-mm-dd".to_owned())
            }
        }
        Intent::GetRules => json!({ "ok": true, "rules": core.runtime.rules_summary() }),
        Intent::GreetPeer(target) => match &core.lan {
            Some(lan) => {
                lan.greet(target.as_deref());
                json!({ "ok": true, "greeted": target })
            }
            None => refuse(core, "the LAN peer port is not available".to_owned()),
        },
        Intent::SendGift { id, gift, value } => {
            // The route checked the published list; the runtime is the
            // authority on which official emotions this build loaded (a mode
            // switch or a fresh emotion directory could move it).
            if gift == GiftKind::Emotion
                && !core
                    .runtime
                    .emotion_names()
                    .iter()
                    .any(|loaded| loaded == &value)
            {
                return refuse(core, "no such emotion".to_owned());
            }
            let sent = core.lan.as_ref().map(|lan| lan.gift(&id, gift, &value));
            match sent {
                Some(Ok(())) => json!({ "ok": true, "queued": true }),
                Some(Err(error)) => refuse(core, format!("gift refused: {error}")),
                None => refuse(core, "the LAN peer port is not available".to_owned()),
            }
        }
        Intent::Chorus(moves) => {
            if active_runtime(core).chorus_active() {
                return refuse(core, "a chorus is already playing".to_owned());
            }
            // The route checked the published list; the runtime is the
            // authority on which official emotions this build loaded.
            if !moves.iter().all(|name| {
                active_runtime(core)
                    .emotion_names()
                    .iter()
                    .any(|loaded| loaded == name)
            }) {
                return refuse(core, "no such emotion".to_owned());
            }
            let id = format!("chorus-{}", active_runtime(core).clock().tick);
            let sent = core.lan.as_ref().map(|lan| lan.chorus(&id, &moves));
            match sent {
                Some(Ok(())) => {
                    let peers = core.lan.as_ref().map_or(0, crate::lan::Lan::peer_count);
                    on_active(core, |runtime| {
                        runtime.start_chorus("lead", moves.clone(), peers, Some(id))
                    });
                    json!({ "ok": true, "queued": true, "moves": moves })
                }
                Some(Err(error)) => refuse(core, format!("chorus refused: {error}")),
                None => refuse(core, "the LAN peer port is not available".to_owned()),
            }
        }
        Intent::PeerName(name) => match &core.lan {
            Some(lan) => {
                lan.set_name(&name);
                json!({ "ok": true, "peers": lan.snapshot() })
            }
            None => refuse(core, "the LAN peer port is not available".to_owned()),
        },
        Intent::StartLan(name) => {
            if core.lan.is_none() {
                core.lan = crate::lan::Lan::start(&name);
                // The wire refuses an emotion gift for a mood this build
                // does not have. The emotion list joined the core before
                // the LAN socket existed, so hand it over here too.
                if let Some(lan) = &core.lan {
                    lan.set_emotions(core.runtime.emotion_names());
                }
            }
            match &core.lan {
                Some(lan) => {
                    lan.set_name(&name);
                    json!({ "ok": true, "peers": lan.snapshot() })
                }
                None => refuse(core, "the LAN peer port is not available".to_owned()),
            }
        }
        Intent::Look { target, neck_pitch } => {
            let gaze = core.runtime.look_at(target, neck_pitch);
            json!({ "ok": true, "head": gaze.joints, "clamped": gaze.clamped })
        }
        Intent::Heard(text) => {
            core.runtime.heard(&text);
            let Some((phrase, heard)) = crate::speech::interpret(&text) else {
                // Silence is not an error the client should have to parse:
                // say what was heard and that nothing matched.
                return json!({ "ok": false, "heard": text, "reason": "no verb matched" });
            };
            let result = match heard {
                crate::speech::Heard::Skill(name) => core
                    .runtime
                    .trigger_skill(name)
                    .map(|started| json!({ "ok": true, "heard": text, "phrase": phrase, "skill": name, "chained": !started }))
                    .map_err(str::to_owned),
                crate::speech::Heard::Sound(tag) => {
                    core.runtime.queue_sound(tag);
                    Ok(json!({ "ok": true, "heard": text, "phrase": phrase, "sound": tag }))
                }
                crate::speech::Heard::Sit => {
                    if core.runtime.is_sitting() {
                        Ok(json!({ "ok": true, "heard": text, "phrase": phrase, "skill": "sit", "did": false }))
                    } else {
                        core.runtime
                            .sit_toggle()
                            .map(|did| json!({ "ok": true, "heard": text, "phrase": phrase, "skill": "sit", "did": did }))
                            .map_err(str::to_owned)
                    }
                }
                crate::speech::Heard::Stand => {
                    if core.runtime.is_sitting() {
                        core.runtime
                            .sit_toggle()
                            .map(|did| json!({ "ok": true, "heard": text, "phrase": phrase, "skill": "stand", "did": did }))
                            .map_err(str::to_owned)
                    } else {
                        Ok(json!({ "ok": true, "heard": text, "phrase": phrase, "skill": "stand", "did": false }))
                    }
                }
                crate::speech::Heard::GroundPick => core
                    .runtime
                    .start_ground_pick()
                    .map(|()| json!({ "ok": true, "heard": text, "phrase": phrase, "skill": "ground_pick" }))
                    .map_err(str::to_owned),
            };
            match result {
                Ok(value) => value,
                Err(error) => refuse(core, error),
            }
        }
        Intent::Flashlight { toggle, on, color } => {
            let face = core.runtime.set_flashlight(toggle, on, &color);
            publish_state(core);
            json!({
                "ok": true,
                "flashlight": face.flashlight,
                "color": face.color(),
            })
        }
        Intent::Mouth(open) => match core.runtime.set_mouth(open) {
            Ok(()) => json!({ "ok": true, "open": open }),
            Err(reason) => refuse(core, format!("beak refused: {reason}")),
        },
        Intent::Push(velocity) => {
            let _ = core.runtime.push(velocity);
            json!({ "ok": true })
        }
        Intent::Carry(offset, active) => {
            core.runtime.set_carry(offset, active);
            json!({ "ok": true, "carrying": active })
        }
        Intent::Reload(dir) if core.runtime.shutting_down() => {
            // A new network under a duck that is on its way down is the same
            // mistake as a mode switch: refused.
            refuse(core, "the duck is powering off".to_owned())
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
                "microphone" => {
                    core.sensors.microphone = Some(reading);
                    core.sound_sequence = core.sound_sequence.wrapping_add(1);
                }
                "depth" => core.sensors.depth = Some(reading),
                _ => {}
            }
            json!({ "ok": true })
        }
        Intent::Fork(label) => {
            // Every branch is stepped every tick: cap the collection.
            if core.branches.len() >= MAX_BRANCHES {
                refuse(core, format!("fork refused: at most {MAX_BRANCHES} branches"))
            } else {
                match core.runtime.fork() {
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
                }
            }
        }
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

fn bringup_code(state: BringupState) -> c_int {
    match state {
        BringupState::Limp => 0,
        BringupState::Homing => 1,
        BringupState::Ready => 2,
    }
}

/// The runtime the tick is actually driving: the selected branch, or the
/// original. Every reader — `/state`, `DuckStatus`, render poses — must use
/// this, or a selected branch drives a duck nobody can see.
fn active_runtime(core: &DuckCore) -> &DuckRuntime<MujocoIo> {
    if core.selected == 0 {
        &core.runtime
    } else {
        core.branches
            .get(core.selected - 1)
            .map_or(&core.runtime, |branch| &branch.runtime)
    }
}

/// Run one runtime-only operation on the duck the client is watching,
/// swapping a selected branch into `core.runtime` for the duration.
fn on_active(core: &mut DuckCore, f: impl FnOnce(&mut DuckRuntime<MujocoIo>)) {
    let selected = core.selected;
    if selected > 0 && selected <= core.branches.len() {
        std::mem::swap(
            &mut core.runtime,
            &mut core.branches[selected - 1].runtime,
        );
        f(&mut core.runtime);
        std::mem::swap(
            &mut core.runtime,
            &mut core.branches[selected - 1].runtime,
        );
    } else {
        f(&mut core.runtime);
    }
}

/// One gift another duck sent, through the same runtime entries a person's
/// treat or mood takes: `gift_treat` for a treat, `gift_emotion` for a mood.
///
/// The gift is not a hand. It feeds the duck and it can cheer it, but it
/// does not end the night, it is not written as a touch, and it does not
/// raise the bond — that belongs to the person holding the phone.
///
/// The emotion NAME is validated here rather than in lan.rs: the peer module
/// has no runtime and cannot know which official emotions this build loaded.
/// An unknown name drops the whole gift — no event, no book note, nothing
/// invented — exactly as the wire contract says.
fn apply_gift(core: &mut DuckCore, from: String, name: String, gift: GiftKind, value: String) {
    let value = if gift == GiftKind::Emotion {
        value
    } else {
        String::new()
    };
    if gift == GiftKind::Emotion
        && !active_runtime(core)
            .emotion_names()
            .iter()
            .any(|loaded| loaded == &value)
    {
        return;
    }
    let mut ok = false;
    on_active(core, |runtime| {
        // Each gift writes its own single line in the book: `gift:treat` or
        // `gift:emotion:<name>`. The counter it moves is the duck's own.
        ok = match gift {
            GiftKind::Treat => matches!(runtime.gift_treat(), Ok(true)),
            GiftKind::Emotion => {
                let played = runtime.gift_emotion(&value);
                if played.is_ok() {
                    runtime
                        .book_mut()
                        .note(format!("gift:emotion:{value}"));
                }
                played.is_ok()
            }
        };
    });
    // The tick the gift landed on, on the duck the client is watching. The
    // shells compare it to avoid showing the same pill twice.
    let tick = active_runtime(core).clock().tick as u32;
    core.intents.set_gift_event(json!({
        "kind": gift.as_str(),
        "value": value,
        "from": from,
        "from_name": name,
        "tick": tick,
        "ok": ok,
    }));
}

fn publish_state(core: &DuckCore) {
    let active = active_runtime(core);
    let trunk = active.io().trunk();
    let gravity = active.io().gravity();
    let clock = active.clock();
    // The route refuses a second chorus without waiting a tick, so the
    // published summary is mirrored onto the queue here.
    let chorus = active.chorus_summary();
    core.intents.set_chorus(chorus.clone());
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
    // The duck's own senses, not the shell's: measured joint positions and
    // velocities (encoder side of the gear play, as the policy sees them),
    // currents, the IMU, and what the loop is commanding. A client that cannot
    // read these cannot tell a duck standing from a duck being held.
    let proprioception = match core.last.as_ref() {
        Some(report) => json!({
            "names": duck_control::model::JOINT_NAMES,
            "positions": report.sensors.positions,
            "velocities": report.sensors.velocities,
            "currents_ma": report.sensors.currents_ma,
            "targets": report.targets,
            "imu": {
                "gyro": report.sensors.imu.gyro,
                "gravity": report.sensors.imu.gravity,
                "quat": report.sensors.imu.quat,
            },
            "imu_ready": report.imu_ready,
            "picked_up": report.picked_up,
            "p_held": report.p_held,
            "mouth": report.mouth,
            "has_mouth": core.runtime.has_mouth(),
            "volts": report.volts,
            "battery_percent": report.volts.map(duck_control::battery_percent),
        }),
        // The same shape before the first tick as after it: a client that
        // parses the state should not have to care whether a tick has run yet.
        None => json!({
            "names": duck_control::model::JOINT_NAMES,
            // `json!` has no repeat syntax; `vec!` is the same array.
            "positions": vec![0.0; duck_control::model::NUM_JOINTS],
            "velocities": vec![0.0; duck_control::model::NUM_JOINTS],
            "currents_ma": vec![0.0; duck_control::model::NUM_JOINTS],
            "targets": vec![0.0; duck_control::model::NUM_JOINTS],
            "imu": {
                "gyro": [0.0, 0.0, 0.0],
                "gravity": [0.0, 0.0, -1.0],
                "quat": [1.0, 0.0, 0.0, 0.0],
            },
            "imu_ready": false,
            "picked_up": false,
            "p_held": null,
            "mouth": 0.0,
            "has_mouth": false,
            "volts": null,
            "battery_percent": null,
        }),
    };
    core.intents.set_state(json!({
        "label": core.label,
        "bringup": bringup_code(active.bringup()),
        "fallen": active.fallen(),
        "gain": core.last.as_ref().map_or(200, |report| report.gain),
        "trunk": trunk,
        "gravity": gravity,
        "tick": clock.tick,
        "paused": clock.paused,
        "speed": clock.speed,
        "mode": active.mode().as_str(),
        // The duck's own estimate of where it is, NOT the simulator's ground
        // truth: it comes from the same legs and IMU the physical duck has.
        "pose": {
            "position": active.position(),
            "yaw": active.yaw(),
            "anchor_xy": active.anchor().0,
            "anchor_foot": active.anchor().1,
        },
        // The painted route, on every publish: what was accepted and how far
        // the walk got, or the inactive shape when nothing is walking.
        "path": active.path_summary(),
        "powered_off": active.powered_off(),
        "carrying": active.carrying(),
        "last_sound": active.last_sound(),
        "last_emotion_sound": active.last_emotion_sound(),
        "last_ride_segment": active.last_ride_segment(),
        // The last room sound the duck answered, with the tick it happened.
        // The microphone itself is never part of the observation.
        "sound_event": active.last_sound_event().map(|(kind, tick)| {
            json!({ "kind": kind, "tick": tick })
        }),
        // The last programmed block that fired, so the child can see their
        // rule run even when the action's own label is gone.
        "rule_event": active.last_rule(),
        // Other ducks on the LAN, and the last greeting or reply.
        "peers": core.intents.peers_value(),
        "peer_event": active.last_peer_event(),
        // The last gift another duck sent, for the shells' one-line pill;
        // null until one arrives this session. The receiver's window and the
        // packet gate live in lan.rs.
        "gift_event": core.intents.gift_event_value(),
        // The room's chorus: the same moves performed together, and the
        // honest count of ducks it is shared with.
        "chorus": chorus,
        // The last words a person said, for a user-supplied model. The audio
        // itself never leaves the phone; only these words are published.
        "last_heard": active.last_heard().map(|(text, tick)| {
            json!({ "text": text, "tick": tick })
        }),
        // Today's three quests, from the same vet book counters.
        "quests": serde_json::from_str::<serde_json::Value>(&core.intents.quests_json())
            .unwrap_or(serde_json::json!({})),
        "life": active.book().summary(),
        // Hunger, mood and bond: the companion layer's three numbers, plus
        // whether the duck has dozed off. `asleep` is a pose overlay, not a
        // power state — the control loop is running either way.
        "care": active.care_summary(),
        "care_time_scale": active.care_scale(),
        // The night the person put the duck to bed for, if any. `care.night`
        // is what a save carries; this is what a shell draws, with the quiet
        // span derived from the tick delta on every publish.
        "night": active.night_summary(),
        "leds": {
            "status": active.face().status,
            "gauge": active.face().gauge,
            "battery_percent": active.face().battery_percent,
            "bars": active.face().bars(),
            "flashlight": active.face().flashlight,
            "color": active.face().color(),
        },
        "emotion": active.emotion_state().map(|(name, elapsed, duration)| {
            json!({ "name": name, "elapsed_s": elapsed, "duration_s": duration })
        }),
        // How much of the head the idle look-around owns this tick, and the
        // head command it produced. A duck that reads as "off" publishes 0
        // forever; CI asserts a still duck really does glance.
        "idle": {
            "takeover": active.idle_takeover(),
            "head": active.idle_head_command(),
        },
        "performance": {
            "fps": core.performance.0,
            "ticks_per_second": core.performance.1,
        },
        "notice": core
            .notice
            .as_ref()
            .filter(|(tick, _)| clock.tick.saturating_sub(*tick) < 100)
            .map(|(_, message)| message.clone()),
        "proprioception": proprioception,
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
    // Daily quests: use the shell's calendar day and this tick's real state.
    // A quest completing is a real event; the duck answers with one of its
    // own official emotions rather than a fake notification.
    let quest_day = core.intents.day();
    if quest_day.len() == 10 {
        // Measure the duck the client is driving: a selected branch's own
        // counters, not the original the branch forked from.
        let values = json!({
            "care": active_runtime(core).care_summary(),
            "life": active_runtime(core).book().summary(),
        });
        let finished = core.quests.update(&quest_day, &values);
        if !finished.is_empty() {
            on_active(core, |runtime| {
                let _ = runtime.celebrate_emotion("excited");
            });
        }
        core.intents.set_quests(core.quests.summary(&values));
    }
    // Other ducks on the LAN: greetings are social events, and the peer list
    // is published with the rest of the state.
    let (peer_events, peer_snapshot) = match &core.lan {
        Some(lan) => (lan.take_events(), Some(lan.snapshot())),
        None => (Vec::new(), None),
    };
    if let Some(snapshot) = peer_snapshot {
        core.intents.set_peers(snapshot);
    }
    let mut gifts = Vec::new();
    let mut choruses = Vec::new();
    for event in peer_events {
        match event {
            // A gift is not a greeting: collect it and apply it after the
            // greetings, so a hello's own reaction cannot be mistaken for the
            // gift's (or vice versa).
            crate::lan::PeerEvent::Gift {
                from,
                name,
                gift,
                value,
            } => gifts.push((from, name, gift, value)),
            // A chorus is a room-wide performance, applied after the gifts;
            // the runtime dedupes by id and the one-at-a-time rule.
            crate::lan::PeerEvent::Chorus { id, moves, .. } => {
                choruses.push((id, moves))
            }
            event => on_active(core, |runtime| runtime.peer_event(&event)),
        }
    }
    // Gifts feed the duck or play one of its official emotions, with the same
    // verdicts the API path gives.
    for (from, name, gift, value) in gifts {
        apply_gift(core, from, name, gift, value);
    }
    for (id, moves) in choruses {
        if active_runtime(core).chorus_active() {
            // One song at a time: a second lead heard mid-chorus is noise.
            continue;
        }
        let peers = core.lan.as_ref().map_or(0, crate::lan::Lan::peer_count);
        on_active(core, |runtime| {
            runtime.start_chorus("join", moves, peers, Some(id))
        });
    }
    for queued in core.intents.drain() {
        let result = apply_intent(core, queued.intent);
        // The client that sent a one-shot is waiting on this; a stream may not
        // be waiting at all, and the queue keeps the answer bounded.
        core.intents.answer(queued.id, result);
    }
    // A sudden loud sound in the room is an external event, never a sense.
    // The sensor registry is on DuckCore; hand its verdict to the loop.
    let loud = core.sensors.loud();
    on_active(core, |runtime| runtime.set_room_loud(loud));
    // The phone's IMU is an external disturbance, not a sense: a shake is a
    // shove, exactly as a hand on the real duck would be. It is one impulse —
    // the reading is consumed here, or a single shake would shove the duck
    // every tick until the next sensor post.
    if let Some(push) = core.sensors.take_imu_push() {
        on_active(core, |runtime| {
            let _ = runtime.push(push);
        });
    }
    // A restore replaces the tick that was about to happen: the duck resumes
    // at the saved tick and this tick's physics runs from there. It belongs to
    // the duck the client sees, which may be a selected branch.
    if let Some(checkpoint) = core.pending_restore.take() {
        let selected = core.selected;
        if selected > 0 && selected <= core.branches.len() {
            std::mem::swap(
                &mut core.runtime,
                &mut core.branches[selected - 1].runtime,
            );
            if !core.runtime.restore(&checkpoint) {
                core.label = "restore refused".to_owned();
            }
            std::mem::swap(
                &mut core.runtime,
                &mut core.branches[selected - 1].runtime,
            );
        } else if !core.runtime.restore(&checkpoint) {
            core.label = "restore refused".to_owned();
        }
        core.timeline.retain(|saved| saved.tick <= checkpoint.tick);
    }
    // Frozen time freezes the duck the client is watching, including a branch.
    let active_paused = active_runtime(core).clock().paused;
    if active_paused {
        publish_state(core);
        return 0;
    }
    // The same microphone channel, classified into a clap or a whistle so a
    // kid's rule can listen for one. Frozen time hears nothing: this runs
    // after the pause gate, so a paused duck does not act on a sound.
    let now_ms = active_runtime(core).clock().tick.saturating_mul(20);
    if let Some(kind) = core.sound_watch.observe(
        core.sensors.microphone.as_ref(),
        now_ms,
        core.sound_sequence,
    ) {
        on_active(core, |runtime| runtime.sound_event(kind));
    }
    // Record the state the tick is about to run from, so a rewind lands on
    // the tick the watched duck (original or selected branch) really ran.
    if core
        .timeline
        .back()
        .is_none_or(|last| last.tick < active_runtime(core).clock().tick)
    {
        checkpoint_now(core);
    }
    // Every branch steps too, on its own body and its own state — except the
    // selected one, which is stepped once below. Stepping it here as well ran
    // the driven duck at double rate (and doubled its odometry).
    for (index, branch) in core.branches.iter_mut().enumerate() {
        if core.selected == index + 1 {
            continue;
        }
        if !branch.runtime.clock().paused {
            let _ = branch.runtime.tick();
            branch.tick = branch.runtime.clock().tick;
        }
    }

    let report = if core.selected == 0 {
        core.runtime.tick()
    } else {
        match core.branches.get_mut(core.selected - 1) {
            Some(branch) => {
                let report = branch.runtime.tick();
                branch.tick = branch.runtime.clock().tick;
                report
            }
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
            // The lifetime record travels with the state: a client reading
            // /state sees the same counters /life serves, for the duck it sees.
            let book = active_runtime(core).book().clone();
            core.intents.set_life(&book);
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

/// Let go of the body pose. `set_body` starts holding a pose (`body_active`),
/// and while that flag is set the controller zeroes the twist — a duck left
/// holding nominal would stand still no matter what the stick said. This is
/// the release the shells need when a pose gesture ends; it is `BodyRelease`
/// on the intent API.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_release_body(core: *mut DuckCore) {
    if !core.is_null() {
        (*core).runtime.release_body();
    }
}

/// Load the pickup classifier — the same `pickup_detector.onnx` `robotd`
/// scores. Refuses loudly rather than silently walking without it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_load_pickup(
    core: *mut DuckCore,
    model_path: *const c_char,
) -> c_int {
    if core.is_null() {
        return -1;
    }
    let Ok(path) = c_string(model_path) else {
        return -1;
    };
    let core = &mut *core;
    match core
        .runtime
        .load_pickup(std::path::Path::new(&path))
    {
        Ok(()) => 0,
        Err(error) => {
            core.label = error;
            -1
        }
    }
}

/// A hand on the trunk: `offset` metres from where it was grabbed, world
/// frame. `active: 0` lets go. An external force, never a joint target.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_carry(
    core: *mut DuckCore,
    dx: c_double,
    dy: c_double,
    dz: c_double,
    active: c_int,
) {
    if !core.is_null() {
        (*core).runtime.set_carry([dx, dy, dz], active != 0);
    }
}

/// The next sound the duck wants played, as a tag the platform looks up.
/// `NULL` when nothing is queued. The pointer stays valid until the next
/// call to this function; the shell owns the speaker and the PCM.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_next_sound(core: *mut DuckCore) -> *const c_char {
    if core.is_null() {
        return std::ptr::null();
    }
    let core = &mut *core;
    let Some(tag) = core.runtime.take_sound() else {
        return std::ptr::null();
    };
    // An emotion's voice is the official wav, rendered through the same
    // `render_sound` cache as a synthesized chirp. The tag is dynamic, so it
    // cannot be one of six static C string literals: keep it in an owned slot
    // and hand out its pointer, exactly like `meshes` / `skills` do.
    core.sound_tag = CString::new(tag).ok();
    core.sound_tag
        .as_ref()
        .map_or(std::ptr::null(), |value| value.as_ptr())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_render_sound(
    core: *mut DuckCore,
    tag: *const c_char,
    out: *mut f32,
    capacity: c_int,
) -> c_int {
    if core.is_null() {
        return 0;
    }
    let Ok(tag) = c_string(tag) else {
        return 0;
    };
    let core = &mut *core;
    // Render once per tag and reuse it: `Voice::render` advances the variant,
    // so a second render is a different sound with a different length.
    let cached = core
        .rendered_sound
        .as_ref()
        .is_some_and(|(name, _)| *name == tag);
    if !cached {
        let Some(pcm) = core.runtime.render_sound(&tag) else {
            return 0;
        };
        core.rendered_sound = Some((tag.clone(), pcm));
    }
    let Some((_, pcm)) = core.rendered_sound.as_ref() else {
        return 0;
    };
    if !out.is_null() {
        if capacity < 0 || (capacity as usize) < pcm.len() {
            return 0;
        }
        std::ptr::copy_nonoverlapping(pcm.as_ptr(), out, pcm.len());
    }
    pcm.len() as c_int
}

/// Queue a sound for the platform to pick up with `duck_core_next_sound`.
/// The shell's quack button uses this so each press is a fresh variant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_queue_sound(core: *mut DuckCore, tag: *const c_char) -> c_int {
    if core.is_null() {
        return -1;
    }
    let Ok(tag) = c_string(tag) else {
        return -1;
    };
    let known = crate::voice::TAGS.contains(&tag.as_str())
        || tag.strip_prefix("emotion:").is_some_and(|name| {
            (*core).runtime.emotion_names().iter().any(|loaded| loaded == name)
        });
    if !known {
        return -1;
    }
    (*core).runtime.queue_sound(&tag);
    0
}

/// The voice's sample rate (48 kHz, the synth's native rate).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_voice_rate() -> c_int {
    crate::synth::SR as c_int
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
    // The shell's ring and `/skill` name the same verbs, so the C entry point
    // accepts them too: `sit` and `stand` are idempotent, `ground_pick` is the
    // loop's own, and anything else is a manifest skill.
    let core = &mut *core;
    let result = match name.as_str() {
        "sit" | "stand" => {
            let sitting = core.runtime.is_sitting();
            if (name == "sit") == sitting {
                Ok(true)
            } else {
                core.runtime.sit_toggle().map(|_| true)
            }
        }
        "ground_pick" => core.runtime.start_ground_pick().map(|()| true),
        _ => core.runtime.trigger_skill(&name),
    };
    match result {
        Ok(started) => i32::from(started),
        Err(error) => {
            core.label = format!("skill refused: {error}");
            0
        }
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
    let active = active_runtime(core);
    out.bringup = match active.bringup() {
        BringupState::Limp => 0,
        BringupState::Homing => 1,
        BringupState::Ready => 2,
    };
    out.fallen = active.fallen() as c_int;
    out.gain = core.last.as_ref().map_or(200, |report| report.gain as c_int);
    out.label = [0; 64];
    let label = core.label.as_bytes();
    let count = label.len().min(63);
    for (index, byte) in label.iter().take(count).enumerate() {
        out.label[index] = *byte as c_char;
    }
    out.trunk = active.io().trunk();
    out.gravity = active.io().gravity();
    let report = core.last.as_ref();
    out.imu_ready = active.imu_ready() as c_int;
    out.picked_up = active.picked_up() as c_int;
    out.p_held = report.and_then(|report| report.p_held).map_or(0.0, f64::from);
    out.mouth = report.map_or(0.0, |report| report.mouth);
    out.volts = report.and_then(|report| report.volts).unwrap_or(0.0);
    out.has_mouth = active.has_mouth() as c_int;
    out.fps = core.performance.0;
    out.ticks_per_second = core.performance.1;
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
    let qpos = active_runtime(&*core).io().render_qpos();
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
        active_runtime(&*core).io().body_count() as c_int
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
    let (position, rotation) = active_runtime(&*core).io().body_pose(body as usize);
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
        active_runtime(&*core).io().geom_count() as c_int
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

/// Time-machine controls for the shell. They take effect on the next tick,
/// on the duck the shell is watching (a selected branch, not the original).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_set_paused(core: *mut DuckCore, paused: c_int) {
    if !core.is_null() {
        on_active(&mut *core, |runtime| runtime.set_paused(paused != 0));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_set_speed(core: *mut DuckCore, speed: c_double) {
    if !core.is_null() {
        on_active(&mut *core, |runtime| runtime.set_speed(speed));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_step(core: *mut DuckCore, ticks: c_int) {
    if core.is_null() {
        return;
    }
    let core = &mut *core;
    on_active(core, |runtime| {
        let was_paused = runtime.clock().paused;
        runtime.set_paused(false);
        for _ in 0..ticks.clamp(1, 500) {
            let _ = runtime.tick();
        }
        runtime.set_paused(was_paused);
    });
    // The published state comes from the last report: without this the shell
    // would keep showing the pose from before the step.
    let mut report = None;
    on_active(core, |runtime| report = runtime.last_report());
    if let Some(report) = report {
        core.label = report.label.clone();
        core.last = Some(report);
        publish_state(core);
    }
}

/// Step back to a saved tick. Returns 0 when the rewind landed. The rewind
/// lands on the duck the shell is watching.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_seek(core: *mut DuckCore, back_ticks: c_int) -> c_int {
    if core.is_null() {
        return -1;
    }
    let core = &mut *core;
    let active_tick = active_runtime(core).clock().tick;
    let target = active_tick.saturating_sub(back_ticks.max(0) as u64);
    let Some(checkpoint) = core
        .timeline
        .iter()
        .rev()
        .find(|checkpoint| checkpoint.tick <= target)
        .cloned()
    else {
        return -1;
    };
    let mut landed = false;
    on_active(core, |runtime| {
        if runtime.restore(&checkpoint) {
            landed = true;
        }
    });
    if !landed {
        return -1;
    }
    core.timeline.retain(|saved| saved.tick <= checkpoint.tick);
    // The published state must follow the rewind: the duck is somewhere
    // else now, and the shell would keep showing where it was.
    let mut report = None;
    on_active(core, |runtime| report = runtime.last_report());
    if let Some(report) = report {
        core.label = report.label.clone();
        core.last = Some(report);
    }
    publish_state(core);
    0
}

/// Fork the duck at this instant. Returns 0 on success. A room only needs a
/// handful of parallel stories, and every branch is stepped every tick: cap
/// them so one client cannot grow the CPU without bound.
pub const MAX_BRANCHES: usize = 8;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_fork(core: *mut DuckCore, label: *const c_char) -> c_int {
    if core.is_null() {
        return -1;
    }
    let core = &mut *core;
    if core.branches.len() >= MAX_BRANCHES {
        return -1;
    }
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
/// Both read the duck the shell is watching.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_paused(core: *mut DuckCore) -> c_int {
    if core.is_null() {
        return 0;
    }
    active_runtime(&*core).clock().paused as c_int
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_speed(core: *mut DuckCore) -> c_double {
    if core.is_null() {
        return 1.0;
    }
    active_runtime(&*core).clock().speed
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
    active_runtime(&*core).clock().tick
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
            imu_ready: 0,
            picked_up: 0,
            p_held: 0.0,
            mouth: 0.0,
            volts: 0.0,
            has_mouth: 0,
            fps: 0.0,
            ticks_per_second: 0.0,
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
