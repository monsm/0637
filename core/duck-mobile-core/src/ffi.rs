//! C ABI for the iOS app (SwiftUI/Metal) and any other shell.
//!
//! One `DuckCore` owns the runtime; the shell calls `duck_core_tick` at 50 Hz,
//! reads `duck_core_status` for the UI and the body/geom accessors for the
//! renderer. All pointers are borrowed for the call only.
#![allow(unsafe_op_in_unsafe_fn)]

use std::collections::VecDeque;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_float, c_int, c_ulonglong};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;

use duck_control::policy::Policy;
use duck_control::safety::SafetyConfig;

use crate::intent::{Intent, IntentQueue};
use crate::lan::GiftKind;
use crate::mujoco::MujocoIo;
use crate::runtime::{BringupState, DuckRuntime, HealthVerdict, PolicySwap, TickReport};

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
    /// What the `robot.*` read methods answer with (identity, slots, skills,
    /// model, vetbook, safe-to-restart). Refreshed once per tick from the duck
    /// the loop actually drove, for the same `Sync` reason as the health
    /// snapshot: the IPC handlers run on connection threads.
    robot_snapshot: Arc<std::sync::Mutex<RobotSnapshot>>,
    /// The power requests `robot.enable` / `init` / `relax` / `rest` /
    /// `shutdown` queue for the next tick. The runtime is owned by the control
    /// thread, so the handlers only queue; the tick applies them through the
    /// same branch swap every other intent takes.
    control: Arc<std::sync::Mutex<Vec<ControlCommand>>>,
    /// The manifest's skill table as the loaded bundle declared it, so
    /// `robot.skills` can answer with parameters rather than bare names.
    skill_defs: Vec<robotd_params::SkillDef>,
    /// The runtime that owns a queued (GoingHome) policy change — the
    /// `selected` index at queue time, 0 for the original — plus the loaded
    /// paths it will install. Branch sessions make the owner essential: a
    /// reload queued on branch B must not be judged against the original, and
    /// a reload of the original must not be consumed by a branch tick.
    swap_landing: Option<(usize, Vec<(String, Option<PathBuf>)>)>,
}

/// What the `robot.*` handlers can read without touching the runtime: the
/// identity frame, the slot report, the skills table and the lifetime record,
/// refreshed by [`publish_ipc`] on the thread that owns the body.
#[derive(Debug, Clone)]
struct RobotSnapshot {
    subscribe: duck_ipc_proto::SubscribeResult,
    policies: duck_ipc_proto::PoliciesResult,
    skills: duck_ipc_proto::SkillsResult,
    model: duck_ipc_proto::ModelResult,
    vetbook: serde_json::Value,
    mode: String,
    policies_dir: PathBuf,
    /// The policy-enabled mirror `robot.enable`'s `toggle` reads. Written by
    /// every door that changes the flag — the IPC control queue, the pad's
    /// Init/Toggle/Home, the C ABI enable/disable, `Intent::PowerOn`, a
    /// branch select or a time-machine restore, and a mode switch — and by
    /// [`publish_ipc`]'s powered-off/limp catch for the runtime's own
    /// transitions.
    enabled: bool,
    safe_to_restart: duck_ipc_proto::SafeToRestartResult,
}

impl RobotSnapshot {
    fn new(model: duck_ipc_proto::ModelResult, policies_dir: PathBuf) -> Self {
        Self {
            subscribe: duck_ipc_proto::SubscribeResult::default(),
            policies: duck_ipc_proto::PoliciesResult::default(),
            skills: duck_ipc_proto::SkillsResult::default(),
            model,
            vetbook: serde_json::Value::Null,
            mode: "walk".to_owned(),
            policies_dir,
            enabled: true,
            safe_to_restart: duck_ipc_proto::SafeToRestartResult {
                safe: true,
                reason: None,
            },
        }
    }
}

/// One power request queued by the IPC handlers for the next tick. The
/// official dispatch only queues these too (`robotd/src/main.rs:5587-5670`),
/// so the answer never waits for the body.
#[derive(Debug, Clone, Copy)]
enum ControlCommand {
    Enable(bool),
    Init,
    Relax,
    Rest,
    Shutdown,
    /// `robot.loadPolicy`: the override is already recorded; re-read the set on
    /// the tick thread with the official *slot change* disturb rule, not the
    /// reload-always-disturbs one.
    ReloadSlot,
    /// `robot.setSkill`/`robot.removeSkill`: the table entry is already
    /// edited; re-read the set on the tick thread with the official *reload*
    /// disturb rule — the official request triggers a `PolicyChange::Reload`
    /// (`robotd/src/main.rs`'s `set_skill_request`), and a skill-table change
    /// has no per-slot reading in `change_disturbs`.
    ReloadSkills,
}

/// Which official `PolicyChange` a re-read stands for
/// (`robotd/src/main.rs:2047-2070`): `Reload` disturbs whatever is driving;
/// a slot change disturbs only when the file behind the *driving* slot moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReloadKind {
    Reload,
    SlotChange,
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
    snapshot: Arc<std::sync::Mutex<RobotSnapshot>>,
    control: Arc<std::sync::Mutex<Vec<ControlCommand>>>,
}

impl RobotHandlers {
    fn queue_control(&self, command: ControlCommand) {
        if let Ok(mut queue) = self.control.lock() {
            queue.push(command);
        }
    }

    fn snapshot<T>(&self, read: impl FnOnce(&RobotSnapshot) -> T, fallback: T) -> T {
        match self.snapshot.lock() {
            Ok(snapshot) => read(&snapshot),
            Err(_) => fallback,
        }
    }
}

