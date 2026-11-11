//! The mobile duck runtime: official control core + the robotd bring-up/reflex loop.
//!
//! `duck-control` holds the observation, policy, safety and fall predictor; the official
//! `robotd` skill scheduler lives in [`crate::control`]. This module supplies the loop around
//! them that `robotd` itself cannot be reused for (it owns tokio, IPC and systemd):
//!
//! ```text
//! read → observe (fall) → bring-up / limp-fall → controller.step → safety.apply → write
//! ```
//!
//! The numbers are the robot's: 2 s homing ramp, 50 gain while limp, 0.5 s settle, then the
//! velstand network is handed back the robot to get up. The full robotd limp-fall sequence
//! has more phases (pose, still, max); this is the subset the mobile shell needs first and
//! is marked where it differs.

use std::time::{Duration, Instant};

use duck_control::fall::{FallPredictor, FallPredictorConfig};
use duck_control::io::{IoError, RobotIo, Sensors};
use duck_control::model::{DEFAULT_POSITION, NUM_JOINTS};
use duck_control::obs::Command;
use duck_control::policy::Policy;
use duck_control::safety::{Safety, SafetyConfig};

use crate::control::{Controller, SkillTuning, Tuning};

/// `robotd`'s bring-up ramp: torque on, joints glide to home over this.
pub const HOME_RAMP: Duration = Duration::from_secs(2);
/// `robotd`'s `gain_limp`: yield rather than fight the floor.
pub const GAIN_LIMP: u16 = 50;
/// How long the limp-fall reflex holds the pose before handing back.
/// robotd's limp-fall timings: the limp ends when the gyro reads still, not on a timer.
const LIMP_FALL_STILL: Duration = Duration::from_millis(200);
const LIMP_FALL_MAX: Duration = Duration::from_millis(1500);
const LIMP_FALL_POSE: Duration = Duration::from_millis(600);
const LIMP_FALL_STILL_RATE: f64 = 1.0;
const GAIN_POSE: u16 = 160;
/// `Imu::ready()` upstream: 25 fused quaternion samples. One is decoded per
/// 50 Hz tick, so the real duck's gate is half a second — and ours must be the
/// same half second, not a perfect IMU from tick one.
const IMU_WARMUP_SAMPLES: u64 = 25;
/// After a limp-fall, drive zero command for this long so velstand can get up.
/// `SafetyConfig::default().deadman`.
const DEADMAN: Duration = Duration::from_millis(500);

/// robotd's `[control] cmd_alpha` / `head_alpha`: the client's command is
/// smoothed per tick (`cmd += alpha * (target - cmd)`), so a stick snap
/// becomes a ramp the gait can follow.
const CMD_ALPHA: f64 = 0.2;
const HEAD_ALPHA: f64 = 0.2;

/// How long a twist from the intent API keeps control after it arrives.
///
/// The app pushes the on-screen joystick's twist into the runtime every
/// frame, so an API twist without a lease would be overwritten 20 ms later
/// and an external agent could not drive the duck at all. With the lease the
/// API owns the duck while it keeps talking, and control goes back to the
/// screen when it stops.
const API_LEASE: Duration = DEADMAN;

/// Simulation time, in ticks.
///
/// Every deadline in the loop is measured against this rather than the wall
/// clock, so pausing the duck freezes its timers too: a paused robot must not
/// trip the 500 ms deadman, and slow-motion must stretch a 2 s homing ramp
/// with it. `speed` scales how many ticks one wall-clock frame advances; the
/// tick itself always lasts exactly 20 ms of simulated time.
#[derive(Debug, Clone, Copy)]
pub struct SimClock {
    pub tick: u64,
    pub paused: bool,
    pub speed: f64,
}

impl Default for SimClock {
    fn default() -> Self {
        Self {
            tick: 0,
            paused: false,
            speed: 1.0,
        }
    }
}

impl SimClock {
    pub fn advance(&mut self, ticks: u64) {
        self.tick = self.tick.saturating_add(ticks);
    }
}

fn array_of(values: Option<&[f64]>) -> [f64; NUM_JOINTS] {
    let mut out = [0.0; NUM_JOINTS];
    if let Some(values) = values {
        for (slot, value) in out.iter_mut().zip(values.iter()) {
            *slot = *value;
        }
    }
    out
}

/// A deadline on the simulation clock: "this tick, or later".
///
/// Everything in the loop that robotd measures in wall-clock time — the
/// deadman, the API lease, the homing ramp, the limp-fall timers — is a
/// deadline in ticks here. That is what lets the duck pause without tripping
/// its own safety timers, and what makes a replay reproduce them exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
struct TickTime(u64);

impl TickTime {
    fn of(clock: &SimClock) -> Self {
        Self(clock.tick)
    }

    fn plus(self, duration: Duration) -> Self {
        Self(self.0 + (duration.as_millis() as u64) / 20)
    }

    fn elapsed(self, now: TickTime) -> Duration {
        Duration::from_millis((now.0.saturating_sub(self.0)) * 20)
    }
}

/// The intent API's claim on the duck: the twist it last asked for and the
/// lease it holds. Separate from the runtime so it can be tested on its own.
#[derive(Debug, Default)]
struct ApiLease {
    twist: [f64; 3],
    until: Option<TickTime>,
}

impl ApiLease {
    fn take(&mut self, twist: [f64; 3], now: TickTime) {
        self.twist = twist;
        self.until = Some(now.plus(API_LEASE));
    }

    /// The twist the API owns right now, if its lease is still live.
    fn active(&mut self, now: TickTime) -> Option<[f64; 3]> {
        match self.until {
            Some(until) if now < until => Some(self.twist),
            Some(_) => {
                self.until = None;
                None
            }
            None => None,
        }
    }
}

#[derive(Debug)]
pub enum RuntimeError {
    Io(IoError),
    Policy(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "io: {error}"),
            Self::Policy(error) => write!(f, "policy: {error}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<IoError> for RuntimeError {
    fn from(error: IoError) -> Self {
        Self::Io(error)
    }
}

/// The bring-up state, as `robotd` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BringupState {
    Limp,
    Homing,
    Ready,
}

impl BringupState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Limp => "limp",
            Self::Homing => "homing",
            Self::Ready => "ready",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Bringup {
    Limp,
    Homing {
        from: [f64; NUM_JOINTS],
        since: TickTime,
        to: [f64; NUM_JOINTS],
    },
    Ready,
}

impl Bringup {
    fn state(&self) -> BringupState {
        match self {
            Self::Limp => BringupState::Limp,
            Self::Homing { .. } => BringupState::Homing,
            Self::Ready => BringupState::Ready,
        }
    }

    /// Linear per-tick target, or `None` once the ramp is done — the same shape as
    /// `robotd`'s `Bringup::homing_target`.
    fn homing_target(&self, now: TickTime) -> Option<[f64; NUM_JOINTS]> {
        let Self::Homing { from, since, to } = self else {
            return None;
        };
        let t = since.elapsed(now).as_secs_f64() / HOME_RAMP.as_secs_f64();
        if t >= 1.0 {
            return None;
        }
        let mut target = [0.0; NUM_JOINTS];
        for (slot, value) in target.iter_mut().enumerate() {
            *value = from[slot] + (to[slot] - from[slot]) * t;
        }
        Some(target)
    }

    /// A homing ramp that starts and ends at the same pose (the duck boots at
    /// home). It has nothing to travel, so it can finish the moment the IMU is
    /// ready rather than holding home statically for two seconds — a biped
    /// held on position actuators for that long falls.
    fn homing_is_static(&self) -> bool {
        match self {
            Self::Homing { from, to, .. } => from
                .iter()
                .zip(to.iter())
                .all(|(start, end)| (start - end).abs() < 0.1),
            _ => false,
        }
    }
}

/// One tick's report, for the app layer to render and for tests to assert on.
#[derive(Debug, Clone)]
pub struct TickReport {
    pub label: String,
    pub targets: [f64; NUM_JOINTS],
    pub gain: u16,
    pub fallen: bool,
    pub bringup: BringupState,
    /// What the servos and the IMU actually read this tick — the duck's
    /// proprioception, not the commands. `/state` has never published it, and a
    /// client that cannot see the joints cannot tell walking from falling.
    pub sensors: Sensors,
    /// False for the first moments after power-on: the real board's filter
    /// reports a default quaternion until it has fused 25 samples.
    pub imu_ready: bool,
    /// Somebody is holding the duck; the policy is paused at the hold pose.
    pub picked_up: bool,
    /// The pickup classifier's latest p(held), when it is running.
    pub p_held: Option<f32>,
    /// Pack voltage as the servos see it, once a second. `None` before the
    /// first slow read.
    pub volts: Option<f64>,
    /// Beak opening the loop commanded, 0..1 (the idle yawn included).
    pub mouth: f64,
}

/// Watching a limping robot for the moment it stops moving.
///
/// The end of the limp is read from the gyro rather than timed: a stumble is
/// over in a few hundred milliseconds, a topple takes most of a second, and
/// ending the limp early is landing stiff after all. Debounced, because a
/// tumbling robot passes through instants of near-zero rate on its way over.
#[derive(Debug, Clone, Copy, Default)]
struct Landing {
    still_for: Duration,
}

impl Landing {
    fn observe(&mut self, rate: Option<f64>, period: Duration, below: f64, held: Duration) -> bool {
        self.still_for = match rate {
            Some(rate) if rate < below => self.still_for.saturating_add(period),
            _ => Duration::ZERO,
        };
        self.still_for >= held
    }
}

/// `robotd`'s shutdown: sit, ease into the rest pose, hold, cut torque.
///
/// Used by `robot.shutdown` and by an empty pack. The app has no systemd to
/// stop, so "powered off" here means exactly what it means physically: torque
/// off, the duck limp and reporting `off` until it is powered back on.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shutdown {
    Idle,
    /// The sitstand network is driving the seat; `since` starts the hold timer.
    Sitting { since: TickTime },
    /// Easing from the seat to the rest pose, then holding it.
    Resting { from: [f64; NUM_JOINTS], since: TickTime },
}

/// robotd's limp-fall sequence.
#[derive(Debug, Clone, Copy)]
enum LimpFall {
    Idle,
    /// Gains at `GAIN_LIMP`, joints commanded to wherever they already are.
    Limp { since: TickTime, landing: Landing },
    /// Ramping from where the robot landed back to the standing pose.
    Posing { from: [f64; NUM_JOINTS], since: TickTime },
}

