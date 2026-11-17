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
use std::time::Instant;

use serde_json::json;

use duck_control::policy::Policy;
use duck_control::safety::SafetyConfig;

use crate::intent::{Intent, IntentQueue};
use crate::lan::GiftKind;
use crate::mujoco::MujocoIo;
use crate::runtime::{
    BringupState, DuckRuntime, HealthVerdict, PolicySwap, TickReport,
};

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
    /// The token a client that is not this phone has to send. The apps reach
    /// the duck over the loopback address and never need it; a tool on
    /// another machine does.
    intent_token: CString,
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
    /// The official UDP pad receiver: off until enabled with a pair code.
    pad: Option<crate::pad::Pad>,
    /// The pad's mapping memory (mode, hold state machines, trigger edges).
    /// Ported from `pad-map/src/mapper.rs` and fed one frame per tick.
    pad_mapper: crate::pad::PadMapper,
    /// The pad's refusal memory: the last pad request refused, kept 30 s and
    /// reported with the DKRB report.
    pad_refused: crate::pad::Refused,
    /// What the sticks currently mean (D-pad mode), as the mapper tracks it.
    pad_mode: crate::pad::PadMode,
    /// When the driving client last got a DKRB report, for the once-a-second
    /// cadence.
    pad_report_at: Option<Instant>,
    /// The `robot.*` JSON-RPC server, started with the core. Dropping it stops
    /// the accept loop; the handlers behind it share the core's intent queue.
    ipc: Option<crate::ipc::RobotIpcServer>,
    /// The calendar day the parent's allowance was last reset on.
    parent_day: String,
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
    /// Whether the loaded set resolved the sitstand and ground-pick slots.
    /// `/skills` lists the loop's own verbs only while the slot behind them
    /// is loaded, exactly as `robotd`'s list is conditional on the loaded set
    /// (`main.rs`' `do_names`); a reload or a mode switch refreshes both.
    sitstand_loaded: bool,
    ground_pick_loaded: bool,
    /// The platform's own measurements, written once a second: display frames
    /// per second and control ticks per second actually achieved.
    performance: (f64, f64),
    /// Bumped whenever the body model changes (a mode switch). The renderer
    /// caches geometry per model; a new generation means rebuild it.
    model_generation: u64,
    /// The health verdict the IPC server answers with, refreshed once per
    /// tick by the control thread (the runtime is not `Sync`, so the handler
    /// reads this shared snapshot instead of the runtime). The verdict is the
    /// one of the duck the tick actually drove — [`active_runtime`], a selected
    /// branch when there is one: the original is not stepped in a branch
    /// session, and judging it would answer `robot.health` with a stalled loop
    /// while `/state`, which reads the same branch, says the duck is fine.
    health_snapshot: Arc<std::sync::Mutex<duck_ipc_proto::HealthResult>>,
}

struct Branch {
    label: String,
    runtime: crate::runtime::DuckRuntime<MujocoIo>,
    tick: u64,
}

/// The `robot.*` protocol's handler side, wired to the core's own execution
/// path.
///
/// `robot.move` / `robot.head` / `robot.do` reach exactly the intent queue the
/// HTTP API drives: `move`/`head` push the same continuous intents `/twist`
/// and `/head` push (fire-and-forget, answered `accepted`), `do` uses the same
/// awaited round `/skill` uses, so the duck's verdict — and its refusal — is the
/// one every other client gets.
///
/// The health verdict is read off the control thread: the runtime is not `Sync`
/// (a MuJoCo body is owned by the thread that loaded it), so the tick refreshes
/// a shared snapshot ([`DuckCore::health_snapshot`]) and the handler reads it.
/// `revision` is the provenance fingerprint, constant for the process.
struct RobotHandlers {
    intents: Arc<IntentQueue>,
    health: Arc<std::sync::Mutex<duck_ipc_proto::HealthResult>>,
    revision: String,
}

impl crate::ipc::RobotHandlers for RobotHandlers {
    fn health(&self) -> duck_ipc_proto::HealthResult {
        match self.health.lock() {
            Ok(snapshot) => snapshot.clone(),
            Err(_) => duck_ipc_proto::HealthResult::default(),
        }
    }

    fn move_twist(&self, params: duck_ipc_proto::MoveParams) -> duck_ipc_proto::IntentResult {
        // The same continuous intent `/twist` queues: no verdict to await.
        self.intents.push(Intent::Twist([params.vx, params.vy, params.vyaw]));
        duck_ipc_proto::IntentResult::accepted()
    }

    fn head(&self, params: duck_ipc_proto::HeadParams) -> duck_ipc_proto::IntentResult {
        self.intents.push(Intent::Head([
            params.neck_pitch,
            params.head_pitch,
            params.head_yaw,
            params.head_roll,
        ]));
        duck_ipc_proto::IntentResult::accepted()
    }

    fn do_skill(&self, params: duck_ipc_proto::DoParams) -> duck_ipc_proto::IntentResult {
        // The same awaited round `/skill` answers with: the next tick decides,
        // and the verdict is the duck's own refusal reason.
        let answer = self
            .intents
            .request(Intent::Skill(params.skill.clone()));
        let accepted = answer.get("ok").and_then(serde_json::Value::as_bool).unwrap_or(false);
        let reason = answer
            .get("error")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        match (accepted, reason) {
            (true, _) => duck_ipc_proto::IntentResult::accepted(),
            (false, Some(reason)) => duck_ipc_proto::IntentResult::refused(reason),
            (false, None) => duck_ipc_proto::IntentResult::refused(format!(
                "skill refused: {skill}",
                skill = params.skill
            )),
        }
    }

    fn revision(&self) -> Option<String> {
        Some(self.revision.clone())
    }
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

thread_local! {
    /// The reason the last failed core call on this thread failed, as the
    /// core's own errors carry it: the path, the shape, expected vs got. The
    /// C ABI has no out-parameter for a reload refusal and compressing it to
    /// `-1` had the shells show success for a load that never happened —
    /// `robotd` promises a load failure you can read
    /// (`duck-control/src/policy.rs`: everything is validated at load, and
    /// the caller can be told why).
    static LAST_ERROR: std::cell::RefCell<Option<CString>> =
        const { std::cell::RefCell::new(None) };
}

/// Record why the call that is about to return non-zero failed. The string is
/// owned until the next failed call on the same thread.
fn set_last_error(reason: &str) {
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = CString::new(reason).ok();
    });
}

