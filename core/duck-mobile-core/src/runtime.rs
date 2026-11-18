//! The mobile duck runtime: official control core + the robotd bring-up/reflex loop.
//!
//! `duck-control` holds the observation, policy, safety and fall predictor; the official
//! `robotd` skill scheduler lives in [`crate::control`]. This module supplies the loop around
//! them that `robotd` itself cannot be reused for (it owns tokio, IPC and systemd):
//!
//! ```text
//! read → observe (fall) → odometry → safety.gate (deadman) → bring-up → shutdown
//!      → limp-fall → pickup → command smoothing → idle head → controller.step
//!      → safety.apply → write
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
use duck_control::safety::{Limit, Safety, SafetyConfig};

use crate::control::posture;
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
/// How long the API twist lease keeps control — `SafetyConfig::default().deadman`.
/// The deadman itself is official now: `Safety::gate` owns it, keyed on this
/// same duration from the safety config the runtime is built with.
const DEADMAN: Duration = Duration::from_millis(500);

/// `robotd`'s `RESET_AFTER_PAUSE`: driving must have been interrupted this long
/// before the controller reset on the next driving edge. A reset zeroes the
/// action history the policy observes and drops the low-pass anchor — right
/// after a real pause, and itself the discontinuity it exists to prevent after
/// a 20 ms hiccup (robotd main.rs:168).
const RESET_AFTER_PAUSE: Duration = Duration::from_millis(200);

/// `robotd`'s `[update_gate] min_achieved_hz` at full speed (deploy/robotd.toml:
/// 45 is 90% of the default 50 Hz). Below this the loop reports unhealthy.
/// The floor is judged against the *expected* rate, not a bare 45: the shell
/// drives the loop at 50 × `clock.speed` ticks per wall second (iOS
/// `DuckController.ticksForThisFrame`), so a deliberately slowed duck is a
/// product feature, not a broken control loop.
const MIN_ACHIEVED_HZ: f64 = 45.0;
/// `robotd`'s `[update_gate] stall_periods`: 25 × 20 ms = 500 ms without a tick
/// at all is a wedged loop, distinct from a slow one.
const STALL_PERIODS: u32 = 25;
/// `robotd`'s `RATE_WINDOW`: the achieved rate is ticks over the wall clock in
/// this window, stored when it closes.
const RATE_WINDOW: Duration = Duration::from_secs(1);

/// `robotd`'s `COAST_TICKS` (main.rs:161): a dropped bus read repeats the last
/// good sample for at most this many ticks. Past it the sample is too old to
/// step the policy from — the loop keeps ticking and holding, and the run
/// becomes a health verdict once it reaches [`MAX_CONSECUTIVE_ERRORS`].
const COAST_TICKS: u32 = 3;
/// `robotd`'s `[update_gate] max_consecutive_errors` default — 10, deploy's
/// `robotd.toml` comment and `robotd-params`' built-in
/// (`robotd-params/src/lib.rs:2699`). The same number turns a run of failed
/// reads into `robot.health`'s unhealthy verdict.
const MAX_CONSECUTIVE_ERRORS: u32 = 10;

/// Voltage adaptation, robotd's: the servos' effective kP tracks their supply,
/// so scaling the action by (nominal / measured) holds the robot's response
/// steady as the pack sags. On by default since 2026-09-14, at the nominal
/// 7.4 V the gains were identified at (robotd-params `voltage_adapt` /
/// `nominal_voltage` defaults; the mobile loop has no per-policy config file,
/// so the defaults are the values).
const NOMINAL_VOLTAGE: f64 = 7.4;

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

/// Stable wire names for the reasons a command was altered — robotd's own
/// `limit_name`, spelled out rather than `Debug`-formatted because these go on
/// the wire (`robot.state.movement.limited_by`).
fn limit_name(limit: Limit) -> &'static str {
    match limit {
        Limit::Deadman => "deadman",
        Limit::Range => "joint_range",
        Limit::NotFinite => "not_finite",
    }
}

/// The tick's action scale, robotd's voltage adaptation: `nominal / measured`,
/// with the measurement clamped to a plausible band so a bad reading cannot
/// become a wild scale. No reading yet, or a dead one, is no adaptation.
fn voltage_scale(volts: Option<f64>) -> f64 {
    match volts {
        Some(volts) if volts > 0.0 => NOMINAL_VOLTAGE / volts.clamp(6.0, 9.5),
        _ => 1.0,
    }
}

/// Every body's pose in the trunk frame, at the measured joint vector —
/// `robotd`'s `mapping::skeleton_at` (main.rs:5918-5931), verbatim in shape.
///
/// The MJCF's joint order is not the wire's: angles are gathered by name from
/// the `JOINT_NAMES`-ordered positions, and a model joint with no wire entry
/// reads 0, exactly as upstream. This is the whole `robot.state.skeleton`,
/// where `frames` (the camera and ToF leaves) would be a few entries of it —
/// the phone has no camera or ToF geometry to publish, so `frames` stays
/// `None` and only the full skeleton is served.
fn skeleton_at(positions: &[f64]) -> Vec<duck_ipc_proto::PoseState> {
    let model = kinematics::Model::alpha();
    let angles: Vec<f64> = model
        .joint_names()
        .map(|name| {
            duck_ipc_proto::JOINT_NAMES
                .iter()
                .position(|wire| *wire == name)
                .and_then(|index| positions.get(index).copied())
                .unwrap_or(0.0)
        })
        .collect();
    model
        .body_poses(&angles)
        .into_iter()
        .map(|pose| duck_ipc_proto::PoseState {
            pos: pose.pos,
            quat: pose.quat.wxyz(),
        })
        .collect()
}

/// Whether resuming from `stopped_at` warrants resetting the controller.
///
/// The reset zeroes the action history the policy observes and drops the
/// low-pass anchor. After a real pause that is right — the robot may be
/// somewhere else entirely. After a 20 ms hiccup it is itself the
/// discontinuity the reset exists to prevent.
fn reset_on_resume(stopped_at: Option<TickTime>, now: TickTime) -> bool {
    stopped_at.is_none_or(|since| since.elapsed(now) >= RESET_AFTER_PAUSE)
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
        Self {
            points,
            world,
            next: 0,
        }
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
    Ok(kept
        .into_iter()
        .map(|[x, y]| [x as f32, y as f32])
        .collect())
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

/// Where a requested policy swap is going to land, as
/// [`DuckRuntime::replace_policy_disturbing`] reports it.
///
/// The caller asked for a swap; this says what the loop did about it, which is
/// the difference between a shell that can tell the user "the duck will change
/// its gait when it has stood up" and one that promises something the safety
/// rule will not deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicySwap {
    /// The change lands in place, wherever the robot is standing: it does not
    /// replace the network driving, or nothing was driving.
    InPlace,
    /// The change replaces the network driving a robot with a gait in motion:
    /// the duck is going to the home pose and the change lands there, torque on
    /// throughout.
    GoingHome,
    /// Refused outright — the robot is shutting down or already off, and the
    /// path home would stand it back up under the sit. The policy it has is the
    /// policy it keeps.
    Refused,
}

/// The health gate's verdict, robotd's `health()` split: degraded and
/// unhealthy are strictly apart. Unhealthy is what the update system would
/// roll a release back over — the loop itself is broken. Degraded is a
/// property of the *board* or of somebody's file — the robot walks, but says
/// so. A flat battery, a fall verdict, or a limp duck is none of these: those
/// are reports, not health.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthVerdict {
    Healthy,
    Degraded { reasons: Vec<String> },
    Unhealthy { reasons: Vec<String> },
}

impl HealthVerdict {
    /// Why the verdict is not healthy, empty when it is.
    pub fn reasons(&self) -> &[String] {
        match self {
            Self::Healthy => &[],
            Self::Degraded { reasons } | Self::Unhealthy { reasons } => reasons,
        }
    }