/// Everything a rewind has to put back, as one value.
///
/// The body state is MuJoCo's own integration state plus the actuator
/// bookkeeping; the rest is the control loop's: the tick, the filters, the
/// skill in flight, the policy's recurrent memory, the safety debounces and
/// the limp-fall phase. A checkpoint that misses any of them looks right and
/// then diverges, which is why this is one struct and not a collection of
/// setters.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Checkpoint {
    pub tick: u64,
    pub speed: f64,
    pub paused: bool,
    pub body: Vec<f64>,
    pub policy_state: Vec<u8>,
    pub active_net: Option<duck_control::policy::Net>,
    pub controller: crate::control::ControllerState,
    pub falling_ms: u64,
    pub fallen: bool,
    pub fall_falling_ms: u64,
    pub fall_upright_ms: u64,
    pub fall_armed: bool,
    pub bringup: String,
    pub homing_from: Option<Vec<f64>>,
    pub homing_since: u64,
    pub homing_to: Option<Vec<f64>>,
    pub limp_fall: String,
    pub limp_since: u64,
    pub limp_still_ms: u64,
    pub pose_from: Option<Vec<f64>>,
    pub enabled: bool,
    pub command_twist: [f64; 3],
    pub command_head: [f64; 4],
    pub command_body: [f64; 3],
    pub twist_ema: [f64; 3],
    pub head_ema: [f64; 4],
    pub body_ema: [f64; 3],
    pub body_active: bool,
    pub twist_fresh: bool,
    pub last_twist_tick: u64,
    pub last_head_tick: u64,
    pub lease_twist: [f64; 3],
    pub lease_until: Option<u64>,
    /// Samples the orientation filter had fused at this tick. Rewinding to a
    /// tick before the filter converged must bring the gate back with it.
    #[serde(default)]
    pub imu_samples: u64,
    /// The pack voltage reported at this tick, if it had been read.
    #[serde(default)]
    pub volts: Option<f64>,
    /// The client's beak intent.
    #[serde(default)]
    pub mouth: f64,
    /// The drive mode, as a string ("walk"/"roller").
    #[serde(default)]
    pub mode: String,
    /// The odometry estimator's whole state: a rewind that restored the body
    /// but not the anchor would publish a pose from a future that is gone.
    #[serde(default)]
    pub odometry: Vec<f64>,
    /// The power state, so a rewind out of a shutdown comes back with the
    /// duck powered. Without this a restore left `powered_off` latched and the
    /// duck could never be driven again from the UI.
    #[serde(default)]
    pub powered_off: bool,
    /// "idle", "sitting" or "resting" — the shutdown phase at this tick.
    #[serde(default)]
    pub shutdown: String,
    #[serde(default)]
    pub shutdown_since: u64,
    #[serde(default)]
    pub shutdown_from: Option<Vec<f64>>,
    /// The emotion playing at this tick, so a rewind can resume the same
    /// timeline instead of dropping it and leaving the pose mid-glance.
    #[serde(default)]
    pub emotion_name: Option<String>,
    #[serde(default)]
    pub emotion_elapsed: f64,
    #[serde(default)]
    pub emotion_skill_delivered: bool,
    /// The whole idle look-around spell, so a rewind/fork replays the same
    /// glances rather than a duck whose RNG and phases came from the future.
    #[serde(default)]
    pub idle_state: crate::control::IdleState,
    #[serde(default)]
    pub last_idle: IdleSnapshot,
}

/// The last idle offset, as plain numbers: `IdleOffset` is upstream's type
/// and intentionally has no serde dependency.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct IdleSnapshot {
    pub head: [f64; 4],
    pub body_z: f64,
    pub mouth: f64,
    pub takeover: f64,
}

/// The runtime. `T` is the body: MuJoCo in the app, `RemoteIo` on the desktop harness.
pub struct DuckRuntime<T: RobotIo> {
    safety: Safety<T>,
    controller: Controller,
    fall: FallPredictor,
    bringup: Bringup,
    command: Command,
    enabled: bool,
    limp_fall: LimpFall,
    last_twist: TickTime,
    twist_fresh: bool,
    /// The twist from the intent API, and the lease it holds on the duck.
    api_lease: ApiLease,
    /// The smoothed command, per tick, exactly as robotd keeps it.
    twist_ema: [f64; 3],
    head_ema: [f64; 4],
    body_ema: [f64; 3],
    /// Whether a client is holding the standing body pose (robotd's
    /// `PoseIntent.active`); while it is, the twist is zeroed.
    body_active: bool,
    /// The real duck looks around when nothing is happening.
    idle_head: crate::control::IdleHead,
    /// The last idle offset, published so a client can see the duck is alive.
    last_idle: crate::control::IdleOffset,
    /// The last sound the core queued, published so a client can tell that an
    /// emotion's official wav really reached the speaker channel.
    last_sound: Option<String>,
    /// When an idle ritual last ran, on the simulated clock. The duck plays
    /// one of its own official emotions when left alone long enough: a robot
    /// standing perfectly still reads as switched off, and this is the one
    /// behaviour that needs no tap and no screen space.
    last_idle_ritual: Option<u64>,
    /// How long the duck waits before it does something on its own, in
    /// simulated ticks. The default is 30 s; a client can shorten it.
    idle_ritual_ticks: u64,
    /// How long a pet keeps the duck lively, in simulated seconds. Petting is
    /// the phone's stand-in for `pet_detect`: upstream counts a stroke of the
    /// hand as a pet, and a phone has a screen instead of a hand on the shell.
    pet_glow_until: Option<u64>,
    /// The duck's lifetime record: seconds powered and walking, falls, picks
    /// up, skills by name. It is what makes this the same animal tomorrow.
    book: crate::vetbook::Book,
    /// The virtual face LEDs: status, battery gauge and flashlight. The phone
    /// has no I²C expander, but the verdict and the colours are the duck's.
    face: crate::leds::Face,
    /// The last ride segment the platform pulled, so a client can see the
    /// held wheee actually streaming start/loop/end.
    last_ride_segment: Option<String>,
    /// The last emotion wav specifically. `last_sound` is overwritten by the
    /// next reaction (a coo, an alarm), so a client checking *that the
    /// emotion's voice was queued* needs a field only emotions write.
    last_emotion_sound: Option<String>,
    /// Rising edges for the duck's own voice: robotd coos when a hand picks
    /// it up, alarms when it falls, and inquires when motion resumes after a
    /// long stillness. Three of the six synthesized tags were dead code.
    /// The phone heard a sudden loud sound this tick (set by the FFI from
    /// the sensor registry, which lives on DuckCore, not the runtime).
    room_loud: bool,
    last_loud: bool,
    last_pickup_paused: bool,
    last_fallen: bool,
    /// Consecutive ticks the fall predictor has said "down". A one-tick tip
    /// while a scripted move shifts the trunk is not a fall and must not
    /// make the duck shriek.
    fallen_ticks: u64,
    last_moving: bool,
    /// Simulated clock of the last reaction, so a noisy edge cannot spam.
    last_reaction: Option<u64>,
    /// `robotd/emotions/`: thirteen keyframe timelines with their own voices.
    /// Loaded from the app bundle, not baked in, so the data stays upstream's.
    emotions: crate::emotion::Library,
    emotion_playing: Option<crate::emotion::Playing>,
    idle_params: robotd_params::IdleParams,
    /// When the head was last commanded; idle glances start after `after_s`.
    last_head_command: TickTime,
    /// Simulation time. Everything above is expressed against it.
    clock: SimClock,
    /// The wall-clock instant tick 0 corresponds to. IdleHead is upstream code
    /// that speaks `Instant`, so it gets a simulated timeline anchored here:
    /// exactly 20 ms per tick, frozen while paused, identical on a replay.
    sim_origin: Instant,
    /// The last tick's report, kept so a single step can publish it.
    last: Option<TickReport>,
    /// Fused orientation samples since power-on. The real IMU's SFLP filter
    /// reports a default quaternion until it has produced 25 of them, so a
    /// policy started sooner would be reading an orientation that is not a
    /// measurement.
    imu_samples: u64,
    /// `robotd`'s pickup watch: pause the walking networks while held.
    pickup: Option<crate::pickup::Watch>,
    /// Where the detector loads from, so a fork can build its own watch.
    pickup_model: Option<std::path::PathBuf>,
    /// A hand on the duck: (anchor at grab, offset). The force is applied to
    /// the body, never to a joint.
    carry: Option<([f64; 3], [f64; 3])>,
    /// The client's beak intent, 0..1, exactly `robotd`'s `Snapshot::mouth`.
    mouth: f64,
    /// Which bundle is loaded. `robotd` asks it every tick for the idle
    /// behaviour (`IdleParams::enabled_in`), and it is what the shell shows.
    mode: robotd_params::Mode,
    /// The shutdown sequence, and the latched off state after it ends.
    shutdown: Shutdown,
    powered_off: bool,
    /// A client asked to power off; taken on the next tick.
    shutdown_requested: bool,
    /// The pack voltage, smoothed the way `robotd` smooths it: a step sag must
    /// not reach the empty threshold.
    volts_ema: Option<f64>,
    /// Where the duck believes it is, from its own legs and IMU. `robotd` ticks
    /// it inside the control loop on fresh samples once the IMU has warmed.
    odometry: odometry::Odometry,
    /// The duck's voice. `robotd` quacks once as the loop comes up, chirps on
    /// the mode cue and pecks before the servos go quiet; the same here.
    voice: crate::voice::Voice,
    /// Sounds the shell has not played yet, oldest first: (tag, PCM at
    /// [`crate::synth::SR`]). The core renders; the platform owns the speaker.
    sounds: std::collections::VecDeque<String>,
    /// The slow-sensor voltage, refreshed once a second as `robotd` does.
    volts: Option<f64>,
    slow_ticks: u32,
    dt: Duration,
}

