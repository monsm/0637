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

#[cfg(feature = "mujoco")]
use crate::mujoco::MujocoIo;
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

/// How long a painted route may be, in kept points and in metres end to
/// end. The route layer refuses anything over these without queueing it, so
/// a client learns its drawing is over budget instead of watching half of it
/// walk.
pub const PATH_MAX_POINTS: usize = 64;
pub const PATH_MAX_LENGTH: f64 = 20.0;

/// Points closer than this to the previous kept point are the same point:
/// the painter's own jitter, not a waypoint.
const PATH_DEDUPE: f64 = 0.1;

/// How close the trunk has to get to a waypoint for it to count as reached.
const PATH_REACH: f64 = 0.15;
/// Cruise speed, and the distance at which a waypoint stops slowing the
/// duck down. The throttle is `PATH_SPEED * clamp(dist / PATH_APPROACH,
/// 0.2..1.0)`, so the last stretch of a segment coasts instead of lunges.
const PATH_SPEED: f64 = 0.35;
const PATH_APPROACH: f64 = 0.4;
/// Heading-error gain and its symmetric bound. A waypoint on the duck's
/// left is a positive error and a positive `wz` is a left turn, the same
/// frame the joystick and the policy use.
const PATH_YAW_GAIN: f64 = 1.5;
const PATH_MAX_WZ: f64 = 1.2;
/// Past this heading error the duck turns in place before walking: lunging
/// at a waypoint behind it is how a biped falls over.
const PATH_TURN_FIRST: f64 = 1.1;

/// A chorus is a shared moment: every duck waits 1.5 s after hearing the
/// packet (or pressing the chip) so the room lands together, and each move
/// follows the previous one 1.5 s later.
const CHORUS_START_TICKS: u64 = 75;
const CHORUS_MOVE_TICKS: u64 = 75;

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

/// One painted route being walked: the points as accepted (they are what
/// `/state.path` publishes) and those same points frozen into the world at
/// the pose the walk started from.
#[derive(Debug, Clone)]
struct PathTask {
    /// The accepted body-frame points, exactly as posted.
    points: Vec<[f32; 2]>,
    /// The same points in the world frame: `start_xy + R(yaw0) * point`.
    world: Vec<[f64; 2]>,
    /// Index of the next waypoint to reach.
    next: usize,
}

impl PathTask {
    /// Freeze the accepted body-frame points into the world. `pose` is the
    /// duck at the route's start: each point is metres in the trunk frame
    /// then — +x forward, +y left.
    fn start(points: Vec<[f32; 2]>, [x, y, yaw]: [f64; 3]) -> Self {
        let (sin, cos) = yaw.sin_cos();
        let world = points
            .iter()
            .map(|[px, py]| {
                let (px, py) = (f64::from(*px), f64::from(*py));
                [x + cos * px - sin * py, y + sin * px + cos * py]
            })
            .collect();
        Self { points, world, next: 0 }
    }

    /// One tick of following, from the duck's pose: step the cursor past
    /// anything already reached and turn the next waypoint into a twist.
    ///
    /// The heading error is `atan2(left, forward)` of the waypoint in the
    /// body frame, already normalized to [-pi, pi], so a waypoint on the
    /// duck's left is a positive error and a positive `wz` is a left turn —
    /// the same frame the joystick and the policy use.
    fn drive(&mut self, [x, y, yaw]: [f64; 3]) -> PathDrive {
        let (sin, cos) = yaw.sin_cos();
        while let Some(target) = self.world.get(self.next) {
            let dx = target[0] - x;
            let dy = target[1] - y;
            let forward = cos * dx + sin * dy;
            let left = -sin * dx + cos * dy;
            let dist = forward.hypot(left);
            if dist <= PATH_REACH {
                self.next += 1;
                continue;
            }
            let error = left.atan2(forward);
            let wz = (PATH_YAW_GAIN * error).clamp(-PATH_MAX_WZ, PATH_MAX_WZ);
            // Turn in place first: walking towards a waypoint behind the
            // duck would be walking backwards at it.
            let vx = if error.abs() > PATH_TURN_FIRST {
                0.0
            } else {
                PATH_SPEED * (dist / PATH_APPROACH).clamp(0.2, 1.0)
            };
            return PathDrive::Drive([vx, 0.0, wz]);
        }
        PathDrive::Done
    }

    /// The `/state.path` object for a live route.
    fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "active": true,
            "reached": self.next,
            "total": self.points.len(),
            "points": self.points,
        })
    }
}

/// What one tick of a route asks of the duck.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PathDrive {
    /// Still walking: the twist for this tick.
    Drive([f64; 3]),
    /// The last waypoint is reached: the task is over.
    Done,
}

/// Whether a client's twist is a hand on the stick rather than the shells'
/// 50 Hz zero keepalive. A directed twist takes the duck back from a painted
/// route; `set_twist` and `set_api_twist` both apply this rule.
fn steers(twist: [f64; 3]) -> bool {
    twist.iter().any(|value| value.abs() > 1e-3)
}

/// One chorus being performed: the same official moves as every other duck
/// in the room, each landing on its own tick. The lead and the joiners share
/// this task; only the role differs in what /state reports.
#[derive(Debug, Clone)]
struct ChorusTask {
    role: &'static str,
    moves: Vec<String>,
    start_tick: u64,
    next_move: usize,
    /// The honest room size at the moment the chorus started.
    peers: u32,
    /// Sound tags interleaved with the moves: one beat may be a voice instead
    /// of a mood. Empty for the classic chorus.
    sounds: Vec<String>,
    /// The vet book note this performance writes when it finishes.
    note: &'static str,
}

impl ChorusTask {
    fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "active": true,
            "role": self.role,
            "moves": self.moves,
            "started_tick": self.start_tick as u32,
            "peers": self.peers,
            "sounds": self.sounds,
        })
    }
}