    pub fn is_healthy(&self) -> bool {
        matches!(self, Self::Healthy)
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
    /// `robotd`'s `Bringup::homing_target`. There is no "static ramp" shortcut:
    /// robotd always holds `HOME_RAMP` out, even when the ramp has nothing to
    /// travel, because the two seconds are also the IMU's convergence window —
    /// the ramp finishing is what makes the policy eligible to drive.
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

/// `robotd`'s shutdown/rest sequence: sit, ease into the rest pose, hold.
///
/// `robot.shutdown` ends at power-off (torque off, the duck limp and reporting
/// `off` until it is powered back on). `robot.rest` runs the same sit and rest
/// pose but does not power off: the official robot ends it with a servo reboot
/// that leaves it limp; the phone has no servos to reboot, so it holds the rest
/// pose with torque kept as it was and stays powered (see [`DuckRuntime::request_rest`]).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shutdown {
    Idle,
    /// The sitstand network is driving the seat; `since` starts the hold timer,
    /// and `power_off` rides through to the rest (`shutdown_power_off`).
    Sitting {
        since: TickTime,
        power_off: bool,
    },
    /// Easing from the seat to the rest pose, then holding it. `power_off` is
    /// robotd's `shutdown_power_off`: true for `robot.shutdown`, false for
    /// `robot.rest`.
    Resting {
        from: [f64; NUM_JOINTS],
        since: TickTime,
        power_off: bool,
    },
    /// A rest that has arrived: the rest pose is held with torque kept on and
    /// the duck stays powered. `robot.init` (or a shutdown) takes it from here.
    AtRest,
}

/// robotd's limp-fall sequence.
#[derive(Debug, Clone, Copy)]
enum LimpFall {
    Idle,
    /// Gains at `GAIN_LIMP`, joints commanded to wherever they already are.
    Limp {
        since: TickTime,
        landing: Landing,
    },
    /// Ramping from where the robot landed back to the standing pose.
    Posing {
        from: [f64; NUM_JOINTS],
        since: TickTime,
    },
}

/// A policy set waiting for a safe moment to land.
///
/// robotd's `PendingSwap`, reduced to what the mobile loop needs: the caller
/// has already loaded the new set (the shell does that off the control
/// thread), so there is nothing to poll. What is left is the *where*: a change
/// that replaces the network driving must not be swapped under a moving gait —
/// the next tick would be a different policy's idea of what the legs were
/// doing — so the robot goes home first and the swap lands there. A change
/// that leaves the driving network alone lands in place, carrying the running
/// state across.
///
/// No `Debug`: it holds a `Policy`, and the ONNX sessions inside it are not
/// printable.
struct PendingSwap {
    policy: Policy,
    tuning: Tuning,
    skills: SkillTuning,
    /// Whether this change replaces the network that has the robot, as the
    /// caller's manifest comparison decided (robotd's `change_disturbs`).
    disturbs: bool,
    /// The drive decision has been made and the duck is on its way home: the
    /// change lands when the ramp completes, and the command channel stays
    /// quiet while it travels. Remembered rather than re-derived, because the
    /// stick the client is still holding would otherwise send the duck home
    /// again on every tick.
    seeking: bool,
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
    /// The driving-edge bookkeeping, for the same reason as everything above:
    /// a fork or rewind that missed these starts its first tick on a false
    /// rising edge. `driving_prev = false` under a non-zero twist is that edge;
    /// `stopped_driving_at = None` makes `reset_on_resume` believe the pause
    /// was long, and the controller reset wipes the recurrent memory the
    /// restore had just put back — while `posture_checked = false` re-runs the
    /// pre-takeover posture check. The branch and its parent diverge on the
    /// very first tick.
    #[serde(default)]
    pub driving_prev: bool,
    /// Tick driving stopped at, as ticks (robotd's wall time, on this clock).
    #[serde(default)]
    pub stopped_driving_at: Option<u64>,
    #[serde(default)]
    pub posture_checked: bool,
    #[serde(default)]
    pub start_from_hold: bool,
    /// The smoothed pack voltage: the empty-pack threshold and the action
    /// scale both read it, and a branch that lost it adapts (or powers off)
    /// on a different number than its parent.
    #[serde(default)]
    pub volts_ema: Option<f64>,
    /// A rest request that had not been taken at this tick.
    #[serde(default)]
    pub rest_requested: bool,
    /// Whether the sit/rest in flight ends at power-off (robotd's
    /// `shutdown_power_off`). Default true for checkpoints written before
    /// `robot.rest` existed: they only ever meant shutdown.
    #[serde(default = "default_true")]
    pub shutdown_power_off: bool,
    /// The unavailable-policy verdict and the bus error run, so a rewind or a
    /// fork carries the same health gate inputs as its parent.
    #[serde(default)]
    pub policy_error: Option<String>,
    #[serde(default)]
    pub consecutive_errors: u32,
    #[serde(default)]
    pub startup_bus_failures: u32,
    /// Whether that checkpoint had ever read the bus. Default true for older
    /// checkpoints: they came from a duck that was already ticking.
    #[serde(default = "default_true")]
    pub has_read: bool,
    /// The pose a stopped policy is holding (home or the measured seat). A
    /// fork or rewind that lost it would command the measured pose instead of
    /// the hold its parent is targeting.
    #[serde(default)]
    pub disabled_hold: Option<Vec<f64>>,
    /// The last good sample's joint positions — what a failed read coasts on —
    /// and its read time. Kept so `has_read` and `coast` cannot disagree after
    /// a fork: a restored duck that says it has read must have something to
    /// coast with.
    #[serde(default)]
    pub coast_positions: Option<Vec<f64>>,
    #[serde(default)]
    pub sensors_read_ns: u64,
}

/// An old checkpoint predates `robot.rest`: a sit/rest in it was a shutdown,
/// whose end is the power-off.
fn default_true() -> bool {
    true
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
    /// A policy change that replaces the network driving, waiting for the home
    /// pose. `None` when nothing is in flight.
    pending_swap: Option<PendingSwap>,
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
    /// A client asked for a rest (`robot.rest`); taken on the next tick. A
    /// shutdown asked for in the same tick wins, robotd's `!shutdown && rest`.
    rest_requested: bool,
    /// Consecutive failed bus reads — robotd's `consecutive_errors`
    /// (main.rs:2574-2609). Reset by any successful read; at
    /// [`MAX_CONSECUTIVE_ERRORS`] the health gate reports unhealthy.
    consecutive_errors: u32,
    /// Reads that failed before anything was ever read — robotd's
    /// `startup_bus_failures`. A loop that never got a tick because the bus
    /// never answered is degraded, not unhealthy (main.rs:921-935).
    startup_bus_failures: u32,
    /// Whether any read has ever succeeded. Official `robotd` counts a tick
    /// only at the end of a full loop; the mobile clock advances before the
    /// read, so "not started" is this flag, not `clock.tick == 0`.
    has_read: bool,
    /// The last good sensor sample. A failed read repeats it — robotd's
    /// `Coast` (main.rs:4267-4306) — for at most [`COAST_TICKS`] ticks.
    coast: Option<Sensors>,
    /// Nanoseconds on the duck's clock at the last fresh read — robotd's
    /// `sensors_read_ns`. It rides the coast: a tick that repeated a sample
    /// republishes that sample's read time, not the tick's.
    sensors_read_ns: u64,
    /// A policy that is unavailable — a bundle that would not build. robotd's
    /// `policy_error` (main.rs:940-942): unhealthy, because the daemon came up
    /// but cannot run its policy at all.
    policy_error: Option<String>,
    /// The pose a stopped policy holds: robotd's falling-edge `hold` — the
    /// home pose, or the seat for a seated robot (main.rs:3761-3793). `None`
    /// while nothing is deliberately stopped.
    disabled_hold: Option<[f64; NUM_JOINTS]>,
    /// The pack voltage, smoothed the way `robotd` smooths it: a step sag must
    /// not reach the empty threshold.
    volts_ema: Option<f64>,
    /// The command-alteration reasons this tick produced, in the official wire
    /// spelling: "deadman" from the gate, "joint_range"/"not_finite" from the
    /// safety apply. Cleared at the top of every tick, filled by the gate and
    /// every apply — what `robot.state.movement.limited_by` publishes.
    limits: Vec<String>,
    /// Whether the pre-takeover posture check has run this enable episode —
    /// robotd's `posture_checked`. Reset when the policy is disabled, run once
    /// the robot is Ready, warm and not driving.
    posture_checked: bool,
    /// Set when the posture check found a seat: the feedback state is seeded
    /// from the held pose at the driving edge, not from zero.
    start_from_hold: bool,
    /// Whether the robot was driving last tick — robotd's `was_driving`. The
    /// limp-fall eligibility, the idle still-check and the book all read the
    /// previous tick's driving, not this one's.
    driving_prev: bool,
    /// When driving stopped, for the `RESET_AFTER_PAUSE` gate on the resume
    /// reset. robotd's `stopped_driving_at`.
    stopped_driving_at: Option<TickTime>,
    /// Health-gate input: reasons a caller has marked (a reload that would not
    /// build, an override that fell back). Degraded, not unhealthy — the robot
    /// kept the policy it had.
    degraded_reasons: Vec<String>,
    /// The rate window, robotd's `RATE_WINDOW` over the wall clock.
    hz_window_start: Option<Instant>,
    hz_window_ticks: u64,
    achieved_hz: f64,
    /// Wall-clock instant of the last completed tick, for the stall check.
    last_tick_at: Instant,
    /// Ticks whose work overran the period, cumulative — robotd's `missed`.
    missed_ticks: u64,
    /// The `robot.state` frame this tick assembled, in the official wire shape.
    /// Kept for the IPC layer: only a subscriber makes it bytes.
    state_frame: Option<duck_ipc_proto::RobotState>,
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
            pending_swap: None,
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
            rest_requested: false,
            consecutive_errors: 0,
            startup_bus_failures: 0,
            has_read: false,
            coast: None,
            sensors_read_ns: 0,
            policy_error: None,
            disabled_hold: None,
            odometry: odometry::Odometry::alpha(),
            volts_ema: None,
            limits: Vec::new(),
            posture_checked: false,
            start_from_hold: false,
            driving_prev: false,
            stopped_driving_at: None,
            degraded_reasons: Vec::new(),
            hz_window_start: Some(Instant::now()),
            hz_window_ticks: 0,
            achieved_hz: 0.0,
            last_tick_at: Instant::now(),
            missed_ticks: 0,
            state_frame: None,
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
    /// hunger, the vet book or the child's block program. The voice travels
    /// too: it is the same duck, and a mode switch must not hand it a new one.
    pub fn carry_companion(&mut self, from: &Self) {
        self.care = from.care;
        self.book = from.book.clone();
        self.rules = from.rules.clone();
        self.last_sad = from.last_sad;
        self.sad_grace_until = from.sad_grace_until;
        self.last_sound_event = from.last_sound_event.clone();
        self.last_peer_event = from.last_peer_event.clone();
        self.last_heard = from.last_heard.clone();
        self.voice = from.voice.clone();
        self.book_position = None;
    }

    /// Ask for a sound by tag. Unknown tags are dropped rather than invented.
    pub fn queue_sound(&mut self, tag: &str) {
        // `sound.rs`'s own rule: one playing child, and a new sound kills the
        // previous one — that is what lets someone spam the chirp trigger
        // cleanly. One-shot sounds replace whatever one-shot was queued;
        // the wheee ride's segments are not one-shots and keep their place.
        let known = crate::voice::TAGS.contains(&tag)
            || tag
                .strip_prefix("emotion:")
                .is_some_and(|name| self.emotions.get(name).is_some());
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

    /// Give the duck its own voice: the seed the shell derives per duck (the
    /// official loop falls back to `sounds::hardware_seed()`, reading the SoC
    /// efuse — a phone has no such number, so the shell supplies one). Must be
    /// called before any sound renders; it replaces the personality whole, so
    /// every later render comes from the new seed. The constructor default is
    /// the fixed 0xD0C, which existing callers and tests keep working on.
    pub fn set_voice_seed(&mut self, seed: u32) {
        self.voice = crate::voice::Voice::new(seed);
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

    /// `robot.rest`: sit down, ease into the rest pose, and stay there.
    ///
    /// The official rest runs the shutdown's sit and rest ramp and ends with a
    /// servo reboot — the robot is left limp but **not** powered off
    /// (`robotd/src/main.rs:3130-3175`). The phone has no servos to reboot and
    /// no systemd to stop, so the honest mobile analogue is: the same seat and
    /// rest pose, torque kept as it was, and no `powered_off` latch. The duck
    /// stays powered; `robot.init` stands it back up, and `robot.shutdown`
    /// still powers it off. Taken on the next tick, like
    /// [`Self::request_shutdown`], and a shutdown asked for in the same tick
    /// wins — robotd's `!shutdown && rest`.
    pub fn request_rest(&mut self) {
        self.rest_requested = true;
    }

    pub fn powered_off(&self) -> bool {
        self.powered_off
    }

    /// A shutdown is sitting or resting its way to off. Nothing may start a
    /// move or a bundle switch while this is true. A duck parked at rest
    /// (`AtRest`) is **not** shutting down: official rest is a nap, and Start
    /// stands it back up (`robotd`'s rest ends limp, not powered off).
    pub fn shutting_down(&self) -> bool {
        self.powered_off
            || matches!(
                self.shutdown,
                Shutdown::Sitting { .. } | Shutdown::Resting { .. }
            )
    }

    /// Power the duck back on after a shutdown. The battery has to have been
    /// charged (or the app restarted) for this to be a sensible request.
    pub fn power_on(&mut self) -> Result<(), RuntimeError> {
        self.powered_off = false;
        self.shutdown = Shutdown::Idle;
        self.shutdown_requested = false;
        self.rest_requested = false;
        // **The seat did not survive the torque cut.** `begin_shutdown_sit`
        // left `Sit::Sitting` in the controller, and `enable`'s `reset` only
        // clears the policy — the sitting network would take the first tick
        // after power-on and sit the duck back down. `forget` is upstream's
        // own answer for "the robot came back limp": the seat and any move are
        // gone, and it stands from home.
        self.controller.forget();
        self.enable()
    }

    /// Start the shutdown sequence. `power_off` is robotd's
    /// `shutdown_power_off`: true is `robot.shutdown`, false is `robot.rest`,
    /// which ends holding the rest pose with torque kept and no off latch.
    ///
    /// `positions` is this tick's measured pose: a rest on a robot that cannot
    /// sit has nothing else to ease into the rest pose from.
    fn begin_shutdown(&mut self, now: TickTime, positions: [f64; NUM_JOINTS], power_off: bool) {
        if self.enabled
            && matches!(self.bringup, Bringup::Ready)
            && !self.safety.fallen()
            && self.controller.has_sitstand()
        {
            tracing::warn!(power_off, "shutdown: sitting down before the rest pose");
            // The goodbye peck is the shutdown's, not the rest's: a rest is a
            // nap, and robotd keeps the loop quiet for it.
            if power_off {
                self.queue_sound("peck");
            }
            self.controller.begin_shutdown_sit();
            self.shutdown = Shutdown::Sitting {
                since: now,
                power_off,
            };
        } else if power_off {
            // Nothing to sit with: cut torque now, which is the last thing the
            // sequence would have done anyway.
            tracing::warn!("shutdown: cannot sit; cutting torque now");
            let _ = self.safety.set_torque(false);
            self.enabled = false;
            self.bringup = Bringup::Limp;
            self.powered_off = true;
            self.book.life.power_offs += 1;
            // No sit/rest is in flight any more: this tick's frame is the
            // power-off, not one more "rest" from a stale state.
            self.shutdown = Shutdown::Idle;
        } else if matches!(self.bringup, Bringup::Ready) && !self.safety.fallen() {
            // A rest with nothing to sit with but a sound body: what is left
            // of it is the rest pose. The torque stays whatever it was — the
            // phone's rest never cuts it — and the duck stays powered.
            tracing::warn!("rest: cannot sit; easing straight into the rest pose");
            self.enabled = false;
            self.shutdown = Shutdown::Resting {
                from: positions,
                since: now,
                power_off: false,
            };
        } else {
            // Limp, mid-ramp, or fallen: nothing that may sit is driving.
            // Official `can_sit` includes `!fallen`, and the rest then takes
            // the cannot-sit path — cut torque, reboot the servos, end
            // (main.rs:3131-3149). With no servos to reboot, the end is the
            // rest state itself: no ramp that a Limp bring-up would strand
            // forever, and no hard sit-up out of a fall. The reboot's
            // `controller.forget()` goes with it (main.rs:2752-2769): the
            // skill, ground pick and seat that were mid-flight must not
            // survive the rest and replay the moment the policy drives again.
            tracing::warn!("rest: nothing that can sit is driving — the rest ends here");
            let _ = self.safety.set_torque(false);
            self.enabled = false;
            self.bringup = Bringup::Limp;
            self.controller.forget();
            self.shutdown = Shutdown::AtRest;
        }
    }

    /// Whether the pickup watch may score this tick: the walk (or stand)
    /// policy has the robot and nothing else owns it. Roller mode is never
    /// watched — the model has never seen wheels, and scoring it there would
    /// pause a wheeled gait on its own joints.
    fn pickup_watching(&self, ready: bool, settled: bool, warm: bool, free: bool) -> bool {
        self.mode == robotd_params::Mode::Walk && ready && settled && warm && free
    }

    /// Torque has just come on: where should the ramp take the robot?
    ///
    /// robotd's `ramp_target`, verbatim in shape. A robot that reads seated
    /// goes to [`crate::SEAT_POSITION`] — ramping it straight-legged to the
    /// standing home pose drags it out of the seat and over backwards — and
    /// the seat is marked on the controller; the next Start rises it. Only on
    /// a clear seated verdict, with a converged IMU and a network to rise
    /// with. Anything else goes home, as it always has.
    fn ramp_target(&mut self, sensors: &Sensors) -> [f64; NUM_JOINTS] {
        if !self.controller.has_sitstand() {
            return DEFAULT_POSITION;
        }
        if !self.imu_ready() {
            tracing::warn!("posture: the IMU has not converged — ramping to home without a check");
            return DEFAULT_POSITION;
        }
        let reading = posture::classify(&sensors.positions, sensors.imu.quat);
        let height = format!("{:.0}%", 100.0 * reading.height_ratio);
        let tilt = format!("{:.0}°", reading.tilt_deg);
        if reading.posture != posture::Posture::Seated {
            tracing::info!(%height, %tilt, posture = ?reading.posture, "posture at torque on");
            return DEFAULT_POSITION;
        }
        tracing::warn!(%height, %tilt, "posture: seated at torque on — ramping to the seat");
        self.controller.enter_seat();
        crate::SEAT_POSITION
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
        self.mouth = if open.is_finite() {
            open.clamp(0.0, 1.0)
        } else {
            0.0
        };
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

    /// Ask for the policy to drive, exactly as `robotd`'s enable intent does:
    /// set the flag, clear the command state, queue the greet. Torque and the
    /// ramp happen in the tick's bring-up transition — after the gate, with a
    /// fresh sensor sample to start the ramp from and a posture check to pick
    /// the ramp's target. There is no "already at home" shortcut: robotd always
    /// enters `Homing` and lets the ramp finish (the ramp doubles as the IMU's
    /// convergence window), so the mobile duck does too.
    pub fn enable(&mut self) -> Result<(), RuntimeError> {
        // Start out of a rest: the duck is not shutting down any more, and
        // the bring-up below is what stands it back up.
        if matches!(self.shutdown, Shutdown::AtRest) {
            self.shutdown = Shutdown::Idle;
            self.rest_requested = false;
        }
        if self.enabled {
            return Ok(());
        }
        self.enabled = true;
        // The pre-takeover posture check is per episode: a robot that sat
        // disabled and is re-enabled is measured again.
        self.posture_checked = false;
        self.start_from_hold = false;
        // A stopped policy's home/seat hold is over: the gait owns the robot.
        self.disabled_hold = None;
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
    ///
    /// This is the shutdown sequence's torque cut, the shell's "power the
    /// duck down". It is deliberately **not** what `robot.enable on:false`
    /// means on the official robot — see [`Self::set_policy_enabled`] for
    /// that, and [`Self::relax`] for the explicit torque switch.
    pub fn disable(&mut self) -> Result<(), RuntimeError> {
        if !self.enabled
            && matches!(self.bringup, Bringup::Limp)
            && !matches!(self.shutdown, Shutdown::AtRest)
        {
            // Already limp: torque is off and the peck would be the second
            // one for a single request. A duck parked at rest is the
            // exception: its shell stop must still be able to cut the torque
            // the rest is holding.
            return Ok(());
        }
        // The low "tock" the robot gives before its servos go quiet.
        self.queue_sound("peck");
        self.safety.set_torque(false)?;
        self.enabled = false;
        self.disabled_hold = None;
        if matches!(self.shutdown, Shutdown::AtRest) {
            // The rest is over once the torque it was holding is cut: the
            // duck is limp, not parked, and the tick must say "limp".
            self.shutdown = Shutdown::Idle;
        }
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

    /// `robot.enable on:false`: stop the policy, keep the torque on.
    ///
    /// Official `robot.enable on:false` only clears the enabled flag; the
    /// driving falling edge stops the recurrent episode (`controller.reset`),
    /// drops a move in flight (`stop_moves`), and the robot holds the home pose
    /// or its seat **with torque still on** (`robotd/src/main.rs:3763-3795`,
    /// `:2701-2719`). `robot.relax` is the one that cuts torque.
    ///
    /// The three verbs are deliberately distinct here:
    /// [`Self::set_policy_enabled`] stops driving and holds torque;
    /// [`Self::relax`] switches torque off and leaves the robot limp but
    /// powered; [`Self::disable`] is the shutdown-path torque cut with the
    /// full state drop the shell has always used.
    pub fn set_policy_enabled(&mut self, on: bool) {
        if on {
            // `enable` has no fallible step today; the ignore is for the
            // signature the shells already call.
            let _ = self.enable();
            return;
        }
        if !self.enabled {
            return;
        }
        self.enabled = false;
        // The pre-takeover posture check runs again on the next Start.
        self.posture_checked = false;
        self.start_from_hold = false;
        // robotd's falling-edge stop: end the recurrent episode and drop a
        // move rather than resume it on the next Start. Torque is untouched.
        self.controller.reset();
        self.controller.stop_moves();
        // The stopped robot is commanded somewhere, not frozen mid-stride: the
        // home pose, or — for a seated robot — the seat it is *measured* to be
        // in, not the nominal seat constant (official `known_positions`,
        // main.rs:3761-3793).
        self.disabled_hold = Some(if self.controller.is_sitting() {
            self.coast
                .map(|sample| sample.positions)
                .unwrap_or(crate::SEAT_POSITION)
        } else {
            DEFAULT_POSITION
        });
        self.twist_fresh = false;
        self.command.twist = [0.0; 3];
        self.twist_ema = [0.0; 3];
        self.head_ema = [0.0; 4];
        self.body_ema = [0.0; 3];
        self.body_active = false;
    }

    /// `robot.relax`: torque off, forget the seat and any move, stay powered.
    ///
    /// Official `robot.relax` cuts torque and returns the bring-up to Limp
    /// (`robotd/src/main.rs:2701-2719`): the robot is still powered but limp,
    /// and the next `robot.init` or Start brings it up from wherever it ends
    /// up. A torque write that fails leaves the robot as it is — robotd warns
    /// and does not pretend the robot went limp.
    pub fn relax(&mut self) {
        if !self.enabled
            && matches!(self.bringup, Bringup::Limp)
            && !matches!(self.shutdown, Shutdown::AtRest)
        {
            // Already limp and not holding a rest: nothing to cut. At the
            // rest pose there is torque to cut even though nothing drives.
            return;
        }
        match self.safety.set_torque(false) {
            Ok(()) => {
                self.enabled = false;
                self.disabled_hold = None;
                if matches!(self.shutdown, Shutdown::AtRest) {
                    // Relaxing out of a rest ends the rest: the duck is limp
                    // and the loop reports that, not a held rest pose.
                    self.shutdown = Shutdown::Idle;
                }
                self.bringup = Bringup::Limp;
                self.limp_fall = LimpFall::Idle;
                self.fall.reset();
                self.carry = None;
                self.controller.forget();
                self.twist_ema = [0.0; 3];
                self.head_ema = [0.0; 4];
                self.body_ema = [0.0; 3];
                self.body_active = false;
                if let Some(watch) = self.pickup.as_mut() {
                    let _ = watch.tick(false, None, self.dt);
                }
            }
            Err(error) => {
                tracing::warn!(%error, "cannot cut torque; the duck is still powered")
            }
        }
    }

    /// `robot.init`: the official power request, in its four branches
    /// (`robotd/src/main.rs:2653-2732`).
    ///
    /// * Powered off: ignored — standing the duck up would hand the power-off a
    ///   robot mid-ramp. The refusal is an `Err` so a caller can say why.
    /// * Limp: torque on and ramp up, which is [`Self::enable`]'s bring-up.
    ///   Unlike Start this needs no policy: "stand up" is a reasonable thing to
    ///   ask of a bench robot.
    /// * Ready and sitting: hold the seat. The home pose is the standing one,
    ///   and ramping a seated robot to it straight-legged pushes it over
    ///   backwards; the next Start hands the seat to the sitstand network.
    /// * Ready otherwise: re-home from wherever the joints are, so a robot
    ///   stopped mid-crouch does not hand that crouch to the policy as its
    ///   starting pose.
    pub fn init(&mut self) -> Result<(), String> {
        if self.powered_off {
            return Err("robot.init ignored: the duck is powering off".to_owned());
        }
        if !matches!(self.shutdown, Shutdown::Idle | Shutdown::AtRest) {
            return Err("robot.init ignored: the duck is shutting down".to_owned());
        }
        // A rest is over the moment the duck is asked to stand: leave the
        // holding state before the bring-up decides where to ramp from.
        if matches!(self.shutdown, Shutdown::AtRest) {
            self.shutdown = Shutdown::Idle;
            self.shutdown_requested = false;
            self.rest_requested = false;
        }
        match self.bringup {
            Bringup::Limp => {
                // `robot.init` is a power request, not Start: torque on and
                // the ramp, **without** enabling the policy (main.rs:2661-2677,
                // 3455-3463). The official loop reads the sample it already
                // has this tick; here init is called between ticks, so it
                // takes one fresh read and leaves the next tick to observe it.
                let sensors = self.safety.read().map_err(|error| error.to_string())?;
                self.safety
                    .set_torque(true)
                    .map_err(|error| error.to_string())?;
                let to = self.ramp_target(&sensors);
                tracing::warn!(
                    ?HOME_RAMP,
                    "robot.init: torque on, ramping to the {}",
                    if to == crate::SEAT_POSITION {
                        "seat"
                    } else {
                        "home pose"
                    }
                );
                self.bringup = Bringup::Homing {
                    from: sensors.positions,
                    since: TickTime::of(&self.clock),
                    to,
                };
                Ok(())
            }
            Bringup::Homing { .. } => {
                tracing::info!("robot.init: the duck is already ramping");
                Ok(())
            }
            Bringup::Ready if self.controller.is_sitting() => {
                tracing::warn!("robot.init: the duck is sitting — holding the seat, not re-homing");
                Ok(())
            }
            Bringup::Ready => {
                let from = self
                    .last_positions()
                    .ok_or_else(|| "robot.init: no measured pose to re-home from".to_owned())?;
                // `init` is a rising request out of a rest: leave the rest
                // state, and keep the policy disabled until Start (the ramp
                // runs regardless; `enabled` only decides who drives after it).
                self.shutdown = Shutdown::Idle;
                self.shutdown_requested = false;
                self.rest_requested = false;
                tracing::warn!("robot.init: re-homing from the current pose");
                self.bringup = Bringup::Homing {
                    from,
                    since: TickTime::of(&self.clock),
                    to: DEFAULT_POSITION,
                };
                Ok(())
            }
        }
    }

    /// The measured joints of the last composed `robot.state` frame, in wire
    /// order. `None` before the first tick (or if a frame was not whole).
    fn last_positions(&self) -> Option<[f64; NUM_JOINTS]> {
        let frame = self.state_frame.as_ref()?;
        if frame.joints.len() != NUM_JOINTS {
            return None;
        }
        let mut positions = [0.0; NUM_JOINTS];
        positions.copy_from_slice(&frame.joints);
        Some(positions)
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
        let now = TickTime::of(&self.clock);
        self.api_lease.take(twist, now);
        // The lease also feeds the duck's twist when the stick is silent, and
        // the deadman ages the loop's one twist clock — so an API driver has
        // to stamp that same clock, or `Safety::gate` would zero the leased
        // twist 500 ms later and a robot with no shell keepalive could never
        // be driven from the API at all. robotd's `robot.move` writes and
        // stamps the one intents channel (main.rs `apply_intent` →
        // `Intents::set_twist`); the deadman punishes a client that stopped
        // talking, not a door the twist came through. The lease still owns
        // the command channel while it is live — its override in the tick is
        // unchanged.
        self.last_twist = now;
        self.twist_fresh = true;
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

    /// The manifest's one-shot skills only — no emotion timelines.
    ///
    /// `skill_names` deliberately speaks `/skills`' shape, where an emotion
    /// is one more skill the robot can perform. The collection page's
    /// "tricks" are the manifest's own moves, and listing the thirteen moods
    /// there a second time would make the page lie about what it holds.
    pub fn manifest_skill_names(&self) -> Vec<String> {
        self.controller.skill_names()
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
        if self.powered_off || self.shutting_down() || !matches!(self.bringup, Bringup::Ready) {
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
        // A celebration is a mood; frozen time skips it quietly rather than
        // refusing a quest that already completed.
        if self.clock.paused {
            return Ok(false);
        }
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
        // One gate for every mood that reaches the body: a rule, speech, a
        // night's wake, a neighbour's cheer, a quest celebration. Frozen time
        // must not count a mood the tick will never perform.
        if self.clock.paused {
            return Err("the duck is paused");
        }
        if self.controller.busy() || self.controller.is_sitting() {
            return Err("a move is running; the duck must be standing still");
        }
        // One at a time, robotd's rule: a request while one plays is
        // dropped (`robotd/src/main.rs:2816-2820`). The duck's own
        // celebrations take the same door, so a completed quest never
        // clobbers a mood a person just asked for.
        if self.emotion_playing.is_some() {
            return Err("an emotion is already playing");
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
        if self.powered_off || self.shutting_down() || !matches!(self.bringup, Bringup::Ready) {
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
        if id
            .as_deref()
            .is_some_and(|id| self.last_chorus_id.as_deref() == Some(id))
        {
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
        if id
            .as_deref()
            .is_some_and(|id| self.last_chorus_id.as_deref() == Some(id))
        {
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
        self.care.budget_s = if minutes == 0 {
            None
        } else {
            Some(minutes * 60)
        };
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
            // `minutes` is OMITTED rather than null: iOS saves this object
            // into UserDefaults, a property list, and a null inside it is
            // an exception at save time - the smoke session proved it.
            let used = (self.book().life.powered_s - self.care.budget_base_s).max(0.0);
            let mut parent = serde_json::json!({
                "used_s": (used * 10.0).round() / 10.0,
                "left_s": self.care.budget_s.map_or(0.0, |budget| {
                    (f64::from(budget) - used).max(0.0)
                }),
            });
            if let Some(seconds) = self.care.budget_s {
                if let Some(map) = parent.as_object_mut() {
                    map.insert("minutes".to_owned(), serde_json::json!(seconds / 60));
                }
            }
            object.insert("parent".to_owned(), parent);
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
            crate::lan::PeerEvent::Replied { from, name } => ("replied", from, name, "yes", "yes"),
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
        self.emotions
            .get(name)
            .map(|emotion| emotion.pcm().to_vec())
    }

    pub fn trigger_skill(&mut self, name: &str) -> Result<bool, &'static str> {
        // Frozen time is the one state where no move may start: every caller
        // — the pad, speech, a child's rule, a neighbour, a model — reaches
        // this primitive, and the book counts what starts. Pet/Feed's own
        // counters are not skills and still happen while frozen.
        if self.clock.paused {
            return Err("the duck is paused");
        }
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

    /// The achieved control-loop rate over the last closed window, in Hz.
    /// Zero until the first window closes — unknown, not stopped.
    pub fn achieved_hz(&self) -> f64 {
        self.achieved_hz
    }

    /// Mark a degraded reason against the health gate: something a caller
    /// knows that the loop itself cannot see — a reload that would not build,
    /// an override that fell back to its default. Degraded, never unhealthy:
    /// the duck kept the policy it had, so there is nothing to roll back.
    pub fn mark_degraded(&mut self, reason: String) {
        if !self.degraded_reasons.contains(&reason) {
            self.degraded_reasons.push(reason);
        }
    }

    /// The marked degraded reasons all cleared — the override that would not
    /// load loaded, or was reset.
    pub fn clear_degraded(&mut self) {
        self.degraded_reasons.clear();
    }

    /// Mark the policy itself unavailable — robotd's `policy_error`
    /// (main.rs:940-942). This is **unhealthy**, not degraded: a daemon that
    /// came up but cannot run its policy holds a pose and never walks, and
    /// that is exactly what the update gate must be able to roll back.
    pub fn mark_policy_error(&mut self, reason: String) {
        self.policy_error = Some(reason);
    }

    /// The policy built again: no unavailable verdict, robotd's
    /// `state.policy_error.store(None)` after a load that lands.
    pub fn clear_policy_error(&mut self) {
        self.policy_error = None;
    }

    /// Consecutive failed bus reads — robotd's `consecutive_errors`, reset by
    /// every good read. At [`MAX_CONSECUTIVE_ERRORS`] the gate says unhealthy.
    pub fn consecutive_errors(&self) -> u32 {
        self.consecutive_errors
    }

    /// The health gate, robotd's `health()` with its two verdicts strictly
    /// apart: unhealthy is a broken control loop or an unusable policy
    /// (stalled, under rate, never ticked, no policy, a run of failed reads),
    /// degraded is a property of the board or of somebody's file — a startup
    /// bus that never answered, an override that fell back. A fall, a limp or a
    /// flat pack never reach this gate — those are reports, and a release must
    /// never be judged by them.
    pub fn health(&self) -> HealthVerdict {
        // "Starting" is not "started". Official `robotd` splits its no-ticks
        // branch: a startup bus that never answered after some attempts is
        // degraded, anything else is unhealthy (main.rs:921-935). The test is
        // "no read has ever succeeded", not a tick counter: the mobile clock
        // advances before the bus read, so after a failed first attempt the
        // tick is already 1 (robotd increments its own count only at the end
        // of a full loop, main.rs:4168).
        if !self.has_read {
            let waiting = self.startup_bus_failures;
            if waiting > 0 {
                return HealthVerdict::Degraded {
                    reasons: vec![format!("no robot on the bus after {waiting} attempts")],
                };
            }
            return HealthVerdict::Unhealthy {
                reasons: vec!["control loop has not completed a cycle yet".to_owned()],
            };
        }
        // A policy that is not there is unhealthy (main.rs:940-942): the loop
        // ticks, but it cannot walk, and reverting the release fixes it.
        if let Some(reason) = &self.policy_error {
            return HealthVerdict::Unhealthy {
                reasons: vec![format!("policy unavailable: {reason}")],
            };
        }
        // A caller-marked degraded reason, robotd's order: before the rate and
        // stall checks, so it never hides behind them.
        if !self.degraded_reasons.is_empty() {
            return HealthVerdict::Degraded {
                reasons: self.degraded_reasons.clone(),
            };
        }
        // A run of failed reads is a fault (main.rs:969-987). The official
        // split between a missing servo (degraded) and a failing read
        // (unhealthy) needs a bus census the phone does not have; the run
        // itself is the honest verdict, in the official wording.
        if self.consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
            return HealthVerdict::Unhealthy {
                reasons: vec![format!(
                    "{} consecutive bus read failures",
                    self.consecutive_errors
                )],
            };
        }
        // The stall and rate floors judge the loop only while it is supposed
        // to be running. A paused duck is stopped on purpose — the shell does
        // not call tick — and is not a wedged loop.
        if !self.clock.paused {
            let stale = self.last_tick_at.elapsed();
            if stale > self.stall_limit() {
                return HealthVerdict::Unhealthy {
                    reasons: vec![format!("control loop stalled for {} ms", stale.as_millis())],
                };
            }
            // The floor moves with the shell's speed dial: the loop is driven
            // at 50 × speed ticks per wall second (iOS
            // `DuckController.ticksForThisFrame`), so a duck deliberately in
            // slow-motion is a feature, not a wedged loop. At 1× this is the
            // official 45 Hz exactly.
            let floor = MIN_ACHIEVED_HZ * self.effective_speed();
            let hz = self.achieved_hz;
            if hz > 0.0 && hz < floor {
                return HealthVerdict::Unhealthy {
                    reasons: vec![format!(
                        "control loop at {hz:.1} Hz, below the {floor:.1} Hz floor"
                    )],
                };
            }
        }
        HealthVerdict::Healthy
    }

    /// The loop's speed for rate judgements: the shell's slow-motion dial,
    /// with a nonsense clock (zero or a NaN from a hand-written checkpoint)
    /// read as full speed rather than dividing by it.
    fn effective_speed(&self) -> f64 {
        if self.clock.speed.is_finite() && self.clock.speed > 0.0 {
            self.clock.speed
        } else {
            1.0
        }
    }

    /// `robotd`'s stall window — 25 periods with no tick at all is a wedged
    /// loop, distinct from a slow one — in wall-clock terms. The shell's
    /// period at the current speed is `dt / speed`, so a 0.25× duck is
    /// entitled to 2 s between ticks before that reads as wedged.
    fn stall_limit(&self) -> Duration {
        let periods = self.dt.as_secs_f64() * f64::from(STALL_PERIODS) / self.effective_speed();
        Duration::from_millis((periods * 1000.0).round().max(1.0) as u64)
    }

    /// What a safety gate or apply did to this tick's command, in the official
    /// wire spelling — `robot.state.movement.limited_by`.
    pub fn limited_by(&self) -> &[String] {
        &self.limits
    }

    /// The `robot.state` frame this tick assembled, in the official wire
    /// shape (duck-ipc-proto). `None` before the first tick. Cloned so the
    /// IPC layer can serialise it off the control thread.
    pub fn robot_state(&self) -> Option<duck_ipc_proto::RobotState> {
        self.state_frame.clone()
    }

    /// `robot.health`, in the official wire shape: the verdict, plus the
    /// reported-but-never-judged facts that ride on the same answer.
    pub fn health_result(&self) -> duck_ipc_proto::HealthResult {
        let (healthy, degraded, reason) = match self.health() {
            HealthVerdict::Healthy => (true, false, None),
            HealthVerdict::Degraded { reasons } => (false, true, Some(reasons.join("; "))),
            HealthVerdict::Unhealthy { reasons } => (false, false, Some(reasons.join("; "))),
        };
        duck_ipc_proto::HealthResult {
            healthy,
            degraded,
            reason,
            // Reported, never judged: a flat pack is a fact about the robot,
            // and a release rolled back over one would be replaced by a
            // release judged on the same flat pack.
            battery: self.volts_ema.map(|volts| duck_ipc_proto::Battery {
                volts,
                percent: duck_control::battery_percent(volts),
            }),
            ..duck_ipc_proto::HealthResult::default()
        }
    }

    /// Read-only access to the body backend, for rendering and diagnostics.
    pub fn io(&self) -> &T {
        self.safety.io()
    }

    pub fn fallen(&self) -> bool {
        self.safety.fallen()
    }

    /// Which file the walking slot resolved to, so a reload can tell whether
    /// the locomotion network itself changes (`robotd`'s `change_disturbs`
    /// question — see [`Self::replace_policy_disturbing`]).
    ///
    /// `Net::Walk` is the one mandatory slot of a loaded [`Policy`], so this is
    /// `Some` for any runtime this crate built: `path_of` returns `None` only
    /// for the optional slots (`stand`, `sitstand`, `ground_pick`, the skills),
    /// never for the walk network.
    pub fn walking_slot_path(&self) -> Option<std::path::PathBuf> {
        self.controller
            .policy_ref()
            .path_of(duck_control::policy::Net::Walk)
    }

    /// Every driving slot and the file behind it, for the shell's disturb gate.
    ///
    /// Official `change_disturbs` compares the slot that is *driving* against
    /// the same slot after the change — walk/stand/sitstand/ground_pick, and
    /// the whole skill list for `Driving::Skill` (`robotd/src/main.rs:2047-2067`).
    /// The mobile core cannot compare a candidate set itself (the shell loads
    /// it), so it publishes the running set's paths by name: the four fixed
    /// slots first, in official `Net` order, then every configured skill by its
    /// own name. `None` means the slot is not loaded — an optional capability
    /// this duck does not have, not an empty path.
    pub fn driving_net_paths(&self) -> Vec<(String, Option<std::path::PathBuf>)> {
        use duck_control::policy::Net;
        let policy = self.controller.policy_ref();
        let mut paths = vec![
            ("walk".to_owned(), policy.path_of(Net::Walk)),
            ("stand".to_owned(), policy.path_of(Net::Stand)),
            ("sitstand".to_owned(), policy.path_of(Net::SitStand)),
            ("ground_pick".to_owned(), policy.path_of(Net::GroundPick)),
        ];
        for (index, skill) in self.controller.skill_tuning().skills.iter().enumerate() {
            paths.push((skill.name.clone(), policy.path_of(Net::Skill(index))));
        }
        paths
    }

    /// Whether restarting motor control now would put the duck on the floor —
    /// robotd's `safe_to_restart` (main.rs:1093-1113).
    ///
    /// A walking robot, a scripted move, a ramp, the limp-fall or a shutdown is
    /// travelling; restarting mid-stride is how a robot falls over
    /// (`updater-design.md` §7.2). A robot merely standing, or already down, is
    /// safe to interrupt — it is going nowhere either way.
    pub fn safe_to_restart(&self) -> bool {
        if self.safety.fallen() {
            return true;
        }
        let moving = matches!(self.bringup, Bringup::Homing { .. })
            // A sit or a rest ramp is travelling; a duck parked at the rest
            // pose is not, and is as safe to interrupt as one standing still.
            || matches!(
                self.shutdown,
                Shutdown::Sitting { .. } | Shutdown::Resting { .. }
            )
            || !matches!(self.limp_fall, LimpFall::Idle)
            || self.controller.busy()
            // The applied (deadman-gated, smoothed) twist, not the raw
            // request: a stick whose lease expired is not a moving robot. The
            // 1e-3 bound is the loop's own "still" threshold (robotd's idle
            // gate), so the EMA's residual tail is not mistaken for a stride.
            || (self.driving_prev
                && self.twist_ema.iter().any(|value| value.abs() >= 1e-3));
        !moving
    }

    /// Swap the policy set, keeping the robot's state: the same body, the
    /// same seat, the same skill, the same filter anchor — robotd's
    /// `Controller::carry_over`. Networks whose digest is unchanged keep
    /// their recurrent memory; a genuinely different file starts fresh.
    ///
    /// This is the in-place swap, applied now, whatever the robot is doing.
    /// A change that replaces the network driving the robot wants
    /// [`Self::replace_policy_disturbing`] instead, which honours robotd's
    /// disturb gate and walks the duck home first.
    pub fn replace_policy(&mut self, policy: Policy, tuning: Tuning, skills: SkillTuning) {
        // An explicit immediate swap supersedes a queued one: letting the
        // stale change land later would undo the set installed right here.
        self.pending_swap = None;
        self.land_swap(policy, tuning, skills);
    }

    /// The swap itself: what [`Self::replace_policy`] does and what the tick
    /// does when a queued change lands.
    fn land_swap(&mut self, policy: Policy, tuning: Tuning, skills: SkillTuning) {
        let old = std::mem::replace(
            &mut self.controller,
            Controller::new(policy, tuning, skills),
        );
        self.controller.carry_over(&old);
        // A reload during a fall must not cancel the reflex: robotd refuses
        // the swap until the robot is at rest, and the mobile core keeps the
        // sequence running and lets it finish.
        if matches!(self.limp_fall, LimpFall::Idle) {
            self.controller.reset();
        }
    }

    /// Ask for the policy set to be swapped, honouring robotd's
    /// `change_disturbs` gate (main.rs:3015-3106): a change that replaces the
    /// network the robot is driving is not swapped under a moving gait — the
    /// duck is sent to the home pose first and the change lands there, torque
    /// on throughout. A change that leaves the driving network alone lands
    /// wherever the robot is.
    ///
    /// `disturbs` is the caller's verdict, because the mobile core has no file
    /// digests to compare: the shell that picked the new set knows whether it
    /// replaces the network in the driving slot (`Policy` exposes paths, not
    /// digests — see `duck-control::policy::Policy::path_of`). Pass `false` to
    /// request the in-place landing without the gate.
    ///
    /// The change is applied by the control loop, not by this call, so it can
    /// wait for the pose; the returned verdict says which path it took and is
    /// what the caller should report. A duck that is off or shutting down
    /// refuses the change outright, as the official loop does.
    pub fn replace_policy_disturbing(
        &mut self,
        policy: Policy,
        tuning: Tuning,
        skills: SkillTuning,
        disturbs: bool,
    ) -> PolicySwap {
        if self.shutting_down() || self.shutdown_requested {
            tracing::warn!("policy change refused: the robot is shutting down");
            return PolicySwap::Refused;
        }
        // Not driving, or a change that leaves the driving network alone: the
        // loop lands it on the next tick without moving the robot. Asked while
        // the limp-fall owns the robot, it waits for the sequence to hand back.
        let driving = self.driving_for_swap();
        let verdict = if disturbs && driving {
            PolicySwap::GoingHome
        } else {
            PolicySwap::InPlace
        };
        self.pending_swap = Some(PendingSwap {
            policy,
            tuning,
            skills,
            disturbs,
            seeking: false,
        });
        verdict
    }

    /// Whether a policy change would take the network out from under a robot
    /// with a gait in motion — robotd's `driving` at the moment the change is
    /// taken (main.rs:3040-3046: the loop only asks what stepped last tick): a
    /// robot that is disabled, limp or mid-ramp has nothing running that a swap
    /// could interrupt, and a parked seat is not a moving gait —
    /// `change_disturbs` lands a change to a seated robot in place, because
    /// standing it up to change a file it is not using is a surprise of its own
    /// (robotd main.rs:2040-2057).
    fn driving_for_swap(&self) -> bool {
        self.driving_prev
            && self
                .controller
                .driving()
                .is_some_and(|d| !matches!(d, crate::control::Driving::Seated))
    }

    /// A queued policy change, if one is waiting for a safe pose.
    pub fn swap_pending(&self) -> bool {
        self.pending_swap.is_some()
    }

    /// Where a queued policy change is in its approach, for the shell to
    /// report: standing by for the robot to be free, walking home, or landed.
    pub fn swap_state(&self) -> Option<PolicySwap> {
        let swap = self.pending_swap.as_ref()?;
        Some(if swap.seeking {
            PolicySwap::GoingHome
        } else if swap.disturbs && self.driving_for_swap() {
            PolicySwap::GoingHome
        } else {
            PolicySwap::InPlace
        })
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
        // The voice is the duck's, not the build's: a branch speaks with the
        // same personality its parent was given.
        fork.voice = self.voice.clone();
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
            LimpFall::Posing { from, since } => {
                ("posing".to_owned(), since.0, 0, Some(from.to_vec()))
            }
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
                Shutdown::Sitting {
                    power_off: true, ..
                } => "sitting".to_owned(),
                Shutdown::Sitting {
                    power_off: false, ..
                } => "sitting_hold".to_owned(),
                Shutdown::Resting {
                    power_off: true, ..
                } => "resting".to_owned(),
                Shutdown::Resting {
                    power_off: false, ..
                } => "resting_hold".to_owned(),
                Shutdown::AtRest => "at_rest".to_owned(),
            },
            shutdown_since: match self.shutdown {
                Shutdown::Sitting { since, .. } | Shutdown::Resting { since, .. } => since.0,
                Shutdown::Idle | Shutdown::AtRest => 0,
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
            driving_prev: self.driving_prev,
            stopped_driving_at: self.stopped_driving_at.map(|stopped| stopped.0),
            posture_checked: self.posture_checked,
            start_from_hold: self.start_from_hold,
            volts_ema: self.volts_ema,
            rest_requested: self.rest_requested,
            shutdown_power_off: match self.shutdown {
                Shutdown::Sitting { power_off, .. } | Shutdown::Resting { power_off, .. } => {
                    power_off
                }
                Shutdown::Idle | Shutdown::AtRest => true,
            },
            policy_error: self.policy_error.clone(),
            consecutive_errors: self.consecutive_errors,
            startup_bus_failures: self.startup_bus_failures,
            has_read: self.has_read,
            disabled_hold: self.disabled_hold.map(|pose| pose.to_vec()),
            coast_positions: self.coast.map(|sample| sample.positions.to_vec()),
            sensors_read_ns: self.sensors_read_ns,
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
        self.emotion_playing =
            checkpoint
                .emotion_name
                .clone()
                .map(|name| crate::emotion::Playing {
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
        // A policy change queued in the future this duck just left goes with
        // it: the `Policy` inside it cannot ride a checkpoint (ONNX sessions
        // are not serialised), and landing a change the rewound timeline never
        // asked for would be a swap from a past that no longer exists. The
        // caller re-asks if it still wants it.
        self.pending_swap = None;
        // The driving-edge bookkeeping comes back with everything else. Left
        // out, the restored duck's first tick sees `driving_prev = false`
        // under the restored non-zero twist: a false rising edge, which
        // `reset_on_resume` (with no `stopped_driving_at`) answers by wiping
        // the controller state the restore had just put back, and the re-run
        // posture check picks the branch's start pose from a future the
        // branch does not have.
        self.driving_prev = checkpoint.driving_prev;
        self.stopped_driving_at = checkpoint.stopped_driving_at.map(TickTime);
        self.posture_checked = checkpoint.posture_checked;
        self.start_from_hold = checkpoint.start_from_hold;
        self.volts_ema = checkpoint.volts_ema;
        self.policy_error = checkpoint.policy_error.clone();
        self.consecutive_errors = checkpoint.consecutive_errors;
        self.startup_bus_failures = checkpoint.startup_bus_failures;
        self.has_read = checkpoint.has_read;
        self.disabled_hold = checkpoint
            .disabled_hold
            .as_deref()
            .filter(|pose| pose.len() == NUM_JOINTS)
            .map(|pose| {
                let mut target = [0.0; NUM_JOINTS];
                target.copy_from_slice(pose);
                target
            });
        self.coast = checkpoint
            .coast_positions
            .as_deref()
            .filter(|positions| positions.len() == NUM_JOINTS)
            .map(|positions| {
                let mut sample = Sensors::default();
                sample.positions.copy_from_slice(positions);
                sample
            });
        self.sensors_read_ns = checkpoint.sensors_read_ns;
        if self.has_read && self.coast.is_none() {
            // A checkpoint with no sample cannot honestly claim a read: the
            // two would disagree on the first failed read after a fork.
            self.has_read = false;
        }
        self.shutdown = match checkpoint.shutdown.as_str() {
            "sitting" => Shutdown::Sitting {
                since: TickTime(checkpoint.shutdown_since),
                power_off: checkpoint.shutdown_power_off,
            },
            "sitting_hold" => Shutdown::Sitting {
                since: TickTime(checkpoint.shutdown_since),
                power_off: false,
            },
            "resting" => Shutdown::Resting {
                from: array_of(checkpoint.shutdown_from.as_deref()),
                since: TickTime(checkpoint.shutdown_since),
                power_off: checkpoint.shutdown_power_off,
            },
            "resting_hold" => Shutdown::Resting {
                from: array_of(checkpoint.shutdown_from.as_deref()),
                since: TickTime(checkpoint.shutdown_since),
                power_off: false,
            },
            "at_rest" => Shutdown::AtRest,
            _ => Shutdown::Idle,
        };
        self.shutdown_requested = false;
        self.rest_requested = checkpoint.rest_requested;
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
        // The expected rate just changed; a window opened under the old speed
        // measures a loop that no longer exists, and closing it would judge
        // the new rate against a stale count.
        self.hz_window_start = Some(Instant::now());
        self.hz_window_ticks = 0;
    }

    /// One 50 Hz tick. The order is `robotd`'s: read, observe, gate, bring-up,
    /// shutdown, limp-fall, pickup, smoothing, policy, safety.
    ///
    /// Simulated time advances by exactly 20 ms per tick, whatever the wall
    /// clock did, so a paused or slowed duck keeps its own timers.
    pub fn tick(&mut self) -> Result<TickReport, RuntimeError> {
        let tick_started_at = Instant::now();
        // A gap longer than the stall window is frozen time (a pause the
        // shell routed around `tick`) or a wedged caller, not a slow loop:
        // the wall-clock window would dilute the rate with the gap and the
        // resume beat would report a second of false slowness. Restart the
        // window; this tick is its first sample. The floor `health()` judges
        // against moves with `clock.speed` for the other half of the same
        // problem — a deliberately slowed duck is a product feature.
        if self.last_tick_at.elapsed() > self.stall_limit() {
            self.hz_window_start = Some(tick_started_at);
            self.hz_window_ticks = 0;
        }
        let report = self.tick_inner();
        // The rate window and the stall clock run on every tick, whatever the
        // tick's state — a limp duck is still a looping duck, and the health
        // gate judges the loop, not the state machine (robotd counts the same
        // way, at the very end of the tick, whatever the tick decided).
        self.hz_window_ticks += 1;
        if self
            .hz_window_start
            .is_none_or(|start| start.elapsed() >= RATE_WINDOW)
        {
            let window = self
                .hz_window_start
                .map_or(RATE_WINDOW, |start| start.elapsed());
            let window = window.as_secs_f64().max(f64::EPSILON);
            self.achieved_hz = self.hz_window_ticks as f64 / window;
            self.hz_window_start = Some(Instant::now());
            self.hz_window_ticks = 0;
        }
        self.last_tick_at = Instant::now();
        if tick_started_at.elapsed() > self.dt {
            // Ticks whose work overran the period, cumulative — robotd's
            // `missed`, distinct from a rate shortfall: this is the loop doing
            // too much, a low rate is the loop being woken late.
            self.missed_ticks += 1;
        }
        let report = report?;
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
        }
        // Timer rules fire on the duck's own clock: every whole simulated
        // period, a rule whose trigger is "timer" gets its moment. This is
        // independent of the parent's allowance — a timer is a rule the child
        // wrote, not a budget feature — and a paused duck never reaches this
        // code, so frozen time fires nothing. A sleeping duck is left alone:
        // a night, or a doze, is not a time to start a skill (COMPANION §5/§9,
        // "定时触发"; care's `asleep` is the one gate every autonomous wake
        // already respects).
        if !self.care.asleep {
            let due: Vec<usize> =
                self.rules
                    .rules
                    .iter()
                    .enumerate()
                    .filter(|(index, rule)| {
                        rule.trigger == "timer"
                            && crate::rules::Rules::timer_period_s(rule.period_s).is_some_and(
                                |period| {
                                    let period_ticks = u64::from(period) * 50;
                                    self.clock.tick.saturating_sub(
                                        *self.rules.timer_last.get(index).unwrap_or(&0),
                                    ) >= period_ticks
                                },
                            )
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
        // The bus read, with robotd's coast (main.rs:2562-2620): a failed read
        // repeats the last good sample and counts the failure; a good read
        // clears the count. Past `COAST_TICKS` repeats the sample is too old to
        // step the policy from, so the duck holds and the loop keeps ticking —
        // a wedged loop is not a bus that dropped a transaction.
        let read_at_ns = self.clock.tick.saturating_mul(self.dt.as_nanos() as u64);
        let read_ok;
        let sensors: Sensors = match self.safety.read() {
            Ok(fresh) => {
                read_ok = true;
                self.has_read = true;
                self.consecutive_errors = 0;
                self.startup_bus_failures = 0;
                self.coast = Some(fresh);
                // `sensors_read_ns` is stamped at the read, and rides the
                // coast: a tick that could not read republishes the last fresh
                // sample's read time, not its own.
                self.sensors_read_ns = read_at_ns;
                fresh
            }
            Err(error) => {
                read_ok = false;
                self.consecutive_errors = self.consecutive_errors.saturating_add(1);
                if self.coast.is_none() {
                    // Nothing has ever been read: this is the startup bus
                    // that never answered, robotd's `startup_bus_failures`.
                    self.startup_bus_failures = self.startup_bus_failures.saturating_add(1);
                }
                tracing::warn!(
                    %error,
                    consecutive = self.consecutive_errors,
                    "bus read failed"
                );
                match self.coast {
                    // Nothing has ever been read: there is no sample to coast
                    // on and no pose to report. This is the one read failure
                    // the loop cannot absorb.
                    None => return Err(error.into()),
                    Some(last) => last,
                }
            }
        };
        // A coasted sample is for holding, not for observing: repeating a
        // stale pose into the fall debounce or the odometry filter would tell
        // them the robot froze, which it did not (main.rs:2612-2629). The IMU
        // sample count follows the same rule — one fused sample per fresh read.
        if read_ok {
            self.imu_samples = self.imu_samples.saturating_add(1);
            self.safety.observe(&sensors, self.dt);
            if self.imu_ready() {
                self.odometry.update(&sensors.positions, sensors.imu.quat);
            }
        }
        // The sample has been repeated for longer than `COAST_TICKS`: it may
        // hold the pose and describe the robot, but no policy steps from it.
        let coast_exhausted = self.consecutive_errors > COAST_TICKS;
        let hold = sensors.positions;

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
                // `robotd`'s battery_v: the slow read EMA'd with
                // `BATTERY_EMA_ALPHA` (0.1), seeded from the first reading
                // rather than blending up from zero. The empty-pack threshold
                // reads the same EMA below.
                self.volts_ema = Some(match self.volts_ema {
                    Some(previous) if previous > 0.0 => 0.1 * slow.volts + 0.9 * previous,
                    _ => slow.volts,
                });
                let volts = self.volts_ema.unwrap_or(slow.volts);
                // One mapping, robotd's: `duck_control::battery_percent`
                // (6.6..8.2 V, BATTERY_EMPTY_V..BATTERY_FULL_V). The same
                // number `robot.health` shows and the gauge colours by — a
                // second window here is how two screens end up disagreeing
                // about the same pack.
                let percent = duck_control::battery_percent(volts);
                self.face.gauge = self.face.gauge.next(Some(percent));
                self.face.battery_percent = Some(percent);
                // The status LED reads the health gate, robotd's leds do: the
                // verdict, and "starting" for a loop that has not ticked yet.
                let (healthy, degraded) = match self.health() {
                    HealthVerdict::Healthy => (true, false),
                    HealthVerdict::Degraded { .. } => (false, true),
                    HealthVerdict::Unhealthy { .. } => (false, false),
                };
                self.face.status = crate::leds::Status::of(healthy, degraded, self.clock.tick == 0);
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

        // Command assembly — the mobile analogue of reading intents, which
        // robotd does before the gate: the painted route feeds its twist into
        // the same channel the stick uses, and the API lease takes the channel
        // while it is live.
        //
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
        // A change already walking the robot home stops the duck on the way:
        // the point of going home first is that the new network takes over a
        // robot that is not mid-stride, so a held stick or a live lease must
        // not keep it moving while it travels.
        let seeking_home = self.pending_swap.as_ref().is_some_and(|swap| swap.seeking);

        // The gate, robotd's: `Safety::gate` owns the deadman — intents older
        // than `SafetyConfig::deadman` have their twist zeroed, and the reason
        // comes back as a `Limit::Deadman` for `limited_by`. A twist that has
        // never arrived reads as maximally stale, exactly like the official
        // loop's stamped-at-zero intents.
        let intent_age = if self.twist_fresh {
            self.last_twist.elapsed(now)
        } else {
            Duration::MAX
        };
        // What the client asked, before the gate: the wire's `requested` is
        // the ask, `applied` is what the robot got, and the deadman is
        // visible in the difference plus `limited_by`.
        let requested_twist = command.twist;
        if seeking_home {
            command.twist = [0.0; 3];
        }
        let (gated, deadman) = self.safety.gate(command, intent_age);
        command = gated;
        // Collected per tick and published: `robot.state.movement.limited_by`.
        self.limits = deadman
            .map(limit_name)
            .map(str::to_owned)
            .into_iter()
            .collect();

        // The disturb gate: robotd's `change_disturbs` and the home-first swap
        // it drives (main.rs:3015-3106, :3511-3575). The caller has decided
        // *whether* the change replaces the network driving — the mobile core
        // has no file digests to compare, only what the shell's manifest
        // comparison says — and the loop decides *where* it lands.
        //
        // Never mid-ramp: that is the one state where the joints are moving
        // somewhere they have not arrived, which is exactly what a swap under a
        // gait must not do. A change that takes the driving network out from
        // under a robot with a gait in motion is sent home first and lands once
        // the ramp is complete. A change that leaves the driving network alone
        // lands wherever the robot is parked — a seated or limp duck is not a
        // moving gait, and standing one up to change a file it is not using
        // would be a surprise of its own.
        if self.pending_swap.is_some() {
            let (disturbing, seeking) = self
                .pending_swap
                .as_ref()
                .map_or((false, false), |swap| (swap.disturbs, swap.seeking));
            // Mid-ramp by the bring-up's own test — the same predicate that
            // flips `Homing` to `Ready` below, so a change whose ramp has just
            // finished lands on the tick it arrives, not one later.
            let mid_ramp = matches!(self.bringup, Bringup::Homing { .. })
                && self.bringup.homing_target(now).is_some();
            // Driving as robotd sees it at the change (main.rs:3040-3046): the
            // network that stepped last tick, and not a parked seat. Decided
            // once and remembered — a change that is already walking the robot
            // home must not be re-decided from a stick the client is still
            // holding, or the duck would go home again every tick.
            let driving_now = self.driving_for_swap();
            if self.shutting_down() || self.shutdown_requested {
                // The runtime's half of the official refusal (the shell's
                // caller refuses too): from inside a shutdown the path home
                // stands the robot up at gain until the sit cuts the torque out
                // from under it. Nothing about a shutdown wants a new network.
                tracing::warn!("policy change refused: the robot is shutting down");
                self.pending_swap = None;
            } else if !matches!(self.limp_fall, LimpFall::Idle) {
                // The limp-fall sequence owns the robot; the change waits for
                // it to hand back.
            } else if disturbing && !seeking && driving_now {
                tracing::warn!(
                    "policy change replaces the network driving: going home before swapping"
                );
                self.bringup = Bringup::Homing {
                    from: report.sensors.positions,
                    since: now,
                    to: DEFAULT_POSITION,
                };
                if let Some(swap) = self.pending_swap.as_mut() {
                    swap.seeking = true;
                }
                // A route walked into the swap is over: the duck is going home,
                // not to the child's waypoint.
                self.cancel_path();
            } else if !mid_ramp {
                let swap = self.pending_swap.take().expect("checked above");
                if swap.seeking {
                    tracing::warn!("at the home pose; the policy change lands");
                } else {
                    tracing::warn!(
                        in_place = !swap.disturbs,
                        "policy change lands where the robot stands"
                    );
                }
                self.land_swap(swap.policy, swap.tuning, swap.skills);
            }
        }

        // The shutdown/rest request and the empty pack are checked before the
        // bring-up transition: a duck that is limp or still homing must
        // still be able to power off (robotd cuts torque immediately when it
        // cannot sit). A rest is taken only when no shutdown was asked for in
        // the same tick — robotd's `!shutdown && rest` — and a shutdown asked
        // for from a rest runs straight to the power-off, which is what an
        // unpowered `begin_shutdown` branch already does on the holding robot.
        if !self.powered_off && matches!(self.shutdown, Shutdown::Idle | Shutdown::AtRest) {
            let empty = self
                .volts_ema
                .is_some_and(|volts| volts <= duck_control::model::BATTERY_EMPTY_V);
            if empty || self.shutdown_requested {
                self.shutdown_requested = false;
                self.rest_requested = false;
                self.begin_shutdown(now, hold, true);
            } else if self.rest_requested {
                self.rest_requested = false;
                // Already at the rest pose: the request is satisfied, and
                // running the ramp again would be a second nap for one press.
                if matches!(self.shutdown, Shutdown::Idle) {
                    self.begin_shutdown(now, hold, false);
                }
            }
        }

        // Off is a state of the duck, not a single tick: after the shutdown's
        // torque cut the body is limp, and reporting "limp" would hide that the
        // duck was deliberately powered down.
        if self.powered_off {
            let applied = self.safety.apply(hold, hold, 0)?;
            self.limits
                .extend(applied.limits.iter().map(|l| limit_name(*l).to_owned()));
            self.driving_prev = false;
            report.label = "off".to_owned();
            report.gain = 0;
            report.targets = hold;
            // Off is reported, not frozen: a subscriber must see the duck off
            // rather than the standing frame from before the torque cut.
            self.state_frame = Some(self.compose_state_frame(
                &sensors,
                requested_twist,
                command.twist,
                command.head,
                &report.label,
                report.picked_up,
                &hold,
            ));
            return Ok(report);
        }

        // The shutdown/rest owns the robot from the moment it starts: sit,
        // rest, torque off. It runs before the bring-up match on purpose: a
        // rest parks with `bringup = Limp`, and behind that match's Limp arm
        // the AtRest steady state would be unreachable (the arm would command
        // the measured pose at gain 0 and call it limp). `robotd` runs the
        // same sequence for `robot.shutdown`, `robot.rest`, and a genuinely
        // empty pack (a 10 s EMA at or below 6.6 V).
        match self.shutdown {
            Shutdown::Idle => {}
            Shutdown::Sitting { since, power_off } => {
                // Named apart from `hold`, which is the measured pose.
                let hold_s = self.controller.shutdown_sit_secs();
                // The sitstand network drives the seat through the same
                // controller.step the walk uses, so the same voltage
                // adaptation scales it — robotd's one step call covers the
                // sit exactly because `driving` stays true while the
                // shutdown sit runs.
                let scale = voltage_scale(self.volts_ema);
                let target = self.controller.step(
                    &sensors,
                    &self.command,
                    false,
                    self.dt.as_secs_f64(),
                    scale,
                );
                match target {
                    Ok(step) => {
                        let applied = self.safety.apply(step.targets, hold, step.gain)?;
                        self.limits
                            .extend(applied.limits.iter().map(|l| limit_name(*l).to_owned()));
                        report.label = "shutdown".to_owned();
                        report.targets = step.targets;
                        report.gain = step.gain;
                    }
                    Err(_) => {
                        let applied = self.safety.apply(hold, hold, 200)?;
                        self.limits
                            .extend(applied.limits.iter().map(|l| limit_name(*l).to_owned()));
                        report.targets = hold;
                    }
                }
                // The sit owns the robot: not driving, and driving stopped
                // when the sit started — the resume reset measures from here.
                if self.driving_prev {
                    self.stopped_driving_at = Some(now);
                }
                self.driving_prev = false;
                if since.elapsed(now).as_secs_f64() >= hold_s {
                    tracing::warn!("shutdown: sit complete, easing into the rest pose");
                    self.shutdown = Shutdown::Resting {
                        from: report.targets,
                        since: now,
                        power_off,
                    };
                }
                report.fallen = self.safety.fallen();
                // The sit owns the robot: publish it as it is, not as the
                // frame from before the shutdown started.
                self.state_frame = Some(self.compose_state_frame(
                    &sensors,
                    requested_twist,
                    command.twist,
                    command.head,
                    &report.label,
                    report.picked_up,
                    &report.targets,
                ));
                return Ok(report);
            }
            Shutdown::Resting {
                from,
                since,
                power_off,
            } => {
                let t = (since.elapsed(now).as_secs_f64() / 1.0).min(1.0);
                let mut target = [0.0; NUM_JOINTS];
                for (index, slot) in target.iter_mut().enumerate() {
                    *slot =
                        from[index] + (duck_control::model::REST_POSITION[index] - from[index]) * t;
                }
                let holding = since.elapsed(now).as_secs_f64() >= 2.0;
                if holding && power_off {
                    // The rest is the last pose the servos see: cutting torque
                    // from the seat let the duck sag forward; from here there
                    // is nowhere left to sag.
                    let applied = self.safety.apply(target, target, 200)?;
                    self.limits
                        .extend(applied.limits.iter().map(|l| limit_name(*l).to_owned()));
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
                } else if holding {
                    // A rest stays powered: the official robot ends it with a
                    // servo reboot that leaves it limp, but the phone has no
                    // servos to reboot, and the shell's own power switch is the
                    // only thing that may cut torque here. The rest pose is
                    // held with torque kept; `robot.init` stands it up again.
                    let applied = self.safety.apply(target, target, 200)?;
                    self.limits
                        .extend(applied.limits.iter().map(|l| limit_name(*l).to_owned()));
                    self.enabled = false;
                    self.bringup = Bringup::Limp;
                    self.controller.forget();
                    self.shutdown = Shutdown::AtRest;
                    report.label = "rest".to_owned();
                    report.gain = 200;
                    report.targets = target;
                    tracing::warn!("rest: at the rest pose, torque held — the duck stays powered");
                } else {
                    let applied = self.safety.apply(target, target, report.gain.max(200))?;
                    self.limits
                        .extend(applied.limits.iter().map(|l| limit_name(*l).to_owned()));
                    report.label = "rest".to_owned();
                    report.targets = target;
                }
                self.driving_prev = false;
                report.fallen = self.safety.fallen();
                // The rest pose, the torque cut and the off latch are all
                // decided above: the frame the subscriber sees must be the one
                // this tick ended in (label, gain and powered state included),
                // never the pre-shutdown frame.
                self.state_frame = Some(self.compose_state_frame(
                    &sensors,
                    requested_twist,
                    command.twist,
                    command.head,
                    &report.label,
                    report.picked_up,
                    &target,
                ));
                return Ok(report);
            }
            Shutdown::AtRest => {
                // A rest that has arrived: the rest pose, held, with torque
                // kept. The duck is powered — `robot.shutdown` is what takes it
                // off — and `robot.init` leaves this state by re-homing.
                let target = duck_control::model::REST_POSITION;
                let applied = self.safety.apply(target, hold, 200)?;
                self.limits
                    .extend(applied.limits.iter().map(|l| limit_name(*l).to_owned()));
                self.enabled = false;
                self.driving_prev = false;
                report.label = "rest".to_owned();
                report.gain = 200;
                report.targets = target;
                report.fallen = self.safety.fallen();
                self.state_frame = Some(self.compose_state_frame(
                    &sensors,
                    requested_twist,
                    command.twist,
                    command.head,
                    &report.label,
                    report.picked_up,
                    &target,
                ));
                return Ok(report);
            }
        }

        // Bring the robot up when someone asks it to drive and it has no torque
        // yet — robotd's enable-driven bring-up, in its own place in the tick:
        // after the gate, before everything that owns the robot.
        //
        // Not gated on the fall: a fallen robot ramps like any other, as the
        // prototype does. Being down is a report, and the humans stay in charge.
        //
        // The ramp target is where the robot actually is: a robot that reads
        // seated ramps to the seat (and the controller marks the seat), never
        // straight-legged to a standing home pose.
        if !self.powered_off && self.enabled && matches!(self.bringup, Bringup::Limp) {
            match self.safety.set_torque(true) {
                Ok(()) => {
                    let to = self.ramp_target(&sensors);
                    tracing::warn!(
                        ?HOME_RAMP,
                        "enabling the policy: torque on, ramping to the {}",
                        if to == crate::SEAT_POSITION {
                            "seat"
                        } else {
                            "home pose"
                        }
                    );
                    self.bringup = Bringup::Homing {
                        from: sensors.positions,
                        since: now,
                        to,
                    };
                }
                // Reported, not fatal, and it stays `Limp` so the next tick
                // tries again: a bus that dropped one transaction is ordinary,
                // and a robot that refused to ever come up because of it would
                // be worse than one that keeps asking.
                Err(e) => tracing::warn!(error = %e, "cannot enable torque; the duck stays limp"),
            }
        }

        // The ramp finishing is what makes the policy eligible to drive. No
        // shortcut: `homing_target` returns None only after the full
        // `HOME_RAMP`, which is also the IMU's convergence window.
        if let Bringup::Homing { to, .. } = self.bringup {
            if self.bringup.homing_target(now).is_none() {
                if to == crate::SEAT_POSITION {
                    tracing::warn!("at the seat pose, holding it; Start again rises");
                } else {
                    tracing::warn!("at the home pose; the policy has the robot");
                }
                self.bringup = Bringup::Ready;
                // robotd refreshes the hold to the pose the ramp reached at
                // its end (main.rs:3535-3537). Without this a stopped policy
                // would replay its old hold — the standing home — over a
                // seated init the very tick the seat ramp finishes, which is
                // the straight-legged stand-up out of the seat the seat ramp
                // exists to avoid. Driving clears this again below.
                self.disabled_hold = Some(to);
                // No controller reset here — robotd resets through the driving
                // rising edge below, gated on `RESET_AFTER_PAUSE`.
            }
        }

        match self.bringup {
            Bringup::Limp => {
                // Torque off, but still write and step: a limp duck has to
                // fall, not freeze in the pose it was standing in.
                let applied = self.safety.apply(hold, hold, 0)?;
                self.limits
                    .extend(applied.limits.iter().map(|l| limit_name(*l).to_owned()));
                // The edge bookkeeping the early return skips: the robot is
                // not driving here, and driving stopped when this started.
                if self.driving_prev {
                    self.stopped_driving_at = Some(now);
                }
                self.driving_prev = false;
                report.label = "limp".to_owned();
                report.gain = 0;
                report.targets = hold;
                if let Some(watch) = self.pickup.as_mut() {
                    watch.commanded(&hold);
                }
                // A limp duck is a state the wire has to show, not the last
                // standing frame replayed.
                self.state_frame = Some(self.compose_state_frame(
                    &sensors,
                    requested_twist,
                    command.twist,
                    command.head,
                    &report.label,
                    report.picked_up,
                    &hold,
                ));
                return Ok(report);
            }
            // Homing falls through: the sequence below still runs on a homing
            // tick (limp-fall, pickup, the command smoothing), and the ramp's
            // own target comes out of the final target match.
            Bringup::Homing { .. } | Bringup::Ready => {}
        }

        // Limp-fall, robotd's sequence: go soft before the landing, let the
        // robot arrive limp, put it back into the standing pose, and only then
        // hand it to the policy. Read from the fall predictor, not the `fallen`
        // verdict: a verdict that waits for the robot to be down cannot make
        // it soft before it lands. Eligibility is robotd's: the robot was
        // driving last tick (`was_driving`), so the sequence never fires out
        // of a state that did not have the policy driving.
        let was_driving = self.driving_prev;

        // Abandoned mid-sequence: something else took the robot — disabled, a
        // shutdown started, the power went. robotd's own condition.
        if !matches!(self.limp_fall, LimpFall::Idle)
            && !(self.enabled
                && matches!(self.bringup, Bringup::Ready)
                && matches!(self.shutdown, Shutdown::Idle)
                && !self.powered_off)
        {
            tracing::warn!("limp-fall abandoned — something else took the robot");
            self.limp_fall = LimpFall::Idle;
            self.fall.reset();
        }

        match self.limp_fall {
            LimpFall::Idle => {
                let eligible = was_driving
                    && !self.safety.fallen()
                    && matches!(self.shutdown, Shutdown::Idle)
                    && !self.powered_off
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
                let rate = Some(sensors.imu.gyro.iter().map(|w| w * w).sum::<f64>().sqrt());
                let landed = landing.observe(rate, self.dt, LIMP_FALL_STILL_RATE, LIMP_FALL_STILL);
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
                    // No controller reset here — robotd resets through the
                    // driving rising edge below, gated on `RESET_AFTER_PAUSE`
                    // against when driving actually stopped.
                }
            }
        }

        // The debounced verdict is a fall too: a duck on the floor cannot walk a
        // route, and standing back up must not resume one.
        if self.safety.fallen() {
            self.cancel_path();
        }

        // Pick-up detection, robotd's: pause the walking networks while somebody
        // holds the robot and hand it back when it is on the floor. Watches only
        // while those two networks have the robot — everything that owns it
        // outright (a skill, a sit, the limp-fall, a disable, the shutdown sit)
        // ends a pause and resets the watch. **Roller mode is not watched: the
        // model has never seen wheels.**
        let mut pickup_paused = false;
        // Read the loop's own facts before the watch borrows itself mutably.
        let dt = self.dt;
        let ready = self.enabled && matches!(self.bringup, Bringup::Ready);
        let settled = matches!(self.limp_fall, LimpFall::Idle);
        let free = !self.controller.busy() && !self.controller.is_sitting();
        let watching = self.pickup_watching(ready, settled, warm, free);
        if let Some(watch) = self.pickup.as_mut() {
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

        // Before the policy takes over a robot at its home pose, look at it:
        // the ramp there is open-loop, and a robot that started folded can end
        // it standing, sat back on its seat or on its back (robotd's
        // `posture::classify`, run once per enable episode). Seated, the
        // sitstand network settles into the seat and rises first. Anything else,
        // the gait takes over as it always has: this only picks how the policy
        // starts, never whether.
        //
        // Measured even when the seat is known — held in it since the first
        // Start, or stopped while sitting: the robot is where the sensors say,
        // not where it was put.
        if !self.enabled {
            self.posture_checked = false;
        }
        if self.enabled
            && !self.posture_checked
            && !self.driving_prev
            && matches!(self.bringup, Bringup::Ready)
            && matches!(self.limp_fall, LimpFall::Idle)
            && warm
            && !self.powered_off
            && !self.controller.busy()
        {
            self.posture_checked = true;
            let reading = posture::classify(&sensors.positions, sensors.imu.quat);
            let height = format!("{:.0}%", 100.0 * reading.height_ratio);
            let tilt = format!("{:.0}°", reading.tilt_deg);
            match reading.posture {
                posture::Posture::Seated if self.controller.has_sitstand() => {
                    tracing::warn!(
                        %height,
                        %tilt,
                        "posture: seated — settling into the seat, then rising via the sitstand policy"
                    );
                    self.controller.settle_then_rise();
                    self.start_from_hold = true;
                }
                verdict => {
                    let what = match verdict {
                        posture::Posture::Standing => "standing",
                        posture::Posture::Lying => "lying down",
                        posture::Posture::Seated => "seated, with no sitstand policy to rise with",
                        posture::Posture::Unsure => "neither clearly standing nor clearly seated",
                    };
                    // Whatever the controller believed about a seat, the robot
                    // is not in one now: the gait takes it from where it is.
                    self.controller.leave_seat();
                    tracing::warn!(%height, %tilt, "posture: {what} — the policy takes over");
                }
            }
        }

        // Driving, robotd's definition: the policy has the robot, warm, with
        // nothing else owning it — no limp-fall, no hand, no shutdown. A sample
        // coasted past `COAST_TICKS` is not stepped: robotd's `Coast` returns
        // no sample there either, and the loop holds while the bus recovers.
        let driving = self.enabled
            && matches!(self.bringup, Bringup::Ready)
            && warm
            && !coast_exhausted
            && matches!(self.limp_fall, LimpFall::Idle)
            && !pickup_paused
            && matches!(self.shutdown, Shutdown::Idle)
            && !self.powered_off;
        if driving {
            // The gait owns the robot again: the stopped policy's home/seat
            // hold is spent.
            self.disabled_hold = None;
        }
        if driving && !self.driving_prev {
            // Starting fresh: a stale previous action in the observation, or a
            // filter anchored to where the robot was a minute ago, would both
            // show up as a lurch. Only after a real pause, though — a reset
            // across a 20 ms hiccup re-introduces exactly the discontinuity the
            // reset is for.
            if reset_on_resume(self.stopped_driving_at, now) {
                self.controller.reset();
            }
            // A seated robot is held in its seat, not at home: start the
            // network from there. Consumed with the edge, like robotd's
            // per-tick local — a later pause does not seed from whatever pose
            // the duck happens to be holding.
            if self.start_from_hold {
                self.controller.seed_from_pose(&hold);
                self.start_from_hold = false;
            }
        }
        if self.driving_prev && !driving {
            self.stopped_driving_at = Some(now);
        }
        self.driving_prev = driving;

        // Smooth the command the way robotd does, so the policy sees a ramp
        // rather than a step. The limp-fall sequence holds the twist at zero
        // outright, and zeroes the EMA with it: the robot is not handed back
        // mid-command, so a fall in the middle of a stride does not restart
        // the walk from the pre-fall twist.
        let in_limp_fall = !matches!(self.limp_fall, LimpFall::Idle);
        let twist_target = if in_limp_fall {
            [0.0; 3]
        } else {
            command.twist
        };
        if in_limp_fall {
            self.twist_ema = [0.0; 3];
        }
        for axis in 0..3 {
            self.twist_ema[axis] += CMD_ALPHA * (twist_target[axis] - self.twist_ema[axis]);
            command.twist[axis] = self.twist_ema[axis];
        }
        for axis in 0..4 {
            if command.head[axis].is_finite() {
                self.head_ema[axis] += HEAD_ALPHA * (command.head[axis] - self.head_ema[axis]);
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
                    if !playing.skill_delivered && emotion.pending_skill(playing.elapsed).is_some()
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
        // (the policy's head block), exactly as on the real robot. The mode is
        // asked every tick because a mode switch changes it under the loop —
        // and the roller has it off by default, its network folding under a
        // head command it was not trained with.
        let still = self.idle_params.enabled_in(self.mode)
            && emotion_frame.is_none()
            && self.driving_prev
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
            command.head[axis] = self.head_ema[axis] * (1.0 - idle.takeover) + idle.head[axis];
        }
        // The breath moves the body pose the standing network was trained on,
        // and only while no client holds that pose itself.
        if !self.body_active && self.controller.driving() == Some(crate::control::Driving::Stand) {
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
        if reaction.is_none() && moving && !self.last_moving && cooldown && emotion_frame.is_none()
        {
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

        // The targets, robotd's match shape: the limp-fall sequence first (it
        // owns the robot and the controller is not stepped under it), then the
        // held hand, then the cold IMU, then the driving policy, then the
        // homing ramp, then the hold. Emitted here — after pickup and after
        // the command smoothing — exactly where the official loop decides
        // them, so the limp-fall pose rides the smoothed command and the
        // pickup ramp instead of pre-empting both.
        let limp_target = match self.limp_fall {
            LimpFall::Idle => None,
            // Command the joints to wherever they are: the softest thing the
            // servos can do short of cutting torque.
            LimpFall::Limp { .. } => Some((hold, GAIN_LIMP)),
            LimpFall::Posing { from, since } => {
                let t = (since.elapsed(now).as_secs_f64() / LIMP_FALL_POSE.as_secs_f64()).min(1.0);
                let mut target = [0.0; NUM_JOINTS];
                for (index, slot) in target.iter_mut().enumerate() {
                    *slot = from[index] + (DEFAULT_POSITION[index] - from[index]) * t;
                }
                Some((target, GAIN_POSE))
            }
        };
        let scale = voltage_scale(self.volts_ema);
        let step = if limp_target.is_none() && !pickup_paused && warm && driving {
            match self.controller.step(
                &sensors,
                &command,
                // An emotion's Frame::apply sets PoseIntent.active for
                // this tick only; the client's own body hold is restored
                // untouched when the timeline ends.
                self.body_active || emotion_frame.is_some(),
                self.dt.as_secs_f64(),
                scale,
            ) {
                Ok(step) => Some(Ok(step)),
                Err(error) => {
                    tracing::warn!(%error, "inference failed; holding the pose");
                    Some(Err(()))
                }
            }
        } else {
            None
        };
        let mut targets = hold;
        let mut gain = 200;
        if let Some((target, limp_gain)) = limp_target {
            targets = target;
            gain = limp_gain;
            report.label = if matches!(self.limp_fall, LimpFall::Limp { .. }) {
                "limp_fall".to_owned()
            } else {
                "pose".to_owned()
            };
        } else if pickup_paused {
            // Held in a hand: ramp from the policy's last target to the pause
            // pose and stay there. The ramp start is the policy's own target,
            // so letting go does not snap.
            targets = self
                .pickup
                .as_ref()
                .map(|watch| watch.target())
                .unwrap_or(hold);
            report.label = "held".to_owned();
        } else if !warm {
            // The orientation is still a default, not a measurement: hold the
            // pose, do not drive. `robotd` gates its whole driving branch on
            // this.
            report.label = "held".to_owned();
        } else if let Some(Ok(step)) = step {
            targets = step.targets;
            gain = step.gain;
            report.label = step.label.to_string();
        } else if let Some(Err(_)) = step {
            report.label = "held".to_owned();
        } else if let Some(target) = self.bringup.homing_target(now) {
            // Ramping to the home pose (or the seat): moving by definition,
            // the joints are travelling somewhere they have not arrived.
            targets = target;
            gain = 200;
            report.label = "homing".to_owned();
        } else if let Some(target) = self.disabled_hold {
            // A policy stopped by `robot.enable on:false` is commanded home —
            // or to its seat — rather than frozen where the stride ended
            // (robotd's falling-edge `hold`, main.rs:3761-3793).
            targets = target;
            gain = 200;
            report.label = "held".to_owned();
        } else {
            report.label = "held".to_owned();
        }
        // The mouth is not part of any policy — every alpha network skips it —
        // so the idle yawn is the whole of mouth control, exactly as it is on
        // the robot. Only while driving: a held or homing duck keeps the mouth
        // its hold pose gave it.
        if driving && emotion_frame.is_none() {
            // A sleeping duck's beak is shut, even if the idle yawn was due.
            let mouth = if self.care.asleep {
                0.0
            } else {
                self.mouth.max(idle.mouth)
            };
            targets[duck_control::model::MOUTH_INDEX] = duck_control::model::mouth_target(mouth);
        }
        // One write per tick: the body steps inside `apply`, so a second call
        // would advance the physics twice as fast.
        let applied = self.safety.apply(targets, hold, gain)?;
        self.limits
            .extend(applied.limits.iter().map(|l| limit_name(*l).to_owned()));
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
        let walking = driving && command.twist.iter().any(|value| value.abs() > 1e-3);
        let standing = driving && !walking && !pickup_paused;
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

        // The robot.state frame, in the official wire shape — the same struct
        // `robotd` publishes, filled from what this tick actually was. Kept
        // for the IPC layer, which serialises it only when someone is
        // subscribed (a per-tick allocation on the thread that should not be
        // visiting the allocator would be the official loop's reason not to).
        self.state_frame = Some(self.compose_state_frame(
            &sensors,
            requested_twist,
            command.twist,
            command.head,
            &report.label,
            report.picked_up,
            &targets,
        ));
        Ok(report)
    }

    /// Assemble the `robot.state` frame from what this tick actually was.
    ///
    /// `robotd` composes it unconditionally at the end of every tick
    /// (main.rs:4103-4140) — including the ticks where the robot is off, limp
    /// or shutting down, which is precisely why the early-return paths here
    /// must compose it too. A subscriber to a powered-down duck must see the
    /// duck powered down: a frame left over from the last standing tick is a
    /// ghost robot, frozen mid-stride on the wire while the real one lies limp.
    ///
    /// On the early paths the twist and head are the gated ones rather than
    /// the EMA-smoothed ones — the smoothing is part of the driving branch
    /// these paths returned out of — and `picked_up` is the entry state, since
    /// the pickup watch only runs under the policy.
    fn compose_state_frame(
        &self,
        sensors: &Sensors,
        requested: [f64; 3],
        applied: [f64; 3],
        head: [f64; 4],
        label: &str,
        picked_up: bool,
        targets: &[f64; NUM_JOINTS],
    ) -> duck_ipc_proto::RobotState {
        duck_ipc_proto::RobotState {
            t: (self.clock.tick as f64) * self.dt.as_secs_f64(),
            movement: duck_ipc_proto::MoveState {
                requested,
                applied,
                limited_by: self.limits.clone(),
            },
            head,
            policy: label.to_owned(),
            safety: duck_ipc_proto::SafetyState {
                fallen: self.safety.fallen(),
                // Limp means "the robot is actually at limp gain" — which
                // happens in exactly one place, the limp-fall sequence.
                limp: matches!(self.limp_fall, LimpFall::Limp { .. }),
                gravity: sensors.imu.gravity,
                gain: self.safety.gain(),
                picked_up,
            },
            control_loop: duck_ipc_proto::LoopState {
                hz: self.achieved_hz,
                missed: self.missed_ticks,
            },
            joints: sensors.positions.to_vec(),
            targets: targets.to_vec(),
            velocities: if self.safety.measures_velocity() {
                sensors.velocities.to_vec()
            } else {
                Vec::new()
            },
            currents_ma: if self.safety.measures_load() {
                sensors.currents_ma.to_vec()
            } else {
                Vec::new()
            },
            odom: duck_ipc_proto::OdomState {
                position: self.odometry.position(),
                yaw: self.odometry.yaw(),
            },
            theremin: None,
            chorale: None,
            // The duck's clock is simulated (20 ms a tick), so the official
            // read-time stamp is the tick's own nanoseconds, not a wall clock
            // a rewind could not reproduce. It rides the coast: a failed read
            // republishes the last fresh sample's read time.
            t_ns: self.sensors_read_ns,
            imu: Some(duck_ipc_proto::ImuState {
                gyro: sensors.imu.gyro,
                quat: sensors.imu.quat,
            }),
            // No camera and no ToF on the phone: `frames` is the geometry of
            // sensors this duck does not have, and stays `None` truthfully
            // rather than publishing a made-up view. The full-body skeleton
            // below is FK the phone can honestly compute.
            frames: None,
            skeleton: skeleton_at(&sensors.positions),
        }
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
            self.targets.copy_from_slice(&state[1 + NUM_JOINTS..]);
            true
        }
        /// The probe is a value, so a branch's body is a copy of it — the same
        /// independence `MujocoIo::fork` gives a real branch.
        fn fork(&self) -> Option<Self>
        where
            Self: Sized,
        {
            Some(self.clone())
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
        assert!(
            twist[2] > 0.0,
            "left waypoint must turn left, got {twist:?}"
        );
        assert!(
            twist[0] > 0.0,
            "a reachable heading may walk, got {twist:?}"
        );
        assert_eq!(twist[1], 0.0, "the path never sidesteps");

        let mut right = PathTask::start(vec![[0.5, -0.5]], [0.0, 0.0, 0.0]);
        let PathDrive::Drive(twist) = right.drive([0.0, 0.0, 0.0]) else {
            panic!("the waypoint is seven tenths of a metre away, not reached");
        };
        assert!(
            twist[2] < 0.0,
            "right waypoint must turn right, got {twist:?}"
        );
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
        assert!(
            twist[2].abs() < 1e-9,
            "dead ahead must not turn, got {twist:?}"
        );
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
        assert!(
            path_points(&over).is_err(),
            "65 kept points must be refused"
        );

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
        assert!(
            path_points(&serde_json::json!({
                "points": [[0.0, 0.0], [0.0, 0.0], [10.0, 0.0], [20.01, 0.0]]
            }))
            .is_err()
        );
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
        assert_eq!(kept, vec![[0.5_f32, 0.0], [0.625, 0.0], [0.625, 0.1875]]);

        // An origin alone, or one point, is not a route at all.
        assert!(path_points(&serde_json::json!({ "points": [] })).is_err());
        assert!(path_points(&serde_json::json!({ "points": [[0.0, 0.0]] })).is_err());
        assert!(
            path_points(&serde_json::json!({
                "points": [[0.0, 0.0], [0.05, 0.0]]
            }))
            .is_err()
        );
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
        assert!(
            steers([0.4, 0.0, 0.0]),
            "a forward stick takes the duck back"
        );
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
    fn one_emotion_at_a_time_and_a_celebration_never_clobbers_one() {
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
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../upstream/microduck/robotd/emotions");
        let loaded = runtime.load_emotions(&dir).expect("the official timelines");
        assert!(loaded >= 13, "all thirteen timelines load");

        assert!(runtime.trigger_emotion("curious").is_ok());
        assert_eq!(
            runtime.trigger_emotion("excited"),
            Err("an emotion is already playing")
        );
        assert_eq!(
            runtime.celebrate_emotion("excited"),
            Err("an emotion is already playing"),
            "a quest celebration takes the same one-at-a-time door"
        );
        let (name, _, duration) = runtime.emotion_state().expect("curious keeps playing");
        assert_eq!(name, "curious");

        let ticks = (duration / runtime.dt.as_secs_f64()).ceil() as usize + 2;
        for _ in 0..ticks {
            let _ = runtime.tick();
        }
        assert!(runtime.emotion_state().is_none(), "the timeline finished");
        assert!(
            runtime.celebrate_emotion("excited").is_ok(),
            "the next emotion lands after the first ends"
        );
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

    /// A checkpoint carries the rest/power/health inputs: a fork or rewind
    /// must not come back believing a different shutdown mode, a different
    /// error run, or with a pending rest that was already taken.
    #[test]
    fn checkpoint_carries_rest_power_and_health_state() {
        let mut runtime = test_runtime();
        let _ = runtime.tick().expect("a tick");
        runtime.policy_error = Some("the set did not build".to_owned());
        runtime.consecutive_errors = 4;
        runtime.startup_bus_failures = 2;
        runtime.rest_requested = true;
        runtime.disabled_hold = Some(DEFAULT_POSITION);
        runtime.shutdown = Shutdown::Resting {
            from: [0.0; NUM_JOINTS],
            since: TickTime::of(&runtime.clock),
            power_off: false,
        };
        let checkpoint = runtime.checkpoint().expect("a checkpoint");
        assert!(checkpoint.rest_requested);
        assert!(checkpoint.has_read);
        assert!(!checkpoint.shutdown_power_off);
        assert_eq!(
            checkpoint.disabled_hold.as_deref(),
            Some(&DEFAULT_POSITION[..])
        );
        assert!(
            checkpoint.coast_positions.is_some(),
            "a duck that has read carries the sample a failed read coasts on"
        );
        assert_eq!(
            checkpoint.sensors_read_ns,
            runtime.clock().tick * runtime.dt.as_nanos() as u64
        );
        assert_eq!(
            checkpoint.policy_error.as_deref(),
            Some("the set did not build")
        );
        assert_eq!(checkpoint.consecutive_errors, 4);
        assert_eq!(checkpoint.startup_bus_failures, 2);

        let json = serde_json::to_string(&checkpoint).expect("serialises");
        let decoded: Checkpoint = serde_json::from_str(&json).expect("parses");
        let mut restored = test_runtime();
        assert!(restored.restore(&decoded));
        assert!(restored.rest_requested);
        assert!(matches!(
            restored.shutdown,
            Shutdown::Resting {
                power_off: false,
                ..
            }
        ));
        assert_eq!(
            restored.policy_error.as_deref(),
            Some("the set did not build")
        );
        assert_eq!(restored.consecutive_errors, 4);
        assert_eq!(restored.startup_bus_failures, 2);
        assert_eq!(
            restored.disabled_hold,
            Some(DEFAULT_POSITION),
            "the stopped hold comes back with the checkpoint"
        );
        assert!(
            restored.coast.is_some(),
            "has_read and the coast sample cannot disagree after a restore"
        );
        assert_eq!(restored.sensors_read_ns, checkpoint.sensors_read_ns);

        // The shutdown flavour is live too: a checkpoint taken mid-shutdown
        // must come back meaning power-off, not the rest hold.
        runtime.shutdown = Shutdown::Resting {
            from: [0.0; NUM_JOINTS],
            since: TickTime::of(&runtime.clock),
            power_off: true,
        };
        let shutdown = runtime.checkpoint().expect("the shutdown checkpoint");
        assert!(shutdown.shutdown_power_off);
        let mut restored = test_runtime();
        assert!(restored.restore(&shutdown));
        assert!(matches!(
            restored.shutdown,
            Shutdown::Resting {
                power_off: true,
                ..
            }
        ));

        // An old checkpoint that predates the rest and the new health fields
        // still parses, and its "resting" means the shutdown's power-off.
        let mut legacy: serde_json::Value = serde_json::from_str(&json).expect("a value");
        let object = legacy.as_object_mut().expect("an object");
        for key in [
            "rest_requested",
            "shutdown_power_off",
            "policy_error",
            "consecutive_errors",
            "startup_bus_failures",
            "has_read",
            "disabled_hold",
            "coast_positions",
            "sensors_read_ns",
        ] {
            object.remove(key);
        }
        let decoded: Checkpoint = serde_json::from_value(legacy).expect("legacy parses");
        assert!(!decoded.rest_requested);
        assert!(decoded.shutdown_power_off, "legacy resting was a shutdown");
        assert_eq!(decoded.consecutive_errors, 0);
        assert_eq!(decoded.startup_bus_failures, 0);
        assert!(
            decoded.has_read,
            "a legacy checkpoint came from a ticking duck"
        );
        assert!(decoded.disabled_hold.is_none());
        assert!(decoded.coast_positions.is_none());
        assert_eq!(decoded.sensors_read_ns, 0);
    }

    /// The stopped policy's hold is part of the duck: a fork or a rewind must
    /// command the same targets as its parent, not fall back to the measured
    /// pose on the first tick.
    #[test]
    fn the_stopped_hold_survives_fork_and_rewind() {
        let mut parent = test_runtime();
        parent.enable().expect("enable");
        for _ in 0..110 {
            let _ = parent.tick().expect("the warm-up");
        }
        assert_eq!(parent.bringup(), BringupState::Ready);
        parent.set_policy_enabled(false);
        let _ = parent.tick().expect("the stopped tick");
        assert_eq!(parent.disabled_hold, Some(DEFAULT_POSITION));

        let saved = parent.checkpoint().expect("a checkpoint");
        assert_eq!(saved.disabled_hold.as_deref(), Some(&DEFAULT_POSITION[..]));
        let mut fork = parent.fork().expect("a fork");
        let mut rewind = test_runtime();
        assert!(rewind.restore(&saved));

        for _ in 0..20 {
            let p = parent.tick().expect("the parent tick");
            let f = fork.tick().expect("the fork tick");
            let r = rewind.tick().expect("the rewind tick");
            assert_eq!(p.targets, f.targets, "the fork holds what its parent holds");
            assert_eq!(
                p.targets, r.targets,
                "the rewind holds what its parent holds"
            );
            assert_eq!(
                p.targets, DEFAULT_POSITION,
                "and that hold is the home pose"
            );
        }
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
        assert!(
            library
                .get("devastated")
                .unwrap()
                .pending_skill(9.0)
                .is_some()
        );
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

    // ── Official-parity workorder A: runtime semantics ──────────────────────

    /// Voltage adaptation, numeric: nominal over the clamped measurement, no
    /// adaptation with no reading or a dead one. The clamp keeps a bad reading
    /// from becoming a wild scale.
    #[test]
    fn voltage_scale_is_nominal_over_the_clamped_pack() {
        let full = 7.4;
        assert_eq!(
            voltage_scale(Some(full)),
            1.0,
            "at nominal there is nothing to scale"
        );
        assert_eq!(voltage_scale(None), 1.0, "no reading yet, no adaptation");
        assert_eq!(
            voltage_scale(Some(0.0)),
            1.0,
            "a dead reading is no adaptation"
        );
        let low = voltage_scale(Some(5.0));
        assert!(
            (low - 7.4 / 6.0).abs() < 1e-12,
            "5 V clamps to the 6 V floor: {low}"
        );
        let high = voltage_scale(Some(11.0));
        assert!(
            (high - 7.4 / 9.5).abs() < 1e-12,
            "11 V clamps to the 9.5 V ceiling: {high}"
        );
        // A sagging pack scales the action UP: the same stick gives the same
        // stride at 6.6 V that it gave at 7.4 V.
        assert!(voltage_scale(Some(6.6)) > 1.0);
    }

    /// One battery mapping, robotd's: `duck_control::battery_percent` over the
    /// 6.6..8.2 V window, feeding the gauge and `/state` alike. The old
    /// 6.0..8.4 window read 25% at the empty threshold — two gauges disagreeing
    /// about the same pack.
    #[test]
    fn the_battery_reads_the_official_window_end_to_end() {
        assert_eq!(
            duck_control::battery_percent(6.6),
            0.0,
            "the empty threshold is 0%"
        );
        assert!((duck_control::battery_percent(8.2) - 100.0).abs() < 1e-9);
        assert!((duck_control::battery_percent(7.4) - 50.0).abs() < 1e-9);

        // The runtime's slow read feeds the same mapping through the EMA the
        // empty-pack check uses, so the gauge and the shutdown threshold can
        // never disagree.
        let mut runtime = test_runtime();
        for _ in 0..50 {
            let _ = runtime.tick();
        }
        // The Probe's slow read reports 7.4 V every window: half the pack.
        let percent = runtime
            .face
            .battery_percent
            .expect("the slow read has landed");
        assert!(
            (percent - 50.0).abs() < 1e-6,
            "7.4 V is half of 6.6..8.2, got {percent}"
        );
    }

    /// Enable enters Homing and the full two seconds pass before the policy
    /// drives — no "already at home" shortcut, because the ramp doubles as the
    /// IMU's convergence window.
    #[test]
    fn enable_ramps_the_full_two_seconds_before_the_policy_drives() {
        let mut runtime = test_runtime();
        runtime.enable().expect("enable");
        let _ = runtime.tick().expect("the bring-up tick");
        assert_eq!(
            runtime.bringup(),
            BringupState::Homing,
            "enable never lands straight in Ready"
        );
        for _ in 0..99 {
            let _ = runtime.tick().expect("homing ticks");
        }
        assert_eq!(
            runtime.bringup(),
            BringupState::Homing,
            "2 s minus a tick is still homing"
        );
        for _ in 0..3 {
            let _ = runtime.tick().expect("the ramp finishing");
        }
        assert_eq!(runtime.bringup(), BringupState::Ready);
    }

    /// The gate runs before the bring-up, on every tick, and its verdict is
    /// collected: a duck that has never been driven reads as maximally stale,
    /// exactly like the official loop's stamped-at-zero intents, and the
    /// reason lands in `limited_by` from the very first tick.
    #[test]
    fn the_gate_runs_before_bring_up_and_names_the_deadman() {
        let mut runtime = test_runtime();
        runtime.enable().expect("enable");
        let _ = runtime.tick().expect("first tick");
        assert!(
            runtime.limited_by().contains(&"deadman".to_owned()),
            "an intent that never arrived is maximally stale: {:?}",
            runtime.limited_by()
        );
    }

    /// A command stream that stops leaves the robot standing, with the reason
    /// on the wire: past the deadman the twist is zeroed and `limited_by`
    /// says so.
    #[test]
    fn a_stopped_stream_trips_the_deadman_into_limited_by() {
        let mut runtime = test_runtime();
        runtime.enable().expect("enable");
        for _ in 0..110 {
            let _ = runtime.tick().expect("warm-up ticks");
        }
        assert_eq!(runtime.bringup(), BringupState::Ready);
        runtime.set_twist([0.4, 0.0, 0.0]);
        let _ = runtime.tick().expect("the driven tick");
        assert!(
            !runtime.limited_by().contains(&"deadman".to_owned()),
            "a fresh twist is not stale: {:?}",
            runtime.limited_by()
        );
        // Twenty-five ticks is 500 ms: the lease is out, the gate fires. The
        // applied twist is the smoothed EMA, which glides to the zeroed
        // target rather than snapping — the smoothing is the point — so the
        // zero is asserted after it has had the ticks to arrive.
        for _ in 0..60 {
            let _ = runtime.tick().expect("stale ticks");
        }
        assert!(runtime.limited_by().contains(&"deadman".to_owned()));
        let state = runtime.robot_state().expect("a state frame");
        assert!(
            state.movement.applied.iter().all(|v| v.abs() < 1e-3),
            "the gated command glided to zero: {:?}",
            state.movement.applied
        );
        assert_eq!(state.movement.limited_by, runtime.limited_by().to_vec());
    }

    /// The limp-fall sequence holds the twist at zero outright and zeroes the
    /// EMA with it: a fall mid-stride must not restart the walk from the
    /// pre-fall command.
    #[test]
    fn limp_fall_holds_the_twist_at_zero() {
        let mut runtime = test_runtime();
        runtime.enable().expect("enable");
        runtime.set_twist([0.5, 0.0, 0.1]);
        for _ in 0..110 {
            let _ = runtime.tick().expect("warm-up ticks");
        }
        // Force the sequence the way a fall would have, and keep the twist on.
        runtime.limp_fall = LimpFall::Limp {
            since: TickTime::of(&runtime.clock),
            landing: Landing::default(),
        };
        let report = runtime.tick().expect("the limp tick");
        assert_eq!(report.label, "limp_fall");
        assert_eq!(report.gain, GAIN_LIMP);
        assert!(
            runtime.twist_ema.iter().all(|v| v.abs() < 1e-9),
            "the EMA is zeroed with the twist: {:?}",
            runtime.twist_ema
        );
        // And the pose phase rides the same zeroed twist.
        runtime.limp_fall = LimpFall::Posing {
            from: hold_of(&report),
            since: TickTime::of(&runtime.clock),
        };
        let report = runtime.tick().expect("the pose tick");
        assert_eq!(report.label, "pose");
        assert_eq!(report.gain, GAIN_POSE);
        assert!(runtime.twist_ema.iter().all(|v| v.abs() < 1e-9));
    }

    /// The controller reset on resume waits for a real pause: `RESET_AFTER_PAUSE`
    /// (200 ms) of stopped driving before the reset, and none across a hiccup.
    #[test]
    fn reset_on_resume_waits_for_a_real_pause() {
        let now = TickTime(100);
        assert!(reset_on_resume(None, now), "never driven resets freely");
        assert!(
            !reset_on_resume(Some(TickTime(91)), now),
            "180 ms of pause is a hiccup, not a pause"
        );
        assert!(
            reset_on_resume(Some(TickTime(90)), now),
            "exactly 200 ms is the boundary robotd draws (>= pauses)"
        );
        assert!(
            reset_on_resume(Some(TickTime(89)), now),
            "220 ms of pause resets"
        );
    }

    /// Driving stopping is recorded where the resume reset measures from —
    /// including the early-return paths the disabled and shutdown ticks take,
    /// or a power-on after a shutdown would skip the reset.
    #[test]
    fn the_disabled_tick_records_where_driving_stopped() {
        let mut runtime = test_runtime();
        runtime.enable().expect("enable");
        for _ in 0..110 {
            let _ = runtime.tick().expect("warm-up ticks");
        }
        assert!(runtime.driving_prev, "the duck is driving at Ready");
        runtime.disable().expect("disable");
        let _ = runtime.tick().expect("the limp tick");
        assert!(!runtime.driving_prev);
        assert!(
            runtime.stopped_driving_at.is_some(),
            "the falling edge was recorded on the disabled tick"
        );
    }

    /// Roller mode is not watched for pickup: the model has never seen wheels,
    /// and scoring it there would pause a wheeled gait on its own joints.
    #[test]
    fn roller_mode_is_never_watched_for_pickup() {
        let mut runtime = test_runtime();
        let (ready, settled, warm, free) = (true, true, true, true);
        assert!(runtime.pickup_watching(ready, settled, warm, free));
        runtime.set_mode(robotd_params::Mode::Roller);
        assert!(
            !runtime.pickup_watching(ready, settled, warm, free),
            "wheels are not a held duck"
        );
        // The gate is the first term: no combination of ready/settled/warm/free
        // rescues a roller tick.
        assert!(!runtime.pickup_watching(false, true, true, true));
    }

    /// The health gate keeps degraded and unhealthy strictly apart, and floors
    /// the achieved rate at 45 Hz with the stall check beside it.
    #[test]
    fn health_splits_degraded_from_unhealthy_and_floors_the_rate() {
        let mut runtime = test_runtime();
        match runtime.health() {
            // Official no-ticks branch: without startup bus failures the loop
            // simply has not completed a cycle (main.rs:921-935).
            HealthVerdict::Unhealthy { reasons } => {
                assert!(reasons[0].contains("has not completed a cycle"));
            }
            other => panic!("a loop that never ticked is not {:?}", other),
        }
        for _ in 0..10 {
            let _ = runtime.tick().expect("ticks");
        }
        assert!(runtime.health().is_healthy(), "a ticking loop is healthy");
        // Force a slow window: 3 ticks over a 5 s window is 0.6 Hz, below the
        // 45 Hz floor.
        runtime.hz_window_start = Some(Instant::now() - Duration::from_secs(5));
        runtime.hz_window_ticks = 3;
        let _ = runtime.tick().expect("the window-closing tick");
        match runtime.health() {
            HealthVerdict::Unhealthy { reasons } => {
                assert!(
                    reasons[0].contains("below the 45.0 Hz floor"),
                    "{:?}",
                    reasons
                );
            }
            other => panic!("0.6 Hz is not {:?}", other),
        }
        // A marked degraded reason wins over the rate floor — official order —
        // and stays degraded, because the robot walks on the policy it had.
        // The same reason marked twice is one reason, not two lines of JSON.
        runtime.mark_degraded("the last policy change did not take".to_owned());
        runtime.mark_degraded("the last policy change did not take".to_owned());
        match runtime.health() {
            HealthVerdict::Degraded { reasons } => {
                assert_eq!(
                    reasons,
                    vec!["the last policy change did not take".to_owned()]
                );
            }
            other => panic!("a marked reason is not {:?}", other),
        }
        runtime.mark_degraded("an override fell back".to_owned());
        match runtime.health() {
            HealthVerdict::Degraded { reasons } => assert_eq!(
                reasons.len(),
                2,
                "distinct reasons are both reported: {reasons:?}"
            ),
            other => panic!("two marked reasons are not {:?}", other),
        }
        runtime.clear_degraded();
        assert!(
            matches!(runtime.health(), HealthVerdict::Unhealthy { .. }),
            "clearing restores the rate verdict"
        );
        // Marking twice is one reason, not a growing list.
        runtime.mark_degraded("x".to_owned());
        runtime.mark_degraded("x".to_owned());
        runtime.clear_degraded();
        // And a healthy rate recovers the verdict.
        runtime.achieved_hz = 50.0;
        assert!(runtime.health().is_healthy());
    }

    /// The duck's voice is given, not baked: the constructor default stays the
    /// fixed 0xD0C, and a seed handed in before the first render replaces the
    /// personality whole.
    #[test]
    fn the_voice_seed_is_given_before_any_render() {
        let mut runtime = test_runtime();
        assert_eq!(
            runtime.voice_seed(),
            0xD0C,
            "the default keeps existing callers working"
        );
        let before = runtime
            .render_tag("greet")
            .expect("the default voice renders");
        runtime.set_voice_seed(0x5EED);
        assert_eq!(runtime.voice_seed(), 0x5EED);
        let after = runtime.render_tag("greet").expect("the new voice renders");
        assert_ne!(before, after, "two seeds must sound like two ducks");
    }

    /// The per-tick `robot.state` frame is the official wire shape: the same
    /// field names duck-ipc-proto publishes, filled from what the tick was.
    #[test]
    fn the_state_frame_is_the_official_robot_state() {
        let mut runtime = test_runtime();
        runtime.enable().expect("enable");
        for _ in 0..110 {
            let _ = runtime.tick().expect("warm-up ticks");
        }
        runtime.set_twist([0.4, 0.0, 0.0]);
        let report = runtime.tick().expect("the driven tick");
        let state = runtime.robot_state().expect("a frame after a tick");
        assert_eq!(state.joints.len(), NUM_JOINTS);
        assert_eq!(state.targets.len(), NUM_JOINTS);
        assert_eq!(
            state.policy, report.label,
            "the frame names the same policy the report does"
        );
        assert_eq!(
            state.movement.requested,
            [0.4, 0.0, 0.0],
            "requested is the raw twist"
        );
        assert!(
            state.movement.applied[0] > 0.0 && state.movement.applied[0] < 0.4,
            "applied is the smoothed twist mid-ramp: {:?}",
            state.movement.applied
        );
        assert_eq!(state.movement.limited_by, runtime.limited_by().to_vec());
        assert_eq!(state.safety.gain, Some(report.gain));
        assert_eq!(state.safety.fallen, report.fallen);
        assert_eq!(state.control_loop.hz, runtime.achieved_hz());
        // `robot.health` rides the same verdict, and the battery — once the
        // slow read has landed — is reported, never judged.
        let health = runtime.health_result();
        assert!(health.healthy, "{:?}", health.reason);
        for _ in 0..50 {
            let _ = runtime.tick().expect("ticks to the slow read");
        }
        let health = runtime.health_result();
        let battery = health.battery.expect("the slow read has landed");
        assert!(
            (battery.percent - 50.0).abs() < 1e-9,
            "7.4 V is half of 6.6..8.2"
        );
        assert!((battery.volts - 7.4).abs() < 1e-9);
    }

    /// A robot the IMU reads seated comes up to the seat, not to a standing
    /// home pose — torque-on posture, robotd's `ramp_target`, with a sitstand
    /// network to rise with. The first episode ramps home (the IMU is still
    /// converging); a re-enable after it has converged goes to the seat.
    #[test]
    fn a_seated_robot_ramps_to_the_seat_once_the_imu_is_warm() {
        let dir = crate::fixtures::policies_dir().expect("the official policy bundle");
        let paths = duck_control::policy::PolicyPaths {
            walk: dir.join("velstand.onnx"),
            stand: None,
            sitstand: Some(dir.join("alpha_sitstand.onnx")),
            ground_pick: None,
            skills: Vec::new(),
        };
        let policy = Policy::load(&paths, 0.05).expect("the bundle with a sitstand network");
        let probe = Probe {
            sensors: Sensors {
                positions: crate::SEAT_POSITION,
                ..Sensors::default()
            },
            targets: [0.0; NUM_JOINTS],
        };
        let mut runtime = DuckRuntime::new(
            probe,
            policy,
            Tuning::default(),
            SkillTuning::default(),
            SafetyConfig::default(),
        );
        runtime.enable().expect("enable");
        for _ in 0..110 {
            let _ = runtime.tick().expect("the first episode");
        }
        assert_eq!(runtime.bringup(), BringupState::Ready);
        assert!(runtime.imu_ready(), "the filter converged across the ramp");
        // A disable/enable cycle brings the torque back on with a warm IMU and
        // the robot still sitting where it was.
        runtime.disable().expect("disable");
        runtime.enable().expect("enable");
        let _ = runtime.tick().expect("the re-enable tick");
        match runtime.bringup {
            Bringup::Homing { to, .. } => {
                assert_eq!(to, crate::SEAT_POSITION, "a seated robot ramps to the seat");
            }
            other => panic!("the second bring-up is {:?}, not Homing", other.state()),
        }
    }

    /// The measured pose of the last tick, for tests that need the limp-fall's
    /// landing point.
    fn hold_of(report: &TickReport) -> [f64; NUM_JOINTS] {
        report.sensors.positions
    }

    /// A runtime over the stub body, with the official policy bundle loaded the
    /// way every behaviour test here builds one.
    fn test_runtime() -> DuckRuntime<Probe> {
        let probe = Probe {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
        };
        DuckRuntime::new(
            probe,
            test_policy(),
            Tuning::default(),
            SkillTuning::default(),
            SafetyConfig::default(),
        )
    }

    // ── Workorder R: the five QA-confirmed defects ─────────────────────────────

    /// A walk network that is not the one `test_policy` loads, for a swap a
    /// caller marks as replacing the driving slot.
    fn other_walk_policy() -> Policy {
        let dir = crate::fixtures::policies_dir().expect("the official policy bundle");
        let paths = duck_control::policy::PolicyPaths {
            walk: dir.join("alpha_walking.onnx"),
            stand: None,
            sitstand: None,
            ground_pick: None,
            skills: Vec::new(),
        };
        Policy::load(&paths, 0.05).expect("the other walk network")
    }

    /// Which walk file the controller is actually running.
    fn walk_file<T: RobotIo>(runtime: &DuckRuntime<T>) -> String {
        runtime
            .controller
            .policy_ref()
            .path_of(duck_control::policy::Net::Walk)
            .expect("a walk network is loaded")
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Raise the duck to Ready and hand it to the walk network, driven.
    fn drive_to_ready(runtime: &mut DuckRuntime<Probe>) {
        runtime.enable().expect("enable");
        for _ in 0..110 {
            runtime.tick().expect("warm-up ticks");
        }
        assert_eq!(runtime.bringup(), BringupState::Ready);
        runtime.set_twist([0.4, 0.0, 0.0]);
        runtime.tick().expect("the driven tick");
        assert!(runtime.driving_prev, "the gait has the robot");
    }

    /// Defect 1: an API-driven duck is a duck with no shell keepalive, and the
    /// deadman punishes a client that stopped talking, not a door the twist came
    /// through (robotd's `robot.move` feeds the one intents channel and stamps it).
    /// A `robot.move` cadence must keep the duck moving, and only a client that
    /// falls silent may trip the gate.
    #[test]
    fn an_api_twist_keeps_the_deadman_off_while_it_keeps_talking() {
        let mut runtime = test_runtime();
        runtime.enable().expect("enable");
        for _ in 0..110 {
            runtime.tick().expect("warm-up ticks");
        }
        assert_eq!(runtime.bringup(), BringupState::Ready);

        // Twenty lease-refreshing beats, each 20 ms of simulated time: far inside
        // the 500 ms deadman, and every one a reason the clock had better be fed.
        for beat in 0..20 {
            runtime.set_api_twist([0.4, 0.0, 0.0]);
            runtime.tick().expect("a leased tick");
            assert!(
                !runtime.limited_by().contains(&"deadman".to_owned()),
                "beat {beat}: a driving API client is gated: {:?}",
                runtime.limited_by()
            );
        }
        let state = runtime.robot_state().expect("a frame");
        assert!(
            state.movement.applied[0] > 0.05,
            "the leased twist reaches the robot: {:?}",
            state.movement.applied
        );

        // Now the agent goes quiet: exactly like a stopped joystick stream, the
        // lease lapses and the gate does its job.
        for _ in 0..60 {
            runtime.tick().expect("post-lease ticks");
        }
        assert!(
            runtime.limited_by().contains(&"deadman".to_owned()),
            "a silent API client trips the deadman: {:?}",
            runtime.limited_by()
        );
        let state = runtime.robot_state().expect("a frame");
        assert!(
            state.movement.applied.iter().all(|v| v.abs() < 1e-3),
            "the gated twist glided to zero: {:?}",
            state.movement.applied
        );
    }

    /// Defect 2: `robot.state` must be composed on every path out of the tick —
    /// robotd assembles it unconditionally at the end of the loop. A duck that is
    /// off, limp or shutting down used to keep broadcasting the last standing
    /// frame: a ghost robot frozen mid-stride while the real one lay limp.
    #[test]
    fn every_early_path_publishes_a_fresh_state_frame() {
        let mut runtime = test_runtime();
        runtime.enable().expect("enable");
        for _ in 0..110 {
            runtime.tick().expect("warm-up ticks");
        }
        runtime.set_twist([0.4, 0.0, 0.0]);
        runtime.tick().expect("a driven tick");
        let standing = runtime.robot_state().expect("a standing frame");

        // The limp early path publishes too: a duck that never enabled has
        // no torque and reports what it is (a limp first tick, not the initial
        // empty frame).
        let mut limp_only = test_runtime();
        limp_only.tick().expect("a limp tick");
        let limp = limp_only.robot_state().expect("a frame on the limp path");
        assert_eq!(
            limp.policy, "limp",
            "the frame says what the duck is: {:?}",
            limp.policy
        );

        // A shutdown with no sitstand network cuts torque on the spot. The
        // power-off is this tick's outcome, so the frame is the off frame —
        // not one more "rest"/"limp" state the duck has already left.
        runtime.request_shutdown();
        runtime.tick().expect("the shutdown tick");
        let off = runtime.robot_state().expect("the off frame");
        assert!(
            off.t > standing.t,
            "the clock moved on: {} vs {}",
            off.t,
            standing.t
        );
        assert_eq!(off.policy, "off");
        assert_eq!(
            off.safety.gain,
            Some(0),
            "the off frame carries the off gain"
        );
        assert_ne!(
            off.targets, standing.targets,
            "not the standing frame replayed"
        );

        // Power back on and watch the shutdown sequence's own early path: with a
        // sitstand network the duck sits, and the sit tick used to freeze the
        // frame too.
        let mut seated = {
            let dir = crate::fixtures::policies_dir().expect("the official policy bundle");
            let paths = duck_control::policy::PolicyPaths {
                walk: dir.join("velstand.onnx"),
                stand: None,
                sitstand: Some(dir.join("alpha_sitstand.onnx")),
                ground_pick: None,
                skills: Vec::new(),
            };
            let probe = Probe {
                sensors: Sensors::default(),
                targets: [0.0; NUM_JOINTS],
            };
            DuckRuntime::new(
                probe,
                Policy::load(&paths, 0.05).expect("the bundle with a sitstand network"),
                Tuning::default(),
                SkillTuning::default(),
                SafetyConfig::default(),
            )
        };
        seated.enable().expect("enable");
        for _ in 0..110 {
            seated.tick().expect("warm-up ticks");
        }
        let before = seated.robot_state().expect("a frame");
        seated.request_shutdown();
        seated.tick().expect("the sit begins");
        for _ in 0..3 {
            seated.tick().expect("sitting ticks");
        }
        let sitting = seated.robot_state().expect("a frame on the sit path");
        assert!(
            sitting.t > before.t,
            "the sit publishes, not the pre-shutdown frame"
        );
        assert_eq!(sitting.policy, "shutdown");
    }

    /// Defect 3: the 45 Hz floor is robotd's floor at robotd's rate — the mobile
    /// loop is driven at 50 × `clock.speed` ticks per wall second (iOS
    /// `ticksForThisFrame`), so slow-motion is a product feature, not a wedged
    /// control loop. And a gap longer than the stall window is frozen time (a
    /// pause the shell routed around `tick`), not a slow loop: its first beat
    /// must not report a diluted rate for a second.
    #[test]
    fn the_rate_floor_follows_the_speed_dial() {
        let mut runtime = test_runtime();
        runtime.set_speed(0.25);
        runtime.enable().expect("enable");
        let _ = runtime.tick().expect("a tick");
        // A window measured at the slowed rate: 12 Hz is a healthy 0.25× loop
        // (its floor is 45 × 0.25 = 11.25 Hz) and an unhealthy 1× one.
        runtime.achieved_hz = 12.0;
        assert!(
            runtime.health().is_healthy(),
            "a deliberately slowed duck is a feature: {:?}",
            runtime.health()
        );
        runtime.achieved_hz = 11.0;
        match runtime.health() {
            HealthVerdict::Unhealthy { reasons } => {
                assert!(reasons[0].contains("Hz floor"), "{reasons:?}")
            }
            other => panic!("11 Hz at 0.25× is {other:?}, not unhealthy"),
        }
        runtime.set_speed(1.0);
        match runtime.health() {
            HealthVerdict::Unhealthy { reasons } => {
                assert!(
                    reasons[0].contains("below the 45.0 Hz floor"),
                    "{reasons:?}"
                )
            }
            other => panic!("11 Hz at 1× is {other:?}, not unhealthy"),
        }
        // The stall window moves the same way: 25 periods of the *shell's* cadence.
        runtime.set_speed(0.25);
        assert_eq!(runtime.stall_limit(), Duration::from_millis(2000));
        runtime.set_speed(1.0);
        assert_eq!(runtime.stall_limit(), Duration::from_millis(500));
    }

    /// The pause-resume half of defect 3: the first beat after a wall-clock gap
    /// longer than the stall window restarts the rate window instead of charging
    /// the gap to the loop.
    #[test]
    fn resuming_after_a_pause_does_not_report_a_second_of_false_slowness() {
        let mut runtime = test_runtime();
        runtime.enable().expect("enable");
        for _ in 0..10 {
            runtime.tick().expect("ticks");
        }
        assert!(runtime.health().is_healthy());
        // A window that closed at full rate, and a two-second gap — longer than
        // the 500 ms stall window, which is what a pause routed around `tick`
        // (the shell keeps firing, the core freezes) or a wedged caller looks
        // like from inside the loop.
        runtime.achieved_hz = 50.0;
        runtime.hz_window_start = Some(Instant::now() - Duration::from_secs(2));
        runtime.hz_window_ticks = 3;
        runtime.last_tick_at = Instant::now() - Duration::from_secs(2);
        runtime.tick().expect("the resume beat");
        assert_eq!(
            runtime.achieved_hz(),
            50.0,
            "the stale window was dropped, not closed and reported as 1.5 Hz"
        );
        assert!(runtime.health().is_healthy(), "{:?}", runtime.health());
    }

    /// Defect 4: the driving-edge bookkeeping (`driving_prev`,
    /// `stopped_driving_at`, `posture_checked`, `start_from_hold`) and the
    /// smoothed pack are part of what a fork is. Left out, the branch's first
    /// tick saw a false rising edge under the restored twist, and
    /// `reset_on_resume` — with no `stopped_driving_at` to measure against —
    /// wiped the controller state the restore had just put back, re-running the
    /// posture check and diverging from the parent on the very first beat.
    #[test]
    fn a_fork_replays_the_parents_control_state_and_trajectory() {
        let mut parent = test_runtime();
        drive_to_ready(&mut parent);
        assert!(parent.posture_checked, "the enable episode ran its check");
        assert!(parent.volts_ema.is_some(), "the slow read has landed");
        assert_eq!(parent.stopped_driving_at, None, "never stopped driving");

        let mut branch = parent.fork().expect("the probe body forks");
        // The five states come across exactly.
        assert_eq!(branch.driving_prev, parent.driving_prev);
        assert_eq!(branch.stopped_driving_at, parent.stopped_driving_at);
        assert_eq!(branch.posture_checked, parent.posture_checked);
        assert_eq!(branch.start_from_hold, parent.start_from_hold);
        assert_eq!(branch.volts_ema, parent.volts_ema);

        // Twenty ticks with the stick held: the branch must not notice it is a
        // branch. With the edge bookkeeping missing this diverged on the first
        // tick (false rising edge → reset → re-seeded posture pass).
        let mut parent_trace = Vec::new();
        let mut branch_trace = Vec::new();
        for tick in 0..20 {
            parent.set_twist([0.4, 0.0, 0.0]);
            branch.set_twist([0.4, 0.0, 0.0]);
            let p = parent
                .tick()
                .unwrap_or_else(|e| panic!("parent tick {tick}: {e:?}"));
            let b = branch
                .tick()
                .unwrap_or_else(|e| panic!("branch tick {tick}: {e:?}"));
            parent_trace.push((p.label.clone(), p.targets));
            branch_trace.push((b.label.clone(), b.targets));
        }
        for (tick, ((pl, pt), (bl, bt))) in parent_trace.iter().zip(&branch_trace).enumerate() {
            assert_eq!(pl, bl, "tick {tick} took a different path");
            for joint in 0..NUM_JOINTS {
                assert!(
                    (pt[joint] - bt[joint]).abs() < 1e-9,
                    "tick {tick} joint {joint} diverged: {} vs {}",
                    pt[joint],
                    bt[joint]
                );
            }
        }
        // The inference ran (a held pose would compare equal by construction and
        // prove nothing): the parent's targets moved away from the zeroed probe.
        assert!(
            parent_trace
                .iter()
                .any(|(_, t)| t.iter().any(|v| v.abs() > 1e-6)),
            "the driven policy produced targets to compare"
        );

        // And the same five states ride a plain checkpoint/restore, not just a
        // fork — the /rewind and /restore doors.
        let saved = parent.checkpoint().expect("a checkpoint");
        let mut restored = test_runtime();
        restored.enable().expect("enable");
        for _ in 0..110 {
            restored.tick().expect("warm-up ticks");
        }
        assert!(restored.restore(&saved), "the checkpoint restores");
        assert_eq!(restored.driving_prev, parent.driving_prev);
        assert_eq!(restored.stopped_driving_at, parent.stopped_driving_at);
        assert_eq!(restored.posture_checked, parent.posture_checked);
        assert_eq!(restored.start_from_hold, parent.start_from_hold);
        assert_eq!(restored.volts_ema, parent.volts_ema);
    }

    /// A checkpoint written before these fields existed must still load — the
    /// timeline holds them on the ffi side across a session.
    #[test]
    fn an_older_checkpoint_still_loads() {
        let runtime = test_runtime();
        let saved = runtime.checkpoint().expect("a checkpoint");
        let mut json = serde_json::to_value(&saved).expect("a checkpoint serialises");
        for field in [
            "driving_prev",
            "stopped_driving_at",
            "posture_checked",
            "start_from_hold",
            "volts_ema",
        ] {
            json.as_object_mut().expect("an object").remove(field);
        }
        let text = serde_json::to_string(&json).expect("the trimmed checkpoint serialises");
        let back: Checkpoint =
            serde_json::from_str(&text).expect("it loads with the new fields defaulted");
        assert!(!back.driving_prev);
        assert_eq!(back.stopped_driving_at, None);
        assert!(!back.posture_checked);
        assert!(!back.start_from_hold);
        assert_eq!(back.volts_ema, None);
    }

    /// Defect 5, the headline: a policy change that replaces the network driving a
    /// robot mid-gait does not swap bytes under a moving gait — robotd sends the
    /// duck to the home pose and lands the change there, torque on throughout
    /// (main.rs:3015-3106, :3546-3575).
    #[test]
    fn a_disturbing_swap_under_a_gait_walks_home_before_landing() {
        let mut runtime = test_runtime();
        assert_eq!(walk_file(&runtime), "velstand.onnx");
        drive_to_ready(&mut runtime);

        let verdict = runtime.replace_policy_disturbing(
            other_walk_policy(),
            Tuning::default(),
            SkillTuning::default(),
            true,
        );
        assert_eq!(verdict, PolicySwap::GoingHome);
        assert_eq!(
            runtime.swap_state(),
            Some(PolicySwap::GoingHome),
            "the shell can read the approach it was promised"
        );

        // The tick the change is taken on sends the duck home rather than
        // swapping: the old network still drives.
        runtime.tick().expect("the sent-home tick");
        assert_eq!(runtime.bringup(), BringupState::Homing);
        assert_eq!(walk_file(&runtime), "velstand.onnx", "nothing landed yet");
        assert!(runtime.swap_pending());
        assert_eq!(runtime.swap_state(), Some(PolicySwap::GoingHome));

        // The client keeps steering on the way; the loop holds the twist at zero
        // regardless, and nothing swaps while the joints are travelling.
        let mut ticks = 1;
        while runtime.bringup() != BringupState::Ready && ticks < 200 {
            runtime.set_twist([0.4, 0.0, 0.0]);
            runtime.tick().expect("a seeking tick");
            ticks += 1;
            if runtime.bringup() == BringupState::Homing {
                assert_eq!(walk_file(&runtime), "velstand.onnx", "swapped mid-ramp");
            }
        }
        // The 2 s ramp is honoured (the duck was already at the probe's pose, but
        // robotd holds HOME_RAMP out regardless), and the change lands on the
        // tick the ramp completes — not before.
        assert!(
            ticks >= 99,
            "the home ramp takes HOME_RAMP: landed after {ticks} ticks"
        );
        assert!(!runtime.swap_pending(), "the change landed");
        assert_eq!(
            walk_file(&runtime),
            "alpha_walking.onnx",
            "the new set is what runs"
        );

        // And it drove from there: a tick with the new network at the helm.
        runtime.set_twist([0.4, 0.0, 0.0]);
        runtime.tick().expect("driving on the new network");
    }

    /// The other side of the gate: a change that does not replace the driving
    /// network — or arrives at a duck nothing drives — lands in place without
    /// moving the robot. This is also the existing `replace_policy` contract:
    /// the unmarked in-place swap stays the default.
    #[test]
    fn a_quiet_change_lands_in_place() {
        // (a) Not disturbing, under a moving gait: in place, immediately.
        let mut runtime = test_runtime();
        drive_to_ready(&mut runtime);
        let verdict = runtime.replace_policy_disturbing(
            other_walk_policy(),
            Tuning::default(),
            SkillTuning::default(),
            false,
        );
        assert_eq!(verdict, PolicySwap::InPlace);
        runtime.tick().expect("the landing tick");
        assert_eq!(
            runtime.bringup(),
            BringupState::Ready,
            "the duck never left home"
        );
        assert_eq!(walk_file(&runtime), "alpha_walking.onnx");

        // (b) Disturbing, but nothing is driving: a limp duck has no gait to
        // protect — the swap waits one tick and lands where the robot is.
        let mut limp = test_runtime();
        limp.enable().expect("enable");
        limp.disable().expect("disable");
        let verdict = limp.replace_policy_disturbing(
            other_walk_policy(),
            Tuning::default(),
            SkillTuning::default(),
            true,
        );
        assert_eq!(verdict, PolicySwap::InPlace);
        limp.tick().expect("the landing tick");
        assert_eq!(limp.bringup(), BringupState::Limp, "still limp");
        assert_eq!(walk_file(&limp), "alpha_walking.onnx");

        // (c) The legacy in-place entry point keeps working unchanged.
        let mut legacy = test_runtime();
        drive_to_ready(&mut legacy);
        legacy.replace_policy(
            other_walk_policy(),
            Tuning::default(),
            SkillTuning::default(),
        );
        assert_eq!(walk_file(&legacy), "alpha_walking.onnx");
    }

    /// A change asked for while the robot is going down is refused outright
    /// (robotd's refusal — from inside a shutdown the path home stands the robot
    /// up at gain until the sit cuts the torque out from under it), and one that
    /// arrives while the limp-fall owns the robot waits for it to hand back
    /// rather than restarting the reflex.
    #[test]
    fn a_change_yields_to_the_shutdown_and_the_limp_fall() {
        // Refused once off: the shutdown here has no sitstand to sit with, so the
        // request powers the duck off on the spot.
        let mut runtime = test_runtime();
        drive_to_ready(&mut runtime);
        runtime.request_shutdown();
        runtime.tick().expect("the shutdown tick");
        runtime.tick().expect("the off tick");
        assert!(runtime.powered_off());
        let verdict = runtime.replace_policy_disturbing(
            other_walk_policy(),
            Tuning::default(),
            SkillTuning::default(),
            true,
        );
        assert_eq!(verdict, PolicySwap::Refused);
        assert!(!runtime.swap_pending());
        assert_eq!(
            walk_file(&runtime),
            "velstand.onnx",
            "the refusal kept the set"
        );

        // Deferred through a limp-fall: the sequence keeps its robot, and the
        // change lands after it hands back.
        let mut falling = test_runtime();
        drive_to_ready(&mut falling);
        falling.limp_fall = LimpFall::Limp {
            since: TickTime::of(&falling.clock),
            landing: Landing::default(),
        };
        let verdict = falling.replace_policy_disturbing(
            other_walk_policy(),
            Tuning::default(),
            SkillTuning::default(),
            true,
        );
        assert_eq!(verdict, PolicySwap::GoingHome);
        falling.tick().expect("a limp-fall tick");
        assert!(falling.swap_pending(), "the change waits for the reflex");
        assert_eq!(
            walk_file(&falling),
            "velstand.onnx",
            "no swap under the fall"
        );
        assert!(
            matches!(falling.limp_fall, LimpFall::Limp { .. }),
            "the reflex was not cancelled by the queued change"
        );
    }

    // ── Workorder R1: rest/shutdown, enable/relax/init, health, coast, restart ──

    /// A body that records the torque line, for the verbs that are defined by
    /// whether they cut it. Everything else is the still `Probe`.
    #[derive(Clone)]
    struct TorqueProbe {
        sensors: Sensors,
        targets: [f64; NUM_JOINTS],
        torque: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl RobotIo for TorqueProbe {
        fn read(&mut self) -> Result<Sensors, IoError> {
            Ok(self.sensors)
        }
        fn write(&mut self, targets: &JointTargets) -> Result<(), IoError> {
            self.targets = targets.positions;
            Ok(())
        }
        fn set_gain(&mut self, _: u16) -> Result<(), IoError> {
            Ok(())
        }
        fn set_torque(&mut self, on: bool) -> Result<(), IoError> {
            self.torque.store(on, std::sync::atomic::Ordering::Relaxed);
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
    }

    /// A body whose bus can be made to fail on demand — the phone has no real bus
    /// to drop, so the failure is injected to prove the coast and the health
    /// classification are real code, not a MuJoCo-only path.
    #[derive(Clone)]
    struct FlakyIo {
        sensors: Sensors,
        targets: [f64; NUM_JOINTS],
        failing: bool,
    }

    impl RobotIo for FlakyIo {
        fn read(&mut self) -> Result<Sensors, IoError> {
            if self.failing {
                return Err(IoError::Simulated);
            }
            Ok(self.sensors)
        }
        fn write(&mut self, targets: &JointTargets) -> Result<(), IoError> {
            self.targets = targets.positions;
            Ok(())
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
    }

    fn policy_with_sitstand() -> Policy {
        let dir = crate::fixtures::policies_dir().expect("the official policy bundle");
        let paths = duck_control::policy::PolicyPaths {
            walk: dir.join("velstand.onnx"),
            stand: None,
            sitstand: Some(dir.join("alpha_sitstand.onnx")),
            ground_pick: None,
            skills: Vec::new(),
        };
        Policy::load(&paths, 0.05).expect("the bundle with a sitstand network")
    }

    fn policy_with_a_skill() -> Policy {
        let dir = crate::fixtures::policies_dir().expect("the official policy bundle");
        let paths = duck_control::policy::PolicyPaths {
            walk: dir.join("velstand.onnx"),
            stand: None,
            sitstand: Some(dir.join("alpha_sitstand.onnx")),
            ground_pick: None,
            skills: vec![dir.join("roulade.onnx")],
        };
        Policy::load(&paths, 0.05).expect("the bundle with roulade")
    }

    /// Rest is not shutdown: the same seat and rest pose, but the duck stays
    /// powered and the torque line is never cut. A shutdown after it still takes
    /// the duck off.
    #[test]
    fn rest_holds_the_rest_pose_without_powering_off() {
        let torque = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = TorqueProbe {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
            torque: torque.clone(),
        };
        let mut runtime = DuckRuntime::new(
            probe,
            policy_with_sitstand(),
            Tuning::default(),
            SkillTuning {
                // The sit hold is the ramp plus a second; shorten the ramp so the
                // test reads the state machine, not the 3 s the duck really takes.
                sitstand_ramp_s: 0.02,
                ..SkillTuning::default()
            },
            SafetyConfig::default(),
        );
        runtime.enable().expect("enable");
        for _ in 0..110 {
            let _ = runtime.tick().expect("the warm-up");
        }
        assert_eq!(runtime.bringup(), BringupState::Ready);
        assert!(
            torque.load(std::sync::atomic::Ordering::Relaxed),
            "the ramp brought the torque on"
        );

        runtime.request_rest();
        let mut saw_rest = false;
        for _ in 0..400 {
            let report = runtime.tick().expect("the rest sequence");
            if report.label == "rest" {
                saw_rest = true;
            }
            if matches!(runtime.shutdown, Shutdown::AtRest) {
                break;
            }
        }
        assert!(saw_rest, "the rest pose was reached");
        assert!(
            matches!(runtime.shutdown, Shutdown::AtRest),
            "the rest ends holding the rest pose, got {:?}",
            runtime.shutdown
        );
        assert!(
            !runtime.powered_off(),
            "rest must not power the duck off (robotd main.rs:3130-3175)"
        );
        assert!(
            torque.load(std::sync::atomic::Ordering::Relaxed),
            "the phone's rest keeps torque on — there are no servos to reboot"
        );
        // The steady state must really be the rest pose, every tick: an
        // AtRest that falls through to the limp arm would command the measured
        // pose at gain 0 and call it rest.
        for _ in 0..35 {
            let report = runtime.tick().expect("a rest steady-state tick");
            assert_eq!(report.label, "rest");
            assert_eq!(
                report.targets,
                duck_control::model::REST_POSITION,
                "AtRest commands the rest pose"
            );
            assert_eq!(report.gain, 200, "AtRest holds at the policy gain");
            assert_eq!(
                runtime.io().targets,
                duck_control::model::REST_POSITION,
                "the bytes written to the body are the rest pose"
            );
            assert!(
                torque.load(std::sync::atomic::Ordering::Relaxed),
                "AtRest keeps torque on"
            );
            assert!(!runtime.powered_off());
            assert!(matches!(runtime.shutdown, Shutdown::AtRest));
        }

        // A shutdown from the rest still takes it off: enabled is false, so the
        // official loop cuts torque immediately rather than sitting again, and
        // the power-off tick reports itself, not one more "rest" frame.
        runtime.request_shutdown();
        let report = runtime.tick().expect("the shutdown tick");
        assert!(runtime.powered_off(), "shutdown still powers off");
        assert_eq!(report.label, "off", "the power-off tick is labelled off");
        assert_eq!(report.gain, 0);
        assert!(
            !torque.load(std::sync::atomic::Ordering::Relaxed),
            "shutdown cut the torque"
        );
    }

    /// A rest asked for on a limp duck is already at its end: official robotd
    /// never sits a robot with nothing driving, it reboots the servos and is
    /// done (`main.rs:3136-3149`). Without servos, that is the rest state
    /// immediately — not a ramp that the limp arm then strands forever.
    #[test]
    fn rest_from_limp_ends_immediately_and_is_not_jammed() {
        let torque = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = TorqueProbe {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
            torque: torque.clone(),
        };
        let mut runtime = DuckRuntime::new(
            probe,
            policy_with_sitstand(),
            Tuning::default(),
            SkillTuning::default(),
            SafetyConfig::default(),
        );
        runtime.enable().expect("enable");
        for _ in 0..110 {
            let _ = runtime.tick().expect("the warm-up");
        }
        runtime.relax();
        assert_eq!(runtime.bringup(), BringupState::Limp);
        assert!(!torque.load(std::sync::atomic::Ordering::Relaxed));

        runtime.request_rest();
        let _ = runtime.tick().expect("the rest tick");
        assert!(
            matches!(runtime.shutdown, Shutdown::AtRest),
            "a limp rest goes straight to its end, got {:?}",
            runtime.shutdown
        );
        for _ in 0..30 {
            let report = runtime.tick().expect("a parked tick");
            assert_eq!(report.label, "rest");
            assert!(!runtime.powered_off(), "the rest is not a shutdown");
            assert_eq!(runtime.bringup(), BringupState::Limp);
        }

        // `disable` at the rest leaves the shutdown state: the duck is limp
        // now, and the tick says so rather than replaying "rest" at gain 200.
        runtime.disable().expect("disable at the rest");
        assert_eq!(runtime.shutdown, Shutdown::Idle);
        let report = runtime.tick().expect("the disabled tick");
        assert_eq!(report.label, "limp");
        assert_eq!(report.gain, 0);

        // And it can be left: `init` is not refused after the rest, and a
        // shutdown from it still takes the duck off.
        runtime
            .init()
            .expect("init must not be refused after the rest");
        assert!(!matches!(runtime.shutdown, Shutdown::AtRest));
        let _ = runtime.tick().expect("the re-home tick");
        assert!(matches!(runtime.bringup, Bringup::Homing { .. }));
        runtime.request_shutdown();
        let _ = runtime.tick().expect("the shutdown tick");
        assert!(runtime.powered_off(), "shutdown from the rest still works");
    }

    /// A rest is parked, not a shutdown: it is safe to swap policies, `Start`
    /// leaves it, and `relax` still cuts the torque it was holding.
    #[test]
    fn at_rest_is_parked_and_can_be_left_by_start_or_relax() {
        let torque = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = TorqueProbe {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
            torque: torque.clone(),
        };
        let mut runtime = DuckRuntime::new(
            probe,
            policy_with_sitstand(),
            Tuning::default(),
            SkillTuning {
                sitstand_ramp_s: 0.02,
                ..SkillTuning::default()
            },
            SafetyConfig::default(),
        );
        runtime.enable().expect("enable");
        for _ in 0..110 {
            let _ = runtime.tick().expect("the warm-up");
        }
        runtime.request_rest();
        for _ in 0..400 {
            let _ = runtime.tick().expect("the rest sequence");
            if matches!(runtime.shutdown, Shutdown::AtRest) {
                break;
            }
        }
        assert!(matches!(runtime.shutdown, Shutdown::AtRest));
        assert!(
            !runtime.shutting_down(),
            "a duck parked at rest is not shutting down (official rest is a nap)"
        );

        // `Start`/`enable` leaves the rest: the next tick ramps.
        runtime.enable().expect("start out of the rest");
        assert!(!matches!(runtime.shutdown, Shutdown::AtRest));
        assert_eq!(runtime.shutdown, Shutdown::Idle);
        let _ = runtime.tick().expect("the bring-up tick");
        assert!(matches!(runtime.bringup, Bringup::Homing { .. }));
        assert!(torque.load(std::sync::atomic::Ordering::Relaxed));

        // Park it again, then `relax` — the one verb that always cuts torque.
        for _ in 0..110 {
            let _ = runtime.tick().expect("back to ready");
        }
        runtime.request_rest();
        for _ in 0..400 {
            let _ = runtime.tick().expect("the rest sequence");
            if matches!(runtime.shutdown, Shutdown::AtRest) {
                break;
            }
        }
        assert!(matches!(runtime.shutdown, Shutdown::AtRest));
        runtime.relax();
        assert!(
            !torque.load(std::sync::atomic::Ordering::Relaxed),
            "relax at the rest must still cut torque (main.rs:2701-2719)"
        );
        assert!(!runtime.powered_off());
        assert_eq!(
            runtime.shutdown,
            Shutdown::Idle,
            "relax leaves the rest state instead of reporting rest forever"
        );
        let report = runtime.tick().expect("the relaxed tick");
        assert_eq!(report.label, "limp", "a relaxed duck is limp, not at rest");
        assert_eq!(report.gain, 0);
    }

    /// The startup branch is about "no successful read yet", not about a tick
    /// counter: the first failed reads still advance the duck's clock, and the
    /// gate must keep saying degraded until something actually answers.
    #[test]
    fn a_dead_startup_bus_is_degraded_until_the_first_good_read() {
        let failing = FlakyIo {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
            failing: true,
        };
        let mut runtime = DuckRuntime::new(
            failing,
            test_policy(),
            Tuning::default(),
            SkillTuning::default(),
            SafetyConfig::default(),
        );
        assert!(runtime.tick().is_err(), "the first attempt fails");
        assert!(runtime.tick().is_err(), "the second attempt fails");
        match runtime.health() {
            HealthVerdict::Degraded { reasons } => assert!(
                reasons[0].contains("after 2 attempts"),
                "the reason names the attempts: {reasons:?}"
            ),
            other => panic!("a bus that never answered is degraded, not {other:?}"),
        }
        runtime.safety.io_mut().failing = false;
        let _ = runtime.tick().expect("the first good read");
        assert!(
            runtime.health().is_healthy(),
            "one good read closes the startup branch"
        );
    }

    /// A ramp that has finished must hold the pose it ramped to. An old
    /// `enable(false)` hold (the standing home) must not be replayed over a
    /// seated init the tick the ramp completes — that is the straight-legged
    /// stand-up out of the seat the seat ramp exists to avoid.
    #[test]
    fn a_finished_ramp_holds_its_target_not_the_stale_home() {
        let probe = Probe {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
        };
        let mut runtime = DuckRuntime::new(
            probe,
            policy_with_sitstand(),
            Tuning::default(),
            SkillTuning {
                sitstand_ramp_s: 0.02,
                ..SkillTuning::default()
            },
            SafetyConfig::default(),
        );
        runtime.enable().expect("enable");
        for _ in 0..110 {
            let _ = runtime.tick().expect("the warm-up");
        }
        assert_eq!(runtime.bringup(), BringupState::Ready);
        // A stopped policy holds the standing home...
        runtime.set_policy_enabled(false);
        assert_eq!(runtime.disabled_hold, Some(DEFAULT_POSITION));

        runtime.request_rest();
        for _ in 0..400 {
            let _ = runtime.tick().expect("the rest sequence");
            if matches!(runtime.shutdown, Shutdown::AtRest) {
                break;
            }
        }
        assert!(matches!(runtime.shutdown, Shutdown::AtRest));

        // ...then the duck is seated when `init` powers it back up.
        runtime.safety.io_mut().sensors.positions = crate::SEAT_POSITION;
        runtime.init().expect("init from the rest");
        assert!(!runtime.enabled, "init does not enable the policy");
        match runtime.bringup {
            Bringup::Homing { to, .. } => {
                assert_eq!(to, crate::SEAT_POSITION, "a seated init ramps to the seat")
            }
            other => panic!("init did not ramp: {:?}", other.state()),
        }
        let mut finished = None;
        for _ in 0..110 {
            let report = runtime.tick().expect("the seat ramp");
            if runtime.bringup() == BringupState::Ready {
                finished = Some(report);
                break;
            }
        }
        let report = finished.expect("the seat ramp finishes");
        assert_eq!(
            report.targets,
            crate::SEAT_POSITION,
            "the finishing tick holds the seat it ramped to, not the stale home hold"
        );
        let report = runtime.tick().expect("the tick after the ramp");
        assert_eq!(
            report.targets,
            crate::SEAT_POSITION,
            "and the stopped hold after it is the seat"
        );
    }

    /// A fallen duck is not sat up for a rest: official `can_sit` includes
    /// `!fallen`, and the rest then takes the cannot-sit path — torque off,
    /// no ramp (main.rs:3131-3146).
    #[test]
    fn rest_on_a_fallen_duck_does_not_sit_it_up() {
        let torque = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = TorqueProbe {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
            torque: torque.clone(),
        };
        let mut runtime = DuckRuntime::new(
            probe,
            policy_with_sitstand(),
            Tuning::default(),
            SkillTuning {
                sitstand_ramp_s: 0.02,
                ..SkillTuning::default()
            },
            SafetyConfig::default(),
        );
        runtime.enable().expect("enable");
        for _ in 0..110 {
            let _ = runtime.tick().expect("the warm-up");
        }
        assert_eq!(runtime.bringup(), BringupState::Ready);
        assert!(torque.load(std::sync::atomic::Ordering::Relaxed));

        // Down for longer than the fall debounce: the robot is genuinely
        // fallen, not merely tipped for a frame.
        runtime.safety.io_mut().sensors.imu.gravity = [0.0, 0.0, 0.8];
        for _ in 0..12 {
            let _ = runtime.tick().expect("the fall");
        }
        assert!(runtime.safety.fallen(), "the fall is latched");

        runtime.request_rest();
        let report = runtime.tick().expect("the rest tick");
        assert!(
            matches!(runtime.shutdown, Shutdown::AtRest),
            "a fallen duck takes the cannot-sit rest, got {:?}",
            runtime.shutdown
        );
        assert_eq!(report.label, "rest");
        assert!(!runtime.powered_off(), "the rest is not a shutdown");
        assert!(
            !torque.load(std::sync::atomic::Ordering::Relaxed),
            "the cannot-sit rest cuts torque, like the official reboot"
        );
    }

    /// A skill that was mid-flight when the duck fell must not survive the
    /// cannot-sit rest and replay when the policy drives again: official
    /// `request_reboot_motors` forgets the controller (main.rs:2752-2769).
    #[test]
    fn a_rest_out_of_a_skill_does_not_replay_it() {
        let probe = TorqueProbe {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
            torque: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let mut runtime = DuckRuntime::new(
            probe,
            policy_with_a_skill(),
            Tuning::default(),
            SkillTuning {
                skills: vec![robotd_params::SkillDef {
                    name: "roulade".to_owned(),
                    duration: 1.0,
                    ..robotd_params::SkillDef::default()
                }],
                ..SkillTuning::default()
            },
            SafetyConfig::default(),
        );
        runtime.enable().expect("enable");
        for _ in 0..110 {
            let _ = runtime.tick().expect("the warm-up");
        }
        assert_eq!(runtime.bringup(), BringupState::Ready);
        runtime.trigger_skill("roulade").expect("the skill starts");
        assert!(runtime.controller.busy(), "the skill is running");

        // Down, with the skill still owning the robot.
        runtime.safety.io_mut().sensors.imu.gravity = [0.0, 0.0, 0.8];
        for _ in 0..12 {
            let _ = runtime.tick().expect("the fall");
        }
        assert!(runtime.safety.fallen());
        runtime.request_rest();
        let _ = runtime.tick().expect("the rest tick");
        assert!(matches!(runtime.shutdown, Shutdown::AtRest));
        assert!(
            !runtime.controller.busy(),
            "the cannot-sit rest forgets the skill"
        );

        // Stand back up and drive: the skill is gone, not paused.
        runtime.safety.io_mut().sensors.imu.gravity = [0.0, 0.0, -1.0];
        runtime.init().expect("init from the rest");
        for _ in 0..110 {
            let _ = runtime.tick().expect("the stand-up ramp");
        }
        runtime.enable().expect("enable");
        let report = runtime.tick().expect("the first driven tick");
        assert_ne!(report.label, "roulade", "the skill must not replay");
        assert!(!runtime.controller.busy());
        assert!(!matches!(
            runtime.driving(),
            Some(crate::control::Driving::Skill(_))
        ));
    }

    /// A rest on a duck with no sitstand network cannot sit; it still eases
    /// straight into the rest pose and stays powered, rather than falling back
    /// to the shutdown's power-off.
    #[test]
    fn rest_without_a_sit_network_still_parks_powered() {
        let mut runtime = test_runtime();
        assert!(
            !runtime.controller.has_sitstand(),
            "the test set has no sitstand"
        );
        drive_to_ready(&mut runtime);
        runtime.request_rest();
        let _ = runtime.tick().expect("the rest starts");
        assert!(
            matches!(
                runtime.shutdown,
                Shutdown::Resting {
                    power_off: false,
                    ..
                }
            ),
            "with nothing to sit with, the rest begins at the rest ramp: {:?}",
            runtime.shutdown
        );
        for _ in 0..120 {
            let _ = runtime.tick().expect("the rest ramp");
        }
        assert!(matches!(runtime.shutdown, Shutdown::AtRest));
        assert!(!runtime.powered_off(), "a rest never latches powered_off");

        // `robot.init` is how the duck leaves the rest: it re-arms the
        // bring-up and the next tick ramps, still powered.
        runtime.init().expect("init leaves the rest");
        assert!(!matches!(runtime.shutdown, Shutdown::AtRest));
        let _ = runtime.tick().expect("the re-home tick");
        assert!(matches!(runtime.bringup, Bringup::Homing { .. }));
        assert!(!runtime.powered_off(), "init does not power off");
    }

    /// `robot.enable on:false` stops the controller and holds torque; `robot.relax`
    /// is the one that cuts it; `disable` keeps the shutdown path's behaviour.
    #[test]
    fn stopping_the_policy_keeps_torque_and_relax_cuts_it() {
        let torque = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = TorqueProbe {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
            torque: torque.clone(),
        };
        let mut runtime = DuckRuntime::new(
            probe,
            test_policy(),
            Tuning::default(),
            SkillTuning::default(),
            SafetyConfig::default(),
        );
        runtime.enable().expect("enable");
        for _ in 0..110 {
            let _ = runtime.tick().expect("the warm-up");
        }
        assert_eq!(runtime.bringup(), BringupState::Ready);

        runtime.set_policy_enabled(false);
        assert!(!runtime.enabled, "the policy is stopped");
        assert_eq!(
            runtime.bringup(),
            BringupState::Ready,
            "stopping is not a bring-up change"
        );
        assert!(
            torque.load(std::sync::atomic::Ordering::Relaxed),
            "enable on:false holds torque (main.rs:3763-3795)"
        );
        let report = runtime.tick().expect("a stopped tick");
        // Official active disable commands the home pose (or the seat), not
        // "freeze wherever the gait had got to" (main.rs:3761-3793).
        assert_eq!(
            report.targets, DEFAULT_POSITION,
            "enable(false) must command the home pose"
        );
        assert_eq!(report.gain, 200);
        assert_ne!(report.label, "walk", "the policy no longer drives");
        assert!(!runtime.driving_prev);
        assert!(
            torque.load(std::sync::atomic::Ordering::Relaxed),
            "the stopped duck is still powered"
        );

        runtime.set_policy_enabled(true);
        assert!(runtime.enabled);

        runtime.relax();
        assert!(!runtime.enabled);
        assert_eq!(
            runtime.bringup(),
            BringupState::Limp,
            "relax returns to Limp"
        );
        assert!(!runtime.powered_off(), "relax is not a power-off");
        assert!(
            !torque.load(std::sync::atomic::Ordering::Relaxed),
            "relax is the verb that cuts torque (main.rs:2701-2719)"
        );
    }

    /// A seated robot stopped by `robot.enable on:false` holds the seat it is
    /// *measured* to be in — official `known_positions` — not the nominal
    /// seat constant.
    #[test]
    fn stopping_a_seated_policy_holds_the_measured_seat() {
        let mut measured = crate::SEAT_POSITION;
        // A seat pose that is not the nominal constant, but still a seat.
        measured[3] += 0.02;
        assert_ne!(measured, crate::SEAT_POSITION);
        let probe = Probe {
            sensors: Sensors {
                positions: measured,
                ..Sensors::default()
            },
            targets: [0.0; NUM_JOINTS],
        };
        let mut runtime = DuckRuntime::new(
            probe,
            policy_with_sitstand(),
            Tuning::default(),
            SkillTuning::default(),
            SafetyConfig::default(),
        );
        runtime.enable().expect("enable");
        for _ in 0..110 {
            let _ = runtime.tick().expect("the seated warm-up");
        }
        assert_eq!(runtime.bringup(), BringupState::Ready);
        assert!(
            runtime.controller.is_sitting(),
            "the posture check found the seat"
        );
        runtime.set_policy_enabled(false);
        let report = runtime.tick().expect("the stopped tick");
        assert_eq!(
            report.targets, measured,
            "a seated stop holds the measured seat (main.rs:3772-3777)"
        );
    }

    /// `robot.init`'s four branches: off is refused, limp powers up, ready+seated
    /// holds the seat, ready otherwise re-homes.
    #[test]
    fn init_takes_the_official_four_branches() {
        // Powered off: ignored, and the caller can say why.
        let mut off = test_runtime();
        off.request_shutdown();
        let _ = off.tick().expect("the shutdown tick");
        assert!(off.powered_off());
        assert!(
            off.init().is_err(),
            "init on a powered-off duck is refused (main.rs:2657-2683)"
        );
        assert!(off.powered_off(), "the refusal leaves it off");

        // Limp: torque on and ramp, but the policy is *not* enabled — official
        // `robot.init` needs no policy (main.rs:2661-2677). The torque probe
        // is what makes this non-vacuous: `Probe::set_torque` is a no-op.
        let torque = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut limp = DuckRuntime::new(
            TorqueProbe {
                sensors: Sensors::default(),
                targets: [0.0; NUM_JOINTS],
                torque: torque.clone(),
            },
            test_policy(),
            Tuning::default(),
            SkillTuning::default(),
            SafetyConfig::default(),
        );
        assert_eq!(limp.bringup(), BringupState::Limp);
        limp.init().expect("init on a limp duck stands it up");
        assert!(
            !limp.enabled,
            "robot.init must not enable the policy (that is Start/enable)"
        );
        assert!(
            matches!(limp.bringup, Bringup::Homing { .. }),
            "init arms the homing ramp immediately, got {:?}",
            limp.bringup
        );
        assert!(
            torque.load(std::sync::atomic::Ordering::Relaxed),
            "init must really turn the torque on"
        );

        // Ready and seated: the seat is held, not re-homed.
        let probe = Probe {
            sensors: Sensors {
                positions: crate::SEAT_POSITION,
                ..Sensors::default()
            },
            targets: [0.0; NUM_JOINTS],
        };
        let mut seated = DuckRuntime::new(
            probe,
            policy_with_sitstand(),
            Tuning::default(),
            SkillTuning::default(),
            SafetyConfig::default(),
        );
        seated.enable().expect("enable");
        for _ in 0..110 {
            let _ = seated.tick().expect("the seated warm-up");
        }
        assert_eq!(seated.bringup(), BringupState::Ready);
        assert!(
            seated.controller.is_sitting(),
            "the posture check found the seat"
        );
        seated.init().expect("init while seated");
        assert_eq!(
            seated.bringup(),
            BringupState::Ready,
            "a seated duck holds the seat instead of re-homing"
        );

        // Ready otherwise: re-home from the measured pose.
        let mut ready = test_runtime();
        ready.enable().expect("enable");
        for _ in 0..110 {
            let _ = ready.tick().expect("the warm-up");
        }
        assert_eq!(ready.bringup(), BringupState::Ready);
        ready.init().expect("init while standing");
        match ready.bringup {
            Bringup::Homing { to, .. } => {
                assert_eq!(to, DEFAULT_POSITION, "a standing duck re-homes");
            }
            other => panic!("init did not re-home: {:?}", other.state()),
        }
    }

    /// The health gate's official inputs: starting is degraded, a missing policy
    /// is unhealthy, and a run of failed reads is unhealthy in the official
    /// wording.
    #[test]
    fn health_reports_starting_policy_and_bus_runs() {
        let mut runtime = test_runtime();
        match runtime.health() {
            // Official no-ticks branch, no startup failures: the loop simply
            // has not completed a cycle (main.rs:921-935).
            HealthVerdict::Unhealthy { reasons } => {
                assert!(reasons[0].contains("has not completed a cycle"))
            }
            other => panic!("a loop that has not ticked is not {other:?}"),
        }
        runtime.startup_bus_failures = 0;
        let _ = runtime.tick().expect("the first tick");
        assert!(
            runtime.health().is_healthy(),
            "a ticked, settled loop is healthy"
        );

        runtime.mark_policy_error("velstand.onnx did not build".to_owned());
        match runtime.health() {
            HealthVerdict::Unhealthy { reasons } => assert!(
                reasons[0] == "policy unavailable: velstand.onnx did not build",
                "{reasons:?}"
            ),
            other => panic!("an unavailable policy is unhealthy (main.rs:940-942), got {other:?}"),
        }
        runtime.clear_policy_error();
        assert!(runtime.health().is_healthy());

        runtime.consecutive_errors = MAX_CONSECUTIVE_ERRORS;
        match runtime.health() {
            HealthVerdict::Unhealthy { reasons } => assert_eq!(
                reasons,
                vec![format!(
                    "{MAX_CONSECUTIVE_ERRORS} consecutive bus read failures"
                )]
            ),
            other => panic!("a run of failed reads is unhealthy (main.rs:969-987), got {other:?}"),
        }
        runtime.consecutive_errors = MAX_CONSECUTIVE_ERRORS - 1;
        assert!(
            !matches!(runtime.health(), HealthVerdict::Unhealthy { .. }),
            "one short of the threshold is not a verdict"
        );
    }

    /// A failed read coasts the last good sample; the count runs, the policy stops
    /// at `COAST_TICKS`, and the health gate turns unhealthy at the official
    /// threshold. The first read has nothing to coast on and is the only fatal one.
    #[test]
    fn a_failed_read_coasts_and_counts() {
        let failing = FlakyIo {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
            failing: true,
        };
        let mut never_read = DuckRuntime::new(
            failing,
            test_policy(),
            Tuning::default(),
            SkillTuning::default(),
            SafetyConfig::default(),
        );
        assert!(
            never_read.tick().is_err(),
            "a first read that fails has no sample to coast on"
        );

        let probe = FlakyIo {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
            failing: false,
        };
        let mut runtime = DuckRuntime::new(
            probe,
            test_policy(),
            Tuning::default(),
            SkillTuning::default(),
            SafetyConfig::default(),
        );
        runtime.enable().expect("enable");
        for _ in 0..110 {
            let _ = runtime.tick().expect("the warm-up");
        }
        assert_eq!(runtime.bringup(), BringupState::Ready);
        assert_eq!(runtime.consecutive_errors(), 0);
        let last_good = runtime.safety.io().sensors;

        runtime.safety.io_mut().failing = true;
        for expected in 1..=COAST_TICKS {
            let report = runtime.tick().expect("a coasted tick");
            assert_eq!(runtime.consecutive_errors(), expected);
            assert_eq!(
                report.sensors, last_good,
                "the coast reuses the last good sample"
            );
        }
        // The sample is now older than robotd lets the policy step from.
        let exhausted = runtime.tick().expect("a tick past the coast");
        assert_eq!(runtime.consecutive_errors(), COAST_TICKS + 1);
        assert_eq!(exhausted.label, "held", "no policy steps a coasted sample");
        for _ in COAST_TICKS + 1..MAX_CONSECUTIVE_ERRORS {
            let _ = runtime.tick().expect("a failed tick");
        }
        assert_eq!(runtime.consecutive_errors(), MAX_CONSECUTIVE_ERRORS);
        match runtime.health() {
            HealthVerdict::Unhealthy { reasons } => assert_eq!(
                reasons,
                vec![format!(
                    "{MAX_CONSECUTIVE_ERRORS} consecutive bus read failures"
                )]
            ),
            other => panic!("the run must read unhealthy, got {other:?}"),
        }

        runtime.safety.io_mut().failing = false;
        let _ = runtime.tick().expect("the recovered read");
        assert_eq!(
            runtime.consecutive_errors(),
            0,
            "a good read clears the run"
        );
        assert!(
            runtime.health().is_healthy(),
            "the recovered loop is healthy again"
        );
    }

    /// `safe_to_restart`: a walking duck is not safe to interrupt; a standing or
    /// fallen one is.
    #[test]
    fn safe_to_restart_only_refuses_a_walking_duck() {
        let mut runtime = test_runtime();
        drive_to_ready(&mut runtime);
        assert!(
            !runtime.safe_to_restart(),
            "a walking duck must not be restarted (main.rs:1093-1113)"
        );

        runtime.set_twist([0.0; 3]);
        for _ in 0..40 {
            let _ = runtime.tick().expect("the standstill ticks");
        }
        assert!(
            runtime.safe_to_restart(),
            "a standing duck is going nowhere either way"
        );

        // Down, with the stick still pushed: a fallen robot is safe to interrupt.
        let mut down = test_runtime();
        drive_to_ready(&mut down);
        down.safety.set_fall_state(1_000, true);
        assert!(
            down.safe_to_restart(),
            "a robot already down is safe to restart"
        );
    }

    /// `robot.state`'s skeleton is the whole body's FK at the measured joints, and
    /// `t_ns` is the tick's nanoseconds — not the empty/zero frame it used to be.
    #[test]
    fn the_state_frame_publishes_the_skeleton_and_the_tick_ns() {
        let mut sensors = Sensors::default();
        // A leg joint, by wire index, so the skeleton really moves.
        sensors.positions[2] = 0.35;
        let probe = Probe {
            sensors,
            targets: [0.0; NUM_JOINTS],
        };
        let mut runtime = DuckRuntime::new(
            probe,
            test_policy(),
            Tuning::default(),
            SkillTuning::default(),
            SafetyConfig::default(),
        );
        let _ = runtime.tick().expect("a frame");
        let state = runtime.robot_state().expect("the first frame");
        assert_eq!(
            state.t_ns,
            runtime.clock().tick * runtime.dt.as_nanos() as u64,
            "t_ns is the tick's own clock in nanoseconds"
        );
        assert_eq!(
            state.joints.as_slice(),
            sensors.positions.as_slice(),
            "the frame still carries the measured joints"
        );

        let model = kinematics::Model::alpha();
        let expected_bodies = model.body_names().count();
        assert_eq!(
            state.skeleton.len(),
            expected_bodies,
            "one pose per body, in ModelResult::skeleton order"
        );
        assert!(expected_bodies > 5, "the alpha model has a whole body");
        let angles: Vec<f64> = model
            .joint_names()
            .map(|name| {
                let wire = duck_ipc_proto::JOINT_NAMES
                    .iter()
                    .position(|wire| *wire == name)
                    .expect("every model joint is on the wire");
                sensors.positions[wire]
            })
            .collect();
        let expected = model.body_poses(&angles);
        for (pose, want) in state.skeleton.iter().zip(expected.iter()) {
            assert_eq!(pose.pos, want.pos, "body position follows the joint map");
            assert_eq!(
                pose.quat,
                want.quat.wxyz(),
                "body orientation follows it too"
            );
        }
        let zeroed = skeleton_at(&[0.0; NUM_JOINTS]);
        assert!(
            state
                .skeleton
                .iter()
                .zip(zeroed.iter())
                .any(|(bent, rest)| bent.pos != rest.pos),
            "the bent knee moved the skeleton"
        );
        assert!(
            state.frames.is_none(),
            "the phone has no camera or ToF to publish a frame for"
        );
    }

    /// Timer rules fire on their period whatever the allowance is; a sleeping duck
    /// is left alone.
    #[test]
    fn timer_rules_fire_independently_of_the_budget_and_never_asleep() {
        let mut runtime = test_runtime();
        runtime
            .set_rules(&serde_json::json!({
                "rules": [{
                    "trigger": "timer",
                    "action": "sound",
                    "value": "coo",
                    "period_s": 5
                }]
            }))
            .expect("the timer rule loads");
        assert!(runtime.care.budget_s.is_none(), "no allowance is set");
        for _ in 0..260 {
            let _ = runtime.tick().expect("the timer ticks");
        }
        let fired = runtime.last_rule().expect("the timer must fire");
        assert_eq!(fired.trigger, "timer");
        assert_eq!(fired.value, "coo");
        assert_eq!(fired.tick, 250, "a 5 s period at 50 Hz is tick 250");

        let mut asleep = test_runtime();
        asleep
            .set_rules(&serde_json::json!({
                "rules": [{
                    "trigger": "timer",
                    "action": "sound",
                    "value": "coo",
                    "period_s": 5
                }]
            }))
            .expect("the timer rule loads");
        asleep.care.enter_night(0);
        for _ in 0..400 {
            let _ = asleep.tick().expect("the sleeping ticks");
        }
        assert!(asleep.care.asleep, "the night is not stirred");
        assert!(
            asleep.last_rule().is_none(),
            "a sleeping duck does not run its timer rules"
        );
    }

    /// Every driving slot and its file, for the shell's disturb comparison: the
    /// four fixed slots are all present in the official set, and skills carry their
    /// own names.
    #[test]
    fn driving_net_paths_lists_every_slot() {
        let dir = crate::fixtures::policies_dir().expect("the official policy bundle");
        let paths = duck_control::policy::PolicyPaths {
            walk: dir.join("velstand.onnx"),
            stand: Some(dir.join("alpha_stand.onnx")),
            sitstand: Some(dir.join("alpha_sitstand.onnx")),
            ground_pick: Some(dir.join("alpha_ground_pick.onnx")),
            skills: vec![dir.join("roulade.onnx"), dir.join("ball_kick_left.onnx")],
        };
        let policy = Policy::load(&paths, 0.05).expect("the full official set");
        let probe = Probe {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
        };
        let runtime = DuckRuntime::new(
            probe,
            policy,
            Tuning::default(),
            SkillTuning {
                skills: vec![
                    robotd_params::SkillDef {
                        name: "roulade".to_owned(),
                        ..robotd_params::SkillDef::default()
                    },
                    robotd_params::SkillDef {
                        name: "kick_left".to_owned(),
                        ..robotd_params::SkillDef::default()
                    },
                ],
                ..SkillTuning::default()
            },
            SafetyConfig::default(),
        );
        let listed = runtime.driving_net_paths();
        let expected = [
            ("walk", "velstand.onnx"),
            ("stand", "alpha_stand.onnx"),
            ("sitstand", "alpha_sitstand.onnx"),
            ("ground_pick", "alpha_ground_pick.onnx"),
            ("roulade", "roulade.onnx"),
            ("kick_left", "ball_kick_left.onnx"),
        ];
        assert_eq!(listed.len(), expected.len());
        for ((name, file), (listed_name, path)) in expected.iter().zip(listed.iter()) {
            assert_eq!(listed_name, name);
            let path = path
                .as_ref()
                .unwrap_or_else(|| panic!("{name} must resolve to its file"));
            assert_eq!(
                path.file_name().and_then(|name| name.to_str()),
                Some(*file),
                "{name}"
            );
        }
    }
}