/// The official `FlashlightColor` wire spelling (`duck-ipc-proto`'s serde
/// rename: snake_case).
fn flashlight_color_name(color: duck_ipc_proto::FlashlightColor) -> &'static str {
    use duck_ipc_proto::FlashlightColor;
    match color {
        FlashlightColor::White => "white",
        FlashlightColor::Red => "red",
        FlashlightColor::Green => "green",
        FlashlightColor::Blue => "blue",
        FlashlightColor::Yellow => "yellow",
        FlashlightColor::Cyan => "cyan",
        FlashlightColor::Magenta => "magenta",
    }
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
        self.intents
            .push(Intent::Twist([params.vx, params.vy, params.vyaw]));
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
        // Official `robot.do`'s door checks (`robotd/src/main.rs:5431-5445`):
        // the policy must be driving, and a reload's walk home must have
        // finished. "Accepted then silently dropped by the loop" is the answer
        // the official dispatch refuses.
        let (enabled, homed) = self.snapshot(
            |snapshot| (snapshot.enabled, snapshot.policies.homed.unwrap_or(false)),
            (false, false),
        );
        if !enabled {
            return duck_ipc_proto::IntentResult::refused(
                "the policy is not driving — press Start on the pad",
            );
        }
        if !homed {
            return duck_ipc_proto::IntentResult::refused(
                "the robot is still going to its home pose; try again in a moment",
            );
        }
        // The same awaited round `/skill` answers with: the next tick decides,
        // and the verdict is the duck's own refusal reason.
        let answer = self.intents.request(Intent::Skill(params.skill.clone()));
        let accepted = answer
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
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

    fn look(&self, params: duck_ipc_proto::LookParams) -> duck_ipc_proto::LookResult {
        // The official dispatch runs the IK on the IPC side against the same
        // MJCF (`robotd/src/main.rs:5395-5410`) and feeds the joints to the
        // head intent path. The phone does the identical call — `look_at` is
        // the same `HeadFk::alpha()` the runtime uses — so the answer and the
        // intent cannot disagree.
        let gaze = kinematics::head::HeadFk::alpha()
            .look_at([params.x, params.y, params.z], params.neck_pitch);
        self.intents.push(Intent::Head(gaze.joints));
        let [neck_pitch, head_pitch, head_yaw, head_roll] = gaze.joints;
        duck_ipc_proto::LookResult {
            head: duck_ipc_proto::HeadParams {
                neck_pitch,
                head_pitch,
                head_yaw,
                head_roll,
            },
            clamped: gaze.clamped,
        }
    }

    fn pose(&self, params: duck_ipc_proto::PoseParams) -> duck_ipc_proto::IntentResult {
        // `PoseIntent`: the loop glides toward the pose while active; false
        // releases it at once (the official B-button exit).
        if params.active {
            self.intents
                .push(Intent::Body(params.z, params.roll, params.pitch));
        } else {
            self.intents.push(Intent::BodyRelease);
        }
        duck_ipc_proto::IntentResult::accepted()
    }

    fn mouth(&self, params: duck_ipc_proto::MouthParams) -> duck_ipc_proto::IntentResult {
        self.intents.push(Intent::Mouth(params.open));
        duck_ipc_proto::IntentResult::accepted()
    }

    fn sound(&self, params: duck_ipc_proto::SoundParams) -> duck_ipc_proto::IntentResult {
        use duck_ipc_proto::SoundTag;
        // The wheee tag is a level, not a one-shot: the official handler routes
        // it to the hold channel rather than the sound queue (`intents.rs:395-407`).
        if params.tag == SoundTag::Wheee {
            self.intents
                .push(Intent::Wheee(params.hold.unwrap_or(true)));
        } else {
            self.intents
                .push(Intent::Sound(params.tag.as_str().to_owned()));
        }
        duck_ipc_proto::IntentResult::accepted()
    }

    fn flashlight(&self, params: duck_ipc_proto::FlashlightParams) -> duck_ipc_proto::IntentResult {
        self.intents.push(Intent::Flashlight {
            toggle: params.toggle,
            on: params.on,
            color: flashlight_color_name(params.color).to_owned(),
        });
        duck_ipc_proto::IntentResult::accepted()
    }

    fn stop(&self) -> duck_ipc_proto::IntentResult {
        // Official `Intents::stop` is `set_twist([0; 3])`, not a disable
        // (`intents.rs:328-330`): the policy keeps running with a zero command.
        self.intents.push(Intent::Twist([0.0; 3]));
        duck_ipc_proto::IntentResult::accepted()
    }

    fn enable(&self, params: duck_ipc_proto::EnableParams) -> duck_ipc_proto::IntentResult {
        // `toggle` flips the robot's own state; evaluated against the shared
        // mirror every policy-enable door writes from the runtime, never
        // against a client-side belief (`robotd/src/main.rs:5711-5745`).
        let on = if params.toggle {
            !self.snapshot(|snapshot| snapshot.enabled, false)
        } else {
            params.on
        };
        self.queue_control(ControlCommand::Enable(on));
        // Match the official answer's reason. Not verified against the body —
        // the request is a queued intent on both sides.
        let reason = if on {
            "enabled — driving"
        } else {
            "disabled — returning to the home pose"
        };
        duck_ipc_proto::IntentResult {
            accepted: true,
            reason: Some(reason.to_owned()),
        }
    }

    fn init(&self) -> duck_ipc_proto::IntentResult {
        self.queue_control(ControlCommand::Init);
        duck_ipc_proto::IntentResult::accepted()
    }

    fn relax(&self) -> duck_ipc_proto::IntentResult {
        self.queue_control(ControlCommand::Relax);
        duck_ipc_proto::IntentResult::accepted()
    }

    fn rest(&self) -> duck_ipc_proto::IntentResult {
        self.queue_control(ControlCommand::Rest);
        duck_ipc_proto::IntentResult::accepted()
    }

    fn shutdown(&self) -> duck_ipc_proto::IntentResult {
        self.queue_control(ControlCommand::Shutdown);
        duck_ipc_proto::IntentResult::accepted()
    }

    fn mode(&self) -> duck_ipc_proto::ModeResult {
        duck_ipc_proto::ModeResult {
            mode: self.snapshot(|snapshot| snapshot.mode.clone(), "walk".to_owned()),
        }
    }

    fn set_mode(&self, params: duck_ipc_proto::SetModeParams) -> duck_ipc_proto::IntentResult {
        if params.mode != "walk" && params.mode != "roller" {
            return duck_ipc_proto::IntentResult::refused("mode must be \"walk\" or \"roller\"");
        }
        // The whole-bundle switch is the same `Intent::Mode` the HTTP `/mode`
        // route queues; "already in that mode" is an acceptance there too.
        self.intents.push(Intent::Mode(params.mode));
        duck_ipc_proto::IntentResult::accepted()
    }

    fn policies(&self) -> duck_ipc_proto::PoliciesResult {
        self.snapshot(
            |snapshot| snapshot.policies.clone(),
            duck_ipc_proto::PoliciesResult::default(),
        )
    }

    fn model(&self) -> duck_ipc_proto::ModelResult {
        self.snapshot(
            |snapshot| snapshot.model.clone(),
            duck_ipc_proto::ModelResult::default(),
        )
    }

    fn vetbook(&self) -> serde_json::Value {
        self.snapshot(|snapshot| snapshot.vetbook.clone(), serde_json::Value::Null)
    }

    fn skills(&self) -> duck_ipc_proto::SkillsResult {
        self.snapshot(
            |snapshot| snapshot.skills.clone(),
            duck_ipc_proto::SkillsResult::default(),
        )
    }

    fn load_policy(
        &self,
        params: duck_ipc_proto::LoadPolicyParams,
    ) -> duck_ipc_proto::IntentResult {
        // The official wire takes an absolute path; a phone has one policies
        // directory and no `robotd` config file, so the same request takes a
        // bare file name resolved against that directory. Separators and `..`
        // are refused: an override is a file beside the manifest, not a route
        // out of the bundle.
        let dir = self.snapshot(|snapshot| snapshot.policies_dir.clone(), PathBuf::new());
        if dir.as_os_str().is_empty() {
            return duck_ipc_proto::IntentResult::refused(
                "the policies directory is not known yet",
            );
        }
        // The one combination the official type allows and nothing means.
        if params.slot.is_none() && params.path.is_some() {
            return duck_ipc_proto::IntentResult::refused(
                "a path needs a slot; omit both to reset every slot to its default",
            );
        }
        match (params.slot.as_deref(), params.path.as_deref()) {
            // Reset every slot to the set's own.
            (None, None) => crate::manifest::clear_slot_overrides(),
            (Some(slot), path) => {
                let value = match path {
                    None => None,
                    Some(path) if crate::manifest::is_none_sentinel(path) => {
                        Some("none".to_owned())
                    }
                    Some(name) => {
                        if name.contains('\\') || name.contains('/') || name.contains("..") {
                            return duck_ipc_proto::IntentResult::refused(format!(
                                "{name:?} is not a file name in the policies directory"
                            ));
                        }
                        if !dir.join(name).is_file() {
                            return duck_ipc_proto::IntentResult::refused(format!(
                                "no policy {name:?} in {}",
                                dir.display()
                            ));
                        }
                        Some(name.to_owned())
                    }
                };
                // The phone's override table is the four networks it drives
                // itself (walk/stand/sitstand/ground_pick); the official
                // kick_left/kick_right/roulade slots are manifest skills here,
                // identified by skill name rather than slot key
                // (`manifest.rs`' `OVERRIDABLE_SLOTS`). The refusal names the
                // slots this build does have.
                if let Err(error) = crate::manifest::set_slot_override(slot, value) {
                    return duck_ipc_proto::IntentResult::refused(error);
                }
            }
            // Covered by the `(None, Some)` refusal above with the official wording.
            (None, Some(_)) => unreachable!("the path-without-slot case was refused above"),
        }
        // The reload is the half that opens and validates the files; a
        // refusal lands in the health gate (degraded) and on `notice`, the
        // same way the HTTP reload's does. It is queued as a *slot change*:
        // the official `robot.loadPolicy` writes one config key and compares
        // only the network that is driving, where `robot.reloadPolicies`
        // disturbs whatever is in motion.
        self.queue_control(ControlCommand::ReloadSlot);
        duck_ipc_proto::IntentResult::accepted()
    }

    fn reload_policies(&self) -> duck_ipc_proto::IntentResult {
        let dir = self.snapshot(|snapshot| snapshot.policies_dir.clone(), PathBuf::new());
        if dir.as_os_str().is_empty() {
            return duck_ipc_proto::IntentResult::refused(
                "the policies directory is not known yet",
            );
        }
        self.intents
            .push(Intent::Reload(dir.to_string_lossy().into_owned()));
        duck_ipc_proto::IntentResult::accepted()
    }

    fn set_skill(&self, params: duck_ipc_proto::SkillParams) -> duck_ipc_proto::IntentResult {
        use robotd_params::{SkillDef, SkillOverrides};

        // The official door, in its own order and words (`robotd/src/main.rs`'s
        // `set_skill_request`): the name first — it is what `robot.do` asks for,
        // and the two names the daemon drives itself cannot be table entries.
        if let Err(error) = crate::manifest::validate_skill_name(&params.name) {
            return duck_ipc_proto::IntentResult::refused(error);
        }
        // An existing entry supplies whatever this call leaves out, so a
        // client may send one field: the table's own entry with that name
        // first, then the entry the duck is running — a set skill re-timed
        // stays a set skill (same file, new length).
        let table_entry = crate::manifest::custom_skills().get(&params.name).cloned();
        let running = self.snapshot(
            |snapshot| {
                snapshot
                    .skills
                    .skills
                    .iter()
                    .find(|skill| skill.name == params.name)
                    .cloned()
            },
            None,
        );
        let existing_duration = table_entry
            .as_ref()
            .map(|skill| skill.duration)
            .or_else(|| running.as_ref().and_then(|skill| skill.duration));
        // The resolved table's `path` is a display path; the table stores the
        // bare name the policies directory resolves.
        let bare = |path: &str| {
            std::path::Path::new(path)
                .file_name()
                .map(|file| PathBuf::from(file.to_string_lossy().into_owned()))
        };
        let existing = table_entry.or_else(|| {
            running.map(|skill| SkillDef {
                name: skill.name.clone(),
                // The duration is merged separately (below); a running entry
                // that names no duration contributes none.
                duration: 0.0,
                path: skill.path.as_deref().and_then(bare),
                command: skill.command.unwrap_or_default(),
                unwind: skill.unwind.unwrap_or_default(),
                unwind_s: skill.unwind_s.unwrap_or(0.0),
                chain: skill.chain.unwrap_or(false),
                params: SkillOverrides {
                    action_scale: skill.action_scale,
                    gain_ratio: skill.gain_ratio,
                },
            })
        });
        let duration = match params.duration.or(existing_duration) {
            Some(duration) if duration > 0.0 => duration,
            Some(_) => {
                return duck_ipc_proto::IntentResult::refused(
                    "a skill's duration must be more than zero",
                );
            }
            None => {
                return duck_ipc_proto::IntentResult::refused(format!(
                    "{:?} is new here, so it needs a duration — how long it runs, in seconds",
                    params.name
                ));
            }
        };
        // The `.onnx`. The official wire takes an absolute path; a phone has
        // one policies directory and no config file, so the same request takes
        // a bare file name resolved against it — the same reading
        // `robot.loadPolicy` gets here. An absent path falls back to the
        // entry's own (or, for a new entry, to the `<name>.onnx` convention
        // the loader resolves). The `"none"` sentinel is the official
        // switch-off, not a file: the merge removes the same-named skill
        // instead of loading anything, so it is stored unchecked. A colon is
        // refused with the separators: on Windows `C:x.onnx` is a drive
        // prefix, and `Path::join` would replace the base with it.
        let path = match &params.path {
            Some(given) => {
                if crate::manifest::is_none_sentinel(given) {
                    Some(PathBuf::from(given))
                } else if given.contains('\\')
                    || given.contains('/')
                    || given.contains(':')
                    || given.contains("..")
                {
                    return duck_ipc_proto::IntentResult::refused(format!(
                        "{given:?} is not a file name in the policies directory"
                    ));
                } else {
                    let dir =
                        self.snapshot(|snapshot| snapshot.policies_dir.clone(), PathBuf::new());
                    if dir.as_os_str().is_empty() {
                        return duck_ipc_proto::IntentResult::refused(
                            "the policies directory is not known yet",
                        );
                    }
                    if !dir.join(given).is_file() {
                        return duck_ipc_proto::IntentResult::refused(format!(
                            "no such file: {}",
                            dir.join(given).display()
                        ));
                    }
                    Some(PathBuf::from(given))
                }
            }
            None => existing.as_ref().and_then(|skill| skill.path.clone()),
        };
        // **Through the same gate `robot.loadPolicy` puts a slot's file
        // through** (`duck_control::policy::validate`, which is what the
        // official `set_skill_request` calls before writing): a file that will
        // not open is refused here, while the caller is listening, instead of
        // surfacing later as a reload that kept the old set. Only when the
        // error is about the *file*: a board with no ONNX runtime validates
        // nothing, and refusing a config edit on those grounds would send
        // somebody to replace a policy that is fine.
        if let Some(file) = &path
            && !crate::manifest::is_none_sentinel(&file.to_string_lossy())
        {
            let dir = self.snapshot(|snapshot| snapshot.policies_dir.clone(), PathBuf::new());
            if let Err(error) = duck_control::policy::validate(&dir.join(file))
                && error.path().is_some()
            {
                return duck_ipc_proto::IntentResult::refused(error.to_string());
            }
        }
        let skill = SkillDef {
            name: params.name.clone(),
            path,
            duration,
            command: params
                .command
                .or(existing.as_ref().map(|skill| skill.command))
                .unwrap_or_default(),
            unwind: params
                .unwind
                .or(existing.as_ref().map(|skill| skill.unwind))
                .unwrap_or_default(),
            unwind_s: params
                .unwind_s
                .or(existing.as_ref().map(|skill| skill.unwind_s))
                .unwrap_or(0.0),
            chain: params
                .chain
                .or(existing.as_ref().map(|skill| skill.chain))
                .unwrap_or(false),
            params: SkillOverrides {
                action_scale: params.action_scale.or(existing
                    .as_ref()
                    .and_then(|skill| skill.params.action_scale)),
                gain_ratio: params
                    .gain_ratio
                    .or(existing.as_ref().and_then(|skill| skill.params.gain_ratio)),
            },
        };
        if let Err(error) = crate::manifest::upsert_custom_skill(skill) {
            return duck_ipc_proto::IntentResult::refused(error);
        }
        // **Written and reloaded here**, one call the whole operation: the
        // official request triggers `PolicyChange::Reload` after the write, so
        // a client never holds a robot whose table and behaviour disagree.
        self.queue_control(ControlCommand::ReloadSkills);
        duck_ipc_proto::IntentResult::accepted()
    }

    fn remove_skill(
        &self,
        params: duck_ipc_proto::SkillNameParams,
    ) -> duck_ipc_proto::IntentResult {
        if crate::manifest::remove_custom_skill(&params.name) {
            // The same reload the official request queues after the edit.
            self.queue_control(ControlCommand::ReloadSkills);
            duck_ipc_proto::IntentResult::accepted()
        } else {
            // `edit::remove_skill`'s `Ok(false)`: an acceptance that changes
            // nothing, saying so.
            duck_ipc_proto::IntentResult::already(format!(
                "no configured skill called {:?}",
                params.name
            ))
        }
    }

    fn safe_to_restart(&self) -> duck_ipc_proto::SafeToRestartResult {
        self.snapshot(
            |snapshot| snapshot.safe_to_restart.clone(),
            duck_ipc_proto::SafeToRestartResult {
                safe: true,
                reason: None,
            },
        )
    }

    fn subscribe_info(&self) -> duck_ipc_proto::SubscribeResult {
        self.snapshot(
            |snapshot| snapshot.subscribe.clone(),
            duck_ipc_proto::SubscribeResult {
                accepted: true,
                ..Default::default()
            },
        )
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
        if let (Ok(tag), Ok(text)) = (
            CString::new("duck-rust"),
            CString::new(format!("stage {name}")),
        ) {
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
    u32::from_str_radix(fingerprint.get(..8).unwrap_or_else(|| fingerprint), 16).unwrap_or_default()
}

/// `robot.model`: the static geometry the per-tick `robot.state` poses are
/// stated in, from the same `kinematics` crate the FK and the policies use
/// (`robotd/src/main.rs:5954-5966`).
///
/// `asset` carries the official family constant (`duck-ipc-proto`'s
/// `ROBOT_MODEL = "microduck"`) rather than "alpha": a client asks which robot
/// it is talking to, and every phone build runs the microduck family's alpha
/// MJCF. The static skeleton tree and `frames_at_zero` poses are left empty —
/// the mobile core has no skeleton publishing path yet (COMPARISON §5.1), and
/// zeros would claim link poses this build never computed.
fn robot_model_result() -> duck_ipc_proto::ModelResult {
    let model = kinematics::Model::alpha();
    let tof = kinematics::tof::Reprojector::alpha();
    duck_ipc_proto::ModelResult {
        asset: duck_ipc_proto::ROBOT_MODEL.to_owned(),
        trunk_height_m: model.trunk_height_m(),
        joint_names: duck_ipc_proto::JOINT_NAMES
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        head_joints: ["neck_pitch", "head_pitch", "head_yaw", "head_roll"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        tof_beams: tof.beams().to_vec(),
        tof_fov_deg: kinematics::tof::FOV_DEG,
        frames_at_zero: duck_ipc_proto::FramesState::default(),
        skeleton: Vec::new(),
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
            let message = CString::new(format!("rust panic: {payload} at {location}"))
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
        let body = load_body(&scene, &policies)?;
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
        // The room's microphone can hear a hand stroking the duck when this
        // build shipped the official CNN. Missing model, missing ONNX
        // Runtime, wrong shape: the feature stays off and the duck starts —
        // the classifier is an interaction, never a health verdict.
        if let Some(model) = pet_model_path(&scene, &policies) {
            match runtime.load_pet_detector(&model) {
                Ok(()) => stage(&format!("pet model {}", model.display())),
                Err(error) => stage(&format!("pet model unavailable: {error}")),
            }
        }
        stage("ready");

        // The shell's ring shows the same verbs `/skills` publishes, so a
        // button and an API call cannot disagree about what exists. The
        // loop's own verbs are listed only while the slot behind them is
        // loaded — the same condition `robotd`'s own list applies.
        let (sitstand_loaded, ground_pick_loaded) = (
            bundle.paths.sitstand.is_some(),
            bundle.paths.ground_pick.is_some(),
        );
        let skills = published_skills(runtime.skill_names(), sitstand_loaded, ground_pick_loaded)
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
        // The collection page lists the manifest's one-shots as tricks, apart
        // from the emotions `/skills` folds in with them.
        intents.set_manifest_skills(runtime.manifest_skill_names());
        intents.set_fingerprint(provenance.to_json());

        // The `robot.*` JSON-RPC server, started with the duck. Its handlers
        // share the core's intent queue, so the protocol's verbs reach the same
        // execution path as the HTTP API. A port that is taken is a label, not
        // a reason to refuse to start the duck.
        let health_snapshot = Arc::new(std::sync::Mutex::new(
            duck_ipc_proto::HealthResult::default(),
        ));
        // The read-side identity and the power-request queue: the IPC
        // handlers run on connection threads and the runtime belongs to the
        // control thread, so they get snapshots to read and a queue to write.
        let robot_snapshot = Arc::new(std::sync::Mutex::new(RobotSnapshot::new(
            robot_model_result(),
            policies.clone(),
        )));
        let control: Arc<std::sync::Mutex<Vec<ControlCommand>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let skill_defs = bundle.skills.clone();
        let handlers = RobotHandlers {
            intents: intents.clone(),
            health: health_snapshot.clone(),
            revision: provenance.fingerprint.clone(),
            snapshot: robot_snapshot.clone(),
            control: control.clone(),
        };
        let (ipc, label) = match crate::ipc::start(crate::ipc::ROBOT_IPC_PORT, Arc::new(handlers)) {
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

        let core = DuckCore {
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
            robot_snapshot,
            control,
            skill_defs,
            swap_landing: None,
        };
        // A read is truthful before the first tick too: the official daemon
        // serves `robot.policies` from the snapshot the loop publishes and
        // answers during startup rather than racing the first load. This is
        // that first publish.
        publish_ipc(&core);
        Ok(core)
    });

    let result = match result {
        Ok(result) => result,
        Err(payload) => Err(format!("panic during core creation: {payload:?}")),
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
    match reload_policies(core, &dir, ReloadKind::Reload, 0) {
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
        set_last_error(&format!(
            "{name:?} is not a file name in the policies directory"
        ));
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
pub unsafe extern "C" fn duck_core_load_emotions(core: *mut DuckCore, dir: *const c_char) -> c_int {
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
pub unsafe extern "C" fn duck_core_start_intent_server(core: *mut DuckCore, port: c_int) -> c_int {
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
            roller
                .exists()
                .then(|| roller.to_string_lossy().into_owned())
        }
    }
}

/// Where the official pet-detect model sits, if this build shipped it.
///
/// The shells stage non-policy models beside the scene/policies; the actual
/// layouts are `<root>/pet-detect/pet_detect.onnx` (Android
/// `filesDir/.../pet-detect/`, iOS `DuckResources/pet-detect/`) and the
/// older `<root>/models/pet_detect.onnx` convention used by the pickup
/// classifier. The core derives every candidate from the `scene_path` and
/// `policies_dir` its caller already passes — no platform path is baked in —
/// and accepts only a file that exists: no model staged means the feature is
/// off. `DUCK_PET_MODEL` names an explicit file (tests and debugging).
fn pet_model_path(scene: &str, policies: &Path) -> Option<std::path::PathBuf> {
    const MODEL: &str = "pet_detect.onnx";
    if let Some(explicit) = std::env::var_os("DUCK_PET_MODEL").map(std::path::PathBuf::from) {
        return explicit.is_file().then_some(explicit);
    }
    let mut candidates = Vec::new();
    let mut add_root = |root: &Path| {
        candidates.push(root.join("pet-detect").join(MODEL));
        candidates.push(root.join("models").join(MODEL));
        candidates.push(root.join(MODEL));
    };
    if let Some(scene_dir) = Path::new(scene).parent() {
        if let Some(root) = scene_dir.parent().and_then(Path::parent) {
            add_root(root);
        }
        add_root(scene_dir);
    }
    if let Some(root) = policies.parent() {
        add_root(root);
    }
    add_root(policies);
    candidates.into_iter().find(|path| path.is_file())
}

/// Where the shell staged its AR depth scan, if it wrote one.
///
/// The contract is `obstacles.xml` in the directory of the `scene_path` the
/// caller already passes — Android writes
/// `filesDir/duck/robot/microduck/obstacles.xml`, iOS plans the same shape
/// beside `DuckResources/robot/microduck/scene_backlash.xml` — with the
/// policy root accepted as a fallback for a build that stages assets
/// differently. No platform path is hard-coded; the scene parameter is the
/// single source of the location. `DUCK_OBSTACLES_DIR` names an explicit
/// directory for tests and debugging. A shell that never scanned leaves no
/// file, and the duck loads exactly as before.
fn obstacles_staging(scene: &str, policies: &Path) -> Option<std::path::PathBuf> {
    let has_scan = |dir: &Path| dir.join(crate::mujoco::OBSTACLES_FILE).is_file();
    if let Some(dir) = std::env::var_os("DUCK_OBSTACLES_DIR").map(std::path::PathBuf::from) {
        return has_scan(&dir).then_some(dir);
    }
    let mut candidates = Vec::new();
    if let Some(scene_dir) = Path::new(scene).parent() {
        candidates.push(scene_dir.to_path_buf());
    }
    if let Some(root) = policies.parent() {
        candidates.push(root.to_path_buf());
    }
    candidates.push(policies.to_path_buf());
    candidates.into_iter().find(|dir| has_scan(dir))
}

/// The body this build loads: the scene, with the room's scanned furniture
/// merged when one was staged.
fn load_body(scene: &str, policies: &Path) -> Result<MujocoIo, String> {
    match obstacles_staging(scene, policies) {
        Some(dir) => MujocoIo::load_with_obstacles(scene, dir),
        None => MujocoIo::load(scene),
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
    if core.runtime.swap_pending() {
        // A policy change is on its way home; swapping the whole bundle under
        // it would land one change on top of another (`robotd/src/main.rs:2906-2919`).
        return Err("a policy change is still being applied; try again when it lands".to_owned());
    }
    let scene = scene_for(core, mode)
        .ok_or_else(|| format!("no scene for {name} beside {}", core.scene))?;
    // Reload the set the duck is actually running — a /reload may have
    // replaced the startup directory with a user's own policy set.
    let bundle = match crate::manifest::load_mode(&core.policies, mode) {
        Ok(bundle) => bundle,
        Err(error) => {
            // A switch that never loaded still discovered which overrides
            // fell back: they are degraded reasons for the duck that keeps
            // running, exactly as a failed reload's are.
            for warning in crate::manifest::take_override_warnings() {
                core.runtime.mark_degraded(warning);
            }
            return Err(error);
        }
    };
    // The new set's skill table is what `robot.skills` reports from now on
    // (`robotd`'s `robot.skills` reads the configured table, not the loaded
    // snapshot; `main.rs:4880-4924`). Kept even if the swap is refused below —
    // then it is the table the next successful switch will run.
    core.skill_defs = bundle.skills.clone();
    let mut policy = Policy::load(&bundle.paths, 0.05).map_err(|error| error.to_string())?;
    if bundle.standing_disabled {
        // No standing network in the roller bundle: command magnitude must
        // not select one.
        policy.set_standing_disabled(true);
    }
    let body = load_body(&scene, &core.policies)?;
    let mut runtime = DuckRuntime::new(
        body,
        policy,
        bundle.tuning(),
        bundle.skill_tuning,
        SafetyConfig::default(),
    );
    // The clock and pose the duck is in now. A limp duck — relaxed, or left
    // in the rest pose by `robot.rest` — swaps modes and **stays limp**; the
    // official switch only ramps a robot that was up (`robotd/src/main.rs:2930-2960`).
    let was_limp = matches!(core.runtime.bringup(), BringupState::Limp);
    let clock = core.runtime.clock();
    runtime.set_paused(clock.paused);
    runtime.set_speed(clock.speed);
    if !was_limp {
        runtime.enable().map_err(|error| error.to_string())?;
    }
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
    // The petting classifier is part of the duck too, and its session was
    // built on the runtime this switch replaces. Same model, same thresholds,
    // fresh ring.
    if let Some(model) = core.runtime.pet_detector_model().map(Path::to_path_buf) {
        let _ = runtime.load_pet_detector(&model);
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
    // The fresh runtime has no pending swap; a landing marker for the old
    // runtime must not be applied to it.
    core.swap_landing = None;
    // The fresh runtime was enabled above, so the `robot.enable toggle`
    // mirror follows the body that is now running.
    if let Ok(mut snapshot) = core.robot_snapshot.lock() {
        snapshot.enabled = runtime_policy_enabled(&core.runtime);
    }
    // Overrides this load fell back from: the official boot-time report, as
    // degraded reasons on the duck that is now running. Never unhealthy — the
    // bundle did load, just not the slot that was asked for.
    for warning in crate::manifest::take_override_warnings() {
        core.runtime.mark_degraded(warning);
    }
    // The new bundle resolved its slots: apply its own policy-error verdict
    // (there is none after a successful load).
    core.runtime.clear_policy_error();
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
    // The tricks are the new bundle's own one-shots; a switch changes them.
    core.intents
        .set_manifest_skills(core.runtime.manifest_skill_names());
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

fn reload_policies(
    core: &mut DuckCore,
    dir: &str,
    kind: ReloadKind,
    owner: usize,
) -> Result<Reloaded, String> {
    let result = reload_policies_inner(core, dir, kind, owner);
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
    // An override the loader had to fall back from is robotd's own degraded
    // reason (`robotd/src/main.rs:561-600`: a failed slot override is reported
    // on the slot and judged degraded). `manifest` queues each one once, at
    // the load that discovered it; drain them here, on the same thread that
    // owns the health gate, so they do not linger into an unrelated reload.
    for warning in crate::manifest::take_override_warnings() {
        core.runtime.mark_degraded(warning);
    }
    // The policy-error (unhealthy) door has no mobile equivalent at this
    // seam: `Policy::load` is mandatory at core creation, and a failed reload
    // or mode switch keeps the set that is running, so the duck never ticks
    // with no policy. The runtime's own `policy_error` is set by the paths
    // that can lose a network; `clear_policy_error` runs when a reload lands.
    if matches!(&result, Ok(Reloaded::Landed)) {
        core.runtime.clear_policy_error();
    }
    result
}

/// `robotd`'s `change_disturbs` (`robotd/src/main.rs:2047-2070`), with the
/// one-network scope the mobile core can see.
///
/// Official rule, verbatim in shape:
/// - nothing driving → not disturbing (`:2053-2055`);
/// - `Driving::Seated` → not disturbing (`:2056-2058`) — a parked duck holds
///   a static pose, so a swap under it is invisible;
/// - `PolicyChange::Reload` → disturbing (`:2059-2061`), because the paths
///   cannot say whether the bytes behind the same path changed;
/// - otherwise only the network that stepped last tick counts: walk vs walk,
///   stand vs stand, sitstand vs sitstand, ground_pick vs ground_pick, and
///   the whole skill list for a skill (skills are addressed by index)
///   (`:2062-2070`).
///
/// `driving` is gated by `was_driving` at the call site (`:3040-3047`): a
/// disabled, limp or mid-ramp duck has nothing running that a swap could
/// interrupt.
fn reload_disturbs(
    kind: ReloadKind,
    driving: Option<&crate::control::Driving>,
    before: &[(String, Option<PathBuf>)],
    after: &duck_control::policy::PolicyPaths,
) -> bool {
    use crate::control::Driving;
    let Some(driving) = driving else {
        return false;
    };
    if matches!(driving, Driving::Seated) {
        return false;
    }
    if kind == ReloadKind::Reload {
        return true;
    }
    let before_slot = |name: &str| {
        before
            .iter()
            .find(|(slot, _)| slot == name)
            .and_then(|(_, path)| path.clone())
    };
    match driving {
        Driving::Walk => before_slot("walk") != Some(after.walk.clone()),
        Driving::Stand => before_slot("stand") != after.stand.clone(),
        Driving::SitStand => before_slot("sitstand") != after.sitstand.clone(),
        Driving::Seated => false,
        Driving::GroundPick => before_slot("ground_pick") != after.ground_pick.clone(),
        Driving::Skill(_) => {
            // `driving_net_paths` lists the four fixed slots first, then every
            // configured skill in index order — the order a running skill is
            // addressed by, which is what the official comparison protects.
            let before_skills: Vec<Option<PathBuf>> = before
                .iter()
                .skip(4)
                .map(|(_, path)| path.clone())
                .collect();
            let after_skills: Vec<Option<PathBuf>> =
                after.skills.iter().cloned().map(Some).collect();
            before_skills != after_skills
        }
    }
}

/// The exact `driving_net_paths` list a bundle will publish once it lands:
/// the four fixed slots, then every configured skill by name in index order.
/// Used to tell a landed swap from one the runtime dropped (a rewind or a
/// shutdown clears `pending_swap` without installing anything).
fn expected_loaded_paths(
    paths: &duck_control::policy::PolicyPaths,
    skills: &[robotd_params::SkillDef],
) -> Vec<(String, Option<PathBuf>)> {
    let mut out = vec![
        ("walk".to_owned(), Some(paths.walk.clone())),
        ("stand".to_owned(), paths.stand.clone()),
        ("sitstand".to_owned(), paths.sitstand.clone()),
        ("ground_pick".to_owned(), paths.ground_pick.clone()),
    ];
    for (index, path) in paths.skills.iter().enumerate() {
        let name = skills
            .get(index)
            .map(|skill| skill.name.clone())
            .unwrap_or_else(|| index.to_string());
        out.push((name, Some(path.clone())));
    }
    out
}

/// A queued policy change landed: publish what the running controller really
/// holds — not the requested bundle — and clear the degraded mark the change
/// carried. The reload's skill table and slot flags are deliberately not
/// published at request time for a `GoingHome` change: the old set is still
/// the one driving until the ramp finishes.
fn publish_loaded_skills(core: &mut DuckCore) {
    let (paths, names, manifest_skills) = {
        let active = active_runtime(core);
        let paths = active.driving_net_paths();
        let sitstand = paths
            .iter()
            .find(|(slot, _)| slot == "sitstand")
            .is_some_and(|(_, path)| path.is_some());
        let ground_pick = paths
            .iter()
            .find(|(slot, _)| slot == "ground_pick")
            .is_some_and(|(_, path)| path.is_some());
        let names = published_skills(active.skill_names(), sitstand, ground_pick);
        (paths, names, active.manifest_skill_names())
    };
    core.sitstand_loaded = paths
        .iter()
        .find(|(slot, _)| slot == "sitstand")
        .is_some_and(|(_, path)| path.is_some());
    core.ground_pick_loaded = paths
        .iter()
        .find(|(slot, _)| slot == "ground_pick")
        .is_some_and(|(_, path)| path.is_some());
    core.skills = names
        .iter()
        .map(|name| CString::new(name.clone()).unwrap_or_default())
        .collect();
    core.intents.set_skills(names);
    core.intents.set_manifest_skills(manifest_skills);
}

/// A queued policy change landed on the duck the client is watching: publish
/// its skill table and clear the degraded mark **on that duck**. A branch
/// session's landing must not clear the original's health, and vice versa.
fn publish_landed_policy_set(core: &mut DuckCore) {
    publish_loaded_skills(core);
    on_active(core, |runtime| {
        runtime.clear_degraded();
        runtime.clear_policy_error();
    });
    core.label = "policy reloaded".to_owned();
}

fn clear_swap_landing_for_active(core: &mut DuckCore) {
    if core
        .swap_landing
        .as_ref()
        .is_some_and(|(owner, _)| *owner == core.selected)
    {
        core.swap_landing = None;
    }
}

/// The production landing hook, shared by the 50 Hz tick and the time
/// machine's step/seek doors. A marker belongs to one runtime (`owner`); a
/// tick of another duck must neither judge nor consume it.
fn settle_policy_swap(core: &mut DuckCore) {
    let Some((owner, _)) = core.swap_landing.as_ref() else {
        return;
    };
    if *owner != core.selected {
        return;
    }
    let (pending, loaded, shutting) = {
        let active = active_runtime(core);
        (
            active.swap_pending(),
            active.driving_net_paths(),
            active.shutting_down(),
        )
    };
    if pending {
        return;
    }
    let Some((_, expected)) = core.swap_landing.take() else {
        return;
    };
    // A rewind or a shutdown clears `pending_swap` without installing the
    // set. Comparing the loaded paths is what tells the two apart.
    if !shutting && loaded == expected {
        publish_landed_policy_set(core);
    }
}

fn reload_policies_inner(
    core: &mut DuckCore,
    dir: &str,
    kind: ReloadKind,
    owner: usize,
) -> Result<Reloaded, String> {
    // Reload the set this duck is actually running, not always the walking one.
    let bundle = crate::manifest::load_mode(Path::new(dir), core.runtime.mode())?;
    let mut policy = Policy::load(&bundle.paths, 0.05).map_err(|error| error.to_string())?;
    if bundle.standing_disabled {
        policy.set_standing_disabled(true);
    }
    let tuning = bundle.tuning();
    // The configured skill table follows the set that was asked for; it is
    // what `robot.skills` reports (`robotd`'s `skills_report` reads config,
    // not the loaded snapshot).
    core.skill_defs = bundle.skills.clone();
    // `robotd`'s `change_disturbs` gate (main.rs:2047-2070): a change that
    // replaces the network the duck is driving is not swapped under a moving
    // gait — the next tick would be a different policy's idea of what the legs
    // were doing. The duck goes to the home pose first and the swap lands
    // there, torque on throughout. The comparison is per-slot, against the
    // duck's own `driving_net_paths` (the loaded paths, not an overlay), and a
    // `Reload` disturbs whenever something is driving even if every path is
    // identical — that is exactly the case it exists for (same path, new
    // bytes). Only a network that stepped last tick counts (`driving_prev` is
    // private; `enabled && Ready` is its public equivalent — a disabled, limp
    // or mid-ramp duck drives nothing), the official `was_driving` gate at
    // main.rs:3040-3047.
    let was_driving = matches!(core.runtime.bringup(), BringupState::Ready)
        && core
            .runtime
            .checkpoint()
            .is_some_and(|checkpoint| checkpoint.enabled);
    let driving = if was_driving {
        core.runtime.driving()
    } else {
        None
    };
    let before = core.runtime.driving_net_paths();
    let disturbs = reload_disturbs(kind, driving.as_ref(), &before, &bundle.paths);
    let landing = expected_loaded_paths(&bundle.paths, &bundle.skills);
    match core
        .runtime
        .replace_policy_disturbing(policy, tuning, bundle.skill_tuning, disturbs)
    {
        PolicySwap::Refused => {
            core.swap_landing = None;
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
            core.swap_landing = Some((owner, landing));
            core.policies = PathBuf::from(dir);
            core.label = "policy swap: walking home first".to_owned();
            return Ok(Reloaded::GoingHome);
        }
        PolicySwap::InPlace => {
            core.swap_landing = None;
        }
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
    let tricks: Vec<String> = bundle
        .skills
        .iter()
        .take(bundle.paths.skills.len())
        .map(|skill| skill.name.clone())
        .collect();
    let mut names = tricks.clone();
    names.extend(core.runtime.emotion_names());
    let names = published_skills(names, core.sitstand_loaded, core.ground_pick_loaded);
    core.skills = names
        .iter()
        .map(|name| CString::new(name.clone()).unwrap_or_default())
        .collect();
    core.intents.set_skills(names);
    core.intents.set_manifest_skills(tricks);
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
        std::mem::swap(&mut core.runtime, &mut core.branches[selected - 1].runtime);
        let checkpoint = core.runtime.checkpoint();
        std::mem::swap(&mut core.runtime, &mut core.branches[selected - 1].runtime);
        checkpoint
    } else {
        core.runtime.checkpoint()
    };
    let Some(checkpoint) = checkpoint else {
        return;
    };
    // One entry per tick: a second checkpoint at the same tick replaces the
    // first rather than stacking.
    if core
        .timeline
        .back()
        .is_some_and(|last| last.tick == checkpoint.tick)
    {
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
    let stepping = matches!(&intent, Intent::Step(_));
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
        let result = apply_intent_active(core, intent);
        // A step runs the selected duck's ticks outside `duck_core_tick`; a
        // queued policy change can land inside them.
        if stepping {
            settle_policy_swap(core);
            publish_state(core);
        }
        return result;
    }
    std::mem::swap(&mut core.runtime, &mut core.branches[selected - 1].runtime);
    let result = apply_intent_active(core, intent);
    std::mem::swap(&mut core.runtime, &mut core.branches[selected - 1].runtime);
    if stepping {
        settle_policy_swap(core);
        publish_state(core);
    }
    result
}

/// The policy-enabled flag the `robot.enable toggle` reply is decided from.
/// The runtime has no getter of its own (`runtime.rs` deliberately keeps the
/// flag private), but a checkpoint carries it — and a power request is rare
/// enough that one snapshot per call costs nothing.
fn runtime_policy_enabled(active: &DuckRuntime<MujocoIo>) -> bool {
    active
        .checkpoint()
        .map(|checkpoint| checkpoint.enabled)
        .unwrap_or(false)
}

/// The official loop's gate for one-shot skills, sit/stand and ground pick:
/// `snapshot.enabled && bringup == Ready && shutdown_sit.is_none()`
/// (`robotd/src/main.rs:2768-2775`). The emotion path is the same driving
/// question (`was_driving`, `:2813-2820`). A duck in the rest pose is Limp and
/// disabled, so this refuses there instead of accepting a move the forgotten
/// controller would freeze until the next Start.
fn policy_drives(active: &DuckRuntime<MujocoIo>) -> bool {
    !active.shutting_down()
        && matches!(active.bringup(), BringupState::Ready)
        && runtime_policy_enabled(active)
}

/// Keep the `robot.enable toggle` mirror in step with the body at every door
/// that changes the policy flag. The tick thread owns the runtime; the IPC
/// handler reads this shared cell, so each door writes what the runtime just
/// reported rather than letting a client-side belief drift.
fn set_policy_enabled_mirror(core: &DuckCore, enabled: bool) {
    if let Ok(mut snapshot) = core.robot_snapshot.lock() {
        snapshot.enabled = enabled;
    }
}

/// Apply one queued `robot.enable`/`init`/`relax`/`rest`/`shutdown` through
/// the duck the client is watching — the same branch swap every intent takes.
/// Returns the watched duck's resulting policy-enabled flag, for the mirror
/// `robot.enable toggle` reads.
fn apply_control(core: &mut DuckCore, command: ControlCommand) -> bool {
    let selected = core.selected;
    // `robot.loadPolicy` re-reads through the same disturb gate as the HTTP
    // reload, but as a slot change; `robot.setSkill`/`removeSkill` re-read as
    // the official reload. Both run on the tick thread so they cannot race a
    // concurrent `Intent::Reload`; the directory is the one this core runs.
    let kind = match command {
        ControlCommand::ReloadSlot => Some(ReloadKind::SlotChange),
        ControlCommand::ReloadSkills => Some(ReloadKind::Reload),
        _ => None,
    };
    let run = |core: &mut DuckCore| -> bool {
        if let Some(kind) = kind {
            let dir = core.policies.to_string_lossy().into_owned();
            if let Err(reason) = reload_policies(core, &dir, kind, selected) {
                core.label = format!("reload failed: {reason}");
                core.notice = Some((core.runtime.clock().tick, core.label.clone()));
            }
            return runtime_policy_enabled(&core.runtime);
        }
        apply_control_to(&mut core.runtime, command)
    };
    let (enabled, swapped) = if selected > 0 && selected <= core.branches.len() {
        std::mem::swap(&mut core.runtime, &mut core.branches[selected - 1].runtime);
        (run(core), true)
    } else {
        (run(core), false)
    };
    if swapped && selected > 0 {
        std::mem::swap(&mut core.runtime, &mut core.branches[selected - 1].runtime);
    }
    enabled
}

fn apply_control_to(runtime: &mut DuckRuntime<MujocoIo>, command: ControlCommand) -> bool {
    match command {
        // Official `robot.enable`: stop/start the *policy*, torque untouched
        // on the way down (`runtime.rs`' `set_policy_enabled`).
        ControlCommand::Enable(on) => runtime.set_policy_enabled(on),
        // `robot.init`'s four official branches live in `runtime.init`.
        ControlCommand::Init => {
            let _ = runtime.init();
        }
        // `robot.relax`: torque off, stay powered and limp.
        ControlCommand::Relax => runtime.relax(),
        // `robot.rest`: sit and ease into the rest pose, no power-off latch.
        ControlCommand::Rest => runtime.request_rest(),
        // `robot.shutdown`: sit, rest, torque off, power-off latch.
        ControlCommand::Shutdown => runtime.request_shutdown(),
        // Handled by `apply_control` before it reaches the runtime: a reload
        // needs the core's manifest and slot table, not a runtime method.
        ControlCommand::ReloadSlot | ControlCommand::ReloadSkills => {}
    }
    runtime_policy_enabled(runtime)
}

/// Dispatch one intent with the runtime `apply_intent` prepared.
///
/// **Invariant — read this before adding an arm.** For every intent that
/// `apply_intent` routes through the branch swap (everything outside its
/// `global` list), `core.runtime` *is* the selected branch when this function
/// runs. Those arms must speak to `core.runtime` and must not call
/// [`on_active`] or [`active_runtime`]: both swap/read the branch slot, which
/// holds the **original** runtime while the swap is in flight. The global
/// arms (`Mode`, `Fork`, `DropBranch`, `SelectBranch`, `QuestDay`, `Backup`,
/// `RestoreBackup`, `TrajectoryExport`) deliberately run without a swap; they
/// are the only place [`active_runtime`] is the right reader.
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
        // Frozen time freezes the duck: a skill, a sit toggle or a ground pick
        // accepted while paused would change the body — and a skill would be
        // counted in the life record — without the simulated tick that
        // performs it ever running. Refuse them at the door, as the typed C
        // entry points do.
        Intent::Skill(_) | Intent::Sit | Intent::GroundPick if core.runtime.clock().paused => {
            refuse(core, "the duck is paused".to_owned())
        }
        // The loop's one-shot gate (`robotd/src/main.rs:2768-2775`): a skill,
        // a sit toggle or a ground pick needs a policy that drove last tick
        // and a robot at home, not merely a powered body. A resting duck is
        // Limp and disabled; accepting here froze the move until Start.
        Intent::Skill(_) | Intent::Sit | Intent::GroundPick if !policy_drives(&core.runtime) => {
            refuse(
                core,
                "the policy is not driving — press Start on the pad".to_owned(),
            )
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
                    .map(|started| json!({ "ok": true, "skill": name, "chained": !started }))
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
            Ok(()) => {
                set_policy_enabled_mirror(core, true);
                json!({ "ok": true, "powered_off": false })
            }
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
                    core.runtime
                        .emotion_names()
                        .iter()
                        .any(|loaded| loaded == name)
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
            // an unmatched deserialization to an empty history. The parent's
            // day view (`days` + `day_started`) travels in the same summary;
            // it is restored last, below, where the whole life is known.
            let book = core.runtime.book_mut();
            let life = &mut book.life;
            let num = |key: &str| -> Option<f64> { saved.get(key).and_then(|v| v.as_f64()) };
            if let Some(v) = num("powered_s") {
                life.powered_s = v;
            }
            if let Some(v) = num("torqued_s") {
                life.torqued_s = v;
            }
            if let Some(v) = num("walking_s") {
                life.walking_s = v;
            }
            if let Some(v) = num("standing_s") {
                life.standing_s = v;
            }
            if let Some(v) = num("sitting_s") {
                life.sitting_s = v;
            }
            if let Some(v) = num("held_s") {
                life.held_s = v;
            }
            if let Some(v) = num("boots") {
                life.boots = v as u64 + 1;
            }
            if let Some(v) = num("falls") {
                life.falls = v as u64;
            }
            if let Some(v) = num("pickups") {
                life.pickups = v as u64;
            }
            if let Some(v) = num("ground_picks") {
                life.ground_picks = v as u64;
            }
            if let Some(v) = num("sit_downs") {
                life.sit_downs = v as u64;
            }
            if let Some(v) = num("stand_ups") {
                life.stand_ups = v as u64;
            }
            if let Some(v) = num("wake_ups") {
                life.wake_ups = v as u64;
            }
            if let Some(v) = num("pets") {
                life.pets = v as u64;
            }
            if let Some(v) = num("sound_pets") {
                life.sound_pets = v as u64;
            }
            if let Some(v) = num("fed") {
                life.fed = v as u64;
            }
            if let Some(v) = num("claps") {
                life.claps = v as u64;
            }
            if let Some(v) = num("whistles") {
                life.whistles = v as u64;
            }
            if let Some(v) = num("metres") {
                life.metres = v;
            }
            if let Some(v) = num("power_offs") {
                life.power_offs = v as u64;
            }
            if let Some(m) = saved.get("skills").and_then(|v| v.as_object()) {
                life.skills = m
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_u64()?)))
                    .collect();
            }
            if let Some(m) = saved.get("emotions").and_then(|v| v.as_object()) {
                life.emotions = m
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_u64()?)))
                    .collect();
            }
            // The collection's first days travel with the same save. An entry
            // is kept only if the name really exists in the count tables this
            // restore just rebuilt (with a count above zero) and the value is
            // an unknown day or a real calendar day. This route answers on the
            // network, and a first day is shown to a person: a client must not
            // be able to write a sentence, a made-up date, or a date for a
            // move the duck never did into it.
            let mut unlocks = std::collections::BTreeMap::new();
            if let Some(m) = saved.get("unlocks").and_then(|v| v.as_object()) {
                for (key, value) in m {
                    let Some(day) = value.as_str() else {
                        continue;
                    };
                    if !crate::vetbook::is_unlock_day(day) {
                        continue;
                    }
                    let counted = if let Some(name) = key.strip_prefix("emotion:") {
                        life.emotions.get(name).copied().unwrap_or(0)
                    } else if let Some(name) = key.strip_prefix("skill:") {
                        life.skills.get(name).copied().unwrap_or(0)
                    } else {
                        0
                    };
                    if counted > 0 {
                        unlocks.insert(key.clone(), day.to_owned());
                    }
                }
            }
            // The flag is not trusted on its own. A record that claims to be
            // backfilled but does not cover every count that exists would let
            // the next tick date the gaps with today — a date that never
            // happened. Missing entries are history from before the record, so
            // they are filled with an unknown day here, once, and the record
            // is left backfilled.
            let saved_flag = saved
                .get("unlocks_backfilled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut complete = true;
            for (name, count) in &life.emotions {
                if *count > 0 && !unlocks.contains_key(&format!("emotion:{name}")) {
                    complete = false;
                }
            }
            for (name, count) in &life.skills {
                if *count > 0 && !unlocks.contains_key(&format!("skill:{name}")) {
                    complete = false;
                }
            }
            if !saved_flag || !complete {
                for (name, count) in &life.emotions {
                    if *count > 0 {
                        unlocks.entry(format!("emotion:{name}")).or_default();
                    }
                }
                for (name, count) in &life.skills {
                    if *count > 0 {
                        unlocks.entry(format!("skill:{name}")).or_default();
                    }
                }
            }
            life.unlocks = unlocks;
            life.unlocks_backfilled = true;
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
            // The parent's day view, after the life it must agree with is in
            // place. `restore_days` keeps the saved baseline only when it is
            // today's and re-bases otherwise — so a save restored after the
            // day rolled over can never have the duck's whole lifetime read
            // as today's play. The completed rows are history and survive the
            // restart as rows.
            let today = core.intents.day();
            book.restore_days(&saved, &today);
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
        Intent::Posture(_) if core.runtime.clock().paused => {
            refuse(core, "the duck is paused".to_owned())
        }
        Intent::Posture(sit) => {
            if core.runtime.is_sitting() == sit {
                // Already in the asked-for posture: no move to gate.
                json!({ "ok": true, "sitting": sit, "unchanged": true })
            } else if !policy_drives(&core.runtime) {
                // The seat is driven by the sitstand network; it needs the same
                // driving robot the official skill gate requires
                // (`robotd/src/main.rs:2768-2775`).
                refuse(
                    core,
                    "the policy is not driving — press Start on the pad".to_owned(),
                )
            } else {
                match core.runtime.sit_toggle() {
                    Ok(_) => json!({ "ok": true, "sitting": sit }),
                    Err(reason) => refuse(core, format!("posture refused: {reason}")),
                }
            }
        }
        Intent::QuestDay(day, board) => {
            if crate::vetbook::is_calendar_day(&day) {
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
            // `apply_intent` swapped the selected branch into `core.runtime`;
            // both the checks and the start speak to that duck. on_active or
            // active_runtime here would swap back to the original.
            if core.runtime.chorus_active() {
                return refuse(core, "a chorus is already playing".to_owned());
            }
            // The route checked the published list; the runtime is the
            // authority on which official emotions this build loaded.
            if !moves
                .iter()
                .all(|name| core.runtime.emotion_names().iter().any(|loaded| loaded == name))
            {
                return refuse(core, "no such emotion".to_owned());
            }
            let id = format!("chorus-{}", core.runtime.clock().tick);
            let sent = core.lan.as_ref().map(|lan| lan.chorus(&id, &moves));
            match sent {
                Some(Ok(())) => {
                    let peers = core.lan.as_ref().map_or(0, crate::lan::Lan::peer_count);
                    core.runtime
                        .start_chorus("lead", moves.clone(), peers, Some(id));
                    json!({ "ok": true, "queued": true, "moves": moves })
                }
                Some(Err(error)) => refuse(core, format!("chorus refused: {error}")),
                None => refuse(core, "the LAN peer port is not available".to_owned()),
            }
        }
        Intent::Ensemble(moves, sounds) => {
            if core.runtime.chorus_active() {
                return refuse(core, "a performance is already playing".to_owned());
            }
            if !moves
                .iter()
                .all(|name| core.runtime.emotion_names().iter().any(|loaded| loaded == name))
                || !sounds
                    .iter()
                    .all(|tag| crate::voice::TAGS.contains(&tag.as_str()))
            {
                return refuse(core, "no such mood or sound".to_owned());
            }
            let id = format!("ensemble-{}", core.runtime.clock().tick);
            let sent = core
                .lan
                .as_ref()
                .map(|lan| lan.ensemble(&id, &moves, &sounds));
            match sent {
                Some(Ok(roles)) => {
                    let peers = core.lan.as_ref().map_or(0, crate::lan::Lan::peer_count);
                    // The lead plays too: its own part is the first role.
                    core.runtime.start_ensemble(
                        "lead",
                        0,
                        roles + 1,
                        moves.clone(),
                        sounds.clone(),
                        peers,
                        Some(id.clone()),
                    );
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
            if value.get("format").and_then(serde_json::Value::as_str) != Some("microduck-backup/1")
            {
                return refuse(core, "unknown backup format".to_owned());
            }
            // A global intent gets no branch swap from `apply_intent`, so the
            // restore must swap explicitly: `on_active` is the correct
            // swapper in this context, and the answers are read after the
            // swap so they describe the duck that was written. (The QuestDay
            // call below is global-safe as written: it reads the watched duck
            // through `active_runtime`.)
            //
            // Three parts, three of the duck's own doors: the care restore
            // clamps what it is given, the quest board re-measures against
            // the duck as it is now, and the rules vocabulary refuses a
            // program with a word the duck does not know. Nothing here
            // accepts a sentence, a number without a source, or a new kind
            // of fact.
            let saved_care = value.get("care").cloned().unwrap_or(json!({}));
            let mut care = json!(null);
            on_active(core, |runtime| {
                runtime.restore_care(&saved_care);
                care = json!({ "ok": true, "care": runtime.care_summary() });
            });
            let day = value
                .get("day")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_owned();
            let quests = if crate::vetbook::is_calendar_day(&day) {
                apply_intent_active(
                    core,
                    Intent::QuestDay(day, value.get("quests").cloned().unwrap_or(json!({}))),
                )
            } else {
                json!({ "ok": false, "error": "the backup carries no calendar day" })
            };
            let saved_rules = value.get("rules").cloned().unwrap_or(json!({}));
            let mut rules_answer = json!(null);
            let mut rules_error = None;
            on_active(core, |runtime| match runtime.set_rules(&saved_rules) {
                Ok(()) => {
                    rules_answer = json!({ "ok": true, "rules": runtime.rules_summary() })
                }
                Err(reason) => rules_error = Some(reason),
            });
            // A refused program still goes through the refusal channel, the
            // same words and side effect `Intent::SetRules` gives.
            let rules = match rules_error {
                Some(reason) => refuse(core, format!("rules refused: {reason}")),
                None => rules_answer,
            };
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
            // core answers honestly rather than pretending. `z` is the
            // offset from the scene's default floor — the route owns the
            // dimension and range, and the runtime moves the duck with the
            // plane so a standing duck does not end up under it.
            #[cfg(feature = "mujoco")]
            {
                // `apply_intent` swapped the selected branch into
                // `core.runtime` for this dispatch; the plane to move is
                // that duck's. Swapping again here would move the original.
                if core.runtime.set_floor_height(z).unwrap_or(false) {
                    json!({ "ok": true, "floor_offset_m": z, "floor_z": z })
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
            // Same branch swap: during the dispatch `core.runtime` *is* the
            // duck the client is watching, so both the budget and the answer
            // come from it. `active_runtime` would read the branch slot,
            // which holds the original while the swap is in flight.
            core.runtime.set_parent_budget(minutes);
            let summary = core.runtime.care_summary();
            json!({ "ok": true, "minutes": minutes, "parent": summary.get("parent") })
        }
        Intent::PadSwitch(on) => {
            if on {
                // The official UDP pad is unauthenticated by design. A
                // family app only opens it AFTER the person set the same
                // six-digit pairing code the neighbours use; a stranger
                // on the Wi-Fi then has no control channel at all.
                let paired = core.lan.as_ref().is_some_and(|lan| lan.pair_code_set());
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
            // The person's taught words and the built-in table contest one
            // longest match; whichever wins names the verb. The table is
            // cloned out of the queue so no lock is held while the verb
            // runs.
            let aliases = core.intents.voice_aliases();
            let Some((phrase, heard)) = crate::speech::interpret_with(&aliases, &text) else {
                // Silence is not an error the client should have to parse:
                // say what was heard and that nothing matched.
                return json!({ "ok": false, "heard": text, "reason": "no verb matched" });
            };
            // Speech reaches the same execution path as a button, and a verb
            // that moves the duck walks the same gates the Skill/Sit/
            // GroundPick intents do: the power-off first — 「press Start on
            // the pad」 would point at a duck that is going down — then
            // frozen time, then the driving policy. A sound does not move
            // anything and stays available through all three.
            if heard.moves_body() && core.runtime.shutting_down() {
                return refuse(core, "the duck is powering off".to_owned());
            }
            if heard.moves_body() && core.runtime.clock().paused {
                return refuse(core, "the duck is paused".to_owned());
            }
            if heard.moves_body() && !policy_drives(&core.runtime) {
                return refuse(
                    core,
                    "the policy is not driving — press Start on the pad".to_owned(),
                );
            }
            // The verb the utterance named, kept even when the verb itself
            // is dropped: a client should still see what the duck heard.
            let (verb_key, verb_value) = match &heard {
                crate::speech::HeardWord::Skill(name) => ("skill", name.clone()),
                crate::speech::HeardWord::Sound(tag) => ("sound", tag.clone()),
                crate::speech::HeardWord::Sit => ("skill", "sit".to_owned()),
                crate::speech::HeardWord::Stand => ("skill", "stand".to_owned()),
                crate::speech::HeardWord::GroundPick => ("skill", "ground_pick".to_owned()),
            };
            let result = match heard {
                crate::speech::HeardWord::Skill(name) => core
                    .runtime
                    .trigger_skill(&name)
                    .map(|started| json!({ "ok": true, "heard": text, "phrase": phrase, "skill": name, "chained": !started }))
                    .map_err(str::to_owned),
                crate::speech::HeardWord::Sound(tag) => {
                    core.runtime.queue_sound(&tag);
                    Ok(json!({ "ok": true, "heard": text, "phrase": phrase, "sound": tag }))
                }
                crate::speech::HeardWord::Sit => {
                    if core.runtime.is_sitting() {
                        Ok(json!({ "ok": true, "heard": text, "phrase": phrase, "skill": "sit", "did": false }))
                    } else {
                        core.runtime
                            .sit_toggle()
                            .map(|did| json!({ "ok": true, "heard": text, "phrase": phrase, "skill": "sit", "did": did }))
                            .map_err(str::to_owned)
                    }
                }
                crate::speech::HeardWord::Stand => {
                    if core.runtime.is_sitting() {
                        core.runtime
                            .sit_toggle()
                            .map(|did| json!({ "ok": true, "heard": text, "phrase": phrase, "skill": "stand", "did": did }))
                            .map_err(str::to_owned)
                    } else {
                        Ok(json!({ "ok": true, "heard": text, "phrase": phrase, "skill": "stand", "did": false }))
                    }
                }
                crate::speech::HeardWord::GroundPick => core
                    .runtime
                    .start_ground_pick()
                    .map(|()| json!({ "ok": true, "heard": text, "phrase": phrase, "skill": "ground_pick" }))
                    .map_err(str::to_owned),
            };
            match result {
                Ok(value) => value,
                Err(error) => {
                    // The verb was understood but did not run. Two different
                    // situations share this shape:
                    //
                    // - An emotion is already playing. robotd's own rule is
                    //   "a request while one plays is dropped" — a voice
                    //   remote firing two moods in a row is the normal case,
                    //   not an error, and the client should not see a
                    //   failure for it.
                    // - A taught word whose skill the last reload dropped,
                    //   say. That is a hearing answer, not a duck state:
                    //   unlike `refuse`, it leaves `core.label` alone, and it
                    //   keeps `heard` and `phrase` so the client can see what
                    //   was meant.
                    if error == "an emotion is already playing" {
                        let mut body = json!({
                            "ok": true,
                            "heard": text,
                            "phrase": phrase,
                            "dropped": "an emotion is already playing",
                        });
                        body[verb_key] = json!(verb_value);
                        body
                    } else {
                        let mut body = json!({
                            "ok": false,
                            "heard": text,
                            "phrase": phrase,
                            "error": error,
                            "reason": "the verb did not run",
                        });
                        body[verb_key] = json!(verb_value);
                        body
                    }
                }
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
        Intent::Reload(dir) => match reload_policies(core, &dir, ReloadKind::Reload, core.selected)
        {
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
            // timer keeps calling tick(), which respects the pause. This is
            // the one deliberate exception to the pause gates: the time
            // machine unpauses for exactly these ticks, so every skill and
            // mood it performs is a real, ticked move.
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
        Intent::Sensor(mut reading) => {
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
                    // The shell's optional PCM16 rides beside the RMS/peak
                    // summary. Pull it out first: `/sensors` publishes the
                    // registry, and a megabyte of audio base64 is neither
                    // useful to a poller nor private to keep. The runtime
                    // owns the model and the pause gate; without a model the
                    // call is a no-op, which is the feature's off switch.
                    if let Some((samples, rate)) = crate::sensors::take_pcm16(&mut reading.value) {
                        // `apply_intent` already swapped the selected branch
                        // into `core.runtime`; this arm speaks to that duck.
                        // Swapping again (`on_active`) fed the original.
                        let _ = core.runtime.pet_by_sound(&samples, rate);
                    }
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
                refuse(
                    core,
                    format!("fork refused: at most {MAX_BRANCHES} branches"),
                )
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
            // Branch indices just shifted; a marker keyed by index can no
            // longer be trusted. The new active duck's table is republished
            // below like any selection change.
            core.swap_landing = None;
            publish_loaded_skills(core);
            let enabled = runtime_policy_enabled(active_runtime(core));
            set_policy_enabled_mirror(core, enabled);
            json!({ "ok": true, "label": label })
        }
        Intent::SelectBranch(label) => {
            let index = core
                .branches
                .iter()
                .position(|branch| branch.label == label)
                .map_or(0, |index| index + 1);
            core.selected = index;
            // The toggle mirror and the published skill table follow the duck
            // the client now watches; both are core-level snapshots of a
            // per-runtime fact.
            let enabled = runtime_policy_enabled(active_runtime(core));
            set_policy_enabled_mirror(core, enabled);
            publish_loaded_skills(core);
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
        HealthVerdict::Unhealthy { reasons } => {
            json!({ "verdict": "unhealthy", "reasons": reasons })
        }
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
///
/// **Never call this from inside [`apply_intent_active`] for a swapped
/// intent**: that dispatch already has the selected branch in `core.runtime`,
/// so the second swap here would operate on the original. Inside those arms
/// speak to `core.runtime` directly. This helper is for callers outside the
/// dispatch — the tick's LAN/peer path, mode carriers, readers.
fn on_active(core: &mut DuckCore, f: impl FnOnce(&mut DuckRuntime<MujocoIo>)) {
    let selected = core.selected;
    if selected > 0 && selected <= core.branches.len() {
        std::mem::swap(&mut core.runtime, &mut core.branches[selected - 1].runtime);
        f(&mut core.runtime);
        std::mem::swap(&mut core.runtime, &mut core.branches[selected - 1].runtime);
    } else {
        f(&mut core.runtime);
    }
}

/// Whether the duck is up — the pad's belief about a toggle's outcome, the
/// same predicate the mapper's `up` flag is fed from: torque on and at the
/// home pose. `Home` (policy off + re-home) leaves the duck up, `Rest` leaves
/// it in the rest pose, `Shutdown` powers it off.
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

/// The DKRB cadence: peek the last-report instant, and stamp it only when a
/// report is actually due. The old `core.pad_report_at.take()` consumed the
/// timestamp on every frame, so a not-due decision turned the next beat due —
/// a report every other tick (≈25 Hz) instead of the official one per second
/// (`pad-map/src/wire.rs:254-344`, `netpadd/src/main.rs:189,317`).
fn pad_report_due(core: &mut DuckCore, pad: &crate::pad::Pad, now: Instant) -> bool {
    if pad.report_due(crate::pad::Accept::Taken, core.pad_report_at, now) {
        core.pad_report_at = Some(now);
        true
    } else {
        false
    }
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
    // at once when it just connected. `pad_report_due` peeks the clock and
    // stamps it only on a report; consuming it on a quiet beat was the ≈25 Hz
    // cadence this ticket fixed.
    if pad_report_due(core, &pad, now) {
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
fn pad_apply_frame(core: &mut DuckCore, frame: &crate::pad::PadFrame, now: Instant) {
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
/// deadzone 0.1, drive vx ±0.4 m/s / vy ±0.3 m/s / vyaw ±1.0 rad/s, head
/// travel 2.5 rad per joint, body z -0.025..0.010 m and roll ±15°.
///
/// `Drive`: the official `DriveAxes` — left stick walks (LeftY) and strafes
/// (LeftX, inverted), right stick turns (RightX, inverted).
/// `HeadDrive`: left stick walks and turns, right stick poses the head (the
/// official mode has no strafe). `Head`: the body is stopped and all four axes
/// pose the head. `BodyPose`: the body holds (move zeroed) and the left stick
/// crouches/leans, the right stick poses the head.
fn pad_frame_commands(
    core: &mut DuckCore,
    frame: &crate::pad::PadFrame,
    mode: crate::pad::PadMode,
) {
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
    let drive_cmd = |vx: &AxisBinding, vy: &AxisBinding, vyaw: &AxisBinding| {
        [
            robotd_params::PadDriveParams::scale(
                evaluate(vx, &sticks, axes.deadzone),
                drive.vx_min,
                drive.vx_max,
            ),
            robotd_params::PadDriveParams::scale(
                evaluate(vy, &sticks, axes.deadzone),
                drive.vy_min,
                drive.vy_max,
            ),
            robotd_params::PadDriveParams::scale(
                evaluate(vyaw, &sticks, axes.deadzone),
                drive.vyaw_min,
                drive.vyaw_max,
            ),
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
        // Drive: the official `DriveAxes` binding (`pad_axes.rs:154-179`) —
        // left stick walks and strafes, the right stick turns. This is a
        // pad-only mapping: the phone's own joystick reaches the loop through
        // the shells' C ABI calls, not through this function.
        crate::pad::PadMode::Drive => on_active(core, |runtime| {
            runtime.set_twist(drive_cmd(&axes.drive.vx, &axes.drive.vy, &axes.drive.vyaw));
        }),
        crate::pad::PadMode::HeadDrive => on_active(core, |runtime| {
            // The right stick is busy with the head, so this official mode
            // has no strafe (`HeadDriveAxes.vy: None`, `pad_axes.rs:196-203`):
            // left stick walks and turns.
            runtime.set_twist(drive_cmd(
                &axes.head_drive.vx,
                &axes.head_drive.vy,
                &axes.head_drive.vyaw,
            ));
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
                robotd_params::PadDriveParams::scale(
                    evaluate(&axes.body_pose.z, &sticks, axes.deadzone),
                    body.z_min,
                    body.z_max,
                ),
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
                // Frozen time freezes the duck: a pad skill accepted while
                // paused would be counted in the life record without the tick
                // that performs it. The refusal is remembered in `pad_refused`
                // (the DKRB report carries it), like every other pad refusal.
                if runtime.clock().paused {
                    refusal = Some("the duck is paused".to_owned());
                    return;
                }
                if runtime.shutting_down() {
                    refusal = Some("the duck is powering off".to_owned());
                    return;
                }
                if !policy_drives(runtime) {
                    // The same gate the intent arm applies: a pad button on a
                    // resting or disabled duck is refused, not queued into a
                    // controller that forgot its moves.
                    refusal = Some("the policy is not driving — press Start on the pad".to_owned());
                    return;
                }
                let result: Result<(), String> = match skill.as_str() {
                    // `sit`/`stand` are idempotent, exactly as the HTTP
                    // `/skill` arm reads them; `sit_toggle` is the pad's own
                    // toggle spelling and flips whatever the duck is doing.
                    "sit" | "stand" => {
                        let sitting = runtime.is_sitting();
                        if (skill == "sit") == sitting {
                            Ok(())
                        } else {
                            runtime
                                .sit_toggle()
                                .map(|_| ())
                                .map_err(|error| format!("skill refused: {error}"))
                        }
                    }
                    "sit_toggle" => runtime
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
        // `robot.init`: the official power request's four branches — a
        // powered-off duck refuses, a limp one ramps up without needing a
        // policy, a seated Ready duck holds its seat, and a standing Ready one
        // re-homes. `runtime.init` is that whole decision.
        PadButtonEvent::Init => {
            let mut refusal = None;
            let mut enabled_after = None;
            on_active(core, |runtime| {
                if runtime.shutting_down() {
                    refusal = Some("the duck is powering off".to_owned());
                    return;
                }
                refusal = runtime
                    .init()
                    .err()
                    .map(|error| format!("init refused: {error}"));
                enabled_after = Some(runtime_policy_enabled(runtime));
            });
            if let Some(enabled) = enabled_after {
                set_policy_enabled_mirror(core, enabled);
            }
            refusal
        }
        // `robot.enable toggle`: the robot owns the state. The reply's reason
        // names the state the robot ended in; a toggle OFF stops the policy
        // and returns to the home pose with torque still on (the official
        // falling edge, `robotd/src/main.rs:3763-3795`), never the torque cut
        // `relax`/`shutdown` own.
        PadButtonEvent::Toggle => {
            let mut refusal = None;
            let mut enabled_after = None;
            on_active(core, |runtime| {
                if runtime.shutting_down() {
                    // The duck is powering off: the toggle is refused, the way
                    // a skill is.
                    refusal = Some("the duck is powering off".to_owned());
                    return;
                }
                if runtime.powered_off() {
                    // Powered off is not a policy state the toggle owns: ask
                    // for the power-on the shell's button would.
                    refusal = runtime
                        .power_on()
                        .err()
                        .map(|error| format!("toggle refused: {error}"));
                } else {
                    let on = !runtime_policy_enabled(runtime);
                    runtime.set_policy_enabled(on);
                }
                enabled_after = Some(runtime_policy_enabled(runtime));
            });
            if let Some(enabled) = enabled_after {
                set_policy_enabled_mirror(core, enabled);
            }
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
        // Start held: home pose, motors stiff, policy off. The official
        // sequence is `robot.enable{on:false}` then `robot.init`
        // (`pad-map/src/mapper.rs:262-281`): the disable comes first so the
        // policy cannot pick the robot up again once the ramp ends. A seated
        // duck is held in its seat by `init`'s Ready+sitting branch; it is not
        // re-homed onto its feet.
        PadButtonEvent::Home => {
            let mut refusal = None;
            let mut enabled_after = None;
            let from = core.pad_mode;
            core.pad_mode = crate::pad::PadMode::Drive;
            on_active(core, |runtime| {
                if runtime.shutting_down() {
                    refusal = Some("the duck is powering off".to_owned());
                    return;
                }
                // Held Start also puts the sticks back to plain driving, so
                // the mode's exit calls run here (the mapper has already set
                // its own mode to Drive and sent no ModeChange event).
                if from == crate::pad::PadMode::BodyPose {
                    runtime.release_body();
                }
                if from.poses_head() {
                    runtime.set_head([0.0; 4]);
                }
                runtime.set_policy_enabled(false);
                refusal = runtime
                    .init()
                    .err()
                    .map(|error| format!("home refused: {error}"));
                enabled_after = Some(runtime_policy_enabled(runtime));
            });
            if let Some(enabled) = enabled_after {
                set_policy_enabled_mirror(core, enabled);
            }
            refusal
        }
        // Select held 2 s: sit down and ease into the rest pose. This is
        // `robot.rest`, not `robot.shutdown`: the official rest runs the same
        // sit and rest ramp and ends without a power-off latch
        // (`robotd/src/main.rs:3130-3175`). The phone has no servos to reboot,
        // so the duck keeps torque as it was and stays powered
        // (`runtime.rs`'s `request_rest`).
        PadButtonEvent::Rest => {
            on_active(core, |runtime| runtime.request_rest());
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
        // the exit calls happen when a mode is **left**, exactly as the
        // official `mode_exit_calls` (`pad-map/src/mapper.rs:40-54`): a body
        // left leaning or a head left turned stays that way, and the next mode
        // does not send the joints the last one did. Entering `BodyPose` does
        // not release the pose it is about to hold.
        PadButtonEvent::ModeChange(mode) => {
            let mode = *mode;
            let from = core.pad_mode;
            core.pad_mode = mode;
            if from == crate::pad::PadMode::BodyPose && mode != crate::pad::PadMode::BodyPose {
                on_active(core, |runtime| runtime.release_body());
            }
            if from.poses_head() && !mode.poses_head() {
                on_active(core, |runtime| runtime.set_head([0.0; 4]));
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
        // The microphone's petting verdict: whether the official CNN hears a
        // hand on the shell now, and the tick the spell started. The audio
        // itself never reaches the observation or the wire.
        "pet_detect": active.pet_detect_summary(),
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
        Ok(()) => {
            set_policy_enabled_mirror(&*core, true);
            0
        }
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_disable(core: *mut DuckCore) -> c_int {
    if core.is_null() {
        return -1;
    }
    match (*core).runtime.disable() {
        Ok(()) => {
            set_policy_enabled_mirror(&*core, false);
            0
        }
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
    if crate::vetbook::is_calendar_day(&quest_day) && quest_day != core.parent_day {
        core.parent_day = quest_day.clone();
        on_active(core, |runtime| {
            runtime.reset_parent_day();
            // The parent's view: the day boundary snapshots yesterday's own
            // deltas into the vet book's rows.
            runtime.book_mut().roll_day(&quest_day);
        });
    }
    if crate::vetbook::is_calendar_day(&quest_day) {
        // Measure the duck the client is driving: a selected branch's own
        // counters, not the original the branch forked from.
        let values = json!({
            "care": active_runtime(core).care_summary(),
            "life": active_runtime(core).book().summary(),
        });
        let finished = core.quests.update(&quest_day, &values);
        // The quest completes either way — the goals are real facts. The
        // celebration is a mood, and a mood played while frozen would be
        // counted in the life record without the tick that performs it.
        if !finished.is_empty() && !active_runtime(core).clock().paused {
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
    // A paused duck listens but does not perform: greetings, gifts, choruses
    // and ensembles are honestly skipped while frozen — no mood is counted
    // and nothing is queued for later. The peer list above is still
    // published: hearing the room is not acting in it.
    let paused_now = active_runtime(core).clock().paused;
    let mut gifts = Vec::new();
    let mut choruses = Vec::new();
    for event in peer_events {
        if paused_now {
            continue;
        }
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
            crate::lan::PeerEvent::Chorus { id, moves, .. } => choruses.push((id, moves)),
            crate::lan::PeerEvent::Ensemble {
                id,
                role,
                roles,
                moves,
                sounds,
                ..
            } => {
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
    // The IPC power requests, on the same tick and the same watched duck as
    // every other intent. The handlers answered `accepted` when they queued
    // them, exactly as the official dispatch does.
    let controls: Vec<ControlCommand> = core
        .control
        .lock()
        .map(|mut queue| queue.drain(..).collect())
        .unwrap_or_default();
    if !controls.is_empty() {
        let mut enabled = None;
        for command in controls {
            enabled = Some(apply_control(core, command));
        }
        // The mirror `robot.enable toggle` reads: the body just decided, so
        // publish its answer rather than the handler's guess.
        if let (Some(enabled), Ok(mut snapshot)) = (enabled, core.robot_snapshot.lock()) {
            snapshot.enabled = enabled;
        }
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
            std::mem::swap(&mut core.runtime, &mut core.branches[selected - 1].runtime);
            if !core.runtime.restore(&checkpoint) {
                core.label = "restore refused".to_owned();
            }
            std::mem::swap(&mut core.runtime, &mut core.branches[selected - 1].runtime);
        } else if !core.runtime.restore(&checkpoint) {
            core.label = "restore refused".to_owned();
        }
        core.timeline.retain(|saved| saved.tick <= checkpoint.tick);
        // `restore` drops any pending policy swap (the future it belonged to
        // is gone), so a marker owned by the duck that was rewound must go
        // with it. A marker on another branch is untouched.
        clear_swap_landing_for_active(core);
        // The policy flag travels with the checkpoint; the toggle mirror
        // follows the duck that now runs the restored tick.
        let enabled_after = runtime_policy_enabled(active_runtime(core));
        set_policy_enabled_mirror(core, enabled_after);
    }
    // A change that landed while the clock was frozen still needs its skill
    // table published; nothing else observes the falling edge.
    settle_policy_swap(core);
    // Frozen time freezes the duck the client is watching, including a branch.
    let active_paused = active_runtime(core).clock().paused;
    if active_paused {
        // Intents above still applied (a pet, a feed): the paused tick does
        // not step physics, but the lifetime record it just changed must be
        // noted and published before this early return, or /life and the
        // collection would lag behind the book until the next unpaused tick.
        let unlock_day = core.intents.day();
        if crate::vetbook::is_calendar_day(&unlock_day) {
            on_active(core, |runtime| runtime.book_mut().note_unlocks(&unlock_day));
        }
        let book = active_runtime(core).book().clone();
        core.intents.set_life(&book);
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
            settle_policy_swap(core);
            // The collection: the tick that just counted a move also notes
            // the day it first happened. The shell's calendar day is the only
            // clock that can date it, and before one has arrived there is
            // nothing honest to write. Re-read it after this tick's intents:
            // a day that rolled over mid-tick dates by the new day. On a
            // branch session this is the branch's own book, the same one
            // /quests measures.
            let unlock_day = core.intents.day();
            if crate::vetbook::is_calendar_day(&unlock_day) {
                on_active(core, |runtime| runtime.book_mut().note_unlocks(&unlock_day));
            }
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
        *snapshot = runtime_health.clone();
    }
    refresh_robot_snapshot(core, active, &runtime_health);
    let Some(ipc) = core.ipc.as_ref() else {
        return;
    };
    if let Some(state) = active.robot_state() {
        ipc.publish_state(&state);
    }
}

/// Refresh the read-side snapshot the `robot.*` handlers answer from. Runs on
/// the thread that owns the body, once per tick, for the duck the loop just
/// drove — [`active_runtime`], the selected branch when there is one.
///
/// Everything here is a fact the runtime itself reports: slot paths from
/// `driving_net_paths`, the skill table the manifest loaded, the lifetime book,
/// the mode, and the health verdict the same tick published. Nothing is
/// inferred from `/state` or from a client's belief.
fn refresh_robot_snapshot(
    core: &DuckCore,
    active: &DuckRuntime<MujocoIo>,
    health: &duck_ipc_proto::HealthResult,
) {
    let paths = active.driving_net_paths();
    let slot_path = |name: &str| {
        paths
            .iter()
            .find(|(slot, _)| slot == name)
            .and_then(|(_, path)| path.clone())
    };
    let file_name = |name: &str| {
        slot_path(name).and_then(|path| {
            path.file_name()
                .map(|file| file.to_string_lossy().into_owned())
        })
    };
    let loaded = |name: &str| slot_path(name).is_some();

    // The six slots a phone can drive (the four fixed networks plus the
    // manifest's skills by name) with their overrides. `origin` stays absent:
    // the phone has no official/community/local provenance split — a file in
    // the policies directory is all this core knows (COMPARISON §5.1).
    let slots: Vec<duck_ipc_proto::PolicySlot> = paths
        .iter()
        .map(|(slot, path)| duck_ipc_proto::PolicySlot {
            slot: slot.clone(),
            path: path.as_ref().map(|path| path.display().to_string()),
            origin: None,
            overridden: crate::manifest::slot_override(slot).is_some(),
            error: None,
        })
        .collect();
    // The skills **this duck** loaded, not the core-level manifest table:
    // in a branch session the selected runtime can be running a different
    // set, and pairing the global `skill_defs` with its slots would describe
    // a robot that is not there. The table still supplies parameters, matched
    // by name and (when it names one) by the same file.
    let loaded_skills: Vec<(String, Option<PathBuf>)> = paths.iter().skip(4).cloned().collect();
    let configured_skills: Vec<String> =
        loaded_skills.iter().map(|(name, _)| name.clone()).collect();
    let emotions = active.emotion_names();

    // `robot.skills`: every loaded one-shot with the parameters the manifest
    // declared for that name, plus the two names the daemon owns itself. A
    // skill entry says `overridden` when the person's own table carries that
    // name — the same reading the official report gives a `[[policy.skill]]`
    // entry (config has an opinion about this skill).
    let custom_skills = crate::manifest::custom_skills();
    let skill_params: Vec<duck_ipc_proto::SkillParams> = loaded_skills
        .iter()
        .map(|(name, path)| {
            let def = core.skill_defs.iter().find(|skill| {
                skill.name == *name
                    && match (&skill.path, path) {
                        (Some(def_path), Some(loaded)) => def_path == loaded,
                        (None, _) => true,
                        (Some(_), None) => false,
                    }
            });
            match def {
                Some(skill) => duck_ipc_proto::SkillParams {
                    name: skill.name.clone(),
                    path: skill.path.as_ref().map(|path| path.display().to_string()),
                    duration: Some(skill.duration),
                    command: Some(skill.command),
                    unwind: Some(skill.unwind),
                    unwind_s: Some(skill.unwind_s),
                    chain: Some(skill.chain),
                    action_scale: skill.params.action_scale,
                    gain_ratio: skill.params.gain_ratio,
                    overridden: custom_skills.contains_key(name),
                },
                // A loaded skill the core-level table does not name (the
                // selected branch runs another set): report what is true —
                // the name and the file — and no invented parameters.
                None => duck_ipc_proto::SkillParams {
                    name: name.clone(),
                    path: path.as_ref().map(|path| path.display().to_string()),
                    overridden: custom_skills.contains_key(name),
                    ..Default::default()
                },
            }
        })
        .collect();
    let built_in: Vec<String> = ["ground_pick", "sit_toggle"]
        .into_iter()
        .filter(|name| {
            // Official `skills_report`: a name that is also a configured
            // table entry is reported there, not twice (`main.rs:4880-4924`).
            !loaded_skills.iter().any(|(loaded, _)| loaded == name)
                && match *name {
                    "ground_pick" => loaded("ground_pick"),
                    _ => loaded("sitstand"),
                }
        })
        .map(str::to_owned)
        .collect();

    // Why nothing is driving, when nothing is. Policy errors come from the
    // health verdict (the runtime's `policy_error` is folded into it); a
    // missing walking network has the official sentence.
    let unavailable = health
        .reason
        .as_deref()
        .and_then(|reason| reason.strip_prefix("policy unavailable: "))
        .map(str::to_owned)
        .or_else(|| {
            (!loaded("walk")).then(|| "no policy configured; holding the startup pose".to_owned())
        });

    let safe = active.safe_to_restart();
    let previous_enabled = core
        .robot_snapshot
        .lock()
        .map(|snapshot| snapshot.enabled)
        .unwrap_or(true);
    // A limp or powered-off duck is not driving a policy, whatever a stale
    // mirror says. Every explicit policy-off door (`robot.enable on:false`,
    // Home, relax, the shell's disable) writes the mirror directly; this catch
    // covers the flags the runtime changes on its own (power-off finishing, a
    // limp fall).
    let enabled = if active.powered_off() || matches!(active.bringup(), BringupState::Limp) {
        false
    } else {
        previous_enabled
    };
    let snapshot = RobotSnapshot {
        subscribe: duck_ipc_proto::SubscribeResult {
            accepted: true,
            walk: file_name("walk"),
            stand: file_name("stand"),
            sitstand: file_name("sitstand"),
            ground_pick: file_name("ground_pick"),
            // The configurable one-shots and the emotions, exactly the list
            // `robot.do` answers to beyond the three policy slots above.
            skills: configured_skills
                .iter()
                .cloned()
                .chain(emotions.iter().cloned())
                .collect(),
            unavailable,
        },
        policies: duck_ipc_proto::PoliciesResult {
            mode: active.mode().as_str().to_owned(),
            // The phone has no `[policy] enabled` config key: a loaded core
            // always runs its policies.
            enabled: true,
            slots,
            skills: configured_skills,
            homed: Some(matches!(active.bringup(), BringupState::Ready)),
            sitting: Some(active.is_sitting()),
            // A failed change publishes its reason on `notice` and the health
            // gate, not here; there is no separate change-error slot.
            change_error: None,
        },
        skills: duck_ipc_proto::SkillsResult {
            skills: skill_params,
            built_in,
        },
        model: core
            .robot_snapshot
            .lock()
            .map(|snapshot| snapshot.model.clone())
            .unwrap_or_else(|_| robot_model_result()),
        vetbook: active.book().summary(),
        mode: active.mode().as_str().to_owned(),
        policies_dir: core.policies.clone(),
        enabled,
        safe_to_restart: duck_ipc_proto::SafeToRestartResult {
            safe,
            reason: (!safe).then(|| "the duck is moving".to_owned()),
        },
    };
    if let Ok(mut slot) = core.robot_snapshot.lock() {
        *slot = snapshot;
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
    match core.runtime.load_pickup(std::path::Path::new(&path)) {
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
            (*core)
                .runtime
                .emotion_names()
                .iter()
                .any(|loaded| loaded == name)
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
    // loop's own, and anything else is a manifest skill. It acts on the duck
    // the client is watching (a selected branch in a branch session) and
    // applies the same driving gate as the HTTP arm and the pad button: a
    // resting or disabled duck is refused, not frozen.
    let core = &mut *core;
    let mut outcome = None;
    on_active(core, |runtime| {
        // Frozen time freezes the duck: accepting a move while paused would
        // count it in the life record without the tick that performs it.
        if runtime.clock().paused {
            outcome = Some(Err("the duck is paused".to_owned()));
            return;
        }
        if !policy_drives(runtime) {
            outcome = Some(Err(
                "the policy is not driving — press Start on the pad".to_owned()
            ));
            return;
        }
        let result = match name.as_str() {
            "sit" | "stand" => {
                let sitting = runtime.is_sitting();
                if (name == "sit") == sitting {
                    Ok(true)
                } else {
                    runtime.sit_toggle().map(|_| true).map_err(str::to_owned)
                }
            }
            "ground_pick" => runtime
                .start_ground_pick()
                .map(|()| true)
                .map_err(str::to_owned),
            _ => runtime.trigger_skill(&name).map_err(str::to_owned),
        };
        outcome = Some(result);
    });
    match outcome {
        Some(Ok(started)) => i32::from(started),
        Some(Err(error)) => {
            core.label = format!("skill refused: {error}");
            0
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_sit_toggle(core: *mut DuckCore) -> c_int {
    if core.is_null() {
        return -1;
    }
    let core = &mut *core;
    let mut paused = false;
    let mut allowed = false;
    on_active(core, |runtime| {
        paused = runtime.clock().paused;
        allowed = !paused && policy_drives(runtime) && runtime.sit_toggle().is_ok();
    });
    if allowed {
        0
    } else {
        core.label = if paused {
            "sit refused: the duck is paused".to_owned()
        } else {
            "sit refused: the policy is not driving".to_owned()
        };
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn duck_core_ground_pick(core: *mut DuckCore) -> c_int {
    if core.is_null() {
        return -1;
    }
    let core = &mut *core;
    let mut paused = false;
    let mut allowed = false;
    on_active(core, |runtime| {
        paused = runtime.clock().paused;
        allowed = !paused && policy_drives(runtime) && runtime.start_ground_pick().is_ok();
    });
    if allowed {
        0
    } else {
        core.label = if paused {
            "ground pick refused: the duck is paused".to_owned()
        } else {
            "ground pick refused: the policy is not driving".to_owned()
        };
        -1
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
    out.gain = core
        .last
        .as_ref()
        .map_or(200, |report| report.gain as c_int);
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
    out.p_held = report
        .and_then(|report| report.p_held)
        .map_or(0.0, f64::from);
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
pub unsafe extern "C" fn duck_core_mesh_name(core: *mut DuckCore, mesh: c_int) -> *const c_char {
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
        // The time machine's explicit advancement is the one exception to the
        // pause gates: these ticks are real, so what they perform is counted.
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
    }
    // A queued policy change can land inside those ticks; settle after the
    // branch swap has been undone, and let its label win over the gait's.
    settle_policy_swap(core);
    publish_state(core);
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
    // `restore` dropped any pending policy swap on this duck; its landing
    // marker must go with the future it belonged to.
    clear_swap_landing_for_active(core);
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

    /// The two process-global test locks, held for a core test's whole run:
    /// the fixed IPC port and the slot-override table. The override lock is
    /// the one the manifest tests take, so a parallel `cargo test` (the CI
    /// job runs the default thread count) cannot load a set through another
    /// test's override.
    struct CoreGuard {
        _core: std::sync::MutexGuard<'static, ()>,
        _overrides: std::sync::MutexGuard<'static, ()>,
    }

    fn new_core() -> (CoreGuard, *mut DuckCore) {
        let guard = CORE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let overrides = crate::manifest::override_test_lock();
        // Resolve the official assets the way every other test does: the
        // workspace checkout, or `DUCK_POLICIES_DIR` on a CI runner. No
        // machine-specific path.
        let scene = crate::fixtures::scene_backlash()
            .expect("the official training scene (fetch upstream/microduck_rl)");
        let policies =
            crate::fixtures::policies_dir().expect("the official policy bundle (fetch policies)");
        let scene = CString::new(scene.to_string_lossy().into_owned()).unwrap();
        let policies = CString::new(policies.to_string_lossy().into_owned()).unwrap();
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
        (
            CoreGuard {
                _core: guard,
                _overrides: overrides,
            },
            core,
        )
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

    /// Tick until a queued policy swap has landed. A disturbing change walks
    /// home first (up to `HOME_RAMP` plus the walk home); 800 ticks is the
    /// same headroom the other reload tests use.
    fn wait_for_swap(core: *mut DuckCore) {
        for _ in 0..800 {
            unsafe { duck_core_tick(core) };
            if !unsafe { (*core).runtime.swap_pending() } {
                return;
            }
        }
        panic!("the policy swap never landed");
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
        let text = unsafe { CStr::from_ptr(reason) }
            .to_string_lossy()
            .into_owned();
        assert!(text.contains("no Walk locomotion policy in"), "{text}");

        // The refusal is a degraded mark, not a healthy duck: the judge can
        // see the reload did not take.
        let verdict = unsafe { (*core).runtime.health() };
        match &verdict {
            crate::runtime::HealthVerdict::Degraded { reasons } => assert!(
                reasons
                    .iter()
                    .any(|reason| reason.starts_with("policy reload refused")),
                "{reasons:?}"
            ),
            other => panic!("a refused reload must mark the duck degraded: {other:?}"),
        }

        // A reload that lands clears the mark again.
        let fixture_dir =
            crate::fixtures::policies_dir().expect("the official policy bundle (fetch policies)");
        let policies = CString::new(fixture_dir.to_string_lossy().into_owned()).unwrap();
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

    /// The collection page is fed by the duck the tick really drove: a mood
    /// played after the shell's day arrives gets that day as its first day,
    /// and the flat /life save the shells keep carries it back, so a restart
    /// does not date it again.
    #[test]
    fn the_collection_page_dates_a_mood_with_the_shells_day() {
        let emotions_dir =
            crate::fixtures::workspace_root().join("upstream/microduck/robotd/emotions");
        if !emotions_dir.is_dir() {
            if crate::fixtures::required() {
                panic!(
                    "the official emotions are missing (looked in {})",
                    emotions_dir.display()
                );
            }
            eprintln!(
                "skipping: the official emotions are not here ({})",
                emotions_dir.display()
            );
            return;
        }
        let (_guard, core) = new_core();
        let dir = CString::new(emotions_dir.to_string_lossy().into_owned()).unwrap();
        assert!(unsafe { duck_core_load_emotions(core, dir.as_ptr()) } > 0);
        tick_until_ready(core);

        let port = 19000 + ((std::process::id() % 1000) + 13) as i32;
        assert_eq!(unsafe { duck_core_start_intent_server(core, port) }, 0);
        let http = |method: &str, path: &str, body: &str| -> String {
            let mut stream = TcpStream::connect(("127.0.0.1", port as u16)).expect("connect");
            let request = format!(
                "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(request.as_bytes()).expect("write");
            let mut response = String::new();
            stream.read_to_string(&mut response).expect("read");
            response
        };
        let body_of = |response: &str| -> serde_json::Value {
            serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap_or("{}"))
                .unwrap_or(serde_json::Value::Null)
        };
        let mood_row = |page: &serde_json::Value, name: &str| -> (u64, Option<String>) {
            page["emotions"]
                .as_array()
                .expect("emotions")
                .iter()
                .find(|row| row["name"] == name)
                .map(|row| {
                    (
                        row["count"].as_u64().expect("count"),
                        row["first_day"].as_str().map(str::to_owned),
                    )
                })
                .unwrap_or_else(|| panic!("{name} is a loaded official mood"))
        };
        let collection = || body_of(&http("GET", "/collection", ""));

        // A mood nobody has played: listed with its real zero and no date.
        let page = collection();
        assert_eq!(mood_row(&page, "curious"), (0, None));
        assert_eq!(page["moods"]["total"], json!(13));
        assert_eq!(
            page["tricks"]["total"],
            json!(page["skills"].as_array().expect("skills").len())
        );

        // The shell's calendar day, then a real mood through the same /skill
        // verb the ring posts. The tick answers both.
        let response = http("POST", "/quests/day", r#"{"day":"2026-11-18"}"#);
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert_eq!(unsafe { duck_core_tick(core) }, 0);
        let response = http("POST", "/skill", r#"{"name":"curious"}"#);
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert_eq!(unsafe { duck_core_tick(core) }, 0);

        let page = collection();
        assert_eq!(
            mood_row(&page, "curious"),
            (1, Some("2026-11-18".to_owned()))
        );
        assert_eq!(page["moods"]["unlocked"], json!(1));
        // A manifest trick that has not happened: count 0, no date at all.
        let roulade = page["skills"]
            .as_array()
            .expect("skills")
            .iter()
            .find(|row| row["name"] == "roulade")
            .expect("roulade is a manifest trick");
        assert_eq!(roulade["count"], json!(0));
        assert!(
            roulade.get("first_day").is_none(),
            "a trick that never happened has no date to send: {roulade}"
        );

        // The shells save the flat /life page and hand it back on the next
        // launch: the first day must ride along and survive the restore.
        let life = body_of(&http("GET", "/life", ""));
        assert_eq!(life["unlocks"]["emotion:curious"], json!("2026-11-18"));
        assert_eq!(life["unlocks_backfilled"], json!(true));
        let response = http("POST", "/life", &life.to_string());
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert_eq!(unsafe { duck_core_tick(core) }, 0);
        let page = collection();
        assert_eq!(
            mood_row(&page, "curious"),
            (1, Some("2026-11-18".to_owned()))
        );

        unsafe { duck_core_free(core) };
    }

    /// QA blocker: a save that claims to be backfilled but does not cover a
    /// count it carries must not have that gap dated "today". The gap is
    /// history from before the record, so it is filled with an unknown day at
    /// restore, once, and stays unknown on later days.
    #[test]
    fn a_claimed_backfill_cannot_date_a_count_it_does_not_cover() {
        let (_guard, core) = new_core();
        unsafe { (*core).intents.set_day("2026-11-19".to_owned()) };
        let saved = json!({
            "emotions": { "curious": 2 },
            "skills": {},
            "unlocks": {},
            "unlocks_backfilled": true,
        });
        unsafe { (*core).intents.push(Intent::RestoreLife(saved)) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);

        let life: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.life_json() }).expect("life");
        assert_eq!(life["emotions"]["curious"], json!(2));
        assert_eq!(
            life["unlocks"]["emotion:curious"], "",
            "the uncovered count is history, not today"
        );
        assert_eq!(life["unlocks_backfilled"], json!(true));

        // A later day changes nothing: unknown stays unknown.
        unsafe { (*core).intents.set_day("2026-11-20".to_owned()) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);
        let life: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.life_json() }).expect("life");
        assert_eq!(life["unlocks"]["emotion:curious"], "");

        unsafe { duck_core_free(core) };
    }

    /// QA blocker: a paused duck refuses a real manifest skill at both doors —
    /// the intent arm and the C ABI — and the refused move never enters the
    /// book. `roulade` is a loaded one-shot: with the gate removed, both doors
    /// would accept it (the policy is driving) and the book would count it.
    #[test]
    fn a_paused_duck_refuses_a_skill_at_both_doors() {
        let (_guard, core) = new_core();
        tick_until_ready(core);
        unsafe { duck_core_set_paused(core, 1) };
        assert_eq!(unsafe { duck_core_paused(core) }, 1);

        let name = CString::new("roulade").unwrap();
        assert_eq!(unsafe { duck_core_trigger_skill(core, name.as_ptr()) }, 0);
        assert!(
            unsafe { (*core).label.contains("paused") },
            "the refusal names the pause: {}",
            unsafe { (*core).label.clone() }
        );

        let refused = unsafe { apply_intent(&mut *core, Intent::Skill("roulade".to_owned())) };
        assert_eq!(refused["ok"], json!(false));
        assert!(
            refused["error"].as_str().unwrap_or("").contains("paused"),
            "{refused}"
        );
        assert!(
            !unsafe {
                active_runtime(&*core)
                    .book()
                    .life
                    .skills
                    .contains_key("roulade")
            },
            "a refused skill is not part of the duck's story"
        );

        // The paused tick still publishes the lifetime record it already has,
        // and the refused move is not in it.
        assert_eq!(unsafe { duck_core_tick(core) }, 0);
        let life: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.life_json() }).expect("life");
        assert!(
            life["skills"].get("roulade").is_none(),
            "the refused move never reached the published record: {life}"
        );

        unsafe { duck_core_set_paused(core, 0) };
        unsafe { duck_core_free(core) };
    }

    /// A word the person taught is heard through the same door as a
    /// built-in one: it reaches the loaded skill and reports the phrase
    /// that matched.
    #[test]
    fn a_taught_word_triggers_the_skill_it_names() {
        let (_guard, core) = new_core();
        tick_until_ready(core);
        let aliases =
            std::collections::BTreeMap::from([("转圈圈".to_owned(), "roulade".to_owned())]);
        assert!(unsafe { (*core).intents.set_voice_aliases(aliases) }.is_ok());

        let answer = unsafe { apply_intent(&mut *core, Intent::Heard("转圈圈".to_owned())) };
        assert_eq!(answer["ok"], json!(true), "{answer}");
        assert_eq!(answer["heard"], json!("转圈圈"), "{answer}");
        assert_eq!(answer["phrase"], json!("转圈圈"), "{answer}");
        assert_eq!(answer["skill"], json!("roulade"), "{answer}");

        unsafe { duck_core_free(core) };
    }

    /// The pause gate is inherited by taught words: an alias is a way of
    /// hearing, not a way around frozen time.
    #[test]
    fn a_paused_duck_refuses_a_taught_word() {
        let (_guard, core) = new_core();
        tick_until_ready(core);
        let aliases =
            std::collections::BTreeMap::from([("转圈圈".to_owned(), "roulade".to_owned())]);
        assert!(unsafe { (*core).intents.set_voice_aliases(aliases) }.is_ok());
        unsafe { duck_core_set_paused(core, 1) };

        let answer = unsafe { apply_intent(&mut *core, Intent::Heard("转圈圈".to_owned())) };
        assert_eq!(answer["ok"], json!(false), "{answer}");
        assert!(
            answer["error"]
                .as_str()
                .unwrap_or_default()
                .contains("paused"),
            "{answer}"
        );
        assert!(
            !unsafe {
                active_runtime(&*core)
                    .book()
                    .life
                    .skills
                    .contains_key("roulade")
            },
            "a refused alias is not part of the duck's story"
        );

        unsafe { duck_core_set_paused(core, 0) };
        unsafe { duck_core_free(core) };
    }

    /// A taught word whose target the runtime no longer has — the queue's
    /// skill snapshot and the loaded network diverged, the gap a reload
    /// leaves behind — is a hearing answer, not a duck state: the reply
    /// keeps `heard` and `phrase`, and the duck's label is not overwritten
    /// with the move's own error.
    #[test]
    fn a_heard_mood_while_one_plays_is_dropped_not_failed() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            let emotions = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../upstream/microduck/robotd/emotions");
            let _ = c.runtime.load_emotions(&emotions);
            tick_until_ready(core);
            let first = apply_intent(c, Intent::Heard("开心".to_owned()));
            assert_eq!(first["ok"], true, "{first}");
            // A voice remote firing a second mood while the first plays is
            // the normal case: robotd drops it, the answer says so, and the
            // client does not see a failure.
            let second = apply_intent(c, Intent::Heard("好奇".to_owned()));
            assert_eq!(second["ok"], true, "{second}");
            assert_eq!(
                second["dropped"], "an emotion is already playing",
                "{second}"
            );
            assert_eq!(
                second["skill"], "curious",
                "the dropped verb is still named"
            );
            assert!(second["error"].is_null(), "{second}");
        }
    }

    #[test]
    fn a_heard_word_whose_target_vanished_keeps_its_context() {
        let (_guard, core) = new_core();
        tick_until_ready(core);
        unsafe {
            let c = &mut *core;
            // The queue publishes a skill the controller does not have, so
            // teaching the word passes the door while hearing it fails as
            // "no such skill".
            c.intents.set_skills(vec!["ghost_skill".to_owned()]);
            let aliases =
                std::collections::BTreeMap::from([("隐身".to_owned(), "ghost_skill".to_owned())]);
            assert!(c.intents.set_voice_aliases(aliases).is_ok());

            let label_before = c.label.clone();
            let answer = apply_intent(c, Intent::Heard("隐身".to_owned()));
            assert_eq!(answer["ok"], json!(false), "{answer}");
            assert_eq!(answer["heard"], json!("隐身"), "{answer}");
            assert_eq!(answer["phrase"], json!("隐身"), "{answer}");
            assert!(
                answer["error"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("no such skill"),
                "{answer}"
            );
            assert_eq!(answer["reason"], json!("the verb did not run"), "{answer}");
            assert_eq!(
                c.label, label_before,
                "a hearing failure is not the duck's state: {}",
                c.label
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// The power-off is the first gate a spoken move meets: mid shutdown the
    /// answer names the power-off, not 「press Start on the pad」 pointing at
    /// a duck that is going down, and a greeting is still just a sound.
    #[test]
    fn a_heard_move_during_the_poweroff_names_the_poweroff() {
        let (_guard, core) = new_core();
        tick_until_ready(core);
        unsafe {
            let c = &mut *core;
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

            let answer = apply_intent(c, Intent::Heard("sit".to_owned()));
            assert_eq!(answer["ok"], json!(false), "{answer}");
            assert!(
                answer["error"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("powering off"),
                "{answer}"
            );
            // A sound moves nothing: still available on the way down.
            let answer = apply_intent(c, Intent::Heard("hello".to_owned()));
            assert_eq!(
                answer["ok"],
                json!(true),
                "a greeting is not a move: {answer}"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// QA blocker: the pad's Skill key is the third door. While paused it is
    /// refused (remembered in `pad_refused`, the DKRB report's channel) and
    /// the book never counts the move.
    #[test]
    fn a_paused_pad_skill_is_refused_and_not_counted() {
        use crate::pad::{Buttons, PadFrame};

        let (_guard, core) = new_core();
        tick_until_ready(core);
        unsafe { duck_core_set_paused(core, 1) };
        // Bind a real counting manifest skill to the spare X button.
        assert!(unsafe { (*core).pad_mapper.bind("x", "roulade") });
        let press = PadFrame {
            held: Buttons::X,
            pressed: Buttons::X,
            ..Default::default()
        };
        let now = Instant::now();
        unsafe { pad_apply_frame(&mut *core, &press, now) };

        let refusal = unsafe { (*core).pad_refused.refusal(now) }.expect("the pad was refused");
        assert_eq!(refusal.method, "robot.do");
        assert!(
            refusal.reason.contains("paused"),
            "the refusal names the pause: {}",
            refusal.reason
        );
        assert!(
            !unsafe {
                active_runtime(&*core)
                    .book()
                    .life
                    .skills
                    .contains_key("roulade")
            },
            "a refused pad skill is not part of the duck's story"
        );

        unsafe { duck_core_set_paused(core, 0) };
        unsafe { duck_core_free(core) };
    }

    /// QA blocker: a spoken move (POST /heard "翻滚" -> roulade) is refused
    /// while paused — no count, no first day — and the answer says why.
    #[test]
    fn a_paused_heard_skill_is_refused_and_not_dated() {
        let (_guard, core) = new_core();
        tick_until_ready(core);
        unsafe { (*core).intents.set_day("2026-11-19".to_owned()) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);
        unsafe { duck_core_set_paused(core, 1) };

        let port = 19000 + ((std::process::id() % 1000) + 17) as i32;
        assert_eq!(unsafe { duck_core_start_intent_server(core, port) }, 0);
        // The handler enqueues the intent and waits for the tick's verdict.
        // The paused tick must answer the request itself with the refusal.
        let heard = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port as u16)).expect("connect");
            let body = r#"{"text":"翻滚"}"#;
            let request = format!(
                "POST /heard HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(request.as_bytes()).expect("write");
            let mut response = String::new();
            stream.read_to_string(&mut response).expect("read");
            response
        });
        // Tick fast enough to land inside the request's 300 ms budget.
        for _ in 0..120 {
            if heard.is_finished() {
                break;
            }
            assert_eq!(unsafe { duck_core_tick(core) }, 0);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let response = heard.join().expect("the heard request thread");
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(
            response.contains("paused"),
            "the answer itself says the duck is paused: {response}"
        );
        assert!(response.contains(r#""ok":false"#), "{response}");

        let life: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.life_json() }).expect("life");
        assert!(
            life["skills"].get("roulade").is_none(),
            "the spoken move was counted while paused: {life}"
        );
        assert!(
            life["unlocks"].get("skill:roulade").is_none(),
            "the spoken move was dated while paused: {life}"
        );

        // The refusal is the pause, not a missing verb.
        let refused = unsafe { apply_intent(&mut *core, Intent::Heard("翻滚".to_owned())) };
        assert_eq!(refused["ok"], json!(false));
        assert!(
            refused["error"].as_str().unwrap_or("").contains("paused"),
            "{refused}"
        );

        unsafe { duck_core_set_paused(core, 0) };
        unsafe { duck_core_free(core) };
    }

    /// QA blocker, the honest half: a paused tick still applies a hand on the
    /// duck (a pet is care, not a move), publishes it through `/state`, and
    /// dates nothing — the refused move leaves the collection locked.
    #[test]
    fn a_paused_tick_still_counts_a_pet_but_dates_nothing() {
        let (_guard, core) = new_core();
        tick_until_ready(core);
        unsafe { (*core).intents.set_day("2026-11-19".to_owned()) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);
        unsafe { duck_core_set_paused(core, 1) };

        let port = 19000 + ((std::process::id() % 1000) + 21) as i32;
        assert_eq!(unsafe { duck_core_start_intent_server(core, port) }, 0);
        let http = |method: &str, path: &str, body: &str| -> String {
            let mut stream = TcpStream::connect(("127.0.0.1", port as u16)).expect("connect");
            let request = format!(
                "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(request.as_bytes()).expect("write");
            let mut response = String::new();
            stream.read_to_string(&mut response).expect("read");
            response
        };
        let body_of = |response: &str| -> serde_json::Value {
            serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap_or("{}"))
                .unwrap_or(serde_json::Value::Null)
        };
        let pets_before = unsafe { active_runtime(&*core).book().life.pets };

        unsafe { (*core).intents.push(Intent::Skill("roulade".to_owned())) };
        unsafe { (*core).intents.push(Intent::Pet) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);

        // What the shell reads is `/state`, not the queue's internal copy:
        // this proves the paused branch's note_unlocks/set_life were published.
        let response = http("GET", "/state", "");
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        let state = body_of(&response);
        assert_eq!(
            state["life"]["pets"],
            json!(pets_before + 1),
            "a paused tick still feels the hand and publishes it: {state}"
        );
        assert_eq!(
            state["life"]["unlocks"],
            json!({}),
            "nothing was dated in a paused tick: {state}"
        );
        assert!(
            state["life"]["skills"].get("roulade").is_none(),
            "the paused move was counted: {state}"
        );

        let page: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.collection_json() })
                .expect("collection");
        let roulade = page["skills"]
            .as_array()
            .expect("skills")
            .iter()
            .find(|row| row["name"] == "roulade")
            .expect("roulade is a manifest trick");
        assert_eq!(roulade["count"], json!(0), "{roulade}");
        assert!(roulade.get("first_day").is_none(), "{roulade}");
        assert_eq!(page["tricks"]["unlocked"], json!(0), "{page}");

        unsafe { duck_core_set_paused(core, 0) };
        unsafe { duck_core_free(core) };
    }

    /// QA blocker B1: a child's "when I pet you, roll over" rule fires from
    /// the pet primitive. A paused duck still feels the pet — that count is
    /// real — but the rule's skill must not start, count or get a first day.
    #[test]
    fn a_paused_pet_rule_cannot_start_or_date_a_skill() {
        let (_guard, core) = new_core();
        tick_until_ready(core);
        unsafe { (*core).intents.set_day("2026-11-19".to_owned()) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);
        unsafe {
            (*core).intents.push(Intent::SetRules(json!({
                "rules": [{ "trigger": "pet", "action": "skill", "value": "roulade" }]
            })))
        };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);
        unsafe { duck_core_set_paused(core, 1) };
        let pets_before = unsafe { active_runtime(&*core).book().life.pets };

        unsafe { (*core).intents.push(Intent::Pet) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);

        let life: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.life_json() }).expect("life");
        assert_eq!(
            life["pets"],
            json!(pets_before + 1),
            "the pet itself is real even while paused: {life}"
        );
        assert!(
            life["skills"].get("roulade").is_none(),
            "the paused rule started a skill: {life}"
        );
        assert!(
            life["unlocks"].get("skill:roulade").is_none(),
            "the paused rule dated a skill: {life}"
        );
        assert!(
            unsafe { (*core).runtime.last_rule().is_none() },
            "a refused rule step must not be reported as fired"
        );

        unsafe { duck_core_set_paused(core, 0) };
        unsafe { duck_core_free(core) };
    }

    /// QA blocker B2: morning's official "yes" is a mood. A paused duck still
    /// ends the night (the event is real) but must not count or date the wake
    /// emotion.
    #[test]
    fn a_paused_morning_does_not_count_the_wake_emotion() {
        let emotions_dir =
            crate::fixtures::workspace_root().join("upstream/microduck/robotd/emotions");
        if !emotions_dir.is_dir() {
            if crate::fixtures::required() {
                panic!(
                    "the official emotions are missing (looked in {})",
                    emotions_dir.display()
                );
            }
            eprintln!(
                "skipping: the official emotions are not here ({})",
                emotions_dir.display()
            );
            return;
        }
        let (_guard, core) = new_core();
        let dir = CString::new(emotions_dir.to_string_lossy().into_owned()).unwrap();
        assert!(unsafe { duck_core_load_emotions(core, dir.as_ptr()) } > 0);
        tick_until_ready(core);
        unsafe { (*core).intents.set_day("2026-11-19".to_owned()) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);
        unsafe { duck_core_set_paused(core, 1) };

        unsafe { (*core).intents.push(Intent::Night(true)) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);
        unsafe { (*core).intents.push(Intent::Night(false)) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);

        let life: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.life_json() }).expect("life");
        assert!(
            life["emotions"].get("yes").is_none(),
            "the paused wake emotion was counted: {life}"
        );
        assert!(
            life["unlocks"].get("emotion:yes").is_none(),
            "the paused wake emotion was dated: {life}"
        );

        // The night itself really ended, and the paused tick published it.
        let state: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.state_json() }).expect("state");
        assert!(
            state["life"]["emotions"].get("yes").is_none(),
            "the paused wake emotion was counted in the published state: {state}"
        );
        assert!(
            state["life"]["unlocks"].get("emotion:yes").is_none(),
            "the paused wake emotion was dated in the published state: {state}"
        );
        assert_eq!(
            state["night"]["active"],
            json!(false),
            "the morning ended the night: {state}"
        );

        unsafe { duck_core_set_paused(core, 0) };
        unsafe { duck_core_free(core) };
    }

    /// QA minor: a restore keeps no first day for a move the duck never did
    /// (count 0) and no date for a key that is not in the count tables at all.
    #[test]
    fn a_restore_drops_dates_for_zero_and_unknown_moves() {
        let (_guard, core) = new_core();
        unsafe { (*core).intents.set_day("2026-11-19".to_owned()) };
        unsafe {
            (*core).intents.push(Intent::RestoreLife(json!({
                "emotions": { "curious": 0 },
                "skills": { "roulade": 0 },
                "unlocks": {
                    "emotion:curious": "2026-11-18",
                    "skill:roulade": "2026-11-18",
                    "skill:nope": "2026-11-18"
                },
                "unlocks_backfilled": true,
            })))
        };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);

        let life: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.life_json() }).expect("life");
        assert_eq!(
            life["unlocks"],
            json!({}),
            "zero and unknown moves keep no date: {life}"
        );
        assert_eq!(life["unlocks_backfilled"], json!(true));

        let page: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.collection_json() })
                .expect("collection");
        for row in page["emotions"].as_array().expect("emotions") {
            assert!(row.get("first_day").is_none(), "{row}");
        }
        for row in page["skills"].as_array().expect("skills") {
            assert!(row.get("first_day").is_none(), "{row}");
        }

        unsafe { duck_core_free(core) };
    }

    /// QA major: the restore keeps an unlock for a name that really exists in
    /// the count tables, whatever characters it contains. A manifest skill
    /// with a space or a dot keeps its real first day instead of being
    /// forgotten and then re-dated as "today" on every launch.
    #[test]
    fn an_odd_skill_name_keeps_its_day_across_a_restore() {
        let (_guard, core) = new_core();
        unsafe {
            let life = &mut (*core).runtime.book_mut().life;
            life.skills.insert("my move.v2".to_owned(), 2);
            life.unlocks
                .insert("skill:my move.v2".to_owned(), "2026-11-18".to_owned());
            life.unlocks_backfilled = true;
        }
        unsafe { (*core).intents.set_life((*core).runtime.book()) };
        let saved: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.life_json() }).expect("saved life");
        assert_eq!(saved["unlocks"]["skill:my move.v2"], "2026-11-18");

        // What the next launch does: hand the save back, on a later day.
        unsafe { (*core).intents.set_day("2026-11-19".to_owned()) };
        unsafe { (*core).intents.push(Intent::RestoreLife(saved)) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);

        let life: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.life_json() }).expect("life");
        assert_eq!(life["skills"]["my move.v2"], json!(2));
        assert_eq!(
            life["unlocks"]["skill:my move.v2"], "2026-11-18",
            "the real name keeps its real first day"
        );

        unsafe { duck_core_free(core) };
    }

    /// QA blocker: a real restart on a later day must not re-date a first day
    /// the saved record already knows. The first session plays a manifest
    /// trick; the second restores its /life save two days later.
    #[test]
    fn a_restart_on_a_later_day_does_not_redate_an_unlock() {
        let (guard, core) = new_core();
        tick_until_ready(core);
        unsafe { (*core).intents.set_day("2026-11-18".to_owned()) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);
        unsafe { (*core).intents.push(Intent::Skill("roulade".to_owned())) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);
        let saved: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.life_json() }).expect("saved life");
        assert_eq!(saved["unlocks"]["skill:roulade"], "2026-11-18");
        unsafe { duck_core_free(core) };
        drop(guard);

        // A new launch, two days later, with the shell's save handed back.
        let (_guard, core) = new_core();
        unsafe { (*core).intents.set_day("2026-11-20".to_owned()) };
        unsafe { (*core).intents.push(Intent::RestoreLife(saved)) };
        assert_eq!(unsafe { duck_core_tick(core) }, 0);
        let life: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.life_json() }).expect("life");
        assert_eq!(life["unlocks"]["skill:roulade"], "2026-11-18");
        for (key, day) in life["unlocks"].as_object().expect("unlocks") {
            assert_ne!(
                day.as_str(),
                Some("2026-11-20"),
                "{key} was re-dated by the restart"
            );
        }

        unsafe { duck_core_free(core) };
    }

    // -----------------------------------------------------------------------
    // H 工单的接线测试：pad 事件、IPC 端到端、嗓音种子、/state 健康字段。

    /// 嗓音种子：同一构建（同一指纹）两次构造得到同一种子，不同指纹得到
    /// 不同种子。种子是指纹前 8 位 hex，稳定且可复现。
    #[test]
    fn the_voice_seed_follows_the_fingerprint() {
        let first = seed_from_fingerprint(
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        );
        let same = seed_from_fingerprint(
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        );
        let other = seed_from_fingerprint(
            "00000000deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        );
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
        assert_eq!(
            runtime_seed, seed,
            "the core's voice seed is the fingerprint's"
        );
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

    /// DKRB cadence: the report clock is peeked, not consumed, so a beat that
    /// is not due does not make the next one due. Official cadence is one
    /// report per second (`pad-map/src/wire.rs:254-344`); the old
    /// `pad_report_at.take()` produced one every other tick.
    #[test]
    fn dkrb_reports_once_a_second_not_every_other_tick() {
        let (_guard, core) = new_core();
        // Fixed UDP port: if another process owns it, this test cannot build
        // a receiver, and the cadence itself is the pure `pad_report_due`
        // arithmetic the rest of this test pins. Skip as the IPC end-to-end
        // test does for its fixed TCP port.
        let Some(pad) = crate::pad::Pad::start() else {
            unsafe { duck_core_free(core) };
            return;
        };
        let base = Instant::now();
        let mut fired = Vec::new();
        // The 50 Hz beat the live loop would run, injected with a synthetic
        // clock: the report is due at t=0 (never reported), then only after a
        // whole second. The old take() made 1.4 s due as well.
        for ms in (0..3000).step_by(100) {
            let due = unsafe {
                pad_report_due(
                    &mut *core,
                    &pad,
                    base + std::time::Duration::from_millis(ms),
                )
            };
            if due {
                fired.push(ms);
            }
        }
        assert_eq!(
            fired,
            vec![0, 1000, 2000],
            "DKRB fires once a second, not on the beat after every quiet one"
        );
        unsafe { duck_core_free(core) };
    }

    /// Drive mode is the official `DriveAxes` mapping, not the phone
    /// joystick's one-stick convention: left stick walks (LeftY) and strafes
    /// (LeftX, inverted), right stick turns (RightX, inverted)
    /// (`robotd-params/src/pad_axes.rs:154-179`).
    #[test]
    fn drive_mode_maps_all_four_axes_like_the_official_default() {
        use crate::pad::PadFrame;
        let (_guard, core) = new_core();
        let frame = PadFrame {
            // Full forward, half left, full right: vx=+0.4, vy=-0.15,
            // vyaw=+1.0 under the official signs and limits.
            left_y: 1.0,
            left_x: 0.5,
            right_x: -1.0,
            ..Default::default()
        };
        unsafe {
            let c = &mut *core;
            c.pad_mode = crate::pad::PadMode::Drive;
            pad_frame_commands(c, &frame, crate::pad::PadMode::Drive);
            let command = c
                .runtime
                .checkpoint()
                .expect("the body has a state")
                .command_twist;
            assert!((command[0] - 0.4).abs() < 1e-9, "vx: {command:?}");
            assert!((command[1] + 0.15).abs() < 1e-9, "vy: {command:?}");
            assert!((command[2] - 1.0).abs() < 1e-9, "vyaw: {command:?}");

            // Non-vacuity against HeadDrive: the same sticks there leave vy at
            // zero and turn from the left stick, which is what Drive used to
            // do. The two mappings must differ.
            pad_frame_commands(c, &frame, crate::pad::PadMode::HeadDrive);
            let head_drive = c
                .runtime
                .checkpoint()
                .expect("the body has a state")
                .command_twist;
            assert!(head_drive[1].abs() < 1e-9, "head-drive has no strafe");
            assert!(
                (head_drive[2] + 0.5).abs() < 1e-9,
                "head-drive turns from LeftX inverted: {head_drive:?}"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// BodyPose is released when the mode is **left**, not when it is entered
    /// (`pad-map/src/mapper.rs:40-54`): a held crouch must survive the D-pad
    /// press that starts it, and the next mode must not inherit it.
    #[test]
    fn body_pose_is_released_on_leaving_not_entering() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            // A body pose is being held.
            on_active(c, |runtime| runtime.set_body(0.01, 0.1, 0.0));
            assert!(c.runtime.checkpoint().unwrap().body_active);

            // Entering BodyPose: the old dispatch released it here.
            assert!(
                dispatch_pad_event(
                    c,
                    &crate::pad::PadButtonEvent::ModeChange(crate::pad::PadMode::BodyPose)
                )
                .is_none()
            );
            assert_eq!(c.pad_mode, crate::pad::PadMode::BodyPose);
            assert!(
                c.runtime.checkpoint().unwrap().body_active,
                "entering BodyPose must keep the pose it is about to hold"
            );

            // Leaving BodyPose releases it.
            assert!(
                dispatch_pad_event(
                    c,
                    &crate::pad::PadButtonEvent::ModeChange(crate::pad::PadMode::Drive)
                )
                .is_none()
            );
            assert_eq!(c.pad_mode, crate::pad::PadMode::Drive);
            assert!(
                !c.runtime.checkpoint().unwrap().body_active,
                "leaving BodyPose releases the pose"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// Start held is the official home sequence: policy off, then `init`
    /// (`pad-map/src/mapper.rs:262-281`). The duck must stay powered with
    /// torque on — not go limp through the shell's `disable` — and the policy
    /// must stay off after the ramp.
    #[test]
    fn start_held_homes_with_the_policy_off_and_torque_on() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            assert!(runtime_policy_enabled(&c.runtime), "driving before Home");

            assert!(dispatch_pad_event(c, &crate::pad::PadButtonEvent::Home).is_none());
            assert!(
                !c.runtime.checkpoint().unwrap().enabled,
                "Home turns the policy off"
            );
            assert!(
                !matches!(c.runtime.bringup(), BringupState::Limp),
                "Home is init, not the shutdown-path disable: torque stays on and the duck ramps"
            );
            assert_eq!(
                c.pad_mode,
                crate::pad::PadMode::Drive,
                "Home puts the sticks back to plain driving"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// Select held: two seconds is `robot.rest` (sit and rest pose, no
    /// power-off), four is `robot.shutdown` (powers off). The two events no
    /// longer share the shutdown path.
    #[test]
    fn pad_rest_parks_powered_and_pad_shutdown_powers_off() {
        use crate::pad::PadButtonEvent;
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            assert!(dispatch_pad_event(c, &PadButtonEvent::Rest).is_none());
            for _ in 0..400 {
                duck_core_tick(core);
            }
            assert!(
                !c.runtime.powered_off(),
                "a pad rest must not latch the duck off"
            );

            assert!(dispatch_pad_event(c, &PadButtonEvent::Shutdown).is_none());
            let _ = duck_core_tick(core);
            assert!(
                c.runtime.powered_off(),
                "a pad shutdown is still the power-off path"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// The pad's bound skill names follow the HTTP intent arm: `sit` and
    /// `stand` are idempotent, `sit_toggle` flips (COMPARISON deviation #11).
    #[test]
    fn pad_sit_and_stand_are_idempotent_while_toggle_flips() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            // Bound exactly as a person could bind them; the button name is
            // the mapper's, the skill name is the table's.
            assert!(c.pad_mapper.bindings.bind("a", "sit"));
            assert!(dispatch_pad_event(c, &crate::pad::PadButtonEvent::Skill("a")).is_none());
            assert!(c.runtime.is_sitting(), "sit sat the duck");
            assert!(
                dispatch_pad_event(c, &crate::pad::PadButtonEvent::Skill("a")).is_none(),
                "a second sit is accepted"
            );
            assert!(c.runtime.is_sitting(), "and it did not toggle the duck up");

            assert!(c.pad_mapper.bindings.bind("a", "stand"));
            // Let the seat settle: the default sitdown ramp is 2.0 s
            // (`DEFAULT_SITSTAND_RAMP_S`), and the controller refuses to rise
            // while it is still running ("still sitting down").
            for _ in 0..140 {
                duck_core_tick(core);
            }
            assert!(dispatch_pad_event(c, &crate::pad::PadButtonEvent::Skill("a")).is_none());
            assert!(
                !c.runtime.is_sitting(),
                "stand started the rise from the seat"
            );
            assert!(
                dispatch_pad_event(c, &crate::pad::PadButtonEvent::Skill("a")).is_none(),
                "a second stand is accepted"
            );
            assert!(
                !c.runtime.is_sitting(),
                "and it did not sit the duck back down"
            );

            // The toggle spelling still flips, which is what the A button's
            // shipped binding means. Let the rise finish first: a toggle
            // mid-rise is the official "already standing up" refusal.
            for _ in 0..120 {
                duck_core_tick(core);
            }
            assert!(c.pad_mapper.bindings.bind("a", "sit_toggle"));
            assert!(dispatch_pad_event(c, &crate::pad::PadButtonEvent::Skill("a")).is_none());
            assert!(
                c.runtime.is_sitting(),
                "sit_toggle sits a duck that is up (mid-rise counts as not sitting)"
            );
        }
        unsafe { duck_core_free(core) };
    }

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
        send(
            &mut stream,
            2,
            "robot.move",
            r#"{"vx":0.4,"vy":0.0,"vyaw":0.0}"#,
        );
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

    /// The extended `robot.*` set, end to end over the real TCP socket: every
    /// method's answer shape and the duck state its intent produced. The
    /// methods are the official calls this build can honestly serve — the
    /// remainder are pinned to `METHOD_NOT_FOUND` in `ipc.rs`.
    #[test]
    fn robot_ipc_serves_the_extended_method_set() {
        use std::io::{BufRead, BufReader};
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            if c.ipc.is_none() {
                // Fixed port taken by another test process: the socket face
                // cannot be exercised here, and the core itself is fine.
                duck_core_free(core);
                return;
            }
        }
        let stream = TcpStream::connect(("127.0.0.1", crate::ipc::ROBOT_IPC_PORT))
            .expect("connect robot ipc");
        let mut reader = BufReader::new(stream.try_clone().expect("dup"));
        let mut stream = stream;

        fn call(
            stream: &mut TcpStream,
            reader: &mut BufReader<TcpStream>,
            id: u64,
            method: &str,
            params: &str,
        ) -> serde_json::Value {
            let line = format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"{method}\",\"params\":{params}}}\n"
            );
            stream.write_all(line.as_bytes()).expect("write");
            stream.flush().expect("flush");
            let mut reply = String::new();
            reader.read_line(&mut reply).expect("reply");
            serde_json::from_str(&reply).expect("json reply")
        }
        let result = |reply: &serde_json::Value| reply["result"].clone();

        unsafe {
            // Mode / model / policies / vetbook / skills / safeToRestart are
            // reads answered from the tick snapshot.
            let reply = call(&mut stream, &mut reader, 1, "robot.mode", "{}");
            assert_eq!(result(&reply)["mode"], "walk", "{reply}");

            let reply = call(&mut stream, &mut reader, 2, "robot.model", "{}");
            let model = result(&reply);
            assert_eq!(model["asset"], duck_ipc_proto::ROBOT_MODEL, "{reply}");
            assert_eq!(model["joint_names"].as_array().unwrap().len(), 15);
            assert_eq!(model["tof_beams"].as_array().unwrap().len(), 64);

            let reply = call(&mut stream, &mut reader, 3, "robot.policies", "{}");
            let policies = result(&reply);
            assert_eq!(policies["mode"], "walk", "{reply}");
            assert_eq!(policies["enabled"], true, "{reply}");
            let slots = policies["slots"].as_array().expect("slots");
            let walk = slots
                .iter()
                .find(|slot| slot["slot"] == "walk")
                .expect("the walking slot is reported");
            assert!(walk["path"].is_string(), "{reply}");

            let reply = call(&mut stream, &mut reader, 4, "robot.vetbook", "{}");
            let vetbook = result(&reply);
            assert!(vetbook["powered_s"].is_number(), "{reply}");
            assert!(vetbook["boots"].is_number(), "{reply}");

            let reply = call(&mut stream, &mut reader, 5, "robot.skills", "{}");
            let skills = result(&reply);
            let names: Vec<&str> = skills["skills"]
                .as_array()
                .expect("skill table")
                .iter()
                .filter_map(|skill| skill["name"].as_str())
                .collect();
            assert!(names.contains(&"roulade"), "{reply}");
            let built_in: Vec<&str> = skills["built_in"]
                .as_array()
                .expect("built-ins")
                .iter()
                .filter_map(|name| name.as_str())
                .collect();
            assert!(built_in.contains(&"ground_pick"), "{reply}");
            assert!(built_in.contains(&"sit_toggle"), "{reply}");

            let reply = call(&mut stream, &mut reader, 6, "robot.safeToRestart", "{}");
            assert_eq!(result(&reply)["safe"], true, "{reply}");

            // robot.look: the IK answer, then the head intent it queued lands
            // on the next tick.
            let reply = call(
                &mut stream,
                &mut reader,
                7,
                "robot.look",
                "{\"x\":0.3,\"y\":0.1,\"z\":0.2,\"neck_pitch\":0.0}",
            );
            let look = result(&reply);
            assert!(look["head"]["head_yaw"].is_number(), "{reply}");
            assert!(look["clamped"].is_boolean(), "{reply}");
            let expected = kinematics::head::HeadFk::alpha().look_at([0.3, 0.1, 0.2], 0.0);
            duck_core_tick(core);
            let head = (*core).runtime.checkpoint().unwrap().command_head;
            for (index, value) in expected.joints.iter().enumerate() {
                assert!(
                    (head[index] - value).abs() < 1e-9,
                    "the look queued the IK joints: {head:?} vs {:?}",
                    expected.joints
                );
            }

            // robot.pose: active glides toward the offsets; false releases.
            let reply = call(
                &mut stream,
                &mut reader,
                8,
                "robot.pose",
                "{\"z\":0.01,\"roll\":0.1,\"pitch\":0.0,\"active\":true}",
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            duck_core_tick(core);
            assert!(
                (*core).runtime.checkpoint().unwrap().body_active,
                "an active pose is held"
            );
            let reply = call(
                &mut stream,
                &mut reader,
                9,
                "robot.pose",
                "{\"active\":false}",
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            duck_core_tick(core);
            assert!(
                !(*core).runtime.checkpoint().unwrap().body_active,
                "an inactive pose releases it"
            );

            // robot.mouth: the intent reaches the beak command.
            let has_mouth = (*core).runtime.has_mouth();
            let reply = call(
                &mut stream,
                &mut reader,
                10,
                "robot.mouth",
                "{\"open\":0.5}",
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            duck_core_tick(core);
            let mouth = (*core).runtime.checkpoint().unwrap().mouth;
            assert!(
                if has_mouth {
                    (mouth - 0.5).abs() < 1e-9
                } else {
                    mouth.abs() < 1e-9
                },
                "the beak command follows the scene: {mouth} (has_mouth {has_mouth})"
            );

            // robot.sound: a queued official tag.
            (*core).runtime.clear_sounds();
            let reply = call(
                &mut stream,
                &mut reader,
                11,
                "robot.sound",
                "{\"tag\":\"chirp\"}",
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            duck_core_tick(core);
            assert_eq!(
                (*core).runtime.take_sound().as_deref(),
                Some("chirp"),
                "the chirp reached the voice queue"
            );

            // robot.flashlight: the virtual face LED.
            let reply = call(
                &mut stream,
                &mut reader,
                12,
                "robot.flashlight",
                "{\"on\":true,\"color\":\"red\"}",
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            duck_core_tick(core);
            let face = (*core).runtime.face();
            assert_ne!(face.flashlight, 0, "the flashlight lit");
            assert_eq!(face.color(), Some("red"));

            // robot.stop: zero the twist, policy untouched.
            (*core).runtime.set_twist([0.4, 0.0, 0.0]);
            let reply = call(&mut stream, &mut reader, 13, "robot.stop", "{}");
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            duck_core_tick(core);
            let requested = (*core)
                .runtime
                .robot_state()
                .expect("the stop tick published a frame")
                .movement
                .requested;
            assert_eq!(requested, [0.0; 3], "stop zeroed the twist the gate saw");

            // robot.enable: off stops the policy with torque on; toggle flips.
            let reply = call(
                &mut stream,
                &mut reader,
                14,
                "robot.enable",
                "{\"on\":false}",
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            assert!(
                result(&reply)["reason"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("disabled"),
                "{reply}"
            );
            duck_core_tick(core);
            assert!(
                !(*core).runtime.checkpoint().unwrap().enabled,
                "enable on:false stopped the policy"
            );
            assert_ne!(
                (*core).runtime.bringup(),
                BringupState::Limp,
                "enable on:false is not relax: torque stays on"
            );
            let reply = call(
                &mut stream,
                &mut reader,
                15,
                "robot.enable",
                "{\"on\":false,\"toggle\":true}",
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            duck_core_tick(core);
            assert!(
                (*core).runtime.checkpoint().unwrap().enabled,
                "the toggle turned the policy back on"
            );

            // robot.init: the official power request, accepted even when there
            // is nothing to bring up.
            let reply = call(&mut stream, &mut reader, 16, "robot.init", "{}");
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            duck_core_tick(core);

            // robot.relax: torque off, limp, still powered.
            let reply = call(&mut stream, &mut reader, 17, "robot.relax", "{}");
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            duck_core_tick(core);
            assert_eq!((*core).runtime.bringup(), BringupState::Limp);
            assert!(!(*core).runtime.powered_off(), "relax is not a power-off");

            // robot.rest: sit and rest, no power-off latch.
            (*core).runtime.init().expect("stand back up from relax");
            for _ in 0..400 {
                duck_core_tick(core);
                if matches!((*core).runtime.bringup(), BringupState::Ready) {
                    break;
                }
            }
            let reply = call(&mut stream, &mut reader, 18, "robot.rest", "{}");
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            for _ in 0..400 {
                duck_core_tick(core);
            }
            assert!(
                !(*core).runtime.powered_off(),
                "robot.rest keeps the duck powered (robotd main.rs:3130-3175)"
            );

            // robot.shutdown: the power-off path, distinct from rest.
            let reply = call(&mut stream, &mut reader, 19, "robot.shutdown", "{}");
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            let _ = duck_core_tick(core);
            assert!(
                (*core).runtime.powered_off(),
                "robot.shutdown powers the duck off"
            );

            // robot.loadPolicy: a slot override is recorded and reloaded; the
            // bare file name is resolved against the policies directory. The
            // duck is powered back on first so the reload is not refused by
            // the shutdown gate.
            (*core).runtime.power_on().expect("power back on");
            for _ in 0..400 {
                duck_core_tick(core);
                if matches!((*core).runtime.bringup(), BringupState::Ready) {
                    break;
                }
            }
            let reply = call(
                &mut stream,
                &mut reader,
                20,
                "robot.loadPolicy",
                "{\"slot\":\"sitstand\",\"path\":\"alpha_sitstand.onnx\"}",
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            assert_eq!(
                crate::manifest::slot_override("sitstand").as_deref(),
                Some("alpha_sitstand.onnx")
            );
            duck_core_tick(core);
            // Reset returns the slot to the set's own.
            let reply = call(
                &mut stream,
                &mut reader,
                21,
                "robot.loadPolicy",
                "{\"slot\":\"sitstand\"}",
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            assert_eq!(crate::manifest::slot_override("sitstand"), None);
            duck_core_tick(core);

            // A path with no slot is the official refusal; a route out of the
            // directory is refused with the mobile wording.
            let reply = call(
                &mut stream,
                &mut reader,
                22,
                "robot.loadPolicy",
                "{\"path\":\"alpha_sitstand.onnx\"}",
            );
            assert_eq!(result(&reply)["accepted"], false, "{reply}");
            assert!(
                result(&reply)["reason"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("a path needs a slot"),
                "{reply}"
            );
            let reply = call(
                &mut stream,
                &mut reader,
                23,
                "robot.loadPolicy",
                "{\"slot\":\"sitstand\",\"path\":\"../escape.onnx\"}",
            );
            assert_eq!(result(&reply)["accepted"], false, "{reply}");

            // robot.reloadPolicies reads the same directory back.
            let reply = call(&mut stream, &mut reader, 24, "robot.reloadPolicies", "{}");
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            duck_core_tick(core);

            // robot.setMode: the mode name is validated at the door; a known
            // mode is queued through the same `Intent::Mode` `/mode` drives.
            // Runs before the subscription, so its `duck_core_tick` state
            // pushes cannot interleave with these request/response pairs.
            let reply = call(
                &mut stream,
                &mut reader,
                25,
                "robot.setMode",
                "{\"mode\":\"walk\"}",
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            duck_core_tick(core);
            let reply = call(
                &mut stream,
                &mut reader,
                26,
                "robot.setMode",
                "{\"mode\":\"hover\"}",
            );
            assert_eq!(result(&reply)["accepted"], false, "{reply}");
            assert!(
                result(&reply)["reason"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("walk"),
                "{reply}"
            );

            // robot.subscribe's acknowledgement carries the identity fields:
            // the four slots and the configurable skills.
            let reply = call(&mut stream, &mut reader, 27, "robot.subscribe", "{}");
            let identity = result(&reply);
            assert_eq!(identity["accepted"], true, "{reply}");
            assert!(identity["walk"].is_string(), "{reply}");
            assert!(identity["sitstand"].is_string(), "{reply}");
            assert!(
                identity["skills"]
                    .as_array()
                    .is_some_and(|skills| !skills.is_empty()),
                "{reply}"
            );

            // The methods this phone cannot honestly serve stay the official
            // not-served refusal.
            let reply = call(
                &mut stream,
                &mut reader,
                28,
                "robot.theremin",
                "{\"active\":false}",
            );
            assert_eq!(
                reply["error"]["code"],
                duck_ipc_proto::code::METHOD_NOT_FOUND,
                "{reply}"
            );
            let reply = call(&mut stream, &mut reader, 29, "update.status", "{}");
            assert_eq!(
                reply["error"]["code"],
                duck_ipc_proto::code::METHOD_NOT_FOUND,
                "{reply}"
            );
        }
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
                matches!(
                    c.runtime.bringup(),
                    BringupState::Ready | BringupState::Homing
                ),
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
        let source = crate::fixtures::policies_dir()
            .expect("the official bundle")
            .join("alpha_walking.onnx");
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

    /// Official `PolicyChange::Reload` disturbs whatever is driving even when
    /// every path is identical — the case it exists for is same path, new
    /// bytes (`robotd/src/main.rs:2059-2061`). A walking duck reloading its own
    /// directory must therefore go home first.
    #[test]
    fn a_same_path_reload_still_disturbs_a_driving_duck() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            let before = c.runtime.walking_slot_path().expect("walk loaded");
            assert!(
                c.runtime.driving().is_some(),
                "the duck is driving at Ready"
            );

            let dir = CString::new(c.policies.to_string_lossy().into_owned()).unwrap();
            assert_eq!(duck_core_reload_policies(core, dir.as_ptr()), 0);
            assert!(
                c.runtime.swap_pending(),
                "the reload is queued as a policy change"
            );
            assert_eq!(
                c.runtime.swap_state(),
                Some(PolicySwap::GoingHome),
                "a Reload disturbs a driving network even with identical paths"
            );
            assert_eq!(
                c.runtime.walking_slot_path().as_deref(),
                Some(before.as_path()),
                "and nothing has swapped yet"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// A reload with nothing driving lands in place: the official `was_driving`
    /// gate (`robotd/src/main.rs:3040-3047`) asks what stepped last tick, and a
    /// disabled duck has nothing a swap could interrupt.
    #[test]
    fn a_reload_with_nothing_driving_lands_in_place() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            let before = c.runtime.walking_slot_path().expect("walk loaded");
            // The official `robot.enable on:false`: policy stopped, torque on,
            // bring-up still Ready — so the body is up but nothing is stepping.
            c.runtime.set_policy_enabled(false);
            let _ = duck_core_tick(core);
            assert!(
                !c.runtime.checkpoint().unwrap().enabled,
                "the policy is off"
            );
            assert_eq!(
                c.runtime.bringup(),
                BringupState::Ready,
                "and the duck is still standing"
            );

            let dir = CString::new(c.policies.to_string_lossy().into_owned()).unwrap();
            assert_eq!(duck_core_reload_policies(core, dir.as_ptr()), 0);
            assert!(
                c.runtime.swap_pending(),
                "the change is queued (an in-place one lands on the next tick)"
            );
            assert_eq!(
                c.runtime.swap_state(),
                Some(PolicySwap::InPlace),
                "nothing driving means nothing to walk home for"
            );
            assert_eq!(
                c.runtime.walking_slot_path().as_deref(),
                Some(before.as_path()),
                "same file in, same file out"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// A slot change compares only the network that stepped last tick
    /// (`robotd/src/main.rs:2062-2070`), per slot, not just walk. Standing,
    /// sitstand, ground-pick and skill driving each make a change to their own
    /// file disturbing; a change to a slot that is not driving stays in place.
    #[test]
    fn a_slot_change_disturbs_only_the_network_that_is_driving() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            // Install a standing network (the shipped set has none in the stand
            // slot), reload, and let zero command select it.
            crate::manifest::set_slot_override("stand", Some("alpha_stand.onnx".to_owned()))
                .expect("stand override");
            let dir = CString::new(c.policies.to_string_lossy().into_owned()).unwrap();
            assert_eq!(duck_core_reload_policies(core, dir.as_ptr()), 0);
            let _ = &dir;
            wait_for_swap(core);
            for _ in 0..200 {
                let _ = duck_core_tick(core);
                if matches!(c.runtime.driving(), Some(crate::control::Driving::Stand)) {
                    break;
                }
            }
            assert!(
                matches!(c.runtime.driving(), Some(crate::control::Driving::Stand)),
                "zero command selects the standing network: {:?}",
                c.runtime.driving()
            );

            // The stand file changes while stand drives: disturbing.
            crate::manifest::set_slot_override("stand", Some("alpha_walking.onnx".to_owned()))
                .expect("stand override");
            let dir = CString::new(c.policies.to_string_lossy().into_owned()).unwrap();
            reload_policies(c, &dir.to_string_lossy(), ReloadKind::SlotChange, 0)
                .expect("the slot change is queued");
            assert!(c.runtime.swap_pending());
            assert_eq!(
                c.runtime.swap_state(),
                Some(PolicySwap::GoingHome),
                "changing the stand file while stand drives goes home first"
            );
            wait_for_swap(core);

            // Ground pick driving: changing its file disturbs.
            assert!(
                c.runtime.start_ground_pick().is_ok(),
                "the ground-pick slot is loaded"
            );
            let _ = duck_core_tick(core);
            assert!(
                matches!(
                    c.runtime.driving(),
                    Some(crate::control::Driving::GroundPick)
                ),
                "the pick is driving: {:?}",
                c.runtime.driving()
            );
            crate::manifest::set_slot_override("ground_pick", Some("alpha_stand.onnx".to_owned()))
                .expect("ground-pick override");
            let dir = CString::new(c.policies.to_string_lossy().into_owned()).unwrap();
            reload_policies(c, &dir.to_string_lossy(), ReloadKind::SlotChange, 0)
                .expect("the slot change is queued");
            assert_eq!(
                c.runtime.swap_state(),
                Some(PolicySwap::GoingHome),
                "changing the ground-pick file while it drives goes home first"
            );
            wait_for_swap(core);

            // A change to a slot that is not the one driving is in place.
            crate::manifest::set_slot_override("sitstand", Some("alpha_walking.onnx".to_owned()))
                .expect("sitstand override");
            // Something must be driving again: let the pick finish and the walk
            // network take the zero command.
            for _ in 0..400 {
                let _ = duck_core_tick(core);
                if c.runtime
                    .driving()
                    .is_some_and(|driving| !matches!(driving, crate::control::Driving::GroundPick))
                {
                    break;
                }
            }
            assert!(
                c.runtime.driving().is_some(),
                "a network is driving again: {:?}",
                c.runtime.driving()
            );
            let dir = CString::new(c.policies.to_string_lossy().into_owned()).unwrap();
            reload_policies(c, &dir.to_string_lossy(), ReloadKind::SlotChange, 0)
                .expect("the slot change is queued");
            assert_eq!(
                c.runtime.swap_state(),
                Some(PolicySwap::InPlace),
                "a non-driving slot's file change is in place: driving {:?}",
                c.runtime.driving()
            );
            wait_for_swap(core);

            // Sitstand driving means a rise, not the parked seat: Seated is
            // the one driving state a swap may go under
            // (`robotd/src/main.rs:2056-2058`).
            assert!(c.runtime.sit_toggle().is_ok(), "sit down");
            for _ in 0..160 {
                let _ = duck_core_tick(core);
            }
            assert!(c.runtime.is_sitting(), "the duck reached the seat");
            assert!(c.runtime.sit_toggle().is_ok(), "start the rise");
            let _ = duck_core_tick(core);
            assert!(
                matches!(c.runtime.driving(), Some(crate::control::Driving::SitStand)),
                "the rising sitstand network is driving: {:?}",
                c.runtime.driving()
            );
            assert!(
                !matches!(c.runtime.driving(), Some(crate::control::Driving::Seated)),
                "a seated duck is the one driving state no swap may disturb; this is a rise"
            );
            crate::manifest::set_slot_override("sitstand", Some("alpha_sitstand.onnx".to_owned()))
                .expect("sitstand override");
            let dir = CString::new(c.policies.to_string_lossy().into_owned()).unwrap();
            reload_policies(c, &dir.to_string_lossy(), ReloadKind::SlotChange, 0)
                .expect("the slot change is queued");
            assert_eq!(
                c.runtime.swap_state(),
                Some(PolicySwap::GoingHome),
                "changing the sitstand file while it drives goes home first"
            );
            wait_for_swap(core);

            // Skill driving compares the whole skill list; a changed file in
            // it disturbs (`robotd/src/main.rs:2069`).
            for _ in 0..200 {
                let _ = duck_core_tick(core);
                if matches!(c.runtime.bringup(), BringupState::Ready) {
                    break;
                }
            }
            // The rise is a travelling move of its own; let it finish before
            // asking for a one-shot.
            for _ in 0..400 {
                if !matches!(c.runtime.driving(), Some(crate::control::Driving::SitStand))
                    && !c.runtime.is_sitting()
                {
                    break;
                }
                let _ = duck_core_tick(core);
            }
            let started = c.runtime.trigger_skill("roulade");
            assert!(
                started.is_ok(),
                "the manifest skill starts: {started:?}, bringup {:?}, sitting {}",
                c.runtime.bringup(),
                c.runtime.is_sitting()
            );
            let _ = duck_core_tick(core);
            assert!(
                matches!(c.runtime.driving(), Some(crate::control::Driving::Skill(_))),
                "the skill is driving: {:?}",
                c.runtime.driving()
            );
            let dir = skill_file_change_dir(c.policies.as_path());
            reload_policies(c, &dir.to_string_lossy(), ReloadKind::SlotChange, 0)
                .expect("the slot change is queued");
            assert_eq!(
                c.runtime.swap_state(),
                Some(PolicySwap::GoingHome),
                "a changed skill file while a skill drives goes home first"
            );
            let _ = std::fs::remove_dir_all(&dir);
            // Leave the global override table as the test found it.
            crate::manifest::clear_slot_overrides();
        }
        unsafe { duck_core_free(core) };
    }

    /// A `GoingHome` reload's landing is the only place the new skill table can
    /// be published: the old set is still the one driving while the duck walks
    /// home. Nothing else observes `swap_pending` fall, so this pins the tick
    /// hook — `/skills` sees the landed set, and the degraded mark the queue
    /// carried is cleared.
    #[test]
    fn a_going_home_reload_publishes_its_skills_and_clears_degraded_when_it_lands() {
        let (_guard, core) = new_core();
        let dir = copy_policies_dir(unsafe { &(*core).policies }, &["roulade.onnx"]);
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            assert!(
                c.intents.skills_json().contains("roulade"),
                "the startup set ships roulade: {}",
                c.intents.skills_json()
            );
            c.runtime.mark_degraded("probe degraded".to_owned());
            assert!(matches!(c.runtime.health(), HealthVerdict::Degraded { .. }));

            let reload_dir = CString::new(dir.to_string_lossy().into_owned()).unwrap();
            assert_eq!(duck_core_reload_policies(core, reload_dir.as_ptr()), 0);
            assert_eq!(
                c.runtime.swap_state(),
                Some(PolicySwap::GoingHome),
                "the walking duck goes home first"
            );
            assert!(
                c.intents.skills_json().contains("roulade"),
                "the old set is still the one running on the way home"
            );

            let mut landed = false;
            for _ in 0..800 {
                duck_core_tick(core);
                if !c.runtime.swap_pending() {
                    landed = true;
                    break;
                }
            }
            assert!(landed, "the queued change landed");
            assert!(
                !c.intents.skills_json().contains("roulade"),
                "the landed set has no roulade, and /skills says so: {}",
                c.intents.skills_json()
            );
            assert!(
                matches!(c.runtime.health(), HealthVerdict::Healthy),
                "the landed change cleared the degraded mark: {:?}",
                c.runtime.health()
            );
            assert_eq!(c.label, "policy reloaded");
        }
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { duck_core_free(core) };
    }

    /// `robot.setSkill`/`robot.removeSkill`, end to end over the real socket:
    /// one call writes the table entry and reloads, the reloaded duck runs
    /// the skill under its new name, the vet book counts it, and the removal
    /// takes it back out. The official refusals — an unknown file, a
    /// daemon-driven name, a new skill without a duration — are answered at
    /// the door, and a remove of nothing is the official already-answer.
    #[test]
    fn robot_set_skill_hot_plugs_a_skill_end_to_end() {
        use std::io::{BufRead, BufReader};
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            // The day the duck lives by: the trigger below dates its unlock.
            c.intents.set_day("2026-11-19".to_owned());
            tick_until_ready(core);
            if c.ipc.is_none() {
                // Fixed port taken by another test process: the socket face
                // cannot be exercised here, and the core itself is fine.
                duck_core_free(core);
                return;
            }
        }
        let stream = TcpStream::connect(("127.0.0.1", crate::ipc::ROBOT_IPC_PORT))
            .expect("connect robot ipc");
        let mut reader = BufReader::new(stream.try_clone().expect("dup"));
        let mut stream = stream;

        fn call(
            stream: &mut TcpStream,
            reader: &mut BufReader<TcpStream>,
            id: u64,
            method: &str,
            params: &str,
        ) -> serde_json::Value {
            let line = format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"{method}\",\"params\":{params}}}\n"
            );
            stream.write_all(line.as_bytes()).expect("write");
            stream.flush().expect("flush");
            let mut reply = String::new();
            reader.read_line(&mut reply).expect("reply");
            serde_json::from_str(&reply).expect("json reply")
        }
        let result = |reply: &serde_json::Value| reply["result"].clone();
        let wait_for_swap = |core: *mut DuckCore| {
            for _ in 0..800 {
                unsafe { duck_core_tick(core) };
                if unsafe { (*core).runtime.swap_pending() } {
                    continue;
                }
                return;
            }
            panic!("the reload never landed");
        };

        unsafe {
            // roulade.onnx under a new name: the official one-shot with a new
            // identity. The answer is the official acceptance; the reload it
            // queued walks the driving duck home first and lands there.
            let reply = call(
                &mut stream,
                &mut reader,
                1,
                "robot.setSkill",
                r#"{"name":"spin_left","path":"roulade.onnx","duration":1.5}"#,
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            wait_for_swap(core);

            // `robot.skills` lists it with the parameters it was set with,
            // marked as a table override.
            let reply = call(&mut stream, &mut reader, 2, "robot.skills", "{}");
            let skills = result(&reply);
            let row = skills["skills"]
                .as_array()
                .and_then(|rows| rows.iter().find(|row| row["name"] == "spin_left"))
                .unwrap_or_else(|| panic!("no spin_left in {skills}"));
            assert_eq!(row["duration"], json!(1.5), "{skills}");
            assert!(
                row["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("roulade.onnx")),
                "{skills}"
            );
            assert_eq!(row["overridden"], json!(true), "{skills}");

            // /skills advertises it too, so a taught word and an MCP call can
            // reach the same skill.
            assert!(
                (*core).intents.skills_json().contains("spin_left"),
                "{}",
                (*core).intents.skills_json()
            );

            // The trigger runs the reloaded network, and the vet book counts
            // it under its own name, dated by the day the shell posted.
            let answer = apply_intent(&mut *core, Intent::Skill("spin_left".to_owned()));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            duck_core_tick(core);
            let book = active_runtime(&*core).book().clone();
            assert_eq!(book.life.skills.get("spin_left"), Some(&1));
            assert_eq!(
                book.life.unlocks.get("skill:spin_left").map(String::as_str),
                Some("2026-11-19")
            );

            // removeSkill takes it back out; the reloaded duck does not list
            // it and refuses it by name.
            let reply = call(
                &mut stream,
                &mut reader,
                3,
                "robot.removeSkill",
                r#"{"name":"spin_left"}"#,
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            wait_for_swap(core);
            let reply = call(&mut stream, &mut reader, 4, "robot.skills", "{}");
            let skills = result(&reply);
            let names: Vec<&str> = skills["skills"]
                .as_array()
                .expect("skill table")
                .iter()
                .filter_map(|skill| skill["name"].as_str())
                .collect();
            assert!(!names.contains(&"spin_left"), "{names:?}");
            let answer = apply_intent(&mut *core, Intent::Skill("spin_left".to_owned()));
            assert_eq!(answer["ok"], json!(false), "{answer}");

            // Removing a name the table does not carry: the official
            // already-answer, word for word.
            let reply = call(
                &mut stream,
                &mut reader,
                5,
                "robot.removeSkill",
                r#"{"name":"spin_left"}"#,
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            assert_eq!(
                result(&reply)["reason"],
                json!("no configured skill called \"spin_left\""),
                "{reply}"
            );

            // The door refusals, in the official words: a file that is not
            // there, a name the daemon drives, and a new skill with no
            // duration.
            let reply = call(
                &mut stream,
                &mut reader,
                6,
                "robot.setSkill",
                r#"{"name":"phantom","path":"gone.onnx","duration":1.0}"#,
            );
            assert_eq!(result(&reply)["accepted"], false, "{reply}");
            assert!(
                result(&reply)["reason"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("no such file"),
                "{reply}"
            );
            let reply = call(
                &mut stream,
                &mut reader,
                7,
                "robot.setSkill",
                r#"{"name":"ground_pick","duration":1.0}"#,
            );
            assert_eq!(result(&reply)["accepted"], false, "{reply}");
            assert!(
                result(&reply)["reason"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("driven by the robot itself"),
                "{reply}"
            );
            let reply = call(
                &mut stream,
                &mut reader,
                8,
                "robot.setSkill",
                r#"{"name":"lengthless","path":"roulade.onnx"}"#,
            );
            assert_eq!(result(&reply)["accepted"], false, "{reply}");
            assert!(
                result(&reply)["reason"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("needs a duration"),
                "{reply}"
            );

            // A name that could walk out of the policies directory is refused
            // at the door: the name is also the `<name>.onnx` file stem when
            // an entry names no file, so it obeys the bare-file-name rule.
            for bad in ["../../evil", "../evil", "sub/evil", "sub\\evil", "C:evil"] {
                let params = json!({ "name": bad, "duration": 1.0 }).to_string();
                let reply = call(&mut stream, &mut reader, 9, "robot.setSkill", &params);
                assert_eq!(result(&reply)["accepted"], false, "{bad}: {reply}");
                assert!(
                    result(&reply)["reason"]
                        .as_str()
                        .unwrap_or_default()
                        .contains("not a file name"),
                    "{bad}: {reply}"
                );
            }
            assert!(
                crate::manifest::custom_skills().is_empty(),
                "no escaping name was stored"
            );

            // `path: "none"` is the official switch-off: the same-named set
            // skill leaves the table and `robot.do` stops answering to it —
            // and removing the entry brings it back.
            let reply = call(
                &mut stream,
                &mut reader,
                10,
                "robot.setSkill",
                r#"{"name":"roulade","path":"none","duration":1.0}"#,
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            wait_for_swap(core);
            let reply = call(&mut stream, &mut reader, 11, "robot.skills", "{}");
            let skills = result(&reply);
            let names: Vec<&str> = skills["skills"]
                .as_array()
                .expect("skill table")
                .iter()
                .filter_map(|skill| skill["name"].as_str())
                .collect();
            assert!(
                !names.contains(&"roulade"),
                "the switched-off set skill is gone: {names:?}"
            );
            assert!(
                !(*core).intents.skills_json().contains("roulade"),
                "{}",
                (*core).intents.skills_json()
            );
            let answer = apply_intent(&mut *core, Intent::Skill("roulade".to_owned()));
            assert_eq!(answer["ok"], json!(false), "{answer}");

            let reply = call(
                &mut stream,
                &mut reader,
                12,
                "robot.removeSkill",
                r#"{"name":"roulade"}"#,
            );
            assert_eq!(result(&reply)["accepted"], true, "{reply}");
            wait_for_swap(core);
            let reply = call(&mut stream, &mut reader, 13, "robot.skills", "{}");
            let skills = result(&reply);
            let names: Vec<&str> = skills["skills"]
                .as_array()
                .expect("skill table")
                .iter()
                .filter_map(|skill| skill["name"].as_str())
                .collect();
            assert!(
                names.contains(&"roulade"),
                "removing the switch-off restores the set's skill: {names:?}"
            );
            assert!(crate::manifest::custom_skills().is_empty(), "nothing stuck");
        }
        unsafe { duck_core_free(core) };
    }

    /// The person's own skill through the config face: a table entry rides
    /// the next reload, `/skills` advertises it, the trigger runs it, and the
    /// vet book counts it under its own name.
    #[test]
    fn a_custom_skill_from_the_config_face_rides_the_reload() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            c.intents.set_day("2026-11-19".to_owned());
            tick_until_ready(core);
            crate::manifest::set_custom_skills(vec![json!({
                "file": "roulade.onnx",
                "name": "spin_left",
                "kind": "episodic",
                "duration_s": 1.5,
            })])
            .expect("the entry is a skill");

            let dir = CString::new(c.policies.to_string_lossy().into_owned()).unwrap();
            assert_eq!(duck_core_reload_policies(core, dir.as_ptr()), 0);
            for _ in 0..800 {
                duck_core_tick(core);
                if !c.runtime.swap_pending() {
                    break;
                }
            }
            assert!(!c.runtime.swap_pending(), "the reload landed");
            assert!(
                c.intents.skills_json().contains("spin_left"),
                "{}",
                c.intents.skills_json()
            );

            let answer = apply_intent(&mut *core, Intent::Skill("spin_left".to_owned()));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            duck_core_tick(core);
            let book = active_runtime(&*core).book().clone();
            assert_eq!(book.life.skills.get("spin_left"), Some(&1));
            assert_eq!(
                book.life.unlocks.get("skill:spin_left").map(String::as_str),
                Some("2026-11-19")
            );
            crate::manifest::clear_custom_skills();
        }
        unsafe { duck_core_free(core) };
    }

    /// The parent's day board, end to end: the shell posts its calendar day,
    /// the duck really counts a day's play, the next day's tick closes the
    /// row — and `/days` serves the row, today's fresh deltas and the week,
    /// exactly what `roll_day` wrote.
    #[test]
    fn the_parent_day_board_serves_what_the_days_really_did() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            // The first tick with a day starts the baseline; nothing is
            // claimed by a day that had not begun.
            c.intents.set_day("2026-11-19".to_owned());
            assert_eq!(duck_core_tick(core), 0);
            let before: serde_json::Value =
                serde_json::from_str(&c.intents.days_json()).expect("days JSON");
            assert!(
                before["days"].as_array().expect("rows").is_empty(),
                "{before}"
            );
            assert!(
                before.get("today").is_some(),
                "the day is running: {before}"
            );

            // The day's play: three hands on the duck.
            for _ in 0..3 {
                apply_intent(&mut *core, Intent::Pet);
            }
            assert_eq!(duck_core_tick(core), 0);

            // The next calendar day: the rollover snapshots 11-19's row.
            c.intents.set_day("2026-11-20".to_owned());
            assert_eq!(duck_core_tick(core), 0);
            let report: serde_json::Value =
                serde_json::from_str(&c.intents.days_json()).expect("days JSON");
            assert_eq!(report["ok"], json!(true));
            let rows = report["days"].as_array().expect("rows");
            assert_eq!(rows.len(), 1, "{report}");
            assert_eq!(rows[0]["day"], json!("2026-11-19"));
            assert_eq!(rows[0]["pets"], json!(3), "{report}");

            let today = &report["today"];
            assert_eq!(today["day"], json!("2026-11-20"));
            assert_eq!(
                today["pets"],
                json!(0),
                "the fresh day claims nothing: {report}"
            );
            assert_eq!(
                report["week"]["pets"],
                json!(3),
                "the row plus today: {report}"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// The cold-start order QA found: the shell posts its day (and the tick
    /// rolls a near-zero baseline) *before* the saved life arrives at the end
    /// of the tick. The restore must not let `restore_days` read the lifetime
    /// as today's play, in either arrival order — and the save/restore cycle
    /// must keep the completed rows and the same-day baseline.
    #[test]
    fn the_restored_lifetime_is_never_counted_as_today() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            let historical = json!({
                "powered_s": 5000.0, "metres": 800.0, "torqued_s": 4000.0,
                "pets": 100, "fed": 40, "claps": 12, "whistles": 3,
                "pickups": 9, "falls": 2, "ground_picks": 7, "sit_downs": 5,
                "skills": { "roulade": 12 },
            });

            // Order A: the day lands first, the restore afterwards — the
            // order that used to count the whole lifetime as today.
            c.intents.set_day("2026-11-19".to_owned());
            assert_eq!(duck_core_tick(core), 0);
            c.intents.push(Intent::RestoreLife(historical.clone()));
            assert_eq!(duck_core_tick(core), 0);
            let report: serde_json::Value =
                serde_json::from_str(&c.intents.days_json()).expect("days JSON");
            assert_eq!(report["today"]["day"], json!("2026-11-19"), "{report}");
            assert_eq!(
                report["today"]["pets"],
                json!(0),
                "the restored lifetime is not today's play: {report}"
            );
            assert_eq!(report["today"]["metres"], json!(0.0), "{report}");
            assert_eq!(report["today"]["rolls"], json!(0), "{report}");
            assert!(
                report["days"].as_array().expect("rows").is_empty(),
                "{report}"
            );

            // Order B: the restore lands before any day is posted; the day
            // that arrives next starts from the life at that moment.
            c.intents.set_day(String::new());
            c.intents.push(Intent::RestoreLife(json!({ "pets": 200 })));
            assert_eq!(duck_core_tick(core), 0);
            c.intents.set_day("2026-11-21".to_owned());
            assert_eq!(duck_core_tick(core), 0);
            let report: serde_json::Value =
                serde_json::from_str(&c.intents.days_json()).expect("days JSON");
            assert_eq!(report["today"]["day"], json!("2026-11-21"), "{report}");
            assert_eq!(report["today"]["pets"], json!(0), "{report}");

            // A real save/restore cycle on the running day: today's play is
            // part of the saved life *and* its saved baseline, so handing the
            // summary back keeps today's delta exactly right.
            for _ in 0..3 {
                apply_intent(&mut *core, Intent::Pet);
            }
            assert_eq!(duck_core_tick(core), 0);
            let saved: serde_json::Value =
                serde_json::from_str(&c.intents.life_json()).expect("life JSON");
            assert_eq!(
                saved["day_started"]["day"],
                json!("2026-11-21"),
                "the summary publishes the baseline: {saved}"
            );
            c.intents.push(Intent::RestoreLife(saved.clone()));
            assert_eq!(duck_core_tick(core), 0);
            let report: serde_json::Value =
                serde_json::from_str(&c.intents.days_json()).expect("days JSON");
            assert_eq!(
                report["today"]["pets"],
                json!(3),
                "the same-day baseline keeps today's real play: {report}"
            );

            // The day closes, then the save of that state is handed back the
            // way a relaunch would: the row and the baseline survive, and
            // nothing is double counted.
            c.intents.set_day("2026-11-22".to_owned());
            assert_eq!(duck_core_tick(core), 0);
            let saved: serde_json::Value =
                serde_json::from_str(&c.intents.life_json()).expect("life JSON");
            assert_eq!(saved["days"].as_array().expect("rows").len(), 1);
            c.intents.push(Intent::RestoreLife(saved.clone()));
            assert_eq!(duck_core_tick(core), 0);
            let report: serde_json::Value =
                serde_json::from_str(&c.intents.days_json()).expect("days JSON");
            let rows = report["days"].as_array().expect("rows");
            assert_eq!(rows.len(), 1, "the completed day survived: {report}");
            assert_eq!(rows[0]["day"], json!("2026-11-21"));
            assert_eq!(rows[0]["pets"], json!(3), "{report}");
            assert_eq!(
                report["today"]["pets"],
                json!(0),
                "the restored baseline makes today start from zero: {report}"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// `robot.setSkill` puts a file through the same ONNX gate
    /// `robot.loadPolicy`/the official daemon do: a non-policy file is refused
    /// while the caller is listening, the table is not polluted, and the duck
    /// still reloads a good set afterwards.
    #[test]
    fn set_skill_refuses_a_file_that_is_not_a_policy() {
        use std::io::{BufRead, BufReader};
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            if c.ipc.is_none() {
                duck_core_free(core);
                return;
            }
            // The duck runs from a copy of the set, so the bogus file is not
            // written into the shared fixtures.
            let dir = copy_policies_dir(&c.policies, &[]);
            std::fs::write(dir.join("broken.onnx"), b"this is not an onnx file")
                .expect("a file that will not open");
            let dir_c = CString::new(dir.to_string_lossy().into_owned()).unwrap();
            assert_eq!(duck_core_reload_policies(core, dir_c.as_ptr()), 0);
            for _ in 0..800 {
                duck_core_tick(core);
                if !c.runtime.swap_pending() {
                    break;
                }
            }
            assert_eq!(c.policies, dir, "the duck now runs the copy");
        }
        let stream = TcpStream::connect(("127.0.0.1", crate::ipc::ROBOT_IPC_PORT))
            .expect("connect robot ipc");
        let mut reader = BufReader::new(stream.try_clone().expect("dup"));
        let mut stream = stream;
        let line = format!(
            "{}\n",
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "robot.setSkill",
                "params": { "name": "broken", "path": "broken.onnx", "duration": 1.0 }
            })
        );
        stream.write_all(line.as_bytes()).expect("write");
        stream.flush().expect("flush");
        let mut reply = String::new();
        reader.read_line(&mut reply).expect("reply");
        let reply: serde_json::Value = serde_json::from_str(&reply).expect("json reply");
        assert_eq!(reply["result"]["accepted"], json!(false), "{reply}");
        assert!(
            reply["result"]["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("broken.onnx"),
            "the refusal names the file: {reply}"
        );
        assert!(
            crate::manifest::custom_skills().is_empty(),
            "the refused skill did not enter the table"
        );
        unsafe {
            let c = &mut *core;
            // The duck is unharmed: a reload still loads the set.
            let dir_c = CString::new(c.policies.to_string_lossy().into_owned()).unwrap();
            assert_eq!(duck_core_reload_policies(core, dir_c.as_ptr()), 0);
            for _ in 0..800 {
                duck_core_tick(core);
                if !c.runtime.swap_pending() {
                    break;
                }
            }
            assert!(
                c.intents.skills_json().contains("roulade"),
                "the good set still runs: {}",
                c.intents.skills_json()
            );
            let _ = std::fs::remove_dir_all(&c.policies);
        }
        unsafe { duck_core_free(core) };
    }

    /// The official vet book's label edge: the tick `sit` becomes the driving
    /// label is one sit-down — not one per tick of the seat — and the day's
    /// row carries the count (no longer a hard-wired zero).
    #[test]
    fn sitting_counts_one_sit_down_and_the_day_row_carries_it() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            c.intents.set_day("2026-11-19".to_owned());
            tick_until_ready(core);
            let answer = apply_intent(&mut *core, Intent::Sit);
            assert_eq!(answer["ok"], json!(true), "{answer}");
            for _ in 0..400 {
                duck_core_tick(core);
                if active_runtime(&*core).book().life.sit_downs > 0 {
                    break;
                }
            }
            assert_eq!(
                active_runtime(&*core).book().life.sit_downs,
                1,
                "one sit is one sit-down"
            );
            // Still in the seat: the label stays "sit" and does not count
            // again.
            for _ in 0..100 {
                duck_core_tick(core);
            }
            assert_eq!(
                active_runtime(&*core).book().life.sit_downs,
                1,
                "a held seat is not a new sit"
            );

            c.intents.set_day("2026-11-20".to_owned());
            assert_eq!(duck_core_tick(core), 0);
            let report: serde_json::Value =
                serde_json::from_str(&c.intents.days_json()).expect("days JSON");
            let rows = report["days"].as_array().expect("rows");
            assert_eq!(rows.len(), 1, "{report}");
            assert_eq!(
                rows[0]["sit_downs"],
                json!(1),
                "the day's row counts the sit: {report}"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// A duck left in the rest pose is Limp and disabled. Skills, sit and
    /// ground pick must be refused there (official loop gate,
    /// `robotd/src/main.rs:2768-2775`), and a mode switch must leave the new
    /// bundle limp rather than enabling it (`:2930-2960`).
    #[test]
    fn a_resting_duck_refuses_moves_and_stays_limp_across_a_mode_switch() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            c.runtime.request_rest();
            for _ in 0..400 {
                let _ = duck_core_tick(core);
            }
            assert!(!c.runtime.powered_off(), "the rest stays powered");
            assert_eq!(c.runtime.bringup(), BringupState::Limp);
            assert!(!c.runtime.checkpoint().unwrap().enabled);

            for (label, intent) in [
                ("skill", Intent::Skill("roulade".to_owned())),
                ("sit", Intent::Sit),
                ("ground_pick", Intent::GroundPick),
            ] {
                let answer = apply_intent(c, intent);
                assert_eq!(answer["ok"], false, "{label}: {answer}");
                assert!(
                    answer["error"]
                        .as_str()
                        .unwrap_or_default()
                        .contains("not driving"),
                    "{label}: {answer}"
                );
            }

            // The pad's discrete Skill arm has the same gate.
            assert!(c.pad_mapper.bindings.bind("a", "sit_toggle"));
            let refusal = dispatch_pad_event(c, &crate::pad::PadButtonEvent::Skill("a"))
                .expect("a pad skill on a resting duck is refused");
            assert!(refusal.contains("not driving"), "{refusal}");

            // `robot.do` refuses at the door for the same reason.
            let handlers = RobotHandlers {
                intents: c.intents.clone(),
                health: c.health_snapshot.clone(),
                revision: String::new(),
                snapshot: c.robot_snapshot.clone(),
                control: c.control.clone(),
            };
            let verdict = crate::ipc::RobotHandlers::do_skill(
                &handlers,
                duck_ipc_proto::DoParams {
                    skill: "roulade".to_owned(),
                },
            );
            assert!(!verdict.accepted, "{verdict:?}");
            assert!(
                verdict.reason.unwrap_or_default().contains("not driving"),
                "the official refusal wording"
            );

            // `/posture` refuses a real change for the same reason, while an
            // already-true posture stays an inert acceptance.
            let answer = apply_intent(c, Intent::Posture(true));
            assert_eq!(answer["ok"], false, "{answer}");
            assert!(
                answer["error"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("not driving")
            );
            let answer = apply_intent(c, Intent::Posture(false));
            assert_eq!(answer["ok"], true, "already standing: {answer}");

            // `/heard` routes a moving verb through the same gate; a sound
            // (which moves nothing) still plays.
            let answer = apply_intent(c, Intent::Heard("sit".to_owned()));
            assert_eq!(answer["ok"], false, "{answer}");
            assert!(
                answer["error"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("not driving")
            );
            let answer = apply_intent(c, Intent::Heard("hello".to_owned()));
            assert_eq!(answer["ok"], true, "a greeting is not a move: {answer}");

            // The C ABI entries the shell's ring and big buttons call.
            let skill = CString::new("roulade").unwrap();
            assert_eq!(unsafe { duck_core_trigger_skill(core, skill.as_ptr()) }, 0);
            assert_eq!(unsafe { duck_core_sit_toggle(core) }, -1);
            assert_eq!(unsafe { duck_core_ground_pick(core) }, -1);

            // The mode switch loads the roller bundle and does not stand the
            // resting duck up.
            let answer = switch_mode(c, "roller").expect("a limp mode switch is allowed");
            assert_eq!(answer["mode"], "roller");
            assert_eq!(c.runtime.mode(), robotd_params::Mode::Roller);
            assert_eq!(
                c.runtime.bringup(),
                BringupState::Limp,
                "the fresh runtime stays limp"
            );
            assert!(
                !c.runtime.checkpoint().unwrap().enabled,
                "and its policy is not enabled"
            );
            assert!(!c.runtime.powered_off());
        }
        unsafe { duck_core_free(core) };
    }

    /// Enable-off on a seated duck holds the seat. The mobile runtime commands
    /// its constant [`crate::SEAT_POSITION`]; the official daemon holds the
    /// measured `known_positions` at the stop (`robotd/src/main.rs:3772-3776`).
    /// The difference is the runtime's, not something this test can change —
    /// what this pins is the part both agree on: the seat survives the stop.
    #[test]
    fn enable_off_on_a_seated_duck_holds_the_seat() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            assert!(c.runtime.sit_toggle().is_ok(), "sit down");
            for _ in 0..160 {
                let _ = duck_core_tick(core);
            }
            assert!(c.runtime.is_sitting(), "the duck reached the seat");

            c.runtime.set_policy_enabled(false);
            let _ = duck_core_tick(core);
            assert!(
                c.runtime.is_sitting(),
                "enable off keeps the seat (the disable that comes with a stop holds it)"
            );
            assert_eq!(
                c.runtime.bringup(),
                BringupState::Ready,
                "stopping the policy is not a bring-up change"
            );
            assert!(!c.runtime.checkpoint().unwrap().enabled);
            assert!(!c.runtime.powered_off());
        }
        unsafe { duck_core_free(core) };
    }

    /// A reload queued in a branch session belongs to that branch. The owner
    /// index is checked before judging the falling edge, so the original's
    /// ticks cannot consume branch B's marker and B's landing publishes B's
    /// skill table — never the other duck's.
    #[test]
    fn a_branch_reload_lands_on_its_owner_and_not_the_other_duck() {
        let (_guard, core) = new_core();
        let dir = copy_policies_dir(unsafe { &(*core).policies }, &["roulade.onnx"]);
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            let original_walk = c
                .runtime
                .driving_net_paths()
                .into_iter()
                .find(|(slot, _)| slot == "walk")
                .and_then(|(_, path)| path);
            let label = CString::new("b-owner").unwrap();
            assert_eq!(duck_core_fork(core, label.as_ptr()), 0);
            let answer = apply_intent(c, Intent::SelectBranch("b-owner".to_owned()));
            assert_eq!(answer["selected"], "b-owner");

            // The HTTP reload path: `apply_intent` swaps the selected branch
            // in, so the owner recorded is B.
            let answer = apply_intent(c, Intent::Reload(dir.to_string_lossy().into_owned()));
            assert_eq!(answer["ok"], true, "{answer}");
            assert_eq!(
                c.swap_landing.as_ref().map(|(owner, _)| *owner),
                Some(1),
                "the marker is owned by the selected branch"
            );

            // Select the original before B's change lands. A's ticks must
            // leave B's marker alone.
            apply_intent(c, Intent::SelectBranch(String::new()));
            for _ in 0..400 {
                let _ = duck_core_tick(core);
            }
            assert_eq!(
                c.swap_landing.as_ref().map(|(owner, _)| *owner),
                Some(1),
                "the original's ticks did not consume the branch's marker"
            );
            let walk_a = c
                .runtime
                .driving_net_paths()
                .into_iter()
                .find(|(slot, _)| slot == "walk")
                .and_then(|(_, path)| path);
            assert_eq!(walk_a, original_walk, "the original never swapped");
            assert!(
                c.intents.skills_json().contains("roulade"),
                "and it still publishes its own table: {}",
                c.intents.skills_json()
            );

            // Select B; its next tick settles its own marker and publishes
            // the landed set.
            apply_intent(c, Intent::SelectBranch("b-owner".to_owned()));
            duck_core_tick(core);
            assert!(
                c.swap_landing.is_none(),
                "B consumed its own landing marker"
            );
            assert!(
                !c.intents.skills_json().contains("roulade"),
                "B's landed set is the published table: {}",
                c.intents.skills_json()
            );
            let walk_b = active_runtime(c)
                .driving_net_paths()
                .into_iter()
                .find(|(slot, _)| slot == "walk")
                .and_then(|(_, path)| path)
                .expect("branch walk");
            assert!(
                walk_b.starts_with(&dir),
                "B runs the reloaded set: {}",
                walk_b.display()
            );
            {
                let snapshot = c.robot_snapshot.lock().unwrap();
                assert!(
                    snapshot
                        .skills
                        .skills
                        .iter()
                        .all(|skill| skill.name != "roulade"),
                    "robot.skills describes the active duck: {:?}",
                    snapshot.skills.skills
                );
            }

            // Back on the original, its table and robot.skills return.
            apply_intent(c, Intent::SelectBranch(String::new()));
            duck_core_tick(core);
            {
                let snapshot = c.robot_snapshot.lock().unwrap();
                assert!(
                    snapshot
                        .skills
                        .skills
                        .iter()
                        .any(|skill| skill.name == "roulade"),
                    "robot.skills follows the selection back to the original: {:?}",
                    snapshot.skills.skills
                );
            }
            assert!(c.intents.skills_json().contains("roulade"));

            // Dropping a branch whose marker is queued clears it; branch
            // indices shifted, so an index-keyed marker cannot survive.
            apply_intent(c, Intent::SelectBranch("b-owner".to_owned()));
            let answer = apply_intent(c, Intent::Reload(dir.to_string_lossy().into_owned()));
            assert_eq!(answer["ok"], true, "{answer}");
            assert!(c.swap_landing.is_some());
            let answer = apply_intent(c, Intent::DropBranch("b-owner".to_owned()));
            assert_eq!(answer["ok"], true, "{answer}");
            assert_eq!(c.selected, 0);
            assert!(c.swap_landing.is_none(), "a dropped owner's marker goes");
            duck_core_tick(core);
            assert!(
                c.intents.skills_json().contains("roulade"),
                "the table falls back to the original: {}",
                c.intents.skills_json()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { duck_core_free(core) };
    }

    /// A mode switch is refused while a policy change is still on its way home
    /// (`robotd/src/main.rs:2906-2919`), rather than landing one change on top
    /// of another.
    #[test]
    fn a_mode_switch_waits_for_a_queued_policy_change() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            let dir = CString::new(c.policies.to_string_lossy().into_owned()).unwrap();
            assert_eq!(duck_core_reload_policies(core, dir.as_ptr()), 0);
            assert!(c.runtime.swap_pending());
            let error = switch_mode(c, "roller").expect_err("the switch waits");
            assert!(error.contains("policy change"), "{error}");
            assert!(
                c.runtime.swap_pending(),
                "and the queued change is still the one in flight"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// A queued change can land inside `duck_core_step` (the time machine's
    /// own loop, which does not call `duck_core_tick`). The step door runs the
    /// same settling hook, so the landed set is published there too.
    #[test]
    fn a_policy_change_landing_inside_step_is_published() {
        let (_guard, core) = new_core();
        let dir = copy_policies_dir(unsafe { &(*core).policies }, &["roulade.onnx"]);
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            let reload = CString::new(dir.to_string_lossy().into_owned()).unwrap();
            assert_eq!(duck_core_reload_policies(core, reload.as_ptr()), 0);
            assert_eq!(c.runtime.swap_state(), Some(PolicySwap::GoingHome));
            assert!(c.intents.skills_json().contains("roulade"));
            duck_core_step(core, 500);
            assert!(
                !c.runtime.swap_pending(),
                "the change landed during the step"
            );
            assert!(
                !c.intents.skills_json().contains("roulade"),
                "the step published the landed table: {}",
                c.intents.skills_json()
            );
            assert_eq!(c.label, "policy reloaded");
        }
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { duck_core_free(core) };
    }

    /// A rewind drops a pending swap without landing it. The marker goes with
    /// it, and the paused early return in the tick does not skip that cleanup.
    #[test]
    fn a_seek_drops_a_pending_landing_marker_even_while_paused() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            tick_until_ready(core);
            // Queue a disturbing change, then freeze the duck.
            let dir = CString::new(c.policies.to_string_lossy().into_owned()).unwrap();
            assert_eq!(duck_core_reload_policies(core, dir.as_ptr()), 0);
            assert!(c.runtime.swap_pending());
            c.intents.push(Intent::Pause(true));
            let _ = duck_core_tick(core);
            assert!(c.runtime.clock().paused, "the duck is frozen");
            assert!(
                c.runtime.swap_pending(),
                "a frozen duck does not land the change"
            );
            // Seek restores the active runtime, which drops the pending swap.
            assert_eq!(unsafe { duck_core_seek(core, 1) }, 0);
            assert!(c.swap_landing.is_none(), "the seek cleared the marker");
        }
        unsafe { duck_core_free(core) };
    }

    /// SENSORS S2 wiring: the `/floor` route's intent reaches the MuJoCo plane.
    /// The dimension is the scene-relative offset pinned in `POST /floor`:
    /// the plane moves to the default floor + offset, the duck moves with it,
    /// and the duck is still standing afterwards — not buried in, or dropped
    /// through, its own floor.
    #[test]
    fn the_floor_offset_moves_the_plane_and_the_duck_stays_on_it() {
        const TRUNK: usize = 2;
        let (_guard, core) = new_core();
        tick_until_ready(core);
        let plane_z = |core: *mut DuckCore| -> Option<f64> {
            let io = unsafe { (*core).runtime.io() };
            (0..io.geom_count())
                .filter_map(|geom| io.geom_info(geom))
                .find(|info| info.geom_type == 0)
                .map(|info| info.pos[2])
        };
        let status = |core: *mut DuckCore| {
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
            status
        };
        assert!(plane_z(core).expect("the scene has a ground plane").abs() < 1e-9);
        let trunk_before = status(core).trunk[TRUNK];

        // +5 cm, inside the route's -0.1..=0.3 band.
        let answer = unsafe { apply_intent(&mut *core, Intent::FloorHeight(0.05)) };
        assert_eq!(answer["ok"], json!(true), "{answer}");
        assert_eq!(answer["floor_offset_m"], json!(0.05));
        let plane = plane_z(core).expect("the plane is still there");
        assert!((plane - 0.05).abs() < 1e-9, "plane z is {plane}");
        let trunk = status(core).trunk[TRUNK];
        assert!(
            (trunk - (trunk_before + 0.05)).abs() < 1e-6,
            "the duck moves with its floor: {trunk_before} -> {trunk}"
        );

        // Let the gait absorb the step: it must still be up, above the
        // plane it stands on.
        for _ in 0..100 {
            assert_eq!(unsafe { duck_core_tick(core) }, 0);
        }
        let settled = status(core);
        assert_eq!(settled.fallen, 0, "the floor move must not trip the duck");
        assert_eq!(settled.bringup, 2, "the duck is still Ready");
        assert!(
            settled.trunk[TRUNK] > plane + 0.04,
            "the trunk must stay above its floor: {} vs {plane}",
            settled.trunk[TRUNK]
        );

        // Back down past the default: the same rule in the other direction.
        // A 5 cm step down is a real one for the gait — the settled trunk
        // sits lower and the fall verdict is not the assertion here, only
        // that the plane and the duck moved together and the duck is still
        // standing on its floor.
        let answer = unsafe { apply_intent(&mut *core, Intent::FloorHeight(-0.05)) };
        assert_eq!(answer["ok"], json!(true), "{answer}");
        let lowered = plane_z(core).expect("plane");
        assert!(
            (lowered + 0.05).abs() < 1e-9,
            "the plane is 0.05 below the default floor, got {lowered}"
        );
        for _ in 0..100 {
            assert_eq!(unsafe { duck_core_tick(core) }, 0);
        }
        let settled = status(core);
        assert!(
            settled.trunk[TRUNK] > lowered,
            "the duck is still above its lowered floor: {} vs {lowered}",
            settled.trunk[TRUNK]
        );
        unsafe { duck_core_free(core) };
    }

    /// `/sensor` microphone PCM: decoded into the detector, stripped from the
    /// registry so `/sensors` never echoes audio, and `/state.pet_detect`
    /// always shaped even when no stroke is in progress.
    #[test]
    fn a_pcm_post_never_echoes_audio_and_publishes_the_verdict() {
        let (_guard, core) = new_core();
        tick_until_ready(core);
        // Attach the official model when the upstream checkout has it, so
        // the post travels the real detector path. Without it the post must
        // still be accepted and stripped — the feature is off, not broken.
        let model = crate::fixtures::workspace_root()
            .join("upstream/microduck/pet-detect/models/pet_detect.onnx");
        if model.is_file() {
            unsafe {
                (*core)
                    .runtime
                    .load_pet_detector(&model)
                    .expect("the official pet model loads")
            };
        } else if crate::fixtures::required() {
            panic!("the official pet model is missing at {}", model.display());
        }

        // 3 200 zero bytes: 1 600 i16 samples, base64 is 1 066 groups of
        // "AAAA" plus the two-byte tail "AAA=".
        let pcm = format!("{}{}", "AAAA".repeat(1_066), "AAA=");
        let reading = crate::sensors::Reading {
            sensor: "microphone".to_owned(),
            mode: "twin".to_owned(),
            value: json!({ "rms": 0.01, "pcm16": pcm, "rate": 16000 }),
            tick_ms: 0,
            in_observation: false,
        };
        let answer = unsafe { apply_intent(&mut *core, Intent::Sensor(reading)) };
        assert_eq!(answer["ok"], json!(true), "{answer}");
        assert_eq!(unsafe { duck_core_tick(core) }, 0);

        let sensors = unsafe { (*core).intents.sensors_json() };
        assert!(
            !sensors.contains("pcm16"),
            "the registry must not echo the audio: {sensors}"
        );
        assert!(sensors.contains("\"rms\":0.01"), "{sensors}");
        let state: serde_json::Value =
            serde_json::from_str(&unsafe { (*core).intents.state_json() }).expect("state");
        assert_eq!(
            state["pet_detect"],
            json!({ "petting": false, "since_tick": null }),
            "{state}"
        );
        unsafe { duck_core_free(core) };
    }

    /// The microphone PCM must reach the duck the client is watching. Every
    /// dispatch arm used to speak `core.runtime`; a branch session swaps the
    /// selected branch in, so an arm that swaps again (`on_active`) feeds the
    /// original instead. The same PCM posted twice — once live, once while
    /// frozen — proves both which duck hears it and that the pause gate holds.
    #[test]
    fn a_pcm_post_reaches_the_selected_branch_and_its_pause_gate() {
        let (_guard, core) = new_core();
        let model = crate::fixtures::workspace_root()
            .join("upstream/microduck/pet-detect/models/pet_detect.onnx");
        if !model.is_file() {
            if crate::fixtures::required() {
                panic!("the official pet model is missing at {}", model.display());
            }
            eprintln!(
                "skipping: the official pet model is not here at {}",
                model.display()
            );
            return;
        }
        unsafe {
            let c = &mut *core;
            let answer = apply_intent(c, Intent::Fork("mic-branch".to_owned()));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            let answer = apply_intent(c, Intent::SelectBranch("mic-branch".to_owned()));
            assert_eq!(answer["selected"], json!("mic-branch"), "{answer}");

            // Inverted thresholds make a silent window a stroke: the test is
            // about which runtime hears the post, not what the CNN thinks of
            // digital silence. Both ducks carry the same detector, so either
            // one feeding itself is visible.
            let config = || crate::petdetect::DetectorConfig {
                stride: crate::petdetect::DEFAULT_STRIDE,
                enter_threshold: -1.0,
                exit_threshold: 2.0,
            };
            c.runtime
                .load_pet_detector_with_config(&model, config())
                .expect("the official pet model loads");
            c.branches[0]
                .runtime
                .load_pet_detector_with_config(&model, config())
                .expect("the official pet model loads on the branch");

            // Exactly one full 16 240-sample window: 32 480 zero bytes.
            let pcm = format!("{}{}", "AAAA".repeat(10_826), "AAA=");
            let reading = || crate::sensors::Reading {
                sensor: "microphone".to_owned(),
                mode: "twin".to_owned(),
                value: json!({ "rms": 0.01, "pcm16": pcm, "rate": 16000 }),
                tick_ms: 0,
                in_observation: false,
            };
            let answer = apply_intent(c, Intent::Sensor(reading()));
            assert_eq!(answer["ok"], json!(true), "{answer}");

            let branch = &c.branches[0].runtime;
            assert_eq!(
                branch.book().life.sound_pets,
                1,
                "the selected branch heard the post"
            );
            assert_eq!(
                branch.pet_detect_summary(),
                json!({ "petting": true, "since_tick": 0 })
            );
            assert_eq!(
                c.runtime.book().life.sound_pets,
                0,
                "the original runtime heard nothing"
            );
            assert_eq!(
                c.runtime.pet_detect_summary(),
                json!({ "petting": false, "since_tick": null }),
                "and its verdict did not change"
            );

            // `/state.pet_detect` publishes the selected duck's verdict.
            assert_eq!(duck_core_tick(core), 0);
            let state: serde_json::Value =
                serde_json::from_str(&c.intents.state_json()).expect("state");
            assert_eq!(
                state["pet_detect"],
                json!({ "petting": true, "since_tick": 0 }),
                "{state}"
            );

            // Frozen time hears nothing, on the selected branch too.
            let answer = apply_intent(c, Intent::Pause(true));
            assert_eq!(answer["paused"], json!(true), "{answer}");
            assert!(c.branches[0].runtime.clock().paused);
            let answer = apply_intent(c, Intent::Sensor(reading()));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            assert_eq!(
                c.branches[0].runtime.book().life.sound_pets,
                1,
                "a paused branch hears nothing"
            );
            assert_eq!(c.runtime.book().life.sound_pets, 0);
        }
        unsafe { duck_core_free(core) };
    }

    /// The floor is the branch's own world: moving it in a branch session
    /// must not move the original's ground (the arm used to swap twice and
    /// land on the original).
    #[test]
    fn a_floor_offset_lands_on_the_selected_branch() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            let plane = |io: &crate::mujoco::MujocoIo| -> Option<f64> {
                (0..io.geom_count())
                    .filter_map(|geom| io.geom_info(geom))
                    .find(|info| info.geom_type == 0)
                    .map(|info| info.pos[2])
            };
            let answer = apply_intent(c, Intent::Fork("floor-branch".to_owned()));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            let answer = apply_intent(c, Intent::SelectBranch("floor-branch".to_owned()));
            assert_eq!(answer["selected"], json!("floor-branch"), "{answer}");
            let original_before = plane(c.runtime.io()).expect("the original floor");
            let branch_before = plane(c.branches[0].runtime.io()).expect("the branch floor");

            let answer = apply_intent(c, Intent::FloorHeight(0.05));
            assert_eq!(answer["ok"], json!(true), "{answer}");

            let branch_after = plane(c.branches[0].runtime.io()).expect("the branch floor");
            let original_after = plane(c.runtime.io()).expect("the original floor");
            assert!(
                (branch_after - (branch_before + 0.05)).abs() < 1e-9,
                "the selected branch's floor moved: {branch_before} -> {branch_after}"
            );
            assert!(
                (original_after - original_before).abs() < 1e-9,
                "the original's floor did not move: {original_before} -> {original_after}"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// The allowance is care state, and care is per runtime: setting it in a
    /// branch session must land on the branch and answer from the same duck.
    #[test]
    fn a_parent_budget_lands_on_the_selected_branch() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            let answer = apply_intent(c, Intent::Fork("budget-branch".to_owned()));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            let answer = apply_intent(c, Intent::SelectBranch("budget-branch".to_owned()));
            assert_eq!(answer["selected"], json!("budget-branch"), "{answer}");

            let answer = apply_intent(c, Intent::ParentBudget(30));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            assert_eq!(answer["parent"]["minutes"], json!(30), "{answer}");
            assert_eq!(
                c.branches[0].runtime.care_summary()["parent"]["minutes"],
                json!(30),
                "the branch owns the allowance"
            );
            assert!(
                c.runtime.care_summary()["parent"]
                    .get("minutes")
                    .is_none(),
                "the original keeps no allowance"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// `sound_pets` is a lifetime counter like every other field in the
    /// summary: a restart must hand it back, not zero it.
    #[test]
    fn a_restored_life_keeps_the_sound_pet_count() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            // A save from before the field existed restores without
            // inventing a count and without an error: on a fresh book the
            // field is the book's own zero.
            let answer = apply_intent(c, Intent::RestoreLife(json!({ "pets": 3 })));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            assert_eq!(c.runtime.book().life.sound_pets, 0);

            let answer =
                apply_intent(c, Intent::RestoreLife(json!({ "sound_pets": 7, "pets": 3 })));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            assert_eq!(c.runtime.book().life.sound_pets, 7);
            assert_eq!(c.runtime.book().life.pets, 3);
            let life: serde_json::Value =
                serde_json::from_str(&c.intents.life_json()).expect("life");
            assert_eq!(life["sound_pets"], json!(7), "{life}");
        }
        unsafe { duck_core_free(core) };
    }

    /// A chorus started in a branch session belongs to the selected duck.
    /// The arm used to validate and start through `active_runtime`/`on_active`
    /// — a second swap that lands on the original while `apply_intent` has
    /// the branch in `core.runtime`.
    #[test]
    fn a_chorus_with_a_selected_branch_sings_on_the_branch() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            // The arm refuses a move this build did not load; fork clones
            // the moods, so loading before the fork seeds both ducks.
            let emotions = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../upstream/microduck/robotd/emotions");
            c.runtime
                .load_emotions(&emotions)
                .expect("the official moods");
            let answer = apply_intent(c, Intent::Fork("song-branch".to_owned()));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            let answer = apply_intent(c, Intent::SelectBranch("song-branch".to_owned()));
            assert_eq!(answer["selected"], json!("song-branch"), "{answer}");
            // The transport is a LAN socket, not a peer: the arm refuses
            // without it. Core creation is serialized, so UDP 8766 is ours.
            let answer = apply_intent(c, Intent::StartLan("song-lead".to_owned()));
            assert_eq!(answer["ok"], json!(true), "LAN for the chorus probe: {answer}");

            let answer = apply_intent(c, Intent::Chorus(vec!["yes".to_owned()]));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            assert!(
                c.branches[0].runtime.chorus_active(),
                "the selected branch is the duck that sings"
            );
            assert!(
                !c.runtime.chorus_active(),
                "the original does not perform a branch's chorus"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// `Backup` is a global intent: `apply_intent` does not swap a branch in,
    /// so `active_runtime` is the selected duck and `core.runtime` is the
    /// original. Its care summary must describe the selected duck. This is
    /// the guard for the global arms' reads: "simplifying" them to
    /// `core.runtime` would back up the wrong animal.
    #[test]
    fn a_backup_summary_describes_the_selected_branch() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            let answer = apply_intent(c, Intent::Fork("backup-branch".to_owned()));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            let answer = apply_intent(c, Intent::SelectBranch("backup-branch".to_owned()));
            assert_eq!(answer["selected"], json!("backup-branch"), "{answer}");
            // Five branch ticks, no touch: the branch's idle is 0.1 s while
            // the original's is still zero.
            let answer = apply_intent(c, Intent::Step(5));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            let backup = apply_intent(c, Intent::Backup);
            assert_eq!(
                backup["care"]["idle_s"],
                json!(0.1),
                "the branch's clock is the one backed up: {backup}"
            );
            assert_eq!(
                c.runtime.care_summary()["idle_s"],
                json!(0.0),
                "the original is still on tick zero"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// `SelectBranch` is global too: after moving `core.selected`, the policy
    /// flag it mirrors must come from the newly selected duck. This is the
    /// second read that must stay `active_runtime`.
    #[test]
    fn a_branch_switch_mirrors_the_new_ducks_policy_flag() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            let answer = apply_intent(c, Intent::Fork("flag-branch".to_owned()));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            let answer = apply_intent(c, Intent::SelectBranch("flag-branch".to_owned()));
            assert_eq!(answer["selected"], json!("flag-branch"), "{answer}");
            // The control door swaps the selected branch in: stop the branch,
            // leave the original driving.
            assert!(!apply_control(c, ControlCommand::Enable(false)));
            assert!(!runtime_policy_enabled(&c.branches[0].runtime));
            assert!(runtime_policy_enabled(&c.runtime));

            // Selecting the original mirrors its enabled flag...
            apply_intent(c, Intent::SelectBranch(String::new()));
            assert!(
                c.robot_snapshot.lock().unwrap().enabled,
                "the original is still driving"
            );
            // ...and selecting the branch mirrors the branch's disabled one.
            apply_intent(c, Intent::SelectBranch("flag-branch".to_owned()));
            assert!(
                !c.robot_snapshot.lock().unwrap().enabled,
                "the branch was stopped"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// `RestoreBackup` is global, so `apply_intent` gives it no branch swap; the
    /// care and rules halves must swap explicitly onto the duck the client
    /// watches. They used to write straight to `core.runtime` — the original —
    /// while `Intent::Backup` read the selected branch: a backup taken from a
    /// branch restored into a hidden duck.
    #[test]
    fn a_restored_backup_lands_on_the_selected_branch() {
        let (_guard, core) = new_core();
        unsafe {
            let c = &mut *core;
            let answer = apply_intent(c, Intent::Fork("restore-branch".to_owned()));
            assert_eq!(answer["ok"], json!(true), "{answer}");
            let answer = apply_intent(c, Intent::SelectBranch("restore-branch".to_owned()));
            assert_eq!(answer["selected"], json!("restore-branch"), "{answer}");
            let original_care = c.runtime.care_summary();
            let original_rules = c.runtime.rules_summary();

            let backup = json!({
                "format": "microduck-backup/1",
                "day": "2026-11-19",
                "care": { "hunger": 0.37, "mood": 0.42, "asleep": false },
                "quests": { "quests": [] },
                "rules": {
                    "rules": [
                        { "trigger": "clap", "action": "sound", "value": "chirp" }
                    ]
                },
            });
            let answer = apply_intent(c, Intent::RestoreBackup(backup));
            assert_eq!(answer["ok"], json!(true), "{answer}");

            // The answer's summaries are the branch's, freshly restored.
            assert_eq!(answer["care"]["care"]["hunger"], json!(0.37), "{answer}");
            assert_eq!(answer["care"]["care"]["mood"], json!(0.42), "{answer}");
            assert_eq!(
                answer["rules"]["rules"]["rules"][0]["trigger"],
                json!("clap"),
                "{answer}"
            );
            assert_eq!(c.branches[0].runtime.care_summary()["hunger"], json!(0.37));
            assert_eq!(
                c.branches[0].runtime.rules_summary()["rules"][0]["value"],
                json!("chirp")
            );

            // The original runtime received neither half.
            assert_eq!(
                c.runtime.care_summary()["hunger"],
                original_care["hunger"],
                "the original's care is untouched"
            );
            assert_eq!(
                c.runtime.rules_summary(),
                original_rules,
                "the original's rules are untouched"
            );
        }
        unsafe { duck_core_free(core) };
    }

    /// Copy the policy bundle into a fresh temporary directory, leaving out
    /// the named files. The manifest is copied unchanged, so an omitted file
    /// drops that entry from the resolved set exactly as a deleted policy does.
    fn copy_policies_dir(source: &Path, omit: &[&str]) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let dir =
            std::env::temp_dir().join(format!("duck-policies-copy-{}-{stamp}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp policies dir");
        for entry in std::fs::read_dir(source).expect("policies dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            let name = entry.file_name();
            if path.is_file() && !omit.iter().any(|omit| name == *omit) {
                std::fs::copy(&path, dir.join(&name)).expect("copy policy file");
            }
        }
        dir
    }

    /// Copy the policies directory and point the `roulade` manifest entry at a
    /// second copy of the same network: the skill list's path changes, which is
    /// what a driving skill protects.
    fn skill_file_change_dir(source: &Path) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("duck-skill-reload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp skill dir");
        for entry in std::fs::read_dir(source).expect("policies dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if path.is_file() {
                let name = entry.file_name();
                std::fs::copy(&path, dir.join(&name)).expect("copy policy file");
            }
        }
        std::fs::copy(source.join("roulade.onnx"), dir.join("roulade_alt.onnx"))
            .expect("copy the skill network");
        let manifest = std::fs::read_to_string(dir.join("manifest.json")).expect("manifest");
        let mut value: serde_json::Value = serde_json::from_str(&manifest).expect("manifest json");
        for policy in value["policies"]
            .as_array_mut()
            .expect("policy list")
            .iter_mut()
        {
            if policy["file"] == "roulade.onnx" {
                policy["file"] = serde_json::json!("roulade_alt.onnx");
            }
        }
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&value).expect("manifest json"),
        )
        .expect("write manifest");
        dir
    }
}