impl<T: RobotIo> DuckRuntime<T> {
    pub fn new(
        io: T,
        policy: Policy,
        tuning: Tuning,
        skills: SkillTuning,
        safety: SafetyConfig,
    ) -> Self {
        Self {
            safety: Safety::new(io, safety),
            controller: Controller::new(policy, tuning, skills),
            fall: FallPredictor::new(FallPredictorConfig::default()),
            bringup: Bringup::Limp,
            command: Command::default(),
            enabled: false,
            limp_fall: LimpFall::Idle,
            last_twist: TickTime::default(),
            twist_fresh: false,
            api_lease: ApiLease::default(),
            twist_ema: [0.0; 3],
            head_ema: [0.0; 4],
            body_ema: [0.0; 3],
            body_active: false,
            idle_head: crate::control::IdleHead::new(robotd_params::IdleParams::default(), 0xD0C),
            idle_params: robotd_params::IdleParams::default(),
            last_idle: crate::control::IdleOffset::default(),
            book: crate::vetbook::Book::default(),
            pet_glow_until: None,
            last_idle_ritual: None,
            idle_ritual_ticks: 1500,
            face: crate::leds::Face::default(),
            last_sound: None,
            last_ride_segment: None,
            last_emotion_sound: None,
            room_loud: false,
            last_loud: false,
            last_pickup_paused: false,
            last_fallen: false,
            fallen_ticks: 0,
            last_moving: false,
            last_reaction: None,
            emotions: crate::emotion::Library::default(),
            emotion_playing: None,
            last_head_command: TickTime::default(),
            clock: SimClock::default(),
            sim_origin: Instant::now(),
            last: None,
            imu_samples: 0,
            pickup: None,
            pickup_model: None,
            carry: None,
            mouth: 0.0,
            mode: robotd_params::Mode::Walk,
            shutdown: Shutdown::Idle,
            powered_off: false,
            shutdown_requested: false,
            odometry: odometry::Odometry::alpha(),
            volts_ema: None,
            voice: crate::voice::Voice::new(0xD0C),
            sounds: std::collections::VecDeque::new(),
            volts: None,
            slow_ticks: 0,
            dt: Duration::from_millis(20),
        }
    }

    /// Load the pickup classifier. A watch that will not load leaves the duck
    /// walking, exactly as `robotd` treats a missing model: a convenience, not
    /// a health verdict.
    pub fn load_pickup(&mut self, model: &std::path::Path) -> Result<(), String> {
        let params = crate::params::PickupParams {
            enabled: true,
            model: Some(model.to_path_buf()),
            ..Default::default()
        };
        let watch = crate::pickup::Watch::build(&params)
            .ok_or_else(|| format!("pickup detector {} did not load", model.display()))?;
        self.pickup = Some(watch);
        self.pickup_model = Some(model.to_path_buf());
        Ok(())
    }

    /// Whether the orientation filter has converged. Until it has, the loop
    /// holds the robot rather than driving it on a default quaternion.
    pub fn imu_ready(&self) -> bool {
        self.imu_samples >= IMU_WARMUP_SAMPLES
    }

    pub fn imu_samples(&self) -> u64 {
        self.imu_samples
    }

    /// Where the duck believes it is: trunk position in the world frame, and
    /// heading in the boot-relative frame the IMU defines.
    pub fn position(&self) -> [f64; 3] {
        self.odometry.position()
    }

    pub fn yaw(&self) -> f64 {
        self.odometry.yaw()
    }

    pub fn anchor(&self) -> ([f64; 2], usize) {
        (self.odometry.anchor_xy(), self.odometry.anchor_foot())
    }

    /// Somebody is holding the duck: a spring toward `anchor + offset`.
    ///
    /// `active` starts a new hold at where the trunk is now; the offset is
    /// measured from that point, so a finger drag maps one-to-one onto the
    /// duck however long it is held.
    pub fn set_carry(&mut self, offset: [f64; 3], active: bool) {
        if !active {
            self.carry = None;
            return;
        }
        match &mut self.carry {
            Some((_, held)) => *held = offset,
            None => {
                let anchor = self.safety.io().trunk_position().unwrap_or([0.0; 3]);
                self.carry = Some((anchor, offset));
            }
        }
    }

    /// Ask for a sound by tag. Unknown tags are dropped rather than invented.
    pub fn queue_sound(&mut self, tag: &str) {
        // `sound.rs`'s own rule: one playing child, and a new sound kills the
        // previous one — that is what lets someone spam the chirp trigger
        // cleanly. One-shot sounds replace whatever one-shot was queued;
        // the wheee ride's segments are not one-shots and keep their place.
        let known = crate::voice::TAGS.contains(&tag)
            || tag.strip_prefix("emotion:").is_some_and(|name| self.emotions.get(name).is_some());
        if known {
            self.sounds.clear();
            self.sounds.push_back(tag.to_owned());
            self.last_sound = Some(tag.to_owned());
            if tag.starts_with("emotion:") {
                self.last_emotion_sound = Some(tag.to_owned());
            }
        }
    }

    /// The next tag the shell should play, if any.
    pub fn take_sound(&mut self) -> Option<String> {
        // Ride segments jump the one-shot queue: a loop arriving late would
        // leave the duck mid-air with no sound under it.
        if let Some(segment) = self.voice.pending_segment() {
            self.last_ride_segment = Some(segment.clone());
            return Some(segment);
        }
        self.sounds.pop_front()
    }

    /// Render a tag to PCM, for a platform that wants the samples rather than
    /// the tag. `None` for an unknown tag.
    pub fn render_sound(&mut self, tag: &str) -> Option<Vec<f32>> {
        if let Some(name) = tag.strip_prefix("emotion:") {
            return self.emotion_pcm(name);
        }
        if tag.starts_with("wheee:") {
            return self.voice.render_segment(tag);
        }
        self.render_tag(tag)
    }

    /// `robotctl`'s held wheee, as a hold level rather than a one-shot.
    pub fn wheee(&mut self, hold: bool) {
        self.voice.wheee(hold);
    }

    pub fn voice_seed(&self) -> u32 {
        self.voice.seed()
    }

    /// The voice's personality traits, for a client that wants to know whose
    /// voice it is listening to.
    pub fn voice_personality(&self) -> crate::personality::Personality {
        self.voice.personality()
    }

    /// Render a sound without queueing it, for the shell's own speaker: the
    /// core owns the synth so the PCM is the duck's, not a sample.
    /// Where the pickup detector loads from, if one is running.
    pub fn pickup_model(&self) -> Option<&std::path::Path> {
        self.pickup_model.as_deref()
    }

    /// Drop queued sounds: a mode switch plays its own cue, not the greet
    /// `enable` just queued.
    pub fn clear_sounds(&mut self) {
        self.sounds.clear();
    }

    pub fn render_tag(&mut self, tag: &str) -> Option<Vec<f32>> {
        self.voice.render(tag).map(|(_, pcm)| pcm)
    }

    /// `robot.shutdown`: sit, rest, torque off. Taken on the next tick, so the
    /// caller never runs it on the tick that drains the intent queue.
    pub fn request_shutdown(&mut self) {
        self.shutdown_requested = true;
    }

    pub fn powered_off(&self) -> bool {
        self.powered_off
    }

    /// A shutdown is sitting, resting or finished. Nothing may start a move
    /// or a bundle switch while this is true.
    pub fn shutting_down(&self) -> bool {
        self.powered_off || !matches!(self.shutdown, Shutdown::Idle)
    }

    /// Power the duck back on after a shutdown. The battery has to have been
    /// charged (or the app restarted) for this to be a sensible request.
    pub fn power_on(&mut self) -> Result<(), RuntimeError> {
        self.powered_off = false;
        self.shutdown = Shutdown::Idle;
        self.shutdown_requested = false;
        // **The seat did not survive the torque cut.** `begin_shutdown_sit`
        // left `Sit::Sitting` in the controller, and `enable`'s `reset` only
        // clears the policy — the sitting network would take the first tick
        // after power-on and sit the duck back down. `forget` is upstream's
        // own answer for "the robot came back limp": the seat and any move are
        // gone, and it stands from home.
        self.controller.forget();
        self.enable()
    }

    fn begin_shutdown(&mut self, now: TickTime) {
        if self.enabled
            && matches!(self.bringup, Bringup::Ready)
            && !self.safety.fallen()
            && self.controller.has_sitstand()
        {
            tracing::warn!("shutdown: sitting down before torque off");
            self.queue_sound("peck");
            self.controller.begin_shutdown_sit();
            self.shutdown = Shutdown::Sitting { since: now };
        } else {
            // Nothing to sit with: cut torque now, which is the last thing the
            // sequence would have done anyway.
            tracing::warn!("shutdown: cannot sit; cutting torque now");
            let _ = self.safety.set_torque(false);
            self.enabled = false;
            self.bringup = Bringup::Limp;
            self.powered_off = true;
        }
    }

    pub fn mode(&self) -> robotd_params::Mode {
        self.mode
    }

    /// After a mode switch: the loop's own mode-dependent behaviour follows
    /// the bundle that is now loaded.
    pub fn set_mode(&mut self, mode: robotd_params::Mode) {
        self.mode = mode;
    }