/// The last call succeeded: nothing for the shell to read.
fn clear_last_error() {
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

/// The reason the last failed call on this thread failed, or NULL when the
/// last call succeeded. The pointer is valid until the next failed call on
/// the same thread, so read it right after the non-zero return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_last_error() -> *const c_char {
    LAST_ERROR.with(|slot| {
        slot.borrow()
            .as_ref()
            .map_or(std::ptr::null(), |reason| reason.as_ptr())
    })
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

/// The voice seed, derived from the build's provenance fingerprint.
///
/// The official robot reads its seed from the SoC efuse (`sounds::hardware_seed`):
/// two ducks built as the same software but with different hardware fuses
/// quack differently, because the seed is the identity. A phone has no such
/// number, so the build's identity stands in — the provenance fingerprint is
/// one hash over the scene, meshes, policies and manifest, stable across
/// launches of the same build. The first eight hex characters of the SHA-256
/// fingerprint are 32 bits of that identity: stable, reproducible, and different
/// for every build with different assets.
fn seed_from_fingerprint(fingerprint: &str) -> u32 {
    u32::from_str_radix(fingerprint.get(..8).unwrap_or_else(|| fingerprint), 16)
        .unwrap_or_default()
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

        // The duck's own voice. The official robot reads its seed from the SoC
        // efuse — two seeds sound like two different creatures. A phone has no
        // such number, so the build's identity stands in: the provenance
        // fingerprint is stable across launches of the same build, and the same
        // fingerprint therefore gives the same quack. The seed is the first
        // 32 bits of the fingerprint's hex, stable by construction.
        let voice_seed = seed_from_fingerprint(&provenance.fingerprint);
        runtime.set_voice_seed(voice_seed);
        stage(&format!("voice seed {voice_seed:#x}"));
        stage("ready");

        // The shell's ring shows the same verbs `/skills` publishes, so a
        // button and an API call cannot disagree about what exists. The
        // loop's own verbs are listed only while the slot behind them is
        // loaded — the same condition `robotd`'s own list applies.
        let (sitstand_loaded, ground_pick_loaded) =
            (bundle.paths.sitstand.is_some(), bundle.paths.ground_pick.is_some());
        let skills = published_skills(
            runtime.skill_names(),
            sitstand_loaded,
            ground_pick_loaded,
        )
        .into_iter()
        .map(|name| CString::new(name).unwrap_or_default())
        .collect();
        let meshes = (0..runtime.io().mesh_count())
            .map(|mesh| {
                CString::new(runtime.io().mesh_name(mesh).unwrap_or_default()).unwrap_or_default()
            })
            .collect();
        let intents = Arc::new(IntentQueue::new());
        intents.set_skills(published_skills(
            runtime.skill_names(),
            sitstand_loaded,
            ground_pick_loaded,
        ));
        intents.set_fingerprint(provenance.to_json());

        // The `robot.*` JSON-RPC server, started with the duck. Its handlers
        // share the core's intent queue, so the protocol's verbs reach the same
        // execution path as the HTTP API. A port that is taken is a label, not
        // a reason to refuse to start the duck.
        let health_snapshot = Arc::new(std::sync::Mutex::new(
            duck_ipc_proto::HealthResult::default(),
        ));
        let handlers = RobotHandlers {
            intents: intents.clone(),
            health: health_snapshot.clone(),
            revision: provenance.fingerprint.clone(),
        };
        let (ipc, label) =
            match crate::ipc::start(crate::ipc::ROBOT_IPC_PORT, Arc::new(handlers)) {
                Ok(server) => {
                    stage(&format!(
                        "robot ipc listening on {}",
                        crate::ipc::ROBOT_IPC_PORT
                    ));
                    (Some(server), "idle".to_owned())
                }
                Err(error) => {
                    let reason = format!("robot ipc unavailable: {error}");
                    stage(&reason);
                    (None, reason)
                }
            };

        Ok(DuckCore {
            runtime,
            skills,
            meshes,
            last: None,
            label,
            intents,
            server: None,
            provenance,
            fingerprint,
            intent_token: CString::default(),
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
            pad: None,
            pad_mapper: crate::pad::PadMapper::new(),
            pad_refused: crate::pad::Refused::default(),
            pad_mode: crate::pad::PadMode::Drive,
            pad_report_at: None,
            ipc,
            parent_day: String::new(),
            quests: crate::quests::QuestBoard::default(),
            notice: None,
            rendered_sound: None,
            sound_tag: None,
            emotions_dir: None,
            scene: scene.clone(),
            policies: policies.clone(),
            sitstand_loaded,
            ground_pick_loaded,
            performance: (0.0, 0.0),
            model_generation: 0,
            // The per-tick health snapshot the IPC server reads: the runtime
            // is not `Sync`, so the tick thread writes and the handler reads.
            health_snapshot,
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
/// bring-up state stay; call it while the duck is standing. 0 on success;
/// -1 with `duck_core_last_error` set when the set is refused — the reason
/// is the loader's own: which file, what it claimed, what the robot builds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_reload_policies(
    core: *mut DuckCore,
    policies_dir: *const c_char,
) -> c_int {
    if core.is_null() {
        set_last_error("null core");
        return -1;
    }
    let Ok(dir) = c_string(policies_dir) else {
        set_last_error("null or invalid policies directory");
        return -1;
    };
    let core = &mut *core;
    match reload_policies(core, &dir) {
        Ok(_) => {
            clear_last_error();
            0
        }
        Err(reason) => {
            // The shell sees the same words an HTTP client would: the refusal
            // goes on the label and the 100-tick notice, both of which the
            // sheets already render. `reload_policies` marks the health gate
            // degraded with the same words.
            set_last_error(&reason);
            core.label = reason.clone();
            core.notice = Some((core.runtime.clock().tick, reason));
            -1
        }
    }
}

/// Point the walk slot at one file in the policies directory — the phone's
/// `robotctl policy load walk <file>`. The name is relative to the directory
/// this core loads its sets from; empty or `"none"` clears the override so
/// the next reload resolves the set's own walk slot again. The change lands
/// on the next reload: call `duck_core_reload_policies` after this. 0 on
/// success; -1 with `duck_core_last_error` set when the name is not a file
/// in the directory (the reload still does the heavy validation).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_set_walk_policy(
    core: *mut DuckCore,
    name: *const c_char,
) -> c_int {
    if core.is_null() {
        set_last_error("null core");
        return -1;
    }
    let core = &mut *core;
    // NULL clears the override — the C idiom for "back to the set's own".
    let name = c_string(name).unwrap_or_default();
    if name.is_empty() || crate::manifest::is_none_sentinel(&name) {
        crate::manifest::set_walk_override(None);
        clear_last_error();
        return 0;
    }
    // A file name, not a route out of the directory: the official config key
    // takes a path, but a phone override is one file beside the manifest.
    if name.contains('\\') || name.contains('/') || name.contains("..") {
        set_last_error(&format!("{name:?} is not a file name in the policies directory"));
        return -1;
    }
    // Cheap check first, the way `robotd`'s set_skill validates what it
    // cheaply can — a name, a path that exists — and leaves the network to
    // the loader.
    if !core.policies.join(&name).is_file() {
        set_last_error(&format!(
            "no walk policy {name:?} in {}",
            core.policies.display()
        ));
        return -1;
    }
    crate::manifest::set_walk_override(Some(name));
    clear_last_error();
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
            let names = published_skills(
                (*core).runtime.skill_names(),
                (*core).sitstand_loaded,
                (*core).ground_pick_loaded,
            );
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
    // The API answers on the LAN, so it gets a token: the phone's own apps
    // are trusted over the loopback address, and anybody else has to say
    // this. The token is handed to the phone so it can show it.
    let token = crate::intent::new_token();
    core.intents.set_token(token.clone());
    match crate::intent::start(port as u16, core.intents.clone()) {
        Ok(server) => {
            core.server = Some(server);
            core.intent_token = CString::new(token.clone()).unwrap_or_default();
            publish_state(core);
            stage(&format!("intent server listening (token {token})"));
            0
        }
        Err(error) => {
            stage(&format!("intent server failed: {error}"));
            -1
        }
    }
}

/// The token a client that is not this phone has to send.
///
/// The apps never need it — they reach the duck over the loopback address —
/// but a tool on another machine does, and the phone shows it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_intent_token(core: *mut DuckCore) -> *const c_char {
    if core.is_null() {
        return std::ptr::null();
    }
    let core = &*core;
    core.intent_token.as_ptr()
}

/// The verbs a client may ask for. `robotd`'s list is the manifest's
/// one-shots; `sit`, `stand` and `ground_pick` are the loop's own verbs, and a
/// client cannot discover what `/skills` never names. `/skill` accepts all
/// three, so the list and the route agree.
///
/// The loop's own verbs are listed only while the slot behind them is loaded,
/// exactly as `robotd`'s list is conditional on the loaded set (`main.rs`'s
/// `do_names` and `skills_report`): a robot whose sitstand slot is empty
/// genuinely cannot sit, and advertising the verb had the client find that
/// out at call time. The manifest one-shots already carry their own
/// condition — the scheduler lists only what loaded.
fn published_skills(
    names: Vec<String>,
    sitstand_loaded: bool,
    ground_pick_loaded: bool,
) -> Vec<String> {
    // `robotd::queue_skill` resolves built-ins first, then the manifest's
    // one-shots; the emotion timelines come from the release library, last.
    // Keep `/skills` in that order so a client reading the list sees the
    // same priority the scheduler uses.
    let mut out = Vec::with_capacity(names.len() + 3);
    if sitstand_loaded {
        for builtin in ["sit", "stand"] {
            out.push(builtin.to_owned());
        }
    }
    if ground_pick_loaded {
        out.push("ground_pick".to_owned());
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
    // The new bundle may resolve different slots: re-derive the condition
    // `/skills` lists the loop's own verbs under.
    core.sitstand_loaded = bundle.paths.sitstand.is_some();
    core.ground_pick_loaded = bundle.paths.ground_pick.is_some();
    let names = published_skills(
        core.runtime.skill_names(),
        core.sitstand_loaded,
        core.ground_pick_loaded,
    );
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

/// Where a reload decided the new set will land. The C ABI answers 0 for both
/// halves — the request took — but what happened is not the same: a change
/// that replaces the network driving a moving gait is queued and walks the
/// duck home first (`robotd`'s `change_disturbs`), the old set is still
/// running until the tick that lands it.
enum Reloaded {
    /// The new set is running (or lands on the next tick without moving the
    /// robot): publish the skill list, clear the degraded mark.
    Landed,
    /// The change is queued behind the path home. Nothing has swapped yet.
    GoingHome,
}

fn reload_policies(core: &mut DuckCore, dir: &str) -> Result<Reloaded, String> {
    let result = reload_policies_inner(core, dir);
    // The health gate learns why a reload did not take — caller-marked
    // degraded, never unhealthy: the duck kept the policy it had, so there is
    // nothing to roll back. A reload that lands clears the mark; one still
    // walking the duck home keeps it — the old set is still the one running,
    // and clearing the mark would say the swap had finished when it has not.
    match &result {
        Ok(Reloaded::Landed) => core.runtime.clear_degraded(),
        Ok(Reloaded::GoingHome) => {}
        Err(reason) => core
            .runtime
            .mark_degraded(format!("policy reload refused: {reason}")),
    }
    result
}

fn reload_policies_inner(core: &mut DuckCore, dir: &str) -> Result<Reloaded, String> {
    // Reload the set this duck is actually running, not always the walking one.
    let bundle = crate::manifest::load_mode(Path::new(dir), core.runtime.mode())?;
    let mut policy = Policy::load(&bundle.paths, 0.05).map_err(|error| error.to_string())?;
    if bundle.standing_disabled {
        policy.set_standing_disabled(true);
    }
    let tuning = bundle.tuning();
    // `robotd`'s `change_disturbs` gate (main.rs:3015-3106): a change that
    // replaces the network the duck is driving is not swapped under a moving
    // gait — the next tick would be a different policy's idea of what the legs
    // were doing. The duck goes to the home pose first and the swap lands
    // there, torque on throughout. The mobile core has no file digests to
    // compare, so the caller's verdict is the walk slot itself: the loaded
    // policy's file against the set being reloaded to. Conservative by design
    // and by the official rule: only the network in the driving slot counts,
    // a change to the standing/sitstand/skill files lands in place because
    // none of them stepped last tick's gait.
    let disturbs = core.runtime.walking_slot_path() != Some(bundle.paths.walk.clone());
    match core
        .runtime
        .replace_policy_disturbing(policy, tuning, bundle.skill_tuning, disturbs)
    {
        PolicySwap::Refused => {
            // The runtime's own half of the official refusal: from inside a
            // shutdown the path home would stand the duck up under the sit.
            // The words go through the same refusal channel as any reload
            // that did not take.
            return Err("the duck is powering off".to_owned());
        }
        PolicySwap::GoingHome => {
            // Queued, not landed. The directory is remembered — a later mode
            // switch re-reads the set the duck is heading into — but the skill
            // list, the loaded-slot flags and the degraded mark all stay the
            // current set's until the tick that lands the swap, because the
            // running policy cannot do the new set's verbs yet. The landing
            // itself is observable through the runtime (`swap_pending`,
            // `swap_state`); publishing it on `/skills` early would be a lie.
            core.policies = PathBuf::from(dir);
            core.label = "policy swap: walking home first".to_owned();
            return Ok(Reloaded::GoingHome);
        }
        PolicySwap::InPlace => {}
    }
    // The set the duck runs changed: a later mode switch re-reads this
    // directory, whichever route did the reload.
    core.policies = PathBuf::from(dir);
    // `/skills` lists the loop's own verbs only while the slot behind them is
    // loaded — the same condition `robotd`'s own list applies. A reload into
    // a set with no sitstand network stops advertising sit/stand with it.
    core.sitstand_loaded = bundle.paths.sitstand.is_some();
    core.ground_pick_loaded = bundle.paths.ground_pick.is_some();
    // The change lands on the NEXT tick (the disturb gate never swaps inside
    // the request), so `core.runtime.skill_names()` still speaks the old set
    // right here. The names the new controller will report are the bundle's
    // own — the same truncation `Controller::skill_names` applies (`take
    // (policy.skill_count())`, and the loaded count is `paths.skills.len()`),
    // with the emotions last, which a reload does not touch.
    let mut names: Vec<String> = bundle
        .skills
        .iter()
        .take(bundle.paths.skills.len())
        .map(|skill| skill.name.clone())
        .collect();
    names.extend(core.runtime.emotion_names());
    let names = published_skills(names, core.sitstand_loaded, core.ground_pick_loaded);
    core.skills = names
        .iter()
        .map(|name| CString::new(name.clone()).unwrap_or_default())
        .collect();
    core.intents.set_skills(names);
    core.label = "policy reloaded".to_owned();
    Ok(Reloaded::Landed)
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
            | Intent::Backup
            | Intent::RestoreBackup(_)
            | Intent::TrajectoryExport
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
        Intent::PairPeer(code, target) => match &core.lan {
            Some(lan) => {
                if !lan.set_pair_code(&code) {
                    return refuse(core, "the pairing code must be six digits".to_owned());
                }
                let offered = target.as_deref().map(|id| lan.pair(id));
                match offered {
                    Some(Err(error)) => refuse(core, format!("pair refused: {error}")),
                    _ => json!({ "ok": true, "code_set": true, "offered": target }),
                }
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
        Intent::Ensemble(moves, sounds) => {
            if active_runtime(core).chorus_active() {
                return refuse(core, "a performance is already playing".to_owned());
            }
            if !moves.iter().all(|name| {
                active_runtime(core).emotion_names().iter().any(|loaded| loaded == name)
            }) || !sounds.iter().all(|tag| crate::voice::TAGS.contains(&tag.as_str())) {
                return refuse(core, "no such mood or sound".to_owned());
            }
            let id = format!("ensemble-{}", active_runtime(core).clock().tick);
            let sent = core
                .lan
                .as_ref()
                .map(|lan| lan.ensemble(&id, &moves, &sounds));
            match sent {
                Some(Ok(roles)) => {
                    let peers = core.lan.as_ref().map_or(0, crate::lan::Lan::peer_count);
                    // The lead plays too: its own part is the first role.
                    on_active(core, |runtime| {
                        runtime.start_ensemble("lead", 0, roles + 1, moves.clone(), sounds.clone(), peers, Some(id.clone()));
                    });
                    json!({ "ok": true, "roles": roles + 1, "moves": moves, "sounds": sounds })
                }
                Some(Err(error)) => refuse(core, format!("ensemble refused: {error}")),
                None => refuse(core, "the LAN peer port is not available".to_owned()),
            }
        }
        Intent::Backup => {
            // The whole duck's portable state, one versioned document. Every
            // part comes from the same intent it restores through, so a
            // backup can never carry a field the restore would not accept.
            let values = json!({
                "care": active_runtime(core).care_summary(),
                "life": active_runtime(core).book().summary(),
            });
            json!({
                "format": "microduck-backup/1",
                "fingerprint": core.fingerprint.to_string_lossy(),
                "day": core.intents.day(),
                "care": active_runtime(core).care_summary(),
                "quests": core.quests.summary(&values),
                "rules": active_runtime(core).rules_summary(),
            })
        }
        Intent::RestoreBackup(value) => {
            if value.get("format").and_then(serde_json::Value::as_str) != Some("microduck-backup/1") {
                return refuse(core, "unknown backup format".to_owned());
            }
            // Three parts, three of the duck's own doors: the care restore
            // clamps what it is given, the quest board re-measures against
            // the duck as it is now, and the rules vocabulary refuses a
            // program with a word the duck does not know. Nothing here
            // accepts a sentence, a number without a source, or a new kind
            // of fact.
            let care = apply_intent_active(
                core,
                Intent::RestoreCare(value.get("care").cloned().unwrap_or(json!({}))),
            );
            let day = value.get("day").and_then(serde_json::Value::as_str).unwrap_or("").to_owned();
            let quests = if day.len() == 10 {
                apply_intent_active(
                    core,
                    Intent::QuestDay(
                        day,
                        value.get("quests").cloned().unwrap_or(json!({})),
                    ),
                )
            } else {
                json!({ "ok": false, "error": "the backup carries no calendar day" })
            };
            let rules = apply_intent_active(
                core,
                Intent::SetRules(value.get("rules").cloned().unwrap_or(json!({}))),
            );
            json!({ "ok": true, "care": care, "quests": quests, "rules": rules })
        }
        Intent::PeerName(name) => match &core.lan {
            Some(lan) => {
                lan.set_name(&name);
                json!({ "ok": true, "peers": lan.snapshot() })
            }
            None => refuse(core, "the LAN peer port is not available".to_owned()),
        },
        Intent::TrajectoryExport => {
            // One JSON line per kept tick: tick, trunk pose, the 15 wire
            // joints and the policy's own command. Every number is the tick
            // the rewind machine saved; none is recomputed or smoothed.
            let lines: Vec<String> = core
                .timeline
                .iter()
                .map(|saved| {
                    json!({
                        "tick": saved.tick,
                        "speed": saved.speed,
                        "paused": saved.paused,
                        "body": saved.body,
                        "fallen": saved.fallen,
                    })
                    .to_string()
                })
                .collect();
            json!({
                "format": "microduck-trajectory/1",
                "fingerprint": core.fingerprint.to_string_lossy(),
                "count": lines.len(),
                "lines": lines,
            })
        }
        Intent::FloorHeight(z) => {
            // Only a MuJoCo body has a ground to move; the feature-gated
            // core answers honestly rather than pretending.
            #[cfg(feature = "mujoco")]
            {
                // The closure carries the verdict out through a captured
                // slot: on_active's own shape returns nothing.
                let mut moved = false;
                on_active(core, |runtime| {
                    moved = runtime.set_floor_height(z).unwrap_or(false);
                });
                if moved {
                    json!({ "ok": true, "floor_z": z })
                } else {
                    refuse(core, "the scene has no ground plane".to_owned())
                }
            }
            #[cfg(not(feature = "mujoco"))]
            {
                let _ = z;
                refuse(core, "this build has no physics ground".to_owned())
            }
        }
        Intent::ParentBudget(minutes) => {
            on_active(core, |runtime| runtime.set_parent_budget(minutes));
            let summary = active_runtime(core).care_summary();
            json!({ "ok": true, "minutes": minutes, "parent": summary.get("parent") })
        }
        Intent::PadSwitch(on) => {
            if on {
                // The official UDP pad is unauthenticated by design. A
                // family app only opens it AFTER the person set the same
                // six-digit pairing code the neighbours use; a stranger
                // on the Wi-Fi then has no control channel at all.
                let paired = core
                    .lan
                    .as_ref()
                    .is_some_and(|lan| lan.pair_code_set());
                if !paired {
                    return refuse(core, "set a six-digit pairing code first".to_owned());
                }
                if core.pad.is_none() {
                    core.pad = crate::pad::Pad::start();
                }
                match &core.pad {
                    Some(pad) => {
                        pad.set_enabled(true);
                        // A fresh session starts like a pad starting with the
                        // robot: Drive mode, no hold in flight, the defaults.
                        core.pad_mapper = crate::pad::PadMapper::new();
                        core.pad_mode = crate::pad::PadMode::Drive;
                        core.pad_report_at = None;
                        json!({ "ok": true, "port": crate::pad::PORT, "on": true })
                    }
                    None => refuse(core, "UDP 4210 is not available".to_owned()),
                }
            } else {
                if let Some(pad) = &core.pad {
                    pad.set_enabled(false);
                }
                core.pad_mapper.pad_gone();
                core.pad_report_at = None;
                json!({ "ok": true, "on": false })
            }
        }
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
            // Both halves are a reload that took: an in-place change, or one
            // queued behind the path home. `/state` answers for the swap's
            // landing through the runtime; `refuse` only on a real failure.
            Ok(_) => json!({ "ok": true }),
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

/// The health gate's verdict, as `/state` publishes it: `"healthy"`, or the
/// verdict's own name with its reasons. Degraded and unhealthy stay apart —
/// a release rolls back over unhealthy, and says why.
fn health_summary(verdict: HealthVerdict) -> serde_json::Value {
    match verdict {
        HealthVerdict::Healthy => json!({ "verdict": "healthy", "reasons": [] }),
        HealthVerdict::Degraded { reasons } => json!({ "verdict": "degraded", "reasons": reasons }),
        HealthVerdict::Unhealthy { reasons } => json!({ "verdict": "unhealthy", "reasons": reasons }),
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

/// Whether the duck is up — the pad's belief about a toggle's outcome, the
/// same predicate the mapper's `up` flag is fed from: torque on and at the
/// home pose. `Home` (disable + enable) leaves the duck up, `Rest`/`Shutdown`
/// leave it down.
///
/// The duck judged is the one the pad drives — [`active_runtime`], the selected
/// branch when there is one — not the original `core.runtime` sits on while a
/// branch session runs. A selected branch's enable/disable state is the truth
/// the next Start tap has to act on.
fn duck_is_up(core: &DuckCore) -> bool {
    let active = active_runtime(core);
    !active.shutting_down()
        && !active.is_sitting()
        && matches!(active.bringup(), BringupState::Homing | BringupState::Ready)
}

/// One tick of the official UDP pad: the frame arrives, the mapper decides the
/// events the official `padd`/`netpadd` mapping would send, and this dispatches
/// them through the same execution paths every other client reaches.
///
/// The twist is the official continuous mapping (`stick_frame` from
/// `pad-map/src/mapper.rs`, `PadDriveParams` scaling) with the phone
/// joystick's one-stick convention: `ly` forward is +vx at the pad's own max
/// (0.4 m/s), `lx` turns at ±1.0 rad/s, deadzoned at the pad's 0.1. The
/// per-mode stick mappings (head, head+move, body) are the same functions with
/// the official default bindings and limits from `robotd_params`.
fn pad_tick(core: &mut DuckCore) {
    // The pad handle is a clone of the shared receiver (`Pad` is `Clone`), so
    // the tick can borrow the core mutably while the wire duties run on the
    // handle.
    let Some(pad) = core.pad.clone() else {
        return;
    };
    // The driving client, if any, is told its DKST answer right away, and its
    // DKRB report once a second — both are wire duties of the receiver, not
    // runtime duties.
    pad.answer_last();

    let Some(frame) = pad.tick_frame() else {
        // No frame: silence. The pad-gone reset is only for a pad that was
        // seen and then went away; a pad that never sent anything has nothing
        // held.
        if pad.is_enabled() {
            core.pad_mapper.pad_gone();
        }
        return;
    };

    let now = Instant::now();

    // The wire's own view of the watched duck, recomputed here from
    // `active_runtime` (a branch session's original is not being driven at
    // all, and the branch is). The mapper's decision and its effect on the
    // duck — belief reconcile, continuous stick commands, discrete event
    // dispatch — all run in `pad_apply_frame`, the core-facing half the tests
    // drive directly so they exercise this same code without a UDP socket.
    let active = active_runtime(core);
    let driving = !active.shutting_down()
        && matches!(active.bringup(), BringupState::Homing | BringupState::Ready);
    pad.set_driving(driving);

    pad_apply_frame(core, &frame, now);

    // DKRB: the driving client is told health and posture once a second, and
    // at once when it just connected.
    if pad.report_due(crate::pad::Accept::Taken, core.pad_report_at.take(), now) {
        let active = active_runtime(core);
        let input = crate::pad::PadReportInput {
            health: Some(active.health_result()),
            health_error: None,
            // The duck's own view of what it is doing, from the same verdicts
            // the control loop reads every tick.
            posture: Some(crate::pad::Posture {
                // The network that drove the last tick, the wire's own
                // vocabulary ("walk", "stand", "seated", …); "held" when no
                // network drove it (limp, idle, mid-bring-up).
                policy: active
                    .driving()
                    .map(|driving| driving.to_string())
                    .unwrap_or_else(|| "held".to_owned()),
                fallen: active.fallen(),
                limp: active.bringup() == BringupState::Limp,
                picked_up: active.picked_up(),
            }),
            posture_error: None,
        };
        pad.send_report(&input, &core.pad_refused, now);
        core.pad_report_at = Some(now);
    }
}

/// The core-facing half of one pad frame: reconcile the mapper's belief with
/// the watched duck, run the mapper, drive the continuous stick commands, and
/// dispatch every discrete event the mapper produced — with its refusals
/// remembered in `pad_refused` and stamped on `notice`.
///
/// Split out of [`pad_tick`] so a test can drive the same dispatch (and its
/// refusal memory and notice clock) with a hand-built [`crate::pad::PadFrame`]
/// instead of a UDP datagram on the fixed port 4210, which only one process
/// can bind and cannot deliver at a deterministic instant. The wire duties —
/// the DKST answer, the driving verdict, the DKRB report — stay in `pad_tick`,
/// because they are the receiver's, not the duck's.
fn pad_apply_frame(
    core: &mut DuckCore,
    frame: &crate::pad::PadFrame,
    now: Instant,
) {
    // The mapper's `up` belief is the robot's own state as the pad last heard
    // it — and under a phone the robot's state is directly readable. Re-read
    // it from the watched duck before the mapper decides what the next Start
    // tap means, so a branch that was selected (or a rewind that landed)
    // between two pad frames cannot leave the pad acting on another duck's
    // enable state.
    core.pad_mapper.set_up(duck_is_up(core));

    let mut events = Vec::new();
    core.pad_mapper.tick(frame, now, &mut events);

    // Continuous commands, in the official order: drive/head/body first, so
    // the discrete intents below run on a command frame that matches the
    // one the sticks just described.
    pad_frame_commands(core, frame, core.pad_mode);

    for event in events {
        // The method is the official wire name the event stands for —
        // `robot.do` for a skill, `robot.enable` for the toggle, … — as the
        // DKRB refusal carries it. Captured before dispatch moves the event.
        let method = match &event {
            crate::pad::PadButtonEvent::Skill(_) => "robot.do",
            crate::pad::PadButtonEvent::Init => "robot.init",
            crate::pad::PadButtonEvent::Toggle => "robot.enable",
            crate::pad::PadButtonEvent::Home => "robot.enable",
            crate::pad::PadButtonEvent::Rest => "robot.rest",
            crate::pad::PadButtonEvent::Shutdown => "robot.shutdown",
            _ => "robot.do",
        };
        let reason = dispatch_pad_event(core, &event);
        if let Some(reason) = &reason {
            core.pad_refused.remember_refused(method, reason, now);
            // The same refusal channel every other client reaches: the shell
            // and `/state` read `notice`, and it must be stamped with the
            // clock of the duck that actually refused — the selected branch,
            // not the original `core.runtime` sits on. `pad_refused` keeps the
            // wall-clock instant the DKRB report ages against; this keeps the
            // simulated tick `/state` compares to.
            core.label = reason.clone();
            core.notice = Some((active_runtime(core).clock().tick, reason.clone()));
        }
        // Feed the robot's own outcome back to the mapper, as `padd` does
        // from the answers: the next Start tap is Init while the duck is not
        // up, the toggle once it is. `Init` believes the duck is up once the
        // enable took — the ramp into `Homing`/`Ready` happens on the next
        // tick, so re-reading `duck_is_up` right here would be false for one
        // frame — but a refused Init (the duck is powering off) leaves the
        // belief alone. The reconcile above re-reads the watched duck before
        // every decision, so a branch switch or a rewind cannot leave the pad
        // acting on another duck's enable state. `Toggle` judges the result on
        // the watched duck (`duck_is_up` reads `active_runtime`): a disable
        // lands limp now, a re-enable is corrected next frame.
        if reason.is_none() {
            match event {
                crate::pad::PadButtonEvent::Init => core.pad_mapper.set_up(true),
                crate::pad::PadButtonEvent::Toggle => core.pad_mapper.set_up(duck_is_up(core)),
                crate::pad::PadButtonEvent::Rest | crate::pad::PadButtonEvent::Shutdown => {
                    core.pad_mapper.set_up(false)
                }
                _ => {}
            }
        }
    }
}

/// The continuous stick commands, the official way (`stick_frame` in
/// `pad-map/src/mapper.rs`), with the default bindings and limits of
/// `robotd_params::PadAxesParams` / `PadHeadParams` / `PadBodyParams`:
/// deadzone 0.1, drive vx ±0.4 m/s and vyaw ±1.0 rad/s, head travel 2.5 rad
/// per joint, body z -0.025..0.010 m and roll ±15°.
///
/// `Drive`: ly is +vx, lx turns at the left stick's vyaw — the mapping the
/// shell's joystick already used (0.4 m/s, 1.0 rad/s, 0.1 deadzone).
/// `HeadDrive`: the same drive plus the right stick poses the head. `Head`:
/// the body is stopped and all four axes pose the head. `BodyPose`: the body
/// holds (move zeroed) and the left stick crouches/leans, the right stick
/// poses the head.
fn pad_frame_commands(core: &mut DuckCore, frame: &crate::pad::PadFrame, mode: crate::pad::PadMode) {
    use robotd_params::pad_axes::{AxisBinding, Sticks};

    // The official default bindings and limits, from `robotd_params` itself —
    // the shipped mapping, signs included (`pad_axes.rs`'s `Default` impls).
    let axes = robotd_params::PadAxesParams::default();
    let head = robotd_params::PadHeadParams::default();
    let body = robotd_params::PadBodyParams::default();
    let drive = robotd_params::PadDriveParams::default();

    fn evaluate(binding: &AxisBinding, sticks: &Sticks, deadzone: f64) -> f64 {
        binding.evaluate(*sticks, deadzone)
    }
    let sticks = Sticks {
        left_x: frame.left_x,
        left_y: frame.left_y,
        right_x: frame.right_x,
        right_y: frame.right_y,
    };
    let drive_cmd = |vx: &AxisBinding, vyaw: &AxisBinding| {
        [
            robotd_params::PadDriveParams::scale(evaluate(vx, &sticks, axes.deadzone), drive.vx_min, drive.vx_max),
            0.0,
            robotd_params::PadDriveParams::scale(evaluate(vyaw, &sticks, axes.deadzone), drive.vyaw_min, drive.vyaw_max),
        ]
    };
    let head_cmd = |bindings: &[AxisBinding; 4]| {
        [
            evaluate(&bindings[0], &sticks, axes.deadzone) * head.neck_pitch_max,
            evaluate(&bindings[1], &sticks, axes.deadzone) * head.head_pitch_max,
            evaluate(&bindings[2], &sticks, axes.deadzone) * head.head_yaw_max,
            evaluate(&bindings[3], &sticks, axes.deadzone) * head.head_roll_max,
        ]
    };
    match mode {
        // Drive: the one-stick formula the shells' joystick already sends —
        // vx = ly * vx_max, wz = -lx * vyaw_max, 0.1 deadzone — which is also
        // the official `HeadDriveAxes` binding (`vx: LeftY`, `vyaw: LeftX`,
        // no strafe). The official `DriveAxes` would put the turn on the right
        // stick; the mobile pad keeps the phone joystick's convention so a
        // person does not have to relearn the sticks between the two.
        crate::pad::PadMode::Drive => on_active(core, |runtime| {
            runtime.set_twist(drive_cmd(&axes.head_drive.vx, &axes.head_drive.vyaw));
        }),
        crate::pad::PadMode::HeadDrive => on_active(core, |runtime| {
            runtime.set_twist(drive_cmd(&axes.head_drive.vx, &axes.head_drive.vyaw));
            runtime.set_head(head_cmd(&[
                axes.head_drive.neck_pitch,
                axes.head_drive.head_pitch,
                axes.head_drive.head_yaw,
                axes.head_drive.head_roll,
            ]));
        }),
        crate::pad::PadMode::Head => on_active(core, |runtime| {
            runtime.set_twist([0.0, 0.0, 0.0]);
            runtime.set_head(head_cmd(&[
                axes.head.neck_pitch,
                axes.head.head_pitch,
                axes.head.head_yaw,
                axes.head.head_roll,
            ]));
        }),
        crate::pad::PadMode::BodyPose => on_active(core, |runtime| {
            runtime.set_twist([0.0, 0.0, 0.0]);
            runtime.set_body(
                robotd_params::PadDriveParams::scale(evaluate(&axes.body_pose.z, &sticks, axes.deadzone), body.z_min, body.z_max),
                evaluate(&axes.body_pose.roll, &sticks, axes.deadzone) * body.roll_max,
                0.0,
            );
            runtime.set_head(head_cmd(&[
                axes.body_pose.neck_pitch,
                axes.body_pose.head_pitch,
                axes.body_pose.head_yaw,
                axes.body_pose.head_roll,
            ]));
        }),
    }
}

/// Dispatch one mapper event to the duck, through the same entries the HTTP
/// API and the shell's buttons reach. Returns the refusal reason when the duck
/// said no — the mapper's event vocabulary does not carry a verdict, the
/// duck's own answer is the verdict.
///
/// Every arm acts on the duck the client is watching — [`on_active`] for the
/// writes, [`active_runtime`] for the reads — the same rule the continuous
/// stick path ([`pad_frame_commands`]) already follows. Before this, a discrete
/// button spoke `core.runtime` directly, so during a branch session (a selected
/// branch) the press hit the original while the screen drove the branch. The
/// pad's own memory (`pad_mapper`, `pad_refused`) and `pad_mode` are core-level,
/// not per-duck, and stay as they were.
fn dispatch_pad_event(core: &mut DuckCore, event: &crate::pad::PadButtonEvent) -> Option<String> {
    use crate::pad::PadButtonEvent;
    match event {
        // One of the six bindable buttons: its bound skill name through the
        // mapper's own table, the same skill the ring triggers.
        PadButtonEvent::Skill(button) => {
            let Some(skill) = core.pad_mapper.skill(button) else {
                return None;
            };
            if skill.is_empty() {
                return None;
            }
            let skill = skill.to_owned();
            // The same path as `/skill` and the ring: sit/stand are the
            // loop's own verbs, ground_pick its own, the rest manifest skills.
            // The bound names are the official `[pad]` bindings —
            // `sit_toggle` is the A button's own spelling of sit/stand, and
            // `excited`/`yes`/`no` are emotion timelines the manifest loads.
            // The shutdown gate matches the intent arm: a skill during the
            // power-off sit would stand the duck up out of the seat the
            // power-off is counting on, so it is refused (and that refusal is
            // remembered in `pad_refused`, which the DKRB report carries).
            let mut refusal = None;
            on_active(core, |runtime| {
                if runtime.shutting_down() {
                    refusal = Some("the duck is powering off".to_owned());
                    return;
                }
                let result: Result<(), String> = match skill.as_str() {
                    "sit" | "stand" | "sit_toggle" => runtime
                        .sit_toggle()
                        .map(|_| ())
                        .map_err(|error| format!("skill refused: {error}")),
                    "ground_pick" => runtime
                        .start_ground_pick()
                        .map_err(|error| format!("skill refused: {error}")),
                    _ => runtime
                        .trigger_skill(&skill)
                        .map(|_| ())
                        .map_err(|error| format!("skill refused: {error}")),
                };
                refusal = result.err();
            });
            refusal
        }
        // `robot.init`: stand up and hold. A duck that was powered off comes
        // back through `power_on` (which forgets the seat and enables); an
        // already-powered duck simply enables. A duck mid power-off is
        // refused, the way every other move is.
        PadButtonEvent::Init => {
            let mut refusal = None;
            on_active(core, |runtime| {
                if runtime.shutting_down() {
                    refusal = Some("the duck is powering off".to_owned());
                    return;
                }
                let result = if runtime.powered_off() {
                    runtime.power_on()
                } else {
                    runtime.enable()
                };
                refusal = result.err().map(|error| format!("init refused: {error}"));
            });
            refusal
        }
        // `robot.enable toggle`: the robot owns the state. Off means the duck
        // sits (the shell's own end-of-drive), on means the policy is asked to
        // drive from home — the toggle answer is what the mapper believes.
        PadButtonEvent::Toggle => {
            let mut refusal = None;
            on_active(core, |runtime| {
                if runtime.shutting_down() {
                    // The duck is powering off: the toggle is refused, the way
                    // a skill is.
                    refusal = Some("the duck is powering off".to_owned());
                    return;
                }
                let result = if runtime.powered_off() {
                    runtime.power_on()
                } else {
                    // The duck is standing and driving: the pad asks it to stop
                    // driving, the same disable the shell's end-of-drive button
                    // runs.
                    runtime.disable()
                };
                refusal = result.err().map(|error| format!("toggle refused: {error}"));
            });
            refusal
        }
        // The virtual face LED, one press.
        PadButtonEvent::Flashlight => {
            on_active(core, |runtime| {
                runtime.set_flashlight(true, false, "white");
            });
            None
        }
        // A chirp, through the same sound rendering path the shell uses.
        PadButtonEvent::Chirp => {
            on_active(core, |runtime| runtime.queue_sound("chirp"));
            None
        }
        // The held wheee ride: a level, not a one-shot.
        PadButtonEvent::Wheee => {
            on_active(core, |runtime| runtime.wheee(true));
            None
        }
        PadButtonEvent::WheeeOff => {
            on_active(core, |runtime| runtime.wheee(false));
            None
        }
        // Start held: home pose, motors stiff, policy off. The official home
        // sequence: disable the policy, then re-enable so the duck ramps back
        // to the home pose with torque on.
        PadButtonEvent::Home => {
            let mut refusal = None;
            on_active(core, |runtime| {
                if runtime.shutting_down() {
                    refusal = Some("the duck is powering off".to_owned());
                    return;
                }
                refusal = runtime
                    .disable()
                    .and_then(|()| runtime.enable())
                    .err()
                    .map(|error| format!("home refused: {error}"));
            });
            refusal
        }
        // Select held 2 s: sit, rest pose, torque off. The mobile runtime's
        // one sit→rest→torque-off sequence is the shutdown sit (`robotd`'s own
        // sequence); a phone has no servo reboot, so the rest ends latched off
        // — the next Start tap (Init) powers the duck back on.
        PadButtonEvent::Rest => {
            on_active(core, |runtime| runtime.request_shutdown());
            None
        }
        // Select held 4 s: power off, latched. The same sequence as the HTTP
        // `/shutdown` route runs.
        PadButtonEvent::Shutdown => {
            on_active(core, |runtime| runtime.request_shutdown());
            None
        }
        // The D-pad switched what the sticks mean. The mode is the mapper's
        // (core-level); the head/body writes belong to the watched duck, and
        // leaving a head mode puts the head back to rest (the official
        // mode-exit call).
        PadButtonEvent::ModeChange(mode) => {
            let mode = *mode;
            core.pad_mode = mode;
            if !mode.poses_head() || mode == crate::pad::PadMode::BodyPose {
                on_active(core, |runtime| {
                    if !mode.poses_head() {
                        runtime.set_head([0.0, 0.0, 0.0, 0.0]);
                    }
                    if mode == crate::pad::PadMode::BodyPose {
                        runtime.release_body();
                    }
                });
            }
            None
        }
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
        ok = match gift {
            GiftKind::Treat => matches!(runtime.gift_treat(), Ok(true)),
            GiftKind::Emotion => runtime.gift_emotion(&value).is_ok(),
        };
        // The gift arrived, whatever the duck was doing at the time: one
        // line saying where it came from. What the duck then *did* — it ate,
        // it played a mood — moves its own counter only when it really
        // happened, so the two facts never pretend to be one.
        runtime.book_mut().note(match gift {
            GiftKind::Treat => "gift:treat".to_owned(),
            GiftKind::Emotion => format!("gift:emotion:{value}"),
        });
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
        // What the safety gate did to this tick's command, in the official
        // wire spelling — `robot.state.movement.limited_by`: the caller learns
        // whether the deadman, a joint range or a non-finite value clamped
        // the command this tick.
        "limited_by": active.limited_by(),
        // The health gate's verdict as it stands: degraded and unhealthy are
        // strictly apart, and a release rolls back over unhealthy. The health
        // numbers themselves are already in the same frame's battery/volts.
        "health": health_summary(active.health()),
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
        "pad": {
            "on": core.pad.as_ref().is_some_and(crate::pad::Pad::is_enabled),
            "port": crate::pad::PORT,
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
    // A new calendar day restarts the parent's allowance: the budget's base
    // re-snapshots the duck's own powered time, so yesterday's play does not
    // eat today's minutes.
    if quest_day.len() == 10 && quest_day != core.parent_day {
        core.parent_day = quest_day.clone();
        on_active(core, |runtime| {
            runtime.reset_parent_day();
            // The parent's view: the day boundary snapshots yesterday's own
            // deltas into the vet book's rows.
            runtime.book_mut().roll_day(&quest_day);
        });
    }
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
            crate::lan::PeerEvent::Ensemble { id, role, roles, moves, sounds, .. } => {
                if !active_runtime(core).chorus_active() {
                    let peers = core.lan.as_ref().map_or(0, crate::lan::Lan::peer_count);
                    on_active(core, |runtime| {
                        runtime.start_ensemble("join", role, roles, moves, sounds, peers, Some(id));
                    });
                }
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
    // The official UDP pad: a CLIENT, not a sense. The frame arrives here and
    // the mapper (ported from `pad-map/src/mapper.rs`) decides the events the
    // official mapping would send; this dispatches them through the same
    // execution paths the HTTP API and the shell's buttons use. The 61-D
    // observation and the safety layer never see it.
    pad_tick(core);
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
        publish_ipc(core);
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
            publish_ipc(core);
            publish_state(core);
            0
        }
        Err(_) => -1,
    }
}

/// Publish one tick to the `robot.*` protocol: refresh the health snapshot the
/// IPC handlers read (the runtime is not `Sync`, so this thread — the one that
/// owns it — writes it), and push the official `robot.state` frame to every
/// subscriber. With nobody subscribed both are cheap and cannot fail.
///
/// Both halves judge the duck the tick actually drove: [`active_runtime`], the
/// selected branch when there is one. A branch session never steps the original
/// (the loop steps it only while `selected == 0`), so its stall clock keeps
/// running past the 25-period floor and its `robot.health` would answer
/// "control loop stalled" to a subscriber whose `robot.state` frames — pushed
/// from the very same branch — show a duck walking. Every reader speaks of the
/// watched duck: `/state` and `DuckStatus` go through `active_runtime`, the
/// branch's own `robot_state()` frame is what is pushed here, and the health
/// snapshot has to come from the same runtime or the two answers contradict
/// each other.
fn publish_ipc(core: &DuckCore) {
    let active = active_runtime(core);
    let runtime_health = active.health_result();
    if let Ok(mut snapshot) = core.health_snapshot.lock() {
        *snapshot = runtime_health;
    }
    let Some(ipc) = core.ipc.as_ref() else {
        return;
    };
    if let Some(state) = active.robot_state() {
        ipc.publish_state(&state);
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

    /// Core creation is serialized across this module's tests. The core binds
    /// the fixed loopback port 8768 for `robot.*`, so two live cores in one
    /// process cannot both have the server; holding this lock for a test's
    /// whole run keeps the IPC end-to-end test exercising its own core
    /// instead of whichever parallel test won the port. (The failure path
    /// itself is covered by construction: a taken port is a label, never a
    /// refused duck.)
    static CORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn new_core() -> (std::sync::MutexGuard<'static, ()>, *mut DuckCore) {
        let guard = CORE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        (guard, core)
    }

    /// Tick until the duck finishes its bring-up ramp and is Ready. The
    /// official loop holds the full `HOME_RAMP` (2 s = 100 ticks at 20 ms) out
    /// even at the home pose — the ramp doubles as the IMU's convergence
    /// window — so a freshly enabled duck is `Homing` for a while, never
    /// Ready on the first tick (`runtime.rs`'s bring-up transition).
    fn tick_until_ready(core: *mut DuckCore) {
        for _ in 0..400 {
            unsafe { duck_core_tick(core) };
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
            if status.bringup == 2 {
                return;
            }
        }
        panic!("the duck never reached Ready");
    }

    #[test]
    fn ffi_creates_ticks_and_exposes_render_data() {
        let (_guard, core) = new_core();

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
        // The official ramp: a freshly enabled duck is Homing, never Ready on
        // the first tick (`HOME_RAMP` is 2 s even at the home pose).
        assert_eq!(status.bringup, 1, "the bring-up ramp has started");
        tick_until_ready(core);
        assert_eq!(unsafe { duck_core_status(core, &mut status) }, 0);
        assert_eq!(status.bringup, 2, "the ramp finishes at the home pose");
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

    /// The reload refusal is readable: the C ABI compresses it to -1, and the
    /// reason comes back on the thread that called it — the path, the claim,
    /// expected vs got. This is the half the shells were missing: they saw
    /// success for a load that never happened.
    #[test]
    fn the_last_error_carries_the_reload_reason() {
        // The mechanism itself, on a thread of its own: set, read, clear.
        unsafe {
            clear_last_error();
            assert!(duck_core_last_error().is_null());
            set_last_error("reading x.onnx: No such file or directory");
            let reason = duck_core_last_error();
            assert!(!reason.is_null());
            assert_eq!(
                CStr::from_ptr(reason).to_string_lossy(),
                "reading x.onnx: No such file or directory"
            );
            clear_last_error();
            assert!(duck_core_last_error().is_null());
        }

        // End to end: a directory with no policies in it is refused with the
        // loader's own reason, not a bare -1.
        let scene = Path::new(
            r"D:\AI\Duck\upstream\microduck_rl\src\mjlab_microduck\robot\microduck\scene_backlash.xml",
        );
        if !scene.is_file() {
            return; // the fixture tests above would have panicked already
        }
        let (_guard, core) = new_core();
        let dir = CString::new(
            std::env::temp_dir()
                .join(format!("duck-reload-refusal-{}", std::process::id()))
                .to_string_lossy()
                .into_owned(),
        )
        .unwrap();
        // A tick first: the health gate's "not started yet" verdict (tick 0)
        // would mask the degraded mark it is about to be given.
        unsafe { duck_core_tick(core) };
        assert_eq!(unsafe { duck_core_reload_policies(core, dir.as_ptr()) }, -1);
        let reason = unsafe { duck_core_last_error() };
        let text = unsafe { CStr::from_ptr(reason) }.to_string_lossy().into_owned();
        assert!(text.contains("no Walk locomotion policy in"), "{text}");

        // The refusal is a degraded mark, not a healthy duck: the judge can
        // see the reload did not take.
        let verdict = unsafe { (*core).runtime.health() };
        match &verdict {
            crate::runtime::HealthVerdict::Degraded { reasons } => assert!(
                reasons.iter().any(|reason| reason.starts_with("policy reload refused")),
                "{reasons:?}"
            ),
            other => panic!("a refused reload must mark the duck degraded: {other:?}"),
        }

        // A reload that lands clears the mark again.
        let policies = CString::new(r"D:\AI\Duck\policies").unwrap();
        assert_eq!(
            unsafe { duck_core_reload_policies(core, policies.as_ptr()) },
            0,
            "the fixture set loads"
        );
        assert!(
            !matches!(
                unsafe { (*core).runtime.health() },
                crate::runtime::HealthVerdict::Degraded { .. }
            ),
            "a reload that lands clears the degraded mark"
        );
        unsafe { duck_core_free(core) };
    }

    #[test]
    fn intent_server_drives_the_core() {
        let (_guard, core) = new_core();
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
        // The official bring-up ramp has to finish before Ready is reported.
        tick_until_ready(core);

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

    // -----------------------------------------------------------------------
    // H 工单的接线测试：pad 事件、IPC 端到端、嗓音种子、/state 健康字段。

    /// 嗓音种子：同一构建（同一指纹）两次构造得到同一种子，不同指纹得到
    /// 不同种子。种子是指纹前 8 位 hex，稳定且可复现。
    #[test]
    fn the_voice_seed_follows_the_fingerprint() {
        let first = seed_from_fingerprint("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
        let same = seed_from_fingerprint("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
        let other = seed_from_fingerprint("00000000deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
        assert_eq!(first, same, "the same fingerprint is the same seed");
        assert_ne!(first, other, "a different fingerprint is a different duck");
        assert_eq!(first, 0xdeadbeef);

        // 端到端：核心构造后 runtime 的种子就是指纹派生的种子。
        let (_guard, core) = new_core();
        let core_ptr = core;
        let fingerprint = unsafe { CStr::from_ptr(duck_core_fingerprint(core_ptr)) }
            .to_string_lossy()
            .into_owned();
        let seed = seed_from_fingerprint(&fingerprint);
        let runtime_seed = unsafe {
            // 通过私有字段读取（同模块测试）：种子必须在构造时已生效。
            (*core_ptr).runtime.voice_seed()
        };
        assert_eq!(runtime_seed, seed, "the core's voice seed is the fingerprint's");
        unsafe { duck_core_free(core) };
    }

    /// `/state` 发布 limited_by 与 health 分档，不破坏现有字段。
    #[test]
    fn state_publishes_limited_by_and_health() {
        let (_guard, core) = new_core();
        unsafe {
            duck_core_tick(core);
            duck_core_tick(core);
        }
        let port = 19000 + ((std::process::id() % 1000) + 7) as i32;
        assert_eq!(unsafe { duck_core_start_intent_server(core, port) }, 0);
        let mut stream = TcpStream::connect(("127.0.0.1", port as u16)).expect("connect");
        stream
            .write_all(b"GET /state HTTP/1.1\r\nHost: x\r\n\r\n")
            .expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read");
        assert!(response.contains("\"bringup\""), "{response}");
        assert!(response.contains("\"limited_by\""), "{response}");
        assert!(
            response.contains(r#""health"#) && response.contains(r#""verdict""#),
            "{response}"
        );
        unsafe { duck_core_free(core) };
    }

    /// pad 事件分发：构造帧喂 mapper，断言事件，再经 dispatch 断言 duck 的
    /// 行为（flashlight 切换、chirp 排队、wheee 保持）。
    #[test]
    fn pad_events_reach_the_duck() {
        use crate::pad::{Buttons, PadFrame};
        let (_guard, core) = new_core();
        let core_ptr = core;

        let mut events = Vec::new();
        let now = std::time::Instant::now();
        let mut mapper = crate::pad::PadMapper::new();

        // A 键按下：Skill("a")，默认绑定 sit_toggle —— 经 duck 的 sit_toggle。
        let press = PadFrame {
            held: Buttons::A,
            pressed: Buttons::A,
            ..Default::default()
        };
        mapper.tick(&press, now, &mut events);
        assert_eq!(events, vec![crate::pad::PadButtonEvent::Skill("a")]);

        // Home 按下：Flashlight。
        let home = PadFrame {
            held: Buttons::HOME,
            pressed: Buttons::HOME,
            ..Default::default()
        };
        mapper.tick(&home, now, &mut events);
        assert_eq!(events, vec![crate::pad::PadButtonEvent::Flashlight]);

        // RT 上升沿：Chirp。
        let rt = PadFrame {
            rt: 0.5,
            ..Default::default()
        };
        mapper.tick(&rt, now, &mut events);
        assert_eq!(events, vec![crate::pad::PadButtonEvent::Chirp]);

        // LT 进入：Wheee（保持），离开：WheeeOff。
        mapper.tick(
            &PadFrame {
                lt: 0.5,
                ..Default::default()
            },
            now,
            &mut events,
        );
        assert_eq!(events, vec![crate::pad::PadButtonEvent::Wheee]);
        mapper.tick(&PadFrame::default(), now, &mut events);
        assert_eq!(events, vec![crate::pad::PadButtonEvent::WheeeOff]);

        // 事件分发到 duck：Home 切闪光灯，Chirp 排队声音。
        unsafe {
            let core = &mut *core_ptr;
            assert!(dispatch_pad_event(core, &crate::pad::PadButtonEvent::Flashlight).is_none());
            assert_eq!(
                (*core_ptr).runtime.face().flashlight,
                1,
                "the flashlight is on"
            );
            assert!(dispatch_pad_event(core, &crate::pad::PadButtonEvent::Chirp).is_none());
            assert_eq!((*core_ptr).runtime.take_sound().as_deref(), Some("chirp"));
            // Wheee 保持进入/离开。
            assert!(dispatch_pad_event(core, &crate::pad::PadButtonEvent::Wheee).is_none());
            assert!(dispatch_pad_event(core, &crate::pad::PadButtonEvent::WheeeOff).is_none());
        }
        unsafe { duck_core_free(core) };
    }

    /// IPC 端到端：起 core（8768），连接，robot.health 有答，robot.move
    /// 驱动意图队列，robot.subscribe 收到 robot.state 推送。
    #[test]
    fn robot_ipc_drives_the_core_end_to_end() {
        use std::io::{BufRead, BufReader};
        let (_guard, core) = new_core();
        // 8768 是核心构造时绑定的固定端口。若被占用（测试并发），core 的
        // ipc 字段为 None，此端到端测试通过字段直接判空跳过——核心本身不
        // 受影响。
        let ipc_up = unsafe { (*core).ipc.is_some() };
        if !ipc_up {
            // 端口被占：构造不阻断（label 记原因），本测试无法走 socket。
            unsafe { duck_core_free(core) };
            return;
        }

        // 等待 accept 线程就绪。
        let mut stream = TcpStream::connect(("127.0.0.1", crate::ipc::ROBOT_IPC_PORT))
            .expect("connect robot ipc");
        let mut reader = BufReader::new(stream.try_clone().expect("dup"));
        fn send(stream: &mut TcpStream, id: u64, method: &str, params: &str) {
            let line = format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"{method}\",\"params\":{params}}}\n"
            );
            stream.write_all(line.as_bytes()).expect("write");
            stream.flush().expect("flush");
        }

        // robot.health：有答，shape 是官方 HealthResult。
        send(&mut stream, 1, "robot.health", "{}");
        let mut line = String::new();
        reader.read_line(&mut line).expect("health reply");
        assert!(line.contains(r#""id":1"#), "{line}");
        assert!(line.contains(r#""healthy""#), "{line}");

        // robot.move：经同一意图队列驱动 twist，duck 的下一拍有 twist。
        send(&mut stream, 2, "robot.move", r#"{"vx":0.4,"vy":0.0,"vyaw":0.0}"#);
        let mut line = String::new();
        reader.read_line(&mut line).expect("move reply");
        assert!(line.contains(r#""accepted":true"#), "{line}");
        // robot.move 走的是 push（不等待），所以等一拍后 twist 生效。
        unsafe { duck_core_tick(core) };

        // 工单 S 的补强：`accepted` 只证明命令进了队列，不证明鸭子动了。
        // 连续 tick，断言安全门真的收到了这条命令：`requested` 是租约的原始
        // 请求，`applied` 是平滑后的非零命令，且 `limited_by` 不含 `deadman`。
        // 租约与 deadman 都记**模拟**拍（500 ms = 25 拍，`TickTime`），与墙钟
        // 无关，所以这十拍不需要任何速度前提。
        //
        // 这一断言正是 runtime.rs `set_api_twist` 刷新 deadman 时钟那处修复
        // 的守门人：没有它，`intent_age` 仍是 `Duration::MAX`，`Safety::gate`
        // 会把租约的 twist 清零，`applied` 归零且 `limited_by` 报 `deadman`
        // ——`accepted` 却照样为真，旧断言红不起来。
        for index in 0..10 {
            let frame = unsafe { active_runtime(&*core).robot_state() }
                .unwrap_or_else(|| panic!("the {index}th tick published no robot.state frame"));
            assert!(
                frame.movement.requested[0].abs() > 1e-9,
                "tick {index} after robot.move: the ask reached the gate: {:?}",
                frame.movement
            );
            assert!(
                frame.movement.applied[0] > 0.0,
                "tick {index} after robot.move: the duck was commanded forward, not zeroed: {:?}",
                frame.movement
            );
            assert!(
                !frame
                    .movement
                    .limited_by
                    .iter()
                    .any(|limit| limit == "deadman"),
                "tick {index}: an API driver that keeps talking is not ignored: {:?}",
                frame.movement.limited_by
            );
            unsafe { duck_core_tick(core) };
        }

        send(&mut stream, 3, "hello", r#"{"api_version":51}"#);
        let mut line = String::new();
        reader.read_line(&mut line).expect("hello reply");
        assert!(line.contains(r#""api_version":51"#), "{line}");
        assert!(line.contains(r#""revision""#), "{line}");

        // robot.subscribe：订阅后每拍收到 robot.state 推送。
        send(&mut stream, 4, "robot.subscribe", "{}");
        let mut line = String::new();
        reader.read_line(&mut line).expect("subscribe ack");
        assert!(line.contains(r#""accepted":true"#), "{line}");
        unsafe { duck_core_tick(core) };
        unsafe { duck_core_tick(core) };
        let mut line = String::new();
        reader.read_line(&mut line).expect("a state push");
        assert!(line.contains("\"method\":\"robot.state\""), "{line}");

        unsafe { duck_core_free(core) };
    }

    /// Ticket S: a branch session must not make `robot.health` contradict
    /// `/state`. The selected branch is what the loop steps; the original is
    /// left alone, so its wall-clock stall timer runs on while nobody drives
    /// it. Both the IPC health snapshot and `/state` answer for the duck that
    /// is actually driven — `active_runtime` — and an ipc subscriber must never
    /// be told the branch it is watching is a wedged loop.
    #[test]
    fn a_selected_branch_is_the_duck_robot_health_judges() {
        /// Tick until `secs` of wall clock has passed, so the loop's own rate
        /// window closes on a duck that really is being stepped.
        fn tick_for(core: *mut DuckCore, secs: f64) {
            let started = std::time::Instant::now();
            for _ in 0..4000 {
                unsafe { duck_core_tick(core) };
                if started.elapsed().as_secs_f64() >= secs {
                    return;
                }
            }
            panic!("4000 ticks did not span {secs} s of wall clock");
        }

        let (_guard, core) = new_core();
        unsafe {
            // Drive the original to Ready, fork it and select the branch.
            tick_until_ready(core);
            let label = CString::new("s-branch").unwrap();
            assert_eq!(duck_core_fork(core, label.as_ptr()), 0, "the fork lands");
            assert_eq!((*core).branches.len(), 1);
            (*core)
                .intents
                .push(Intent::SelectBranch("s-branch".to_owned()));
            duck_core_tick(core); // drains the queue: `selected = 1`
            assert_eq!((*core).selected, 1, "the branch is the watched duck");

            // The branch keeps ticking; in a branch session the original is not
            // stepped at all — and after this span, longer than the stall
            // window (25 periods at full speed), the duck nobody woke reads as
            // a wedged loop.
            let original_tick = (*core).runtime.clock().tick;
            tick_for(core, 1.2);
            assert_eq!(
                (*core).runtime.clock().tick,
                original_tick,
                "the original is not stepped once a branch is selected"
            );
            let active = active_runtime(&*core);
            assert!(
                active.clock().tick > original_tick,
                "the selected branch is the duck that ticked"
            );

            // Non-vacuity: the two runtimes really disagree here. The original
            // — what `publish_ipc` used to read — is judged stalled, and still
            // is, because a branch session never steps it.
            let stalled = (*core).runtime.health_result();
            assert!(
                !stalled.healthy
                    && stalled
                        .reason
                        .as_deref()
                        .is_some_and(|reason| reason.contains("stalled")),
                "the untouched original reports a stalled loop: {stalled:?}"
            );

            // The snapshot the `robot.health` handler answers from is the
            // watched duck's verdict, and not the original's.
            let snapshot = (*core).health_snapshot.lock().unwrap().clone();
            assert_eq!(
                snapshot,
                active.health_result(),
                "the ipc health snapshot is the active runtime's verdict"
            );
            assert!(
                !snapshot
                    .reason
                    .as_deref()
                    .unwrap_or_default()
                    .contains("stalled"),
                "a branch the loop keeps stepping is not reported as wedged: {snapshot:?}"
            );

            // And `/state`, which already read `active_runtime`, agrees with
            // the snapshot: both were written by the same tick, for the same
            // duck. The verdict's name is derived from the snapshot's own
            // flags — the same mapping `health_summary` applies to the verdict
            // it was built from — so the comparison cannot straddle a tick.
            let state: serde_json::Value =
                serde_json::from_str(&(*core).intents.state_json()).expect("/state json");
            let published = state["health"]["verdict"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let expected = if snapshot.healthy {
                "healthy"
            } else if snapshot.degraded {
                "degraded"
            } else {
                "unhealthy"
            };
            assert_eq!(
                published, expected,
                "/state and robot.health are one verdict for one duck: snapshot {snapshot:?}, \
                 /state {}",
                state["health"]
            );
            let reasons: Vec<String> = state["health"]["reasons"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            assert_eq!(
                reasons.join("; "),
                snapshot.reason.clone().unwrap_or_default(),
                "and they say the same reasons: /state {}",
                state["health"]
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// Ticket P: a discrete pad button drives the duck the client is watching,
    /// not the original. Fork, select the branch, then feed pad frames through
    /// the same dispatch the live tick runs (`pad_apply_frame`, the core-facing
    /// half of `pad_tick` — no UDP socket, so the timing is the test's).
    ///
    /// Two facts are pinned, per the boss's call:
    /// 1. the press lands on the **branch** (its enable/seat state moves);
    /// 2. the parent trajectory is **undisturbed** — its tick never advances
    ///    (a branch session does not step the original) and no side effect of
    ///    the press shows up on it.
    #[test]
    fn a_pad_button_drives_the_branch_and_not_the_original() {
        use crate::pad::{Buttons, PadFrame};
        let (_guard, core) = new_core();

        unsafe {
            let c = &mut *core;
            // Drive the original to Ready, fork, select the branch.
            tick_until_ready(core);
            let label = CString::new("p-branch").unwrap();
            assert_eq!(duck_core_fork(core, label.as_ptr()), 0, "the fork lands");
            assert_eq!(c.branches.len(), 1);
            c.intents.push(Intent::SelectBranch("p-branch".to_owned()));
            duck_core_tick(core); // drains the queue: `selected = 1`
            assert_eq!(c.selected, 1, "the branch is the watched duck");

            // The forked branch is Ready too: `fork` restores the parent's
            // checkpoint, enable state and bring-up included.
            assert_eq!(
                active_runtime(c).bringup(),
                BringupState::Ready,
                "the branch starts at the parent's Ready"
            );

            // The parent's identity across the whole scenario: its tick must
            // not move (a branch session steps the branch only) and its seat
            // must stay where it is.
            let parent_tick = c.runtime.clock().tick;
            assert!(!c.runtime.is_sitting(), "the parent stood up");

            // ── 1. A bindable button: the A press is `sit_toggle`. ───────────
            // One frame with A on the press edge: the mapper turns it into
            // `Skill("a")`, dispatch sits the watched duck.
            let now = Instant::now();
            pad_apply_frame(
                c,
                &PadFrame {
                    held: Buttons::A,
                    pressed: Buttons::A,
                    ..Default::default()
                },
                now,
            );
            assert!(
                active_runtime(c).is_sitting(),
                "the A press sat the branch — the watched duck"
            );
            assert!(
                !c.runtime.is_sitting(),
                "the parent was not sat: the press never reached `core.runtime`"
            );
            assert_eq!(
                c.runtime.clock().tick,
                parent_tick,
                "a branch session never steps the original"
            );

            // ── 2. Init: stand the branch back up, the parent is untouched. ──
            // Take the branch limp (so the pad's next Start tap means Init,
            // not Toggle), then feed a Start press-and-release through the
            // mapper. Init enable-lands on the branch; the parent is left
            // standing where it was.
            on_active(c, |runtime| {
                let _ = runtime.disable();
            });
            assert_eq!(
                active_runtime(c).bringup(),
                BringupState::Limp,
                "the branch is down before Init"
            );
            // Frame one: Start down. Frame two: Start up with the release edge
            // — the hold machine calls that a tap, and with the watched duck
            // not up the tap is Init.
            pad_apply_frame(
                c,
                &PadFrame {
                    held: Buttons::START,
                    pressed: Buttons::START,
                    ..Default::default()
                },
                Instant::now(),
            );
            pad_apply_frame(
                c,
                &PadFrame {
                    released: Buttons::START,
                    ..Default::default()
                },
                Instant::now(),
            );
            // The enable happened on the branch: the watched duck is no longer
            // Limp — the bring-up entry in the tick ramps an enabled duck from
            // Limp to Homing. Under the old `core.runtime` dispatch, Init was
            // a no-op on the already-enabled parent and the branch (disabled)
            // stayed Limp forever, since the ramp only runs for `enabled`;
            // that is what makes this non-vacuous.
            for _ in 0..3 {
                duck_core_tick(core);
            }
            assert_eq!(
                active_runtime(c).bringup(),
                BringupState::Homing,
                "the branch came back up off the branch's own Init (the ramp is Homing, not Limp)"
            );
            assert!(
                matches!(c.runtime.bringup(), BringupState::Ready | BringupState::Homing),
                "the parent was never touched by the branch's Init: still up, still its own state"
            );
            assert_eq!(
                c.runtime.clock().tick,
                parent_tick,
                "and the parent is still not stepped"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// Ticket P: a pad Skill during the power-off sit is refused the way the
    /// intent arm refuses it (`:1109-1114`), and the refusal reaches the pad's
    /// refusal memory (`pad_refused`, the DKRB report) and `notice` — stamped on
    /// the watched duck's clock. The old pad Skill arm had no gate at all, so
    /// this whole test is the missing behaviour.
    #[test]
    fn a_pad_skill_is_refused_during_the_poweroff_and_remembered() {
        use crate::pad::{Buttons, PadFrame};
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            // selected == 0, so the watched duck is the original: put it into
            // the power-off sit and advance until the sequence has begun.
            on_active(c, |runtime| runtime.request_shutdown());
            for _ in 0..20 {
                duck_core_tick(core);
                if active_runtime(c).shutting_down() {
                    break;
                }
            }
            assert!(
                active_runtime(c).shutting_down(),
                "the duck is mid power-off"
            );

            let before = c.notice.clone();
            let now = Instant::now();
            // An A press = `Skill("a")` = sit_toggle: refused, the way a skill
            // is, because the power-off is counting on the seat.
            pad_apply_frame(
                c,
                &PadFrame {
                    held: Buttons::A,
                    pressed: Buttons::A,
                    ..Default::default()
                },
                now,
            );

            // The refusal is in the pad's refusal memory as the wire reports it:
            // method `robot.do`, the powering-off reason.
            let refusal = c
                .pad_refused
                .refusal(now)
                .expect("the refused pad skill is remembered for the DKRB report");
            assert_eq!(refusal.method, "robot.do", "{refusal:?}");
            assert!(
                refusal.reason.contains("powering off"),
                "the reason is the shutdown gate: {refusal:?}"
            );
            // And on the refusal channel every client reads, stamped on the
            // watched duck's clock — and it is a new notice, not the old one.
            let (notice_tick, notice_text) = c.notice.clone().expect("the refusal is a notice");
            assert_ne!(
                c.notice, before,
                "the gate put a fresh notice, it did not leave the old one"
            );
            assert!(notice_text.contains("powering off"), "{notice_text}");
            assert_eq!(notice_tick, active_runtime(c).clock().tick);
        }
        unsafe { duck_core_free(core) };
    }

    /// Ticket P+: a reload that replaces the network the duck is driving is
    /// queued behind the path home (robotd's `change_disturbs`), not swapped
    /// under a moving gait. End to end through the C ABI reload door: drive the
    /// duck, reload a directory whose walk slot resolves to a different file,
    /// and watch the swap wait for home before landing.
    ///
    /// Non-vacuity: the old `reload_policies_inner` called
    /// `runtime.replace_policy` (the immediate in-place swap), which never sets
    /// `pending_swap` and never reports `GoingHome` — under that code the very
    /// first assertion here (`swap_pending`) is false and the second (path still
    /// the old gait) is already swapped, so the test is red.
    #[test]
    fn a_reload_replacing_the_driving_network_walks_home_first() {
        // A policies directory with only the other walk network in it and no
        // manifest: `walk_gait` falls back to `alpha_walking.onnx` (velstand is
        // absent), so the walk slot really changes and nothing else loads.
        let dir = std::env::temp_dir().join(format!("duck-disturb-reload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp reload dir");
        let source = crate::fixtures::policies_dir().expect("the official bundle").join("alpha_walking.onnx");
        std::fs::copy(&source, dir.join("alpha_walking.onnx")).expect("copy the other walk file");

        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            // Hand the duck to the walk network and keep it driving, so a
            // change to the walk slot replaces the network in motion.
            on_active(c, |runtime| runtime.set_twist([0.4, 0.0, 0.0]));
            for _ in 0..10 {
                duck_core_tick(core);
            }
            let before = c
                .runtime
                .walking_slot_path()
                .expect("a walk network is loaded");
            assert!(
                before.ends_with("velstand.onnx"),
                "the startup set walks on velstand: {}",
                before.display()
            );

            let dir_str = CString::new(dir.to_string_lossy().into_owned()).unwrap();
            assert_eq!(
                duck_core_reload_policies(core, dir_str.as_ptr()),
                0,
                "the reload request took"
            );
            // Queued behind the path home, not swapped yet.
            assert!(
                c.runtime.swap_pending(),
                "a disturbing change to a driving duck is queued"
            );
            assert_eq!(
                c.runtime.swap_state(),
                Some(PolicySwap::GoingHome),
                "and the verdict is going home, not in place"
            );
            assert_eq!(
                c.runtime.walking_slot_path().as_deref(),
                Some(before.as_path()),
                "the running gait is still the old one until home is reached"
            );
            assert_eq!(c.label, "policy swap: walking home first");

            // Tick until the swap lands on the home pose.
            for _ in 0..400 {
                duck_core_tick(core);
                if !c.runtime.swap_pending() {
                    break;
                }
            }
            assert!(
                !c.runtime.swap_pending(),
                "the change lands once the duck is home"
            );
            let after = c.runtime.walking_slot_path().expect("still walking");
            assert!(
                after.ends_with("alpha_walking.onnx"),
                "the swapped set walks on the new file: {}",
                after.display()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { duck_core_free(core) };
    }

    /// Ticket P+: a reload that leaves the driving network alone lands in
    /// place. Reloading the set the duck already runs (the same walk file) is
    /// the cheap case: `disturbs` is false, so `change_disturbs` swaps wherever
    /// the duck stands and the verdict is `InPlace`, never the home walk.
    ///
    /// Non-vacuity: the old immediate `replace_policy` left `swap_pending`
    /// false; this asserts the queued swap is there with the `InPlace` verdict,
    /// which the old code never set.
    #[test]
    fn a_reload_that_leaves_the_driving_network_alone_lands_in_place() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            let before = c.runtime.walking_slot_path().expect("walk loaded");

            // The same directory the duck started from, read back off the
            // core: the walk slot resolves to exactly the path the running
            // policy holds, so the walk slot does not change and nothing about
            // the driving network moves. (Comparing a freshly-joined path here
            // rather than the core's own `policies` would be lexically unequal
            // to the loaded one and falsely report a disturbing change.)
            let dir = CString::new(c.policies.to_string_lossy().into_owned()).unwrap();
            assert_eq!(duck_core_reload_policies(core, dir.as_ptr()), 0);
            assert!(
                c.runtime.swap_pending(),
                "the change is queued (even an in-place one lands on the next tick)"
            );
            assert_eq!(
                c.runtime.swap_state(),
                Some(PolicySwap::InPlace),
                "leaving the driving network alone is an in-place change"
            );
            assert_eq!(
                c.runtime.walking_slot_path().as_deref(),
                Some(before.as_path()),
                "same file in, same file out"
            );
        }
        unsafe { duck_core_free(core) };
    }
}