/// The body-frame points of a `/path` request after the wire's own rules:
/// the duck's origin goes, points under [`PATH_DEDUPE`] from the previous
/// kept point go, and what is left has to fit the route caps. The route
/// layer calls this before an intent is ever queued.
pub(crate) fn path_points(value: &serde_json::Value) -> Result<Vec<[f32; 2]>, String> {
    let Some(items) = value.get("points").and_then(serde_json::Value::as_array) else {
        return Err("missing points".to_owned());
    };
    let mut kept: Vec<[f64; 2]> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let (Some(x), Some(y)) = (
            item.get(0).and_then(serde_json::Value::as_f64),
            item.get(1).and_then(serde_json::Value::as_f64),
        ) else {
            return Err("points must be [x, y] pairs of metres".to_owned());
        };
        if !x.is_finite() || !y.is_finite() {
            return Err("points must be finite".to_owned());
        }
        // The first element is the duck's own origin: that is where the body
        // frame was defined, not somewhere to walk to.
        if index == 0 {
            continue;
        }
        if kept
            .last()
            .is_some_and(|last| (x - last[0]).hypot(y - last[1]) < PATH_DEDUPE)
        {
            continue;
        }
        kept.push([x, y]);
    }
    if kept.len() < 2 {
        return Err("a path needs at least two points".to_owned());
    }
    if kept.len() > PATH_MAX_POINTS {
        return Err(format!("a path may keep at most {PATH_MAX_POINTS} points"));
    }
    let mut length = 0.0;
    for pair in kept.windows(2) {
        length += (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
    }
    if length > PATH_MAX_LENGTH {
        return Err(format!("a path may be at most {PATH_MAX_LENGTH} m long"));
    }
    Ok(kept.into_iter().map(|[x, y]| [x as f32, y as f32]).collect())
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
    /// The companion state at this tick. A rewind must bring the tummy, the
    /// mood and the doze back with the body: otherwise a forked duck inherits
    /// a future — or a past — that belongs to another branch.
    #[serde(default)]
    pub care: Option<crate::care::Care>,
    #[serde(default)]
    pub last_sad: Option<u64>,
    /// The last room sound answered at this tick, for the same reason: a
    /// rewind should not carry a clap from a future the duck no longer has.
    #[serde(default)]
    pub last_sound_event: Option<(String, u64)>,
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
    /// The painted route being walked, if any. It drives through the same
    /// command channel as the stick, so the real walk policy executes it and
    /// a fall is still possible — that is the point.
    path_task: Option<PathTask>,
    /// The chorus being performed, if any, and the id of the last one heard
    /// so a bounced packet cannot restart the same song.
    chorus: Option<ChorusTask>,
    last_chorus_id: Option<String>,
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
    /// The companion layer: hunger, mood and bond. It only decides when the
    /// duck shows one of its own emotions (sad after neglect, a doze when
    /// left alone); it never filters the policies or the safety layer.
    care: crate::care::Care,
    /// How fast the care clock runs against the simulated clock. 1.0 is real
    /// time; CI runs it faster to reach a doze without waiting twelve minutes.
    care_scale: f64,
    /// Last time the duck showed sadness, so a low mood is one scene a
    /// minute rather than a loop.
    last_sad: Option<u64>,
    /// A short break after waking: being petted awake must produce a coo,
    /// not immediately the sad timeline because the mood is still low.
    sad_grace_until: Option<u64>,
    /// The last room sound the duck answered: ("clap"/"whistle", tick).
    last_sound_event: Option<(String, u64)>,
    /// The child's block program. Configuration, not simulation state: a
    /// rewind replays the duck under the same rules it has now.
    rules: crate::rules::Rules,
    /// The last LAN greeting or reply, for the peers sheet and /state.
    last_peer_event: Option<serde_json::Value>,
    /// The odometry position the vet book last counted, for metres walked.
    /// A rewind or mode switch can jump the estimate; a step that large is a
    /// teleport, not walking.
    book_position: Option<[f64; 3]>,
    /// The last words a person said to the duck, and the tick. A user's own
    /// model reads this with /state to decide what to do next.
    last_heard: Option<(String, u64)>,
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
    /// Whether a ground pick was running last tick, so the book counts the
    /// start of each peck exactly once.
    last_ground_pick: bool,
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

#[cfg(feature = "mujoco")]
impl DuckRuntime<MujocoIo> {
    /// The AR floor (SENSORS_PLAN S2): only the MuJoCo body can move its
    /// ground, so the typed impl is where the intent lands. Refused on a
    /// body without a plane geom.
    pub fn set_floor_height(&mut self, z: f64) -> Result<bool, RuntimeError> {
        Ok(self.safety.io_mut().set_floor_z(z).is_some())
    }
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
            path_task: None,
            chorus: None,
            last_chorus_id: None,
            twist_ema: [0.0; 3],
            head_ema: [0.0; 4],
            body_ema: [0.0; 3],
            body_active: false,
            idle_head: crate::control::IdleHead::new(robotd_params::IdleParams::default(), 0xD0C),
            idle_params: robotd_params::IdleParams::default(),
            last_idle: crate::control::IdleOffset::default(),
            book: {
                let mut book = crate::vetbook::Book::default();
                // This launch is a boot; a shell restore adds its saved count
                // on top instead of replacing this one.
                book.life.boots = 1;
                book
            },
            care: crate::care::Care::default(),
            care_scale: 1.0,
            last_sad: None,
            sad_grace_until: None,
            last_sound_event: None,
            rules: crate::rules::Rules::default(),
            last_peer_event: None,
            book_position: None,
            last_heard: None,
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
            last_ground_pick: false,
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
                // A hand picking it up is attention, and the duck answers a
                // hand with its coo. The pickup classifier coos too when it
                // confirms the lift; a second coo replaces the first, so the
                // moment never turns into a chorus.
                self.care.touch(self.clock.tick);
                self.queue_sound("coo");
            }
        }
    }

    /// The companion layer follows the animal, not the body: a drive-mode
    /// switch replaces the MuJoCo scene and the policies but must not reset
    /// hunger, the vet book or the child's block program.
    pub fn carry_companion(&mut self, from: &Self) {
        self.care = from.care;
        self.book = from.book.clone();
        self.rules = from.rules.clone();
        self.last_sad = from.last_sad;
        self.sad_grace_until = from.sad_grace_until;
        self.last_sound_event = from.last_sound_event.clone();
        self.last_peer_event = from.last_peer_event.clone();
        self.last_heard = from.last_heard.clone();
        self.book_position = None;
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
            self.book.life.power_offs += 1;
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
        // A stick with a direction is attention; the shells' 50 Hz zero
        // keepalive is not, or the duck could never doze while the app is open.
        if steers(twist) {
            // A hand on the stick takes the duck back: the painted path must
            // not fight the child's hand.
            self.cancel_path();
            self.care.touch(self.clock.tick);
        }
        self.feed_twist(twist, TickTime::of(&self.clock));
    }

    /// Put a twist on the policy's command channel — the exact fields
    /// [`Self::set_twist`] keeps — without its ownership rules. The path
    /// follower drives through this too, so "drive" means the same thing
    /// whether the stick or a painted route asked for it.
    fn feed_twist(&mut self, twist: [f64; 3], now: TickTime) {
        self.command.twist = twist;
        self.last_twist = now;
        self.twist_fresh = true;
    }

    /// A twist from the intent API. It takes the duck over from the
    /// on-screen joystick for [API_LEASE], so an external agent can drive it
    /// while it keeps sending twists.
    pub fn set_api_twist(&mut self, twist: [f64; 3]) {
        self.api_lease.take(twist, TickTime::of(&self.clock));
        if steers(twist) {
            // An external agent with a direction on the stick is a driver,
            // not a keepalive: it takes over from a painted route too.
            self.cancel_path();
            self.care.touch(self.clock.tick);
        }
    }

    /// Start walking a painted route. `points` are metres in the body frame
    /// at this instant (+x forward, +y left); they are frozen into the world
    /// frame here, so the route stays where it was drawn. A new route
    /// replaces one already walking.
    pub fn start_path(&mut self, points: Vec<[f32; 2]>) {
        let [x, y, _] = self.position();
        self.path_task = Some(PathTask::start(points, [x, y, self.yaw()]));
    }

    /// Stop the painted route. A no-op when nothing is walking.
    pub fn cancel_path(&mut self) {
        if self.path_task.take().is_some() {
            // The route was the one driving: stop with it rather than let
            // the last twist coast until the deadman notices.
            self.command.twist = [0.0; 3];
        }
    }

    /// The painted route as `/state` publishes it, on every publish: the
    /// accepted points and how far the walk has got, or the inactive shape.
    pub fn path_summary(&self) -> serde_json::Value {
        match &self.path_task {
            Some(task) => task.summary(),
            None => serde_json::json!({
                "active": false,
                "reached": 0,
                "total": 0,
                "points": [],
            }),
        }
    }

    /// Follow the painted route for one tick: feed its twist into the same
    /// channel the stick uses and return it, or `None` when there is no
    /// route (or it just finished — the duck celebrates that like a
    /// completed quest, without counting it as a touch).
    fn step_path(&mut self, now: TickTime) -> Option<[f64; 3]> {
        let [x, y, _] = self.position();
        let pose = [x, y, self.yaw()];
        let outcome = self.path_task.as_mut()?.drive(pose);
        match outcome {
            PathDrive::Drive(twist) => {
                // The real walk policy executes this, so a route can still
                // slip and fall — which is the whole point of driving it
                // through the walking channel.
                self.feed_twist(twist, now);
                // A route being walked is attention, exactly as a held stick
                // is: a duck must not doze off mid-route.
                self.care.touch(self.clock.tick);
                Some(twist)
            }
            PathDrive::Done => {
                self.cancel_path();
                let _ = self.celebrate_emotion("excited");
                None
            }
        }
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
        // A person steering the head is paying attention; the idle sweep does
        // not come through here, so it cannot keep the duck awake by itself.
        if head.iter().any(|value| value.abs() > 1e-3) {
            self.care.touch(self.clock.tick);
        }
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
        // A person or a rule asking for a mood is attention.
        self.care.touch(self.clock.tick);
        self.start_emotion(name)
    }

    /// A treat that arrived from a neighbour.
    ///
    /// The duck really eats it — that is the gift — but nobody's hand was
    /// here: it must not end the night, it must not be written as a touch,
    /// and it must not raise the bond, which is between the duck and the
    /// person holding the phone. One gift is one line in the book.
    pub fn gift_treat(&mut self) -> Result<bool, &'static str> {
        if self.powered_off
            || self.shutting_down()
            || !matches!(self.bringup, Bringup::Ready)
        {
            return Err("the duck is not standing");
        }
        self.start_ground_pick()?;
        self.book.life.fed += 1;
        // The book's line says where the treat came from; the gift's arrival
        // is recorded by the caller, once, whether or not the duck was in a
        // state to eat it.
        self.queue_sound("yes");
        self.care.feed_from_away();
        Ok(true)
    }

    /// A mood a neighbour sent.
    ///
    /// The emotion plays if the duck is awake — somebody else's cheer does
    /// not stir a sleeping duck — and the cheer is not a touch. The book
    /// says where it came from; it does not also claim the duck felt it on
    /// its own.
    pub fn gift_emotion(&mut self, name: &str) -> Result<bool, &'static str> {
        if self.care.asleep {
            return Ok(false);
        }
        self.play_emotion(name, false)
    }

    /// A celebration the duck performs by itself (a completed quest). It must
    /// not count as a touch: the pet that completed the goal is the touch,
    /// and an automatic cheer would otherwise reset a doze or a neglect timer.
    /// A dozing duck — a nap of its own or a night — is not stirred by its
    /// own celebration; the quest is still counted, the emotion waits.
    pub fn celebrate_emotion(&mut self, name: &str) -> Result<bool, &'static str> {
        if self.care.asleep {
            return Ok(false);
        }
        self.start_emotion(name)
    }

    fn start_emotion(&mut self, name: &str) -> Result<bool, &'static str> {
        self.play_emotion(name, true)
    }

    /// The emotion itself. `record` says whether the book gets a line of its
    /// own: a caller that says where the emotion came from does not also
    /// want the duck's own note, or one cheer is two lines.
    fn play_emotion(&mut self, name: &str, record: bool) -> Result<bool, &'static str> {
        if self.controller.busy() || self.controller.is_sitting() {
            return Err("a move is running; the duck must be standing still");
        }
        let Some(emotion) = self.emotions.get(name) else {
            return Err("no such emotion");
        };
        if emotion.duration() <= 0.0 {
            return Err("emotion has no keyframes");
        }
        if record {
            self.book.emotion(name);
        } else {
            self.book.count_emotion(name);
        }
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
        self.care.pet(self.clock.tick);
        // A child can program "when I pet you, nod". The coo is still the
        // duck's answer; the rule is what this person taught it.
        self.fire_rules("pet");
    }

    /// A treat on the floor: the official ground-pick peck serves as the
    /// bite, and a satisfied sound seals it. Digital-only ritual, but every
    /// part of it is an official move — the robot eats nothing, and this
    /// one pretends to no hardware.
    pub fn feed(&mut self) -> Result<bool, &'static str> {
        // A treat is a real peck, not a state change. If the body is limp,
        // homing, powered off or mid-shutdown the controller's pick can never
        // advance, so committing hunger/fed now would feed a duck that never
        // pecked. Refuse in words instead.
        if self.powered_off
            || self.shutting_down()
            || !matches!(self.bringup, Bringup::Ready)
        {
            return Err("the duck is not standing");
        }
        // `ground_pick` is the loop's own verb, not one of the controller's
        // scripted skills: `trigger_skill` cannot find it, and asking it was
        // a treat the duck never pecked. The controller refuses a second peck
        // while one runs, so a held feed button cannot double-count.
        self.start_ground_pick()?;
        self.book.life.fed += 1;
        self.book.note("feed".to_owned());
        self.queue_sound("yes");
        self.care.feed(self.clock.tick);
        Ok(true)
    }

    /// The companion state, for `/state.care` and the shells' status pill.
    pub fn care(&self) -> crate::care::Care {
        self.care
    }

    /// Put the duck to bed on purpose: the same doze a long neglect reaches,
    /// chosen. Idempotent, and it travels with the rest of the care numbers
    /// through a checkpoint and a mode switch.
    pub fn enter_night(&mut self) {
        // A duck going to bed stops walking and stops singing: a painted
        // route and a pending chorus must not run in its sleep.
        self.cancel_path();
        self.chorus = None;
        self.care.enter_night(self.clock.tick);
    }

    /// Start a chorus: the moves land one every 1.5 s after a 1.5 s count-in.
    /// `id` is the room-wide identifier (the lead's own new id, or the one
    /// heard from the peer that started it); a repeat of the same id is
    /// refused so a bounced packet cannot restart the song.
    pub fn start_chorus(
        &mut self,
        role: &'static str,
        moves: Vec<String>,
        peers: u32,
        id: Option<String>,
    ) {
        if id.as_deref().is_some_and(|id| self.last_chorus_id.as_deref() == Some(id)) {
            return;
        }
        self.last_chorus_id = id;
        self.chorus = Some(ChorusTask {
            role,
            moves,
            start_tick: self.clock.tick.saturating_add(CHORUS_START_TICKS),
            next_move: 0,
            peers,
            sounds: Vec::new(),
            note: "chorus",
        });
    }

    /// Start a room-wide ensemble. The score is a sequence of official mood
    /// names and sound tags; `role`/`roles` split it into parts so each duck
    /// plays ITS OWN beat (a role dance, a formation, or a voice choir).
    /// Every beat is still performed by this duck's own runtime — the wire
    /// never carries a pose or a control verb.
    #[allow(clippy::too_many_arguments)]
    pub fn start_ensemble(
        &mut self,
        role: &'static str,
        role_index: usize,
        roles: usize,
        moves: Vec<String>,
        sounds: Vec<String>,
        peers: u32,
        id: Option<String>,
    ) {
        if id.as_deref().is_some_and(|id| self.last_chorus_id.as_deref() == Some(id)) {
            return;
        }
        self.last_chorus_id = id;
        // Each role takes the beat whose index matches it; with more beats
        // than roles the part grows, with fewer it is an honest rest.
        let parts: Vec<String> = moves
            .iter()
            .enumerate()
            .filter(|(beat, _)| roles == 0 || beat % roles.max(1) == role_index)
            .map(|(_, name)| name.clone())
            .collect();
        let voices: Vec<String> = sounds
            .iter()
            .enumerate()
            .filter(|(beat, _)| roles == 0 || beat % roles.max(1) == role_index)
            .map(|(_, tag)| tag.clone())
            .collect();
        self.chorus = Some(ChorusTask {
            role,
            moves: parts,
            start_tick: self.clock.tick.saturating_add(CHORUS_START_TICKS),
            next_move: 0,
            peers,
            sounds: voices,
            note: "ensemble",
        });
    }

    /// Whether a chorus is being performed right now.
    pub fn chorus_active(&self) -> bool {
        self.chorus.is_some()
    }

    /// The chorus as /state publishes it, on every publish.
    pub fn chorus_summary(&self) -> serde_json::Value {
        match &self.chorus {
            Some(task) => task.summary(),
            None => serde_json::json!({
                "active": false,
                "role": "",
                "moves": [],
                "started_tick": 0,
                "peers": 0,
            }),
        }
    }

    /// Morning: end a night that is running and wake the duck. `Some(quiet_s)`
    /// when a night was running, `None` when the duck was not in night (a
    /// no-op: a duck merely dozing on its own is not in night).
    ///
    /// The wake plays the official "yes" emotion when this build loaded it,
    /// and the synth's chirp otherwise. The vet book keeps "morning" — or
    /// "woken_by_touch" when a hand got there first.
    pub fn end_night(&mut self) -> Option<f64> {
        let quiet_s = self.care.end_night(self.clock.tick)?;
        self.book.note("morning".to_owned());
        // The official greeting, when this build loaded it; a move already
        // owning the body still gets its voice. A build without the emotion
        // answers with the synth's chirp.
        if self.start_emotion("yes").is_err() {
            let voice = if self.emotions.get("yes").is_some() {
                "emotion:yes"
            } else {
                "chirp"
            };
            self.queue_sound(voice);
        }
        Some(quiet_s)
    }

    /// The night as `/state` publishes it, on every publish: whether one is
    /// running, how quiet it has been, and the tick it began. `quiet_s` is
    /// recomputed from the simulated tick delta and never taken from a client.
    pub fn night_summary(&self) -> serde_json::Value {
        match self.care.night {
            Some(night) => serde_json::json!({
                "active": true,
                "quiet_s": self.care.quiet_s(self.clock.tick),
                "started_tick": night.started_tick.min(u32::MAX as u64) as u32,
            }),
            None => serde_json::json!({
                "active": false,
                "quiet_s": 0.0,
                "started_tick": 0,
            }),
        }
    }

    /// How fast care ages. 1.0 is real time; a client may speed it up for a
    /// demo or a test, exactly as `idle_ritual_s` shortens the duck's
    /// self-entertainment clock.
    pub fn set_care_scale(&mut self, scale: f64) {
        self.care_scale = scale.clamp(0.0, 10_000.0);
    }

    pub fn care_scale(&self) -> f64 {
        self.care_scale
    }

    /// Hand back the care numbers a shell saved across launches.
    pub fn restore_care(&mut self, saved: &serde_json::Value) {
        self.care.restore(saved);
        if let Some(idle) = saved.get("idle_s").and_then(serde_json::Value::as_f64) {
            // Ticks restart at zero in a new process. Persist the idle span,
            // not the old absolute tick, so twelve minutes of neglect (or a
            // nap) survive a restart instead of looking like a fresh touch.
            let ticks = (idle.max(0.0) / 0.02) as u64;
            self.care.last_touch = self.clock.tick.saturating_sub(ticks);
        } else if self.care.last_touch > self.clock.tick {
            // Legacy payload from another boot: its tick cannot be mapped.
            // Treat it as long ago rather than as "touched now".
            self.care.last_touch = 0;
        }
        // The night's start tick has the same boot problem. A number from a
        // previous process's clock is in the future here, so the night
        // restarts from this boot instead of publishing a quiet span that
        // waits minutes to count at all.
        if let Some(night) = &mut self.care.night {
            if night.started_tick > self.clock.tick {
                night.started_tick = self.clock.tick;
            }
        }
        // `/care` with `asleep:false` is a hand, not the morning button: a
        // live night ends honestly. A wholesale launch restore carries
        // `asleep:true` with its night, so it does not land here.
        if saved.get("asleep").and_then(serde_json::Value::as_bool) == Some(false)
            && self.care.night.is_some()
        {
            self.care.touch(self.clock.tick);
            // The note is written now; the touch's own flag would write it
            // again on the next tick.
            self.care.woken_by_touch = false;
            self.book.note("woken_by_touch".to_owned());
        }
    }

    /// The care numbers plus the idle span, for persistence and /state.
    /// The parent's daily allowance, in minutes; zero clears it. The base
    /// snapshots the duck's own powered time NOW, so the day already spent
    /// is not double-counted against the new allowance.
    pub fn set_parent_budget(&mut self, minutes: u32) {
        self.care.budget_s = if minutes == 0 { None } else { Some(minutes * 60) };
        self.care.budget_base_s = self.book().life.powered_s;
    }

    /// The calendar day rolled over: the allowance restarts from the
    /// powered time the duck has now. The caller (the tick) decides when a
    /// day begins — the shell posts the day it lives by.
    pub fn reset_parent_day(&mut self) {
        self.care.budget_base_s = self.book().life.powered_s;
    }

    pub fn care_summary(&self) -> serde_json::Value {
        let mut value = self.care.summary();
        let idle = self.clock.tick.saturating_sub(self.care.last_touch) as f64 * 0.02;
        if let Some(object) = value.as_object_mut() {
            object.insert("idle_s".to_owned(), serde_json::json!(idle));
            // The parent's day, as the duck keeps it: allowance in minutes,
            // and how much of the duck's own powered time it has used.
            let used = (self.book().life.powered_s - self.care.budget_base_s).max(0.0);
            object.insert(
                "parent".to_owned(),
                serde_json::json!({
                    "minutes": self.care.budget_s.map(|s| s / 60),
                    "used_s": (used * 10.0).round() / 10.0,
                    "left_s": self.care.budget_s.map_or(0.0, |budget| {
                        (f64::from(budget) - used).max(0.0)
                    }),
                }),
            );
        }
        value
    }

    /// A clap or a whistle from the room. The duck answers with its own
    /// official emotion; the count and the event are published so a shell or
    /// a kid's rule can see what was heard. The microphone is never a sense.
    pub fn sound_event(&mut self, kind: crate::sensors::SoundKind) {
        self.last_sound_event = Some((kind.as_str().to_owned(), self.clock.tick));
        // Someone made that sound at the duck; it is attention either way.
        self.care.touch(self.clock.tick);
        match kind {
            crate::sensors::SoundKind::Clap => {
                self.book.life.claps += 1;
                self.book.note("clap".to_owned());
            }
            crate::sensors::SoundKind::Whistle => {
                self.book.life.whistles += 1;
                self.book.note("whistle".to_owned());
            }
        }
        // A programmed rule owns the moment: the child built this to see it,
        // so the duck must not also play its default reaction over the top.
        if self.fire_rules(kind.as_str()) {
            return;
        }
        let emotion = match kind {
            crate::sensors::SoundKind::Clap => "laugh",
            crate::sensors::SoundKind::Whistle => "curious",
        };
        // An emotion can be refused while a skill or a hold owns the duck;
        // the event still happened, so answer with a voice instead of
        // pretending nothing was heard.
        if self.emotions.get(emotion).is_none() || self.trigger_emotion(emotion).is_err() {
            self.queue_sound(match kind {
                crate::sensors::SoundKind::Clap => "coo",
                crate::sensors::SoundKind::Whistle => "inquire",
            });
        }
    }

    /// Run every block whose trigger just happened. Returns whether any rule
    /// matched, which is also "the default reaction should stand down".
    fn fire_rules(&mut self, trigger: &str) -> bool {
        let rules = self.rules.matching(trigger);
        if rules.is_empty() {
            return false;
        }
        let mut executed = false;
        for rule in &rules {
            if self.fire_rule(rule, trigger) {
                executed = true;
            }
        }
        executed
    }

    /// One rule, by name: the shared body of every firing path. The timer
    /// checker hands in the exact rule it found due; the sound and pet paths
    /// hand in whatever matched their trigger word.
    fn fire_rule(&mut self, rule: &crate::rules::Rule, trigger: &str) -> bool {
        let mut executed = false;
        let mut steps = Vec::with_capacity(rule.then.len() + 1);
            steps.push(crate::rules::Action {
                action: rule.action.clone(),
                value: rule.value.clone(),
            });
            steps.extend(rule.then.iter().cloned());
            for step in &steps {
                let ok = self.run_rule_action(&step.action, &step.value);
                if ok {
                    executed = true;
                    self.rules.last = Some(crate::rules::Fired {
                        trigger: trigger.to_owned(),
                        action: step.action.clone(),
                        value: step.value.clone(),
                        tick: self.clock.tick,
                    });
                }
            }
        // If nothing could run (busy, unknown value), let the duck's default
        // reaction have the moment rather than suppressing it silently.
        executed
    }

    /// One block, through the loop's own idempotent entries where it owns the
    /// verb. A refused step returns false; a chain keeps going.
    fn run_rule_action(&mut self, action: &str, value: &str) -> bool {
        match action {
            "skill" => match value {
                // The loop owns sit/stand and ground-pick; send them to
                // their idempotent entries instead of the skill table.
                "sit" => {
                    if self.is_sitting() {
                        true
                    } else {
                        self.sit_toggle().is_ok()
                    }
                }
                "stand" => {
                    if self.is_sitting() {
                        self.sit_toggle().is_ok()
                    } else {
                        true
                    }
                }
                "ground_pick" => self.start_ground_pick().is_ok(),
                _ => self.trigger_skill(value).is_ok(),
            },
            "emotion" => self.trigger_emotion(value).is_ok(),
            "sound" => {
                if crate::voice::TAGS.contains(&value) {
                    self.queue_sound(value);
                    true
                } else {
                    false
                }
            }
            // The flashlight as a step: the same virtual LED the drawer row
            // and robot.flashlight toggle, named "off" or one of the seven
            // official colours. It is the duck's own lamp — robotd owns it —
            // so a choreography or a rule may light it between moves.
            "light" => {
                if value == "off" {
                    self.set_flashlight(false, false, "white");
                    true
                } else if crate::leds::COLORS.contains(&value) {
                    self.set_flashlight(false, true, value);
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    pub fn last_peer_event(&self) -> Option<&serde_json::Value> {
        self.last_peer_event.as_ref()
    }

    /// The words the duck's ear last heard. The words are text, not audio;
    /// the microphone reading stays the room's, never a sense.
    pub fn heard(&mut self, text: &str) {
        self.last_heard = Some((text.chars().take(200).collect(), self.clock.tick));
        self.care.touch(self.clock.tick);
    }

    pub fn last_heard(&self) -> Option<(&str, u64)> {
        self.last_heard
            .as_ref()
            .map(|(text, tick)| (text.as_str(), *tick))
    }

    /// A greeting from another duck arrived, or ours was answered. Both are
    /// social events on the phone side: the duck answers with its own voice
    /// and one of its own emotions, never a control input.
    pub fn peer_event(&mut self, event: &crate::lan::PeerEvent) {
        let (kind, from, name, sound, emotion) = match event {
            crate::lan::PeerEvent::Greeted { from, name } => {
                ("greeted", from, name, "greet", "excited")
            }
            crate::lan::PeerEvent::Replied { from, name } => {
                ("replied", from, name, "yes", "yes")
            }
            // A gift is not a greeting: the tick applies it through feed or
            // trigger_emotion (ffi.rs), so it never reaches this voice path
            // and must not publish a peer_event. A chorus is not a greeting
            // either: it is a scheduled performance of its own moves.
            crate::lan::PeerEvent::Gift { .. } => return,
            crate::lan::PeerEvent::Chorus { .. } => return,
            // An ensemble is its own scheduled performance too: the tick
            // already started the duck's part, so no greeting voice here.
            crate::lan::PeerEvent::Ensemble { .. } => return,
        };
        self.last_peer_event = Some(serde_json::json!({
            "kind": kind,
            "from": from,
            "name": name,
            "tick": self.clock.tick,
        }));
        if self.trigger_emotion(emotion).is_err() {
            self.queue_sound(sound);
        }
    }

    /// Replace the child's block program from the wire.
    pub fn set_rules(&mut self, value: &serde_json::Value) -> Result<(), String> {
        self.rules.set(value)
    }

    pub fn rules_summary(&self) -> serde_json::Value {
        self.rules.summary()
    }

    pub fn last_rule(&self) -> Option<&crate::rules::Fired> {
        self.rules.last.as_ref()
    }

    pub fn last_sound_event(&self) -> Option<(&str, u64)> {
        self.last_sound_event
            .as_ref()
            .map(|(kind, tick)| (kind.as_str(), *tick))
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
        self.care.touch(self.clock.tick);
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
        self.controller.start_skill(index).and_then(|started| {
            // A chaining move that only refreshed is not a new move: the vet
            // book counts starts, not held-button frames.
            if started {
                self.book.skill(name);
            }
            Ok(started)
        })
    }

    pub fn start_ground_pick(&mut self) -> Result<(), &'static str> {
        self.care.touch(self.clock.tick);
        self.controller.start_ground_pick()
    }

    pub fn sit_toggle(&mut self) -> Result<&'static str, &'static str> {
        self.care.touch(self.clock.tick);
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
            care: Some(self.care),
            last_sad: self.last_sad,
            last_sound_event: self.last_sound_event.clone(),
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
        self.book_position = Some(self.position());
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
        if let Some(care) = checkpoint.care {
            self.care = care;
        }
        self.last_sad = checkpoint.last_sad;
        self.last_sound_event = checkpoint.last_sound_event.clone();
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
        // Care ages on the simulated clock: a rewind restores the tummy and
        // the mood that went with that tick, and a paused duck does not get
        // hungrier while the world is frozen.
        let was_asleep = self.care.asleep;
        self.care
            .tick(self.clock.tick, self.dt.as_secs_f64(), self.care_scale);
        if self.care.woke {
            self.care.woke = false;
            // Five simulated seconds: enough for the coo (or the hand's
            // skill) to own the moment, not enough to hide a sad duck.
            self.sad_grace_until = Some(self.clock.tick.saturating_add(250));
            // A hand that wakes the duck is not stillness: the idle ritual
            // waits a full period instead of speaking over the wake coo on
            // the next tick.
            self.last_idle_ritual = Some(self.clock.tick);
        }
        if self.care.woken_by_touch {
            // A hand ended the night: the book keeps the honest reason, the
            // way a morning gets its own note and the same wake gets a coo.
            self.care.woken_by_touch = false;
            self.book.note("woken_by_touch".to_owned());
        }
        // The parent's allowance: when the day's powered time is spent, the
        // duck goes to bed — the same real sleep the switch puts it in, and
        // only a hand (or the morning) wakes it. A sleeping duck stays
        // asleep; the check never wakes anybody.
        if self.care.budget_s.is_some() && self.care.budget_spent(self.book().life.powered_s) {
            self.care.enter_night(self.clock.tick);
        // Timer rules fire on the duck's own clock: every whole simulated
        // period, a rule whose trigger is "timer" gets its moment. A paused
        // duck never reaches this code, so frozen time fires nothing.
        let due: Vec<usize> = self
            .rules
            .rules
            .iter()
            .enumerate()
            .filter(|(index, rule)| {
                rule.trigger == "timer"
                    && crate::rules::Rules::timer_period_s(&rule.value).is_some_and(|period| {
                        let period_ticks = u64::from(period) * 50;
                        self.clock.tick
                            .saturating_sub(*self.rules.timer_last.get(index).unwrap_or(&0))
                            >= period_ticks
                    })
            })
            .map(|(index, _)| index)
            .collect();
        for index in due {
            let rule = self.rules.rules[index].clone();
            self.rules.timer_last.insert(index, self.clock.tick);
            self.fire_rule(&rule, "timer");
        }
        }
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
                    self.book.life.power_offs += 1;
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
                        // A fall ends the painted route: the duck is going
                        // down, and the pose it would resume from is not the
                        // one the route was drawn for.
                        self.cancel_path();
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

        // The debounced verdict is a fall too: a duck on the floor cannot walk a
        // route, and standing back up must not resume one.
        if self.safety.fallen() {
            self.cancel_path();
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

        // A painted path drives through the same command channel as the
        // stick; feeding it here gives it the same smoothing, deadman and
        // emotion overrides the joystick's twist gets.
        if self.path_task.is_some() {
            self.step_path(now);
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

        // Whoever is steering the head, the body or the stick is paying the
        // duck attention. It keeps a hand on the duck from counting as
        // neglect, and it wakes a dozing one. (The per-tick zero keepalives
        // are filtered at the entry points, not here.)
        if self.body_active {
            self.care.touch(self.clock.tick);
        }

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
        // A dozing duck tucks its neck and shuts its beak. This is a pose
        // overlay on top of the same idle behaviour, not a policy switch: the
        // safety layer still sees every joint, and any touch removes it on
        // the next tick.
        if self.care.asleep {
            command.head[0] -= 0.12;
            command.head[1] -= 0.30;
        }

        // The duck's voice reacts to the world, on rising edges only. robotd
        // coos in a hand, alarms on a fall, and inquires when the pad comes
        // back after a long stillness; a two-second cooldown keeps a bouncing
        // classifier from turning one pickup into a chorus.
        let loud = self.room_loud;
        // A clap or whistle was already classified this tick. The generic
        // startle alarm would double-answer it; let the specific reaction
        // (or the child's rule) own the moment.
        let just_heard = self
            .last_sound_event
            .as_ref()
            .is_some_and(|(_, tick)| tick.saturating_add(1) == self.clock.tick);
        let moving = command.twist.iter().any(|value| value.abs() > 1e-3);
        let cooldown = self
            .last_reaction
            .is_none_or(|tick| self.clock.tick.saturating_sub(tick) >= 100);
        let mut reaction = None;
        // A sudden bang in the room startles it: the official alarm chirp
        // and an impatient emotion, on the rising edge with a cooldown so
        // one clap is one reaction.
        if loud && !self.last_loud && cooldown && emotion_frame.is_none() && !just_heard {
            reaction = Some("alarm");
            // A bang wakes a dozing duck. The alarm is the reaction; the
            // touch is what keeps it from falling straight back asleep.
            self.care.touch(self.clock.tick);
        }
        if pickup_paused && !self.last_pickup_paused {
            reaction = Some("coo");
            self.book.life.pickups += 1;
            self.book.note("picked_up".to_owned());
        } else if self.safety.fallen() {
            self.fallen_ticks += 1;
            // Twenty 50 Hz ticks = 0.4 s of continuous fall. robotd's own
            // fall predictor debounces; a scripted head move can tip the
            // trunk for a frame or two without being a real fall. One spell
            // of lying down is one fall, however long it lasts.
            if self.fallen_ticks == 20 {
                reaction = Some("alarm");
                self.book.life.falls += 1;
                self.book.note("fell".to_owned());
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
        // Waking up is its own little moment: a coo, once, not every tick.
        if reaction.is_none() && was_asleep && !self.care.asleep {
            reaction = Some("coo");
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
            && !self.care.asleep
            && !self.emotions.is_empty()
        {
            // Only the emotions that carry no skill: an unprompted sit would
            // leave the duck seated until someone stands it up by hand.
            let names = self.emotions.ritual_names();
            if !names.is_empty() {
                let index = (self.clock.tick / period.max(1)) as usize % names.len();
                let _ = self.trigger_emotion(&names[index]);
                self.last_idle_ritual = Some(self.clock.tick);
                self.book.note("idle_ritual".to_owned());
            }
        }
        // Neglect shows. A low mood is not a number in a menu: after a few
        // minutes without attention the duck plays its own official sad
        // timeline, at most once a minute, and never while it is dozing.
        if self.care.sad()
            && !self.care.asleep
            && self
                .sad_grace_until
                .is_none_or(|until| self.clock.tick >= until)
            && emotion_frame.is_none()
            && self.emotion_playing.is_none()
            && !self.controller.busy()
            && !pickup_paused
            && !self.shutting_down()
            && self
                .last_sad
                .is_none_or(|tick| self.clock.tick.saturating_sub(tick) >= 3000)
            && self.emotions.get("sad").is_some()
        {
            let _ = self.trigger_emotion("sad");
            self.last_sad = Some(self.clock.tick);
            self.book.note("sad".to_owned());
        }
        if let Some(tag) = reaction {
            self.queue_sound(tag);
            self.last_reaction = Some(self.clock.tick);
        }
        // The room's chorus: each move lands on its own tick. A move refused
        // by a busy skill is skipped — the song degrades, it does not stall —
        // and the last move closes the chorus with its honest note.
        let mut due_moves: Vec<String> = Vec::new();
        let mut due_sounds: Vec<String> = Vec::new();
        let beat_count = self
            .chorus
            .as_ref()
            .map_or(0, |task| task.moves.len().max(task.sounds.len()));
        if let Some(task) = self.chorus.as_mut() {
            while task.next_move < beat_count
                && self.clock.tick
                    >= task
                        .start_tick
                        .saturating_add(task.next_move as u64 * CHORUS_MOVE_TICKS)
            {
                if let Some(name) = task.moves.get(task.next_move) {
                    due_moves.push(name.clone());
                }
                if let Some(tag) = task.sounds.get(task.next_move) {
                    due_sounds.push(tag.clone());
                }
                task.next_move += 1;
            }
        }
        for tag in due_sounds {
            // A voice is the duck's OWN formant synth, never a system TTS.
            if crate::voice::TAGS.contains(&tag.as_str()) {
                self.queue_sound(&tag);
            }
        }
        for move_name in due_moves {
            // The room is singing, not a hand: a beat is not attention, so
            // it must not end a night or reset a neglect timer.
            let _ = self.celebrate_emotion(&move_name);
        }
        let finished = self.chorus.as_ref().is_some_and(|task| {
            let beats = task.moves.len().max(task.sounds.len());
            beats > 0
                && task.next_move >= beats
                && self.clock.tick
                    >= task
                        .start_tick
                        .saturating_add((beats as u64 - 1) * CHORUS_MOVE_TICKS)
        });
        if finished {
            let note = self.chorus.as_ref().map_or("chorus", |task| task.note);
            self.chorus = None;
            self.book.note(note.to_owned());
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
            // A sleeping duck's beak is shut, even if the idle yawn was due.
            let mouth = if self.care.asleep { 0.0 } else { mouth };
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
        } else if self.care.asleep {
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
        let position = self.position();
        if let Some(last) = self.book_position {
            let dx = position[0] - last[0];
            let dy = position[1] - last[1];
            let step = (dx * dx + dy * dy).sqrt();
            // 5 cm in 20 ms is 2.5 m/s: faster than any gait, slower than a
            // checkpoint restore or a mode switch teleport.
            if step < 0.05 {
                self.book.life.metres += step;
            }
        }
        self.book_position = Some(position);
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
        // A ground pick is the controller's own state, not a scripted skill,
        // so its rising edge is the only place the book can count it.
        let picking = self.controller.ground_pick_running();
        if picking && !self.last_ground_pick {
            self.book.life.ground_picks += 1;
            self.book.note("ground_pick".to_owned());
        }
        self.last_ground_pick = picking;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use duck_control::io::{IoError, JointTargets, RobotIo, Sensors};

    /// A body with no hardware behind it: the sensors stay still and
    /// `Controller::step` fails, so the loop holds the pose while the state
    /// machine advances.
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

    /// Heading-error sign, verified against the frame the shells and the
    /// policy use: a waypoint on the duck's left is a positive error and a
    /// positive `wz` is a left turn (the iOS stick maps a rightward drag to
    /// negative `wz`, and odometry's yaw grows counter-clockwise).
    #[test]
    fn a_waypoint_on_the_left_turns_left() {
        let mut task = PathTask::start(vec![[0.5, 0.5]], [0.0, 0.0, 0.0]);
        let PathDrive::Drive(twist) = task.drive([0.0, 0.0, 0.0]) else {
            panic!("the waypoint is seven tenths of a metre away, not reached");
        };
        assert!(twist[2] > 0.0, "left waypoint must turn left, got {twist:?}");
        assert!(twist[0] > 0.0, "a reachable heading may walk, got {twist:?}");
        assert_eq!(twist[1], 0.0, "the path never sidesteps");

        let mut right = PathTask::start(vec![[0.5, -0.5]], [0.0, 0.0, 0.0]);
        let PathDrive::Drive(twist) = right.drive([0.0, 0.0, 0.0]) else {
            panic!("the waypoint is seven tenths of a metre away, not reached");
        };
        assert!(twist[2] < 0.0, "right waypoint must turn right, got {twist:?}");
    }

    /// The start pose is the body frame's origin and rotation: a route
    /// drawn "straight ahead" stays dead ahead no matter which way the duck
    /// was facing when it started.
    #[test]
    fn the_route_is_frozen_into_the_world_at_the_start_pose() {
        // Facing +y (yaw pi/2), [1, 0] in the body frame is one metre up +y.
        let mut task = PathTask::start(vec![[1.0, 0.0]], [2.0, 3.0, std::f64::consts::FRAC_PI_2]);
        assert!((task.world[0][0] - 2.0).abs() < 1e-9);
        assert!((task.world[0][1] - 4.0).abs() < 1e-9);
        let PathDrive::Drive(twist) = task.drive([2.0, 3.0, std::f64::consts::FRAC_PI_2]) else {
            panic!("one metre ahead is not reached");
        };
        assert!(twist[2].abs() < 1e-9, "dead ahead must not turn, got {twist:?}");
        assert!(twist[0] > 0.0);
    }

    /// A waypoint is reached within 0.15 m, and passing it hands the next
    /// one to the same tick.
    #[test]
    fn a_waypoint_is_reached_within_fifteen_centimetres() {
        let mut task = PathTask::start(vec![[0.3, 0.0], [0.6, 0.0]], [0.0, 0.0, 0.0]);
        // 0.16 m short of the first point: still walking to it.
        let PathDrive::Drive(_) = task.drive([0.14, 0.0, 0.0]) else {
            panic!("0.16 m away is not reached");
        };
        assert_eq!(task.next, 0);
        // 0.14 m short: reached, and the same tick already steers to the
        // second point.
        let PathDrive::Drive(twist) = task.drive([0.16, 0.0, 0.0]) else {
            panic!("0.14 m away is reached");
        };
        assert_eq!(task.next, 1);
        assert!(twist[0] > 0.0);
        // Reaching the last point ends the route.
        assert_eq!(task.drive([0.5, 0.0, 0.0]), PathDrive::Done);
        assert_eq!(task.next, 2);
    }

    /// `/state.path`'s shape is a frozen contract: the accepted points
    /// exactly as posted, plus the reached count and the total.
    #[test]
    fn the_live_summary_publishes_the_accepted_points() {
        let mut task = PathTask::start(vec![[0.5, 0.0], [1.0, 0.0]], [0.0, 0.0, 0.0]);
        let _ = task.drive([0.45, 0.0, 0.0]);
        let summary = task.summary();
        assert_eq!(summary["active"], serde_json::json!(true));
        assert_eq!(summary["reached"], serde_json::json!(1));
        assert_eq!(summary["total"], serde_json::json!(2));
        assert_eq!(
            summary["points"],
            serde_json::json!([[0.5, 0.0], [1.0, 0.0]])
        );
    }

    /// A waypoint behind the duck withholds the throttle: turning in place
    /// first is the whole point of the `PATH_TURN_FIRST` gate.
    #[test]
    fn a_waypoint_behind_turns_in_place_first() {
        let mut task = PathTask::start(vec![[-0.5, 0.0]], [0.0, 0.0, 0.0]);
        let PathDrive::Drive(twist) = task.drive([0.0, 0.0, 0.0]) else {
            panic!("half a metre behind is not reached");
        };
        assert_eq!(twist[0], 0.0, "walking backwards is a fall, not a route");
        assert!((twist[2].abs() - PATH_MAX_WZ).abs() < 1e-12);
    }

    /// The route layer's caps: 64 kept points and 20 m end to end fit, 65
    /// points or one extra centimetre do not, and nothing is queued either
    /// way the runtime sees it.
    #[test]
    fn a_route_over_the_caps_is_refused() {
        let mut points: Vec<[f64; 2]> = vec![[0.0, 0.0]];
        for step in 0..=PATH_MAX_POINTS {
            points.push([step as f64 * 0.11, 0.0]);
        }
        let over = serde_json::json!({ "points": points });
        assert!(path_points(&over).is_err(), "65 kept points must be refused");

        // 64 points, each 0.11 m from its neighbour, fit exactly.
        points.pop();
        let kept = path_points(&serde_json::json!({ "points": points }))
            .expect("64 points are the budget");
        assert_eq!(kept.len(), PATH_MAX_POINTS);

        // Exactly 20 m end to end is allowed; a centimetre more is refused.
        let at_cap = path_points(&serde_json::json!({
            "points": [[0.0, 0.0], [0.0, 0.0], [20.0, 0.0]]
        }))
        .expect("20 m fits");
        assert_eq!(at_cap.len(), 2);
        assert!(path_points(&serde_json::json!({
            "points": [[0.0, 0.0], [0.0, 0.0], [10.0, 0.0], [20.01, 0.0]]
        }))
        .is_err());
    }

    /// The wire's thinning: the first element is the duck's own origin and
    /// goes, and anything under 0.1 m from the previous *kept* point goes
    /// too. A route that dissolves to nothing is not a route.
    #[test]
    fn the_origin_is_dropped_and_close_points_are_thinned() {
        let kept = path_points(&serde_json::json!({
            "points": [
                [0.0, 0.0],   // the duck's own origin: dropped
                [0.5, 0.0],   // kept
                [0.5625, 0.0], // 6.25 cm from [0.5, 0]: dropped
                [0.625, 0.0], // 12.5 cm from [0.5, 0]: kept
                [0.625, 0.0625], // 6.25 cm from [0.625, 0]: dropped
                [0.625, 0.1875], // 12.5 cm from [0.625, 0]: kept
            ]
        }))
        .expect("a short route fits");
        assert_eq!(
            kept,
            vec![[0.5_f32, 0.0], [0.625, 0.0], [0.625, 0.1875]]
        );

        // An origin alone, or one point, is not a route at all.
        assert!(path_points(&serde_json::json!({ "points": [] })).is_err());
        assert!(path_points(&serde_json::json!({ "points": [[0.0, 0.0]] })).is_err());
        assert!(path_points(&serde_json::json!({
            "points": [[0.0, 0.0], [0.05, 0.0]]
        }))
        .is_err());
        // Malformed and missing points are refused, not silently skipped.
        assert!(path_points(&serde_json::json!({ "points": [[0.0, 0.0], ["x", 0.0]] })).is_err());
        assert!(path_points(&serde_json::json!({})).is_err());
    }

    /// A painted path is owned by the child's hand: any directed twist from
    /// the stick or the API cancels it (`set_twist` and `set_api_twist`
    /// clear the task on the same predicate), while the shells' 50 Hz zero
    /// keepalive must not, or a route could never run while the app is open.
    #[test]
    fn a_directed_twist_cancels_the_path_but_a_keepalive_does_not() {
        assert!(!steers([0.0; 3]), "the zero keepalive is not a hand");
        assert!(steers([0.4, 0.0, 0.0]), "a forward stick takes the duck back");
        assert!(steers([0.0, 0.0, -0.3]), "so does a yaw stick");
        assert!(steers([0.0, 0.2, 0.0]), "and so does a sideways one");
    }

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
        let dir = crate::fixtures::policies_dir().expect("the official policy bundle");
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

    /// A hand that wakes the duck is not stillness: the idle ritual keeps
    /// its distance for a full period instead of speaking over the wake coo
    /// on the very next tick.
    #[test]
    fn a_waking_duck_keeps_its_coo_before_the_idle_ritual() {
        let probe = Probe {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
        };
        let mut runtime = DuckRuntime::new(
            probe,
            test_policy(),
            Tuning::default(),
            SkillTuning::default(),
            SafetyConfig::default(),
        );
        runtime.enable().ok();

        // The official timelines are data; the ritual picks one of them.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../upstream/microduck/robotd/emotions");
        runtime.emotions = crate::emotion::Library::load(&dir).expect("official emotions");
        // A one-tick period makes the ritual due the moment the duck is
        // still; the warm-up lets the idle head take over first.
        runtime.set_idle_ritual(0.02);
        runtime.last_idle_ritual = None;
        runtime.care.asleep = true;
        for _ in 0..400 {
            let _ = runtime.tick();
        }
        assert!(runtime.care.asleep, "the duck must still be dozing");
        assert!(
            runtime.last_idle.takeover > 0.9,
            "the head must be idle enough for the ritual: {}",
            runtime.last_idle.takeover
        );

        runtime.pet();
        assert_eq!(
            runtime.last_sound(),
            Some("coo"),
            "a pet answers with a coo"
        );
        let _ = runtime.tick();
        assert_eq!(
            runtime.last_sound(),
            Some("coo"),
            "a due idle ritual must not speak over the wake coo"
        );
    }
}