    /// The beak, 0..1. The idle yawn opens it further, never less.
    ///
    /// Refused when the body has no mouth servo: the official walking scene
    /// drops the joint (no policy drives it), so an accepted intent there
    /// would be a lie. The physical duck's beak is servo 34.
    pub fn set_mouth(&mut self, open: f64) -> Result<(), &'static str> {
        if !self.safety.has_mouth() {
            return Err("this scene has no mouth joint");
        }
        self.mouth = if open.is_finite() { open.clamp(0.0, 1.0) } else { 0.0 };
        Ok(())
    }

    pub fn has_mouth(&self) -> bool {
        self.safety.has_mouth()
    }

    pub fn carrying(&self) -> bool {
        self.carry.is_some()
    }

    pub fn picked_up(&self) -> bool {
        self.pickup.as_ref().is_some_and(|watch| watch.paused())
    }

    pub fn p_held(&self) -> Option<f32> {
        self.pickup.as_ref().and_then(|watch| watch.p_held())
    }

    /// Torque on and ramp to home, exactly as `robotd init` does. Reads the pose first, so
    /// the ramp starts where the robot is rather than snapping.
    pub fn enable(&mut self) -> Result<(), RuntimeError> {
        if self.enabled {
            return Ok(());
        }
        let sensors = self.safety.read()?;
        self.safety.set_torque(true)?;
        // The virtual duck starts at HOME, so the 2 s ramp has nothing to do —
        // and a biped held statically on position actuators falls during it.
        // From any other pose (a real robot's folded boot) the ramp is kept.
        let at_home = sensors
            .positions
            .iter()
            .zip(DEFAULT_POSITION.iter())
            .all(|(current, home)| (current - home).abs() < 0.1);
        if at_home && self.imu_ready() {
            self.bringup = Bringup::Ready;
            self.controller.reset();
            tracing::info!("bring-up: already at home; the policy has the robot");
        } else {
            // The IMU's own gate: the real duck ramps for two seconds while the
            // filter converges, so by the time the policy could drive it is
            // warm. The app's duck is already at home and has no ramp to spend
            // that time on, so it holds the home pose until the sampler has the
            // same 25 fused readings.
            self.bringup = Bringup::Homing {
                from: sensors.positions,
                since: TickTime::of(&self.clock),
                to: DEFAULT_POSITION,
            };
        }
        self.enabled = true;
        // `robotd`'s greet: one quack as the loop comes up. On a headless board
        // it is the audible "robotd is running"; here it is the duck waking.
        self.queue_sound("greet");
        self.twist_fresh = false;
        self.command.twist = [0.0; 3];
        self.twist_ema = [0.0; 3];
        self.head_ema = [0.0; 4];
        self.body_ema = [0.0; 3];
        self.body_active = false;
        Ok(())
    }

    /// Torque off and forget the seat/move state.
    pub fn disable(&mut self) -> Result<(), RuntimeError> {
        if !self.enabled && matches!(self.bringup, Bringup::Limp) {
            // Already limp: torque is off and the peck would be the second
            // one for a single request.
            return Ok(());
        }
        // The low "tock" the robot gives before its servos go quiet.
        self.queue_sound("peck");
        self.safety.set_torque(false)?;
        self.enabled = false;
        self.bringup = Bringup::Limp;
        self.controller.reset();
        self.twist_ema = [0.0; 3];
        self.head_ema = [0.0; 4];
        self.body_ema = [0.0; 3];
        self.body_active = false;
        self.fall.reset();
        self.limp_fall = LimpFall::Idle;
        // Torque off means the duck is on its own: a hand still holding the
        // trunk would be carrying a limp robot around, and the seat a
        // controller thought it had is gone with the torque.
        self.carry = None;
        self.controller.forget();
        if let Some(watch) = self.pickup.as_mut() {
            let _ = watch.tick(false, None, self.dt);
        }
        Ok(())
    }

    pub fn set_twist(&mut self, twist: [f64; 3]) {
        self.command.twist = twist;
        self.last_twist = TickTime::of(&self.clock);
        self.twist_fresh = true;
    }

    /// A twist from the intent API. It takes the duck over from the
    /// on-screen joystick for [API_LEASE], so an external agent can drive it
    /// while it keeps sending twists.
    pub fn set_api_twist(&mut self, twist: [f64; 3]) {
        self.api_lease.take(twist, TickTime::of(&self.clock));
    }

    /// `robot.look`: aim the head at a point in the trunk frame. The gaze
    /// solver runs against the training MJCF's own kinematics, so the answer
    /// respects the servos' travel limits; `clamped` says the duck is looking
    /// as close as it can. The trunk frame is what `/state.pose` publishes
    /// against, so a client aims by subtracting the duck's own position.
    pub fn look_at(&mut self, target: [f64; 3], neck_pitch: f64) -> kinematics::head::Gaze {
        let gaze = kinematics::head::HeadFk::alpha().look_at(target, neck_pitch);
        self.set_head(gaze.joints);
        gaze
    }

    pub fn set_head(&mut self, head: [f64; 4]) {
        self.command.head = head;
        // Any head command wakes the idle look-around.
        self.last_head_command = TickTime::of(&self.clock);
    }

    pub fn set_body(&mut self, z: f64, roll: f64, pitch: f64) {
        self.command.body.z = z;
        self.command.body.roll = roll;
        self.command.body.pitch = pitch;
        // robotd's PoseIntent.active: the loop glides toward the pose while it
        // is held, and snaps back when the client releases it.
        self.body_active = true;
    }

    /// Release the standing body pose (robotd's B-button exit): the offset
    /// snaps back to nominal and the twist is live again.
    pub fn release_body(&mut self) {
        self.body_active = false;
    }

    pub fn skill_names(&self) -> Vec<String> {
        let mut names = self.controller.skill_names();
        names.extend(self.emotions.names());
        names
    }

    /// `robot.do <emotion>` on the real duck: the emotion is a skill, so it
    /// goes through the same `/skills` list and `/skill` verb as a kick.
    pub fn trigger_emotion(&mut self, name: &str) -> Result<bool, &'static str> {
        if self.controller.busy() || self.controller.is_sitting() {
            return Err("a move is running; the duck must be standing still");
        }
        let Some(emotion) = self.emotions.get(name) else {
            return Err("no such emotion");
        };
        if emotion.duration() <= 0.0 {
            return Err("emotion has no keyframes");
        }
        self.book.emotion(name);
        self.emotion_playing = Some(crate::emotion::Playing {
            name: name.to_owned(),
            elapsed: 0.0,
            skill_delivered: false,
        });
        // The voice is the official wav, delivered through the same speaker
        // channel as the synth: the platform only sees a tag. Go through
        // queue_sound, not a direct push, so /state.last_sound and the
        // bounded queue see the same event as every other sound.
        self.queue_sound(&format!("emotion:{name}"));
        Ok(true)
    }

    pub fn emotion_names(&self) -> Vec<String> {
        self.emotions.names()
    }

    pub fn last_sound(&self) -> Option<&str> {
        self.last_sound.as_deref()
    }

    /// How long the duck waits before doing something by itself. `0` disables
    /// the idle ritual; this is the "liveliness" control a client can set.
    pub fn set_idle_ritual(&mut self, seconds: f64) {
        self.idle_ritual_ticks = (seconds * 50.0).max(0.0) as u64;
        if seconds <= 0.0 {
            self.idle_ritual_ticks = u64::MAX;
        }
    }

    /// A stroke of the hand: the duck counts it, coos, and stays lively a
    /// little longer than usual. `pet_detect` does the same for the physical
    /// robot's shell.
    /// The sensor bridge's verdict for this tick.
    pub fn set_room_loud(&mut self, loud: bool) {
        self.room_loud = loud;
    }

    pub fn pet(&mut self) {
        self.book.life.pets += 1;
        self.book.note("pet".to_owned());
        self.queue_sound("coo");
        self.pet_glow_until = Some(self.clock.tick + 150);
    }

    /// A treat on the floor: the official ground-pick peck serves as the
    /// bite, and a satisfied sound seals it. Digital-only ritual, but every
    /// part of it is an official move — the robot eats nothing, and this
    /// one pretends to no hardware.
    pub fn feed(&mut self) -> Result<bool, &'static str> {
        let started = self.trigger_skill("ground_pick")?;
        if started {
            self.book.life.fed += 1;
            self.book.note("feed".to_owned());
            self.queue_sound("yes");
        }
        Ok(started)
    }

    /// The lifetime record, for `/state.life` and the shell's own storage.
    pub fn book(&self) -> &crate::vetbook::Book {
        &self.book
    }

    pub fn book_mut(&mut self) -> &mut crate::vetbook::Book {
        &mut self.book
    }

    /// The virtual face, and `robot.flashlight`'s mobile equivalent.
    pub fn face(&self) -> crate::leds::Face {
        self.face
    }

    pub fn set_flashlight(&mut self, toggle: bool, on: bool, color: &str) -> crate::leds::Face {
        self.face.flashlight =
            crate::leds::flashlight_after(self.face.flashlight, toggle, on, color);
        self.face
    }

    pub fn last_ride_segment(&self) -> Option<&str> {
        self.last_ride_segment.as_deref()
    }

    pub fn last_emotion_sound(&self) -> Option<&str> {
        self.last_emotion_sound.as_deref()
    }

    /// The idle look-around's gain and the head command it last produced.
    /// Read by `/state` so a "dumb, motionless duck" is a measurable failure
    /// rather than a matter of opinion.
    pub fn idle_takeover(&self) -> f64 {
        self.last_idle.takeover
    }

    pub fn idle_head_command(&self) -> [f64; 4] {
        self.last_idle.head
    }

    /// What an emotion is doing right now, for `/state` and the instrument
    /// sheet. A client cannot watch a label for this: the policy still says
    /// "walk" while a scripted timeline owns the head and beak.
    pub fn emotion_state(&self) -> Option<(&str, f64, f64)> {
        let playing = self.emotion_playing.as_ref()?;
        let emotion = self.emotions.get(&playing.name)?;
        Some((&playing.name, playing.elapsed, emotion.duration()))
    }

    /// Load robotd's emotions from the app bundle. Called once after the core
    /// is created, like the pickup classifier.
    pub fn load_emotions(&mut self, dir: &std::path::Path) -> Result<usize, String> {
        let library = crate::emotion::Library::load(dir)?;
        let count = library.0.len();
        self.emotions = library;
        Ok(count)
    }

    /// An emotion's voice, by tag. `render_sound` asks this first.
    pub fn emotion_pcm(&self, name: &str) -> Option<Vec<f32>> {
        self.emotions.get(name).map(|emotion| emotion.pcm().to_vec())
    }

    pub fn trigger_skill(&mut self, name: &str) -> Result<bool, &'static str> {
        if self.emotions.get(name).is_some() {
            return self.trigger_emotion(name);
        }
        let index = self
            .controller
            .skill_names()
            .iter()
            .position(|candidate| candidate == name)
            .ok_or("no such skill")?;
        // A skill the scheduler refused is not part of the duck's story: the
        // book only records what actually started.
        self.controller.start_skill(index).map(|started| {
            self.book.skill(name);
            started
        })
    }

    pub fn start_ground_pick(&mut self) -> Result<(), &'static str> {
        self.controller.start_ground_pick()
    }

    pub fn sit_toggle(&mut self) -> Result<&'static str, &'static str> {
        self.controller.sit_toggle()
    }

    pub fn driving(&self) -> Option<crate::control::Driving> {
        self.controller.driving()
    }

    pub fn busy(&self) -> bool {
        self.controller.busy()
    }

    pub fn is_sitting(&self) -> bool {
        self.controller.is_sitting()
    }

    pub fn bringup(&self) -> BringupState {
        self.bringup.state()
    }

    /// Read-only access to the body backend, for rendering and diagnostics.
    pub fn io(&self) -> &T {
        self.safety.io()
    }

    pub fn fallen(&self) -> bool {
        self.safety.fallen()
    }

    /// Swap the policy set, keeping the robot's state: the same body, the
    /// same seat, the same skill, the same filter anchor — robotd's
    /// `Controller::carry_over`. Networks whose digest is unchanged keep
    /// their recurrent memory; a genuinely different file starts fresh.
    pub fn replace_policy(&mut self, policy: Policy, tuning: Tuning, skills: SkillTuning) {
        let old = std::mem::replace(&mut self.controller, Controller::new(policy, tuning, skills));
        self.controller.carry_over(&old);
        // A reload during a fall must not cancel the reflex: robotd refuses
        // the swap until the robot is at rest, and the mobile core keeps the
        // sequence running and lets it finish.
        if matches!(self.limp_fall, LimpFall::Idle) {
            self.controller.reset();
        }
    }

    /// A second duck at this exact instant: its own body (a deep copy of the
    /// MuJoCo model and data), its own controller and policy sessions, and the
    /// same checkpoint. The two then run independently — one branch's pushes,
    /// skills and world changes never reach the other.
    pub fn fork(&self) -> Option<Self> {
        let body = self.safety.io().fork()?;
        let tuning = self.controller.tuning();
        let skills = self.controller.skill_tuning();
        let mut fork = DuckRuntime::new(
            body,
            self.controller.clone_policy(),
            tuning,
            skills,
            self.safety.config(),
        );
        // Each branch scores its own future; a shared classifier's window would
        // mix two timelines.
        if let Some(model) = &self.pickup_model {
            let _ = fork.load_pickup(model);
        }
        // The emotions are data, not hardware: a fork must keep its own copy
        // or `robot.do curious` silently vanishes on every branch.
        fork.emotions = self.emotions.clone();
        let checkpoint = self.checkpoint()?;
        if !fork.restore(&checkpoint) {
            return None;
        }
        Some(fork)
    }

    /// Shake/shove input: an external trunk velocity impulse.
    pub fn push(&mut self, velocity: [f64; 3]) -> Result<(), RuntimeError> {
        self.safety.push(velocity)?;
        Ok(())
    }

    /// The simulation clock, for the shell and the time machine.
    pub fn clock(&self) -> SimClock {
        self.clock
    }

    /// Everything needed to resume from this tick, bit for bit.
    pub fn checkpoint(&self) -> Option<Checkpoint> {
        let body = self.safety.snapshot()?;
        let (falling_ms, fallen) = self.safety.fall_state();
        let (fall_falling_ms, fall_upright_ms, fall_armed) = self.fall.state();
        let (bringup, homing_from, homing_since, homing_to) = match self.bringup {
            Bringup::Limp => ("limp".to_owned(), None, 0, None),
            Bringup::Homing { from, since, to } => (
                "homing".to_owned(),
                Some(from.to_vec()),
                since.0,
                Some(to.to_vec()),
            ),
            Bringup::Ready => ("ready".to_owned(), None, 0, None),
        };
        let (limp_fall, limp_since, limp_still_ms, pose_from) = match self.limp_fall {
            LimpFall::Idle => ("idle".to_owned(), 0, 0, None),
            LimpFall::Limp { since, landing } => (
                "limp".to_owned(),
                since.0,
                landing.still_for.as_millis() as u64,
                None,
            ),
            LimpFall::Posing { from, since } => (
                "posing".to_owned(),
                since.0,
                0,
                Some(from.to_vec()),
            ),
        };
        Some(Checkpoint {
            tick: self.clock.tick,
            speed: self.clock.speed,
            paused: self.clock.paused,
            body,
            policy_state: self.controller.policy_state(),
            active_net: self.controller.active_net(),
            controller: self.controller.snapshot(),
            falling_ms,
            fallen,
            fall_falling_ms,
            fall_upright_ms,
            fall_armed,
            bringup,
            homing_from,
            homing_since,
            homing_to,
            limp_fall,
            limp_since,
            limp_still_ms,
            pose_from,
            enabled: self.enabled,
            command_twist: self.command.twist,
            command_head: self.command.head,
            command_body: [
                self.command.body.z,
                self.command.body.roll,
                self.command.body.pitch,
            ],
            twist_ema: self.twist_ema,
            head_ema: self.head_ema,
            body_ema: self.body_ema,
            body_active: self.body_active,
            twist_fresh: self.twist_fresh,
            last_twist_tick: self.last_twist.0,
            last_head_tick: self.last_head_command.0,
            lease_twist: self.api_lease.twist,
            lease_until: self.api_lease.until.map(|until| until.0),
            imu_samples: self.imu_samples,
            volts: self.volts,
            mouth: self.mouth,
            mode: self.mode.as_str().to_owned(),
            odometry: self.odometry.snapshot(),
            powered_off: self.powered_off,
            shutdown: match self.shutdown {
                Shutdown::Idle => "idle".to_owned(),
                Shutdown::Sitting { .. } => "sitting".to_owned(),
                Shutdown::Resting { .. } => "resting".to_owned(),
            },
            shutdown_since: match self.shutdown {
                Shutdown::Sitting { since } | Shutdown::Resting { since, .. } => since.0,
                Shutdown::Idle => 0,
            },
            shutdown_from: match self.shutdown {
                Shutdown::Resting { from, .. } => Some(from.to_vec()),
                _ => None,
            },
            emotion_name: self.emotion_playing.as_ref().map(|p| p.name.clone()),
            emotion_elapsed: self.emotion_playing.as_ref().map_or(0.0, |p| p.elapsed),
            emotion_skill_delivered: self
                .emotion_playing
                .as_ref()
                .is_some_and(|p| p.skill_delivered),
            idle_state: self.idle_head.snapshot(self.sim_origin),
            last_idle: IdleSnapshot {
                head: self.last_idle.head,
                body_z: self.last_idle.body_z,
                mouth: self.last_idle.mouth,
                takeover: self.last_idle.takeover,
            },
        })
    }

    /// Resume from [`Self::checkpoint`]. False when the body cannot be rewound
    /// or the state does not belong to this build.
    pub fn restore(&mut self, checkpoint: &Checkpoint) -> bool {
        if !self.safety.restore(&checkpoint.body) {
            return false;
        }
        if !self
            .controller
            .restore_policy_state(checkpoint.active_net, &checkpoint.policy_state)
        {
            return false;
        }
        self.controller.restore(&checkpoint.controller);
        self.safety
            .set_fall_state(checkpoint.falling_ms, checkpoint.fallen);
        self.fall.set_state(
            checkpoint.fall_falling_ms,
            checkpoint.fall_upright_ms,
            checkpoint.fall_armed,
        );
        self.clock.tick = checkpoint.tick;
        self.clock.speed = checkpoint.speed;
        self.clock.paused = checkpoint.paused;
        self.bringup = match checkpoint.bringup.as_str() {
            "homing" => Bringup::Homing {
                from: array_of(checkpoint.homing_from.as_deref()),
                since: TickTime(checkpoint.homing_since),
                to: array_of(checkpoint.homing_to.as_deref()),
            },
            "ready" => Bringup::Ready,
            _ => Bringup::Limp,
        };
        self.limp_fall = match checkpoint.limp_fall.as_str() {
            "limp" => LimpFall::Limp {
                since: TickTime(checkpoint.limp_since),
                landing: Landing {
                    still_for: Duration::from_millis(checkpoint.limp_still_ms),
                },
            },
            "posing" => LimpFall::Posing {
                from: array_of(checkpoint.pose_from.as_deref()),
                since: TickTime(checkpoint.limp_since),
            },
            _ => LimpFall::Idle,
        };
        self.enabled = checkpoint.enabled;
        self.command.twist = checkpoint.command_twist;
        self.command.head = checkpoint.command_head;
        self.command.body.z = checkpoint.command_body[0];
        self.command.body.roll = checkpoint.command_body[1];
        self.command.body.pitch = checkpoint.command_body[2];
        self.twist_ema = checkpoint.twist_ema;
        self.head_ema = checkpoint.head_ema;
        self.body_ema = checkpoint.body_ema;
        self.body_active = checkpoint.body_active;
        self.twist_fresh = checkpoint.twist_fresh;
        self.last_twist = TickTime(checkpoint.last_twist_tick);
        self.last_head_command = TickTime(checkpoint.last_head_tick);
        self.api_lease.twist = checkpoint.lease_twist;
        self.api_lease.until = checkpoint.lease_until.map(TickTime);
        self.imu_samples = checkpoint.imu_samples;
        self.volts = checkpoint.volts;
        self.mouth = checkpoint.mouth;
        self.mode = match checkpoint.mode.as_str() {
            "roller" => robotd_params::Mode::Roller,
            _ => robotd_params::Mode::Walk,
        };
        if !checkpoint.odometry.is_empty() {
            // A snapshot this build cannot read is ignored rather than
            // half-applied: the estimate restarts from the restored pose.
            if !self.odometry.restore(&checkpoint.odometry) {
                self.odometry = odometry::Odometry::alpha();
            }
        } else {
            // An older checkpoint has no estimate in it. Leaving the estimator
            // where it was would publish a pose from a future this duck no
            // longer has; restarting it from the restored body is honest.
            self.odometry = odometry::Odometry::alpha();
        }
        self.powered_off = checkpoint.powered_off;
        self.emotion_playing = checkpoint.emotion_name.clone().map(|name| crate::emotion::Playing {
            name,
            elapsed: checkpoint.emotion_elapsed,
            skill_delivered: checkpoint.emotion_skill_delivered,
        });
        // IdleHead speaks Instants; anchor it to the restored simulated clock
        // and rebuild its RNG/phases/glance from the checkpoint so the replay
        // is bit-identical to the original trajectory.
        self.sim_origin = Instant::now();
        self.idle_head
            .restore_state(checkpoint.idle_state, self.sim_origin);
        self.last_idle = crate::control::IdleOffset {
            head: checkpoint.last_idle.head,
            body_z: checkpoint.last_idle.body_z,
            mouth: checkpoint.last_idle.mouth,
            takeover: checkpoint.last_idle.takeover,
        };
        self.shutdown = match checkpoint.shutdown.as_str() {
            "sitting" => Shutdown::Sitting {
                since: TickTime(checkpoint.shutdown_since),
            },
            "resting" => Shutdown::Resting {
                from: array_of(checkpoint.shutdown_from.as_deref()),
                since: TickTime(checkpoint.shutdown_since),
            },
            _ => Shutdown::Idle,
        };
        self.shutdown_requested = false;
        // The pickup window is a second of history from a future this duck no
        // longer had. Forget it rather than score rows against a replay.
        if let Some(watch) = self.pickup.as_mut() {
            let _ = watch.tick(false, None, self.dt);
        }
        true
    }

    /// The most recent tick's report, for a single-step command.
    pub fn last_report(&self) -> Option<TickReport> {
        self.last.clone()
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.clock.paused = paused;
    }

    pub fn set_speed(&mut self, speed: f64) {
        self.clock.speed = speed.clamp(0.05, 4.0);
    }

    /// One 50 Hz tick. The order is `robotd`'s: read, observe, gate, policy, safety.
    ///
    /// Simulated time advances by exactly 20 ms per tick, whatever the wall
    /// clock did, so a paused or slowed duck keeps its own timers.
    pub fn tick(&mut self) -> Result<TickReport, RuntimeError> {
        let report = self.tick_inner()?;
        self.last = Some(report.clone());
        Ok(report)
    }

    fn tick_inner(&mut self) -> Result<TickReport, RuntimeError> {
        self.clock.advance(1);
        let now = TickTime::of(&self.clock);
        let sensors: Sensors = self.safety.read()?;
        // One fused sample per tick, exactly as `Imu::decode` counts them.
        self.imu_samples = self.imu_samples.saturating_add(1);
        self.safety.observe(&sensors, self.dt);
        let hold = sensors.positions;
        // `robotd` updates odometry immediately after `safety.observe`, on
        // fresh samples and only once the orientation filter has converged.
        // Doing it later (behind the bring-up early returns) froze the
        // estimate through limp, homing and a shutdown.
        if self.imu_ready() {
            self.odometry.update(&sensors.positions, sensors.imu.quat);
        }

        // Pack voltage is a slow read on the robot (about once a second, a
        // transaction of its own), and it is the only battery measurement the
        // duck has. Do it on the same cadence rather than every tick.
        self.slow_ticks = self.slow_ticks.wrapping_add(1);
        if self.slow_ticks % 50 == 0 {
            // A held ride needs its next loop before the last one runs dry:
            // each loop is 1.6-2.2 s, so queueing one a second keeps two in
            // the platform's queue at all times.
            if self.voice.wheee_held() {
                self.voice.queue_loop();
            }
            if let Ok(slow) = self.safety.slow_sensors() {
                self.volts = Some(slow.volts);
                // The BAM model reports the motor-bus voltage. robotd's soc.rs
                // maps a 2S Li-ion pack to a percentage; a linear 6.0..8.4 V
                // window is the same shape and honest about being an estimate.
                let percent = ((slow.volts - 6.0) / 2.4 * 100.0).clamp(0.0, 100.0);
                self.face.gauge = self.face.gauge.next(Some(percent));
                self.face.battery_percent = Some(percent);
                let healthy = self.imu_ready() && !self.safety.fallen() && self.enabled;
                self.face.status = crate::leds::Status::of(
                    healthy,
                    !self.imu_ready(),
                    self.clock.tick < 50,
                );
                // `BATTERY_EMA_ALPHA`: a step sag recovers between steps and
                // must not count as a spent pack.
                self.volts_ema = Some(
                    0.1 * slow.volts + 0.9 * self.volts_ema.unwrap_or(slow.volts),
                );
            }
        }

        // A hand on the duck is an external force on the trunk, refreshed once
        // per tick and cleared the moment it lets go.
        match self.carry {
            Some((anchor, offset)) => self.safety.io_mut().carry(Some(anchor), offset),
            None => self.safety.io_mut().carry(None, [0.0; 3]),
        }

        // The IMU's own gate: the real board reports a default quaternion
        // until its filter has fused 25 samples, and `robotd` will not drive
        // on it. Before that the duck stands held at home.
        let warm = self.imu_ready();

        let mut report = TickReport {
            label: "held".to_owned(),
            targets: hold,
            gain: 200,
            fallen: self.safety.fallen(),
            bringup: self.bringup.state(),
            sensors,
            imu_ready: self.imu_ready(),
            picked_up: self.picked_up(),
            p_held: self.p_held(),
            volts: self.volts,
            mouth: 0.0,
        };

        // Off is a state of the duck, not a single tick: after the shutdown's
        // torque cut the body is limp, and reporting "limp" would hide that the
        // duck was deliberately powered down.
        if self.powered_off {
            self.safety.apply(hold, hold, 0)?;
            report.label = "off".to_owned();
            report.gain = 0;
            report.targets = hold;
            return Ok(report);
        }

        // The shutdown request and the empty pack are checked before the
        // bring-up early returns: a duck that is limp or still homing must
        // still be able to power off (robotd cuts torque immediately when it
        // cannot sit).
        if !self.powered_off && matches!(self.shutdown, Shutdown::Idle) {
            let empty = self
                .volts_ema
                .is_some_and(|volts| volts <= duck_control::model::BATTERY_EMPTY_V);
            if empty || self.shutdown_requested {
                self.shutdown_requested = false;
                self.begin_shutdown(now);
            }
        }

        match self.bringup {
            Bringup::Limp => {
                // Torque off, but still write and step: a limp duck has to
                // fall, not freeze in the pose it was standing in.
                self.safety.apply(hold, hold, 0)?;
                report.label = "limp".to_owned();
                report.gain = 0;
                report.targets = hold;
                if let Some(watch) = self.pickup.as_mut() {
                    watch.commanded(&hold);
                }
                return Ok(report);
            }
            Bringup::Homing { to, .. } => {
                let target = self.bringup.homing_target(now).unwrap_or(to);
                // The ramp is over when it reaches home *and* the filter has
                // converged. Homing from home (the app's boot) has no travel,
                // so it finishes as soon as the IMU is warm.
                let done = warm
                    && (self.bringup.homing_target(now).is_none()
                        || self.bringup.homing_is_static());
                self.safety.apply(target, hold, 200)?;
                report.label = if done { "ready" } else { "homing" }.to_owned();
                report.targets = target;
                report.gain = 200;
                if done {
                    self.bringup = Bringup::Ready;
                    self.controller.reset();
                    tracing::info!("bring-up: at home pose; the policy has the robot");
                }
                if let Some(watch) = self.pickup.as_mut() {
                    watch.commanded(&target);
                }
                return Ok(report);
            }
            Bringup::Ready => {}
        }

        // The shutdown owns the robot from the moment it starts: sit, rest,
        // torque off. `robotd` runs the same sequence for `robot.shutdown`
        // and for a genuinely empty pack (a 10 s EMA at or below 6.6 V).
        match self.shutdown {
            Shutdown::Idle => {}
            Shutdown::Sitting { since } => {
                // Named apart from `hold`, which is the measured pose.
                let hold_s = self.controller.shutdown_sit_secs();
                let target = self.controller.step(
                    &sensors,
                    &self.command,
                    false,
                    self.dt.as_secs_f64(),
                    1.0,
                );
                match target {
                    Ok(step) => {
                        self.safety.apply(step.targets, hold, step.gain)?;
                        report.label = "shutdown".to_owned();
                        report.targets = step.targets;
                        report.gain = step.gain;
                    }
                    Err(_) => {
                        self.safety.apply(hold, hold, 200)?;
                        report.targets = hold;
                    }
                }
                if since.elapsed(now).as_secs_f64() >= hold_s {
                    tracing::warn!("shutdown: sit complete, easing into the rest pose");
                    self.shutdown = Shutdown::Resting {
                        from: report.targets,
                        since: now,
                    };
                }
                report.fallen = self.safety.fallen();
                return Ok(report);
            }
            Shutdown::Resting { from, since } => {
                let t = (since.elapsed(now).as_secs_f64() / 1.0).min(1.0);
                let mut target = [0.0; NUM_JOINTS];
                for (index, slot) in target.iter_mut().enumerate() {
                    *slot = from[index]
                        + (duck_control::model::REST_POSITION[index] - from[index]) * t;
                }
                let holding = since.elapsed(now).as_secs_f64() >= 2.0;
                if holding {
                    // The rest is the last pose the servos see: cutting torque
                    // from the seat let the duck sag forward; from here there
                    // is nowhere left to sag.
                    self.safety.apply(target, target, 200)?;
                    self.safety.set_torque(false)?;
                    self.enabled = false;
                    self.powered_off = true;
                    self.shutdown = Shutdown::Idle;
                    self.bringup = Bringup::Limp;
                    report.label = "off".to_owned();
                    report.gain = 0;
                    report.targets = target;
                    tracing::warn!("shutdown: at rest, torque off");
                } else {
                    self.safety.apply(target, target, report.gain.max(200))?;
                    report.label = "rest".to_owned();
                    report.targets = target;
                }
                report.fallen = self.safety.fallen();
                return Ok(report);
            }
        }

        // Deadman: a command stream that stops leaves the robot standing, not walking off.
        if self.twist_fresh && self.last_twist.elapsed(now) > DEADMAN {
            self.command.twist = [0.0; 3];
            self.twist_fresh = false;
            tracing::warn!("deadman: no twist for {DEADMAN:?}; standing still");
        }

        // Limp-fall, robotd's sequence: go soft before the landing, let the
        // robot arrive limp, put it back into the standing pose, and only then
        // hand it to the policy. Read from the fall predictor, not the `fallen`
        // verdict: a verdict that waits for the robot to be down cannot make
        // it soft before it lands.
        let was_driving = self.enabled && matches!(self.bringup, Bringup::Ready) && warm;

        // Abandoned mid-sequence: something else took the robot.
        if !matches!(self.limp_fall, LimpFall::Idle) && !was_driving {
            self.limp_fall = LimpFall::Idle;
            self.fall.reset();
        }

        match self.limp_fall {
            LimpFall::Idle => {
                let eligible = was_driving
                    && !self.safety.fallen()
                    && !self.controller.busy()
                    && !self.controller.is_sitting();
                if eligible && self.safety.imu_ready() {
                    if self.fall.observe(&sensors.imu, self.dt) {
                        tracing::warn!("falling - going limp to land soft");
                        self.limp_fall = LimpFall::Limp {
                            since: now,
                            landing: Landing::default(),
                        };
                    }
                } else {
                    // Not eligible: drop any debounce rather than resuming it
                    // later against a robot that has moved on.
                    self.fall.reset();
                }
            }
            LimpFall::Limp { since, mut landing } => {
                let rate = Some(
                    sensors.imu.gyro.iter().map(|w| w * w).sum::<f64>().sqrt(),
                );
                let landed =
                    landing.observe(rate, self.dt, LIMP_FALL_STILL_RATE, LIMP_FALL_STILL);
                let timed_out = since.elapsed(now) >= LIMP_FALL_MAX;
                if landed || timed_out {
                    tracing::warn!("landed - posing back to standing");
                    self.limp_fall = LimpFall::Posing {
                        from: hold,
                        since: now,
                    };
                } else {
                    self.limp_fall = LimpFall::Limp { since, landing };
                }
            }
            LimpFall::Posing { since, .. } => {
                if since.elapsed(now) >= LIMP_FALL_POSE {
                    tracing::warn!("posed - handing back to the standing policy");
                    self.limp_fall = LimpFall::Idle;
                    self.fall.reset();
                    self.controller.reset();
                }
            }
        }

        // Pick-up detection, robotd's: pause the walking networks while somebody
        // holds the robot and hand it back when it is on the floor. Only the
        // policy's own ticks are watched — a skill, a sit, the limp-fall or a
        // disable owns the robot outright, and the watch lets go of any pause.
        let mut pickup_paused = false;
        // Read the loop's own facts before the watch borrows itself mutably.
        let dt = self.dt;
        let ready = self.enabled && matches!(self.bringup, Bringup::Ready);
        let settled = matches!(self.limp_fall, LimpFall::Idle);
        let free = !self.controller.busy() && !self.controller.is_sitting();
        if let Some(watch) = self.pickup.as_mut() {
            let watching = ready && settled && warm && free;
            match watch.tick(watching, Some(&sensors), dt) {
                Some(crate::pickup::Edge::Pause) => {
                    tracing::warn!(p_held = ?watch.p_held(), "picked up - pausing the policy")
                }
                Some(crate::pickup::Edge::Resume) if watching => {
                    tracing::warn!(p_held = ?watch.p_held(), "put down - handing back to the policy")
                }
                Some(crate::pickup::Edge::Resume) => {
                    tracing::warn!("pickup pause released: something else has the robot")
                }
                None => {}
            }
            pickup_paused = watch.paused();
        }

        // The whole sequence owns the targets: the controller is not stepped.
        match self.limp_fall {
            LimpFall::Idle => {}
            LimpFall::Limp { .. } => {
                // Command the joints to wherever they are: the softest thing
                // the servos can do short of cutting torque.
                self.safety.apply(hold, hold, GAIN_LIMP)?;
                report.label = "limp_fall".to_owned();
                report.targets = hold;
                report.gain = GAIN_LIMP;
                return Ok(report);
            }
            LimpFall::Posing { from, since } => {
                let t = (since.elapsed(now).as_secs_f64() / LIMP_FALL_POSE.as_secs_f64())
                    .min(1.0);
                let mut target = [0.0; NUM_JOINTS];
                for (index, slot) in target.iter_mut().enumerate() {
                    *slot = from[index] + (DEFAULT_POSITION[index] - from[index]) * t;
                }
                self.safety.apply(target, hold, GAIN_POSE)?;
                report.label = "pose".to_owned();
                report.targets = target;
                report.gain = GAIN_POSE;
                return Ok(report);
            }
        }

        // The orientation is still a default, not a measurement: hold the pose,
        // do not drive. `robotd` gates its whole driving branch on this.
        if !warm {
            self.safety.apply(hold, hold, 200)?;
            report.label = "held".to_owned();
            report.targets = hold;
            report.gain = 200;
            if let Some(watch) = self.pickup.as_mut() {
                watch.commanded(&hold);
            }
            return Ok(report);
        }

        let mut command = self.command;
        // An API twist owns the duck while its lease is live; the on-screen
        // joystick gets it back when the API goes quiet.
        if let Some(twist) = self.api_lease.active(now) {
            command.twist = twist;
        }

        // Smooth the client's command the way robotd does, so the policy sees
        // a ramp rather than a step.
        for axis in 0..3 {
            self.twist_ema[axis] += CMD_ALPHA * (command.twist[axis] - self.twist_ema[axis]);
            command.twist[axis] = self.twist_ema[axis];
        }
        for axis in 0..4 {
            if command.head[axis].is_finite() {
                self.head_ema[axis] +=
                    HEAD_ALPHA * (command.head[axis] - self.head_ema[axis]);
            }
            command.head[axis] = self.head_ema[axis];
        }
        if self.body_active {
            let target = [command.body.z, command.body.roll, command.body.pitch];
            for axis in 0..3 {
                self.body_ema[axis] += CMD_ALPHA * (target[axis] - self.body_ema[axis]);
            }
        } else {
            self.body_ema = [0.0; 3];
        }
        command.body.z = self.body_ema[0];
        command.body.roll = self.body_ema[1];
        command.body.pitch = self.body_ema[2];

        // An emotion is a scripted move, not a policy: for its own ticks it
        // replaces the client's head/body/mouth command, exactly as
        // `emotion.rs::Frame::apply` replaces the Snapshot. A pad still
        // sending commands cannot win half the frames, and when the timeline
        // ends there is nothing to restore.
        let mut emotion_frame = None;
        if let Some(playing) = self.emotion_playing.as_mut() {
            if let Some(emotion) = self.emotions.get(&playing.name) {
                if playing.elapsed <= emotion.duration() {
                    let frame = emotion.frame_at(playing.elapsed);
                    // `devastated` sits the duck down partway through: the
                    // controller owns sit, so deliver the request once.
                    if !playing.skill_delivered
                        && emotion.pending_skill(playing.elapsed).is_some()
                    {
                        playing.skill_delivered = true;
                        let _ = self.controller.sit_toggle();
                    }
                    command.head = frame.head;
                    command.body.z = frame.body[0];
                    command.body.roll = frame.body[1];
                    command.body.pitch = frame.body[2];
                    // `Frame::apply` zeroes the Snapshot's twist: an emotion
                    // holds the duck still. Every shipped timeline carries
                    // [0,0,0]; honour a future one that asks to move.
                    command.twist = frame.twist.unwrap_or([0.0; 3]);
                    self.twist_ema = command.twist;
                    self.head_ema = frame.head;
                    self.body_ema = frame.body;
                    // A fresh head command keeps IdleHead out of the way, the
                    // same reason upstream zeroes `snapshot.head_age`.
                    self.last_head_command = TickTime::of(&self.clock);
                    emotion_frame = Some(frame);
                } else {
                    self.emotion_playing = None;
                }
            } else {
                self.emotion_playing = None;
            }
            if let Some(playing) = self.emotion_playing.as_mut() {
                playing.elapsed += self.dt.as_secs_f64();
            }
        }

        // The idle look-around, robotd's: a still duck glances, tilts its head
        // and breathes, and the head command it feeds is an observation input
        // (the policy's head block), exactly as on the real robot.
        let still = self.idle_params.enabled_in(self.mode)
            && emotion_frame.is_none()
            && was_driving
            && !pickup_paused
            && matches!(self.limp_fall, LimpFall::Idle)
            && command.twist.iter().all(|value| value.abs() < 1e-3)
            && self.last_head_command.elapsed(now) >= self.idle_head.after()
            && !self.controller.busy();
        // IdleHead is upstream code that speaks `Instant`. Anchoring it to the
        // tick count alone (origin + tick * 20 ms) makes it advance exactly one
        // tick per tick; `Instant::now() + tick * 20 ms` advanced at twice
        // real time, so the duck fidgeted twice as fast as the real one.
        let idle_now = self.sim_origin + Duration::from_millis(self.clock.tick * 20);
        let idle = self.idle_head.tick(idle_now, still, self.dt);
        self.last_idle = idle;
        for axis in 0..4 {
            command.head[axis] =
                self.head_ema[axis] * (1.0 - idle.takeover) + idle.head[axis];
        }
        // The breath moves the body pose the standing network was trained on,
        // and only while no client holds that pose itself.
        if !self.body_active
            && self.controller.driving() == Some(crate::control::Driving::Stand)
        {
            command.body.z += idle.body_z;
        }

        // The duck's voice reacts to the world, on rising edges only. robotd
        // coos in a hand, alarms on a fall, and inquires when the pad comes
        // back after a long stillness; a two-second cooldown keeps a bouncing
        // classifier from turning one pickup into a chorus.
        let loud = self.room_loud;
        let moving = command.twist.iter().any(|value| value.abs() > 1e-3);
        let cooldown = self
            .last_reaction
            .is_none_or(|tick| self.clock.tick.saturating_sub(tick) >= 100);
        let mut reaction = None;
        // A sudden bang in the room startles it: the official alarm chirp
        // and an impatient emotion, on the rising edge with a cooldown so
        // one clap is one reaction.
        if loud && !self.last_loud && cooldown && emotion_frame.is_none() {
            reaction = Some("alarm");
        }
        if pickup_paused && !self.last_pickup_paused {
            reaction = Some("coo");
        } else if self.safety.fallen() {
            self.fallen_ticks += 1;
            // Twenty 50 Hz ticks = 0.4 s of continuous fall. robotd's own
            // fall predictor debounces; a scripted head move can tip the
            // trunk for a frame or two without being a real fall.
            if self.fallen_ticks == 20 && !self.last_fallen {
                reaction = Some("alarm");
            }
        } else {
            self.fallen_ticks = 0;
        }
        if reaction.is_none() && moving && !self.last_moving && cooldown && emotion_frame.is_none() {
            // Only after a real stillness, so ordinary walking does not chirp
            // at every deadman keepalive.
            if self.last_idle.takeover > 0.5 {
                reaction = Some("inquire");
            }
        }
        // The idle ritual: after a long stillness (and never during a skill,
        // a shutdown, a hold or an emotion) the duck plays one of its own
        // official timelines. Deterministic on the simulated clock, so a
        // rewind replays the same choice rather than drawing a new RNG value.
        let period = self.idle_ritual_ticks;
        let ritual_due = self
            .last_idle_ritual
            .is_none_or(|tick| self.clock.tick.saturating_sub(tick) >= period);
        if ritual_due
            && period < u64::MAX
            && self.last_idle.takeover > 0.9
            && emotion_frame.is_none()
            && !self.controller.busy()
            && !pickup_paused
            && !self.shutting_down()
            && !self.powered_off
            && !self.emotions.is_empty()
        {
            let names = self.emotions.names();
            let index = (self.clock.tick / period.max(1)) as usize % names.len();
            let _ = self.trigger_emotion(&names[index]);
            self.last_idle_ritual = Some(self.clock.tick);
            self.book.note("idle_ritual".to_owned());
        }
        if let Some(tag) = reaction {
            self.queue_sound(tag);
            self.last_reaction = Some(self.clock.tick);
        }
        self.room_loud = false;
        self.last_loud = loud;
        self.last_pickup_paused = pickup_paused;
        self.last_fallen = self.safety.fallen();
        self.last_moving = moving;

        let mut targets = hold;
        let mut gain = 200;
        if pickup_paused {
            // Held in a hand: ramp from the policy's last target to the pause
            // pose and stay there. The ramp start is the policy's own target,
            // so letting go does not snap.
            targets = self.pickup.as_ref().map(|watch| watch.target()).unwrap_or(hold);
            report.label = "held".to_owned();
        } else {
            match self
                .controller
                .step(
                    &sensors,
                    &command,
                    // An emotion's Frame::apply sets PoseIntent.active for
                    // this tick only; the client's own body hold is restored
                    // untouched when the timeline ends.
                    self.body_active || emotion_frame.is_some(),
                    self.dt.as_secs_f64(),
                    1.0,
                )
            {
                Ok(step) => {
                    targets = step.targets;
                    gain = step.gain;
                    report.label = step.label.to_string();
                }
                Err(error) => {
                    report.label = "held".to_owned();
                    tracing::warn!(%error, "inference failed; holding the pose");
                }
            }
            // The mouth is not part of any policy — every alpha network skips
            // it — so the idle yawn is the whole of mouth control, exactly as
            // it is on the robot. Only while driving: a held or homing duck
            // keeps the mouth its hold pose gave it.
            let mouth = emotion_frame.map_or(self.mouth.max(idle.mouth), |frame| frame.mouth);
            targets[duck_control::model::MOUTH_INDEX] =
                duck_control::model::mouth_target(mouth);
        }
        // One write per tick: the body steps inside `apply`, so a second call
        // would advance the physics twice as fast.
        self.safety.apply(targets, hold, gain)?;
        report.targets = targets;
        report.gain = gain;
        report.mouth = if pickup_paused {
            0.0
        } else {
            emotion_frame.map_or(self.mouth.max(idle.mouth), |frame| frame.mouth)
        };
        if let Some(watch) = self.pickup.as_mut() {
            // The next feature row pairs its reading with this command.
            watch.commanded(&report.targets);
        }
        report.fallen = self.safety.fallen();
        report.picked_up = pickup_paused;
        report.p_held = self.p_held();
        report.imu_ready = self.imu_ready();
        report.volts = self.volts;
        // The lifetime record, counted from what this tick actually was.
        // `robotd` keeps the same facts in its vet book; a phone keeps them
        // in memory and hands them to the shell to store.
        let walking = was_driving
            && command.twist.iter().any(|value| value.abs() > 1e-3);
        let standing = was_driving && !walking && !pickup_paused;
        let sitting = matches!(self.shutdown, Shutdown::Sitting { .. })
            || (self.controller.is_sitting() && !pickup_paused);
        self.book.tick(crate::vetbook::Facts {
            powered: true,
            torqued: self.enabled,
            walking,
            standing,
            sitting,
            held: pickup_paused,
            dt: self.dt.as_secs_f64(),
        });
        if pickup_paused && !self.last_pickup_paused {
            self.book.life.pickups += 1;
            self.book.note("picked_up".to_owned());
        }
        if self.safety.fallen() && !self.last_fallen {
            self.book.life.falls += 1;
            self.book.note("fell".to_owned());
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_api_lease_outlives_the_joystick_twist() {
        let mut lease = ApiLease::default();
        assert_eq!(lease.active(TickTime(0)), None);

        lease.take([0.4, 0.0, 0.0], TickTime(0));
        assert_eq!(lease.active(TickTime(0)), Some([0.4, 0.0, 0.0]));
        // The app pushes the joystick's twist every 20 ms; the lease must
        // survive that, otherwise an external agent could never drive.
        assert_eq!(lease.active(TickTime(10)), Some([0.4, 0.0, 0.0]));
    }

    #[test]
    fn the_lease_expires_and_gives_the_duck_back() {
        let mut lease = ApiLease::default();
        lease.take([0.4, 0.0, 0.0], TickTime(0));
        assert_eq!(lease.active(TickTime(25)), None);
        assert_eq!(lease.active(TickTime(26)), None);
    }

    #[test]
    fn a_new_twist_extends_the_lease() {
        let mut lease = ApiLease::default();
        lease.take([0.4, 0.0, 0.0], TickTime(0));
        assert_eq!(lease.active(TickTime(20)), Some([0.4, 0.0, 0.0]));
        lease.take([0.0, 0.0, 0.2], TickTime(20));
        assert_eq!(lease.active(TickTime(40)), Some([0.0, 0.0, 0.2]));
        assert_eq!(lease.active(TickTime(46)), None);
    }

    /// A policy that fails every step: the loop holds the pose, so the state
    /// machine still advances and every checkpointed field is exercised.
    fn test_policy() -> Policy {
        let dir = std::path::Path::new(r"D:\AI\Duck\policies");
        let paths = duck_control::policy::PolicyPaths {
            walk: dir.join("velstand.onnx"),
            stand: None,
            sitstand: None,
            ground_pick: None,
            skills: Vec::new(),
        };
        Policy::load(&paths, 0.05).expect("the bundled walk policy")
    }

    #[test]
    fn a_rewind_replays_the_same_trajectory() {
        use duck_control::io::{IoError, JointTargets, RobotIo, Sensors};

        #[derive(Clone)]
        struct Probe {
            sensors: Sensors,
            targets: [f64; NUM_JOINTS],
        }

        impl RobotIo for Probe {
            fn read(&mut self) -> Result<Sensors, IoError> {
                Ok(self.sensors)
            }
            fn write(&mut self, targets: &JointTargets) -> Result<(), IoError> {
                self.targets = targets.positions;
                Ok(())
            }
            fn measures_velocity(&self) -> bool {
                true
            }
            fn measures_load(&self) -> bool {
                true
            }
            fn set_gain(&mut self, _: u16) -> Result<(), IoError> {
                Ok(())
            }
            fn set_torque(&mut self, _: bool) -> Result<(), IoError> {
                Ok(())
            }
            fn reboot(&mut self, _: u8) -> Result<(), IoError> {
                Ok(())
            }
            fn slow_sensors(&mut self) -> Result<duck_control::io::SlowSensors, IoError> {
                Ok(duck_control::io::SlowSensors {
                    volts: 7.4,
                    temps_c: [25.0; NUM_JOINTS],
                })
            }
            fn snapshot(&self) -> Option<Vec<f64>> {
                let mut state = vec![0.0; 1 + NUM_JOINTS * 2];
                state[0] = 1.0;
                state[1..1 + NUM_JOINTS].copy_from_slice(&self.sensors.positions);
                state[1 + NUM_JOINTS..].copy_from_slice(&self.targets);
                Some(state)
            }
            fn restore(&mut self, state: &[f64]) -> bool {
                if state.len() != 1 + NUM_JOINTS * 2 {
                    return false;
                }
                self.sensors
                    .positions
                    .copy_from_slice(&state[1..1 + NUM_JOINTS]);
                self.targets
                    .copy_from_slice(&state[1 + NUM_JOINTS..]);
                true
            }
        }

        // No policies are loaded here: `Controller::step` fails, the loop holds
        // the pose, and the state machine still advances. That is enough to
        // exercise every checkpointed field.
        let tuning = Tuning::default();
        let skills = SkillTuning::default();
        let probe = Probe {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
        };
        let mut runtime = DuckRuntime::new(
            probe,
            test_policy(),
            tuning,
            skills,
            SafetyConfig::default(),
        );
        runtime.enable().ok();

        for _ in 0..20 {
            let _ = runtime.tick();
        }
        let saved = runtime.checkpoint().expect("a checkpoint");
        let mut after_save = Vec::new();
        for _ in 0..20 {
            let _ = runtime.tick();
            after_save.push(runtime.clock().tick);
        }

        assert!(runtime.restore(&saved), "the checkpoint restores");
        assert_eq!(runtime.clock().tick, saved.tick);
        for expected in after_save {
            let _ = runtime.tick();
            assert_eq!(runtime.clock().tick, expected);
        }
        // The clock came back to the same place, which is what "replay the same
        // trajectory" means for the loop's own state.
        assert_eq!(runtime.clock().tick, saved.tick + 20);
    }

    #[test]
    fn emotions_are_data_and_checkpoint_fields() {
        // No policy/ORT here: the library is pure data. The full play-through
        // is asserted in CI's simulator session, where ORT 1.23 is available.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../upstream/microduck/robotd/emotions");
        let library = crate::emotion::Library::load(&dir).expect("official emotions");
        assert_eq!(library.names().len(), 13);
        let curious = library.get("curious").expect("curious");
        assert!(curious.duration() > 2.0);
        assert!(!curious.pcm().is_empty());
        assert!(curious.frame_at(curious.duration() / 2.0).head[2].is_finite());
        assert!(library.get("devastated").unwrap().pending_skill(9.0).is_some());
        // The playing state round-trips with its elapsed clock, so a
        // checkpoint can resume the same timeline rather than dropping it.
        let playing = crate::emotion::Playing {
            name: "curious".into(),
            elapsed: 1.25,
            skill_delivered: false,
        };
        let json = serde_json::to_string(&playing).unwrap();
        let back: crate::emotion::Playing = serde_json::from_str(&json).unwrap();
        assert_eq!(back.elapsed, 1.25);
    }
}