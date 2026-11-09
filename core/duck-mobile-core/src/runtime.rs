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

/// The intent API's claim on the duck: the twist it last asked for and the
/// lease it holds. Separate from the runtime so it can be tested on its own.
#[derive(Debug, Default)]
struct ApiLease {
    twist: [f64; 3],
    until: Option<Instant>,
}

impl ApiLease {
    fn take(&mut self, twist: [f64; 3]) {
        self.twist = twist;
        self.until = Some(Instant::now() + API_LEASE);
    }

    /// The twist the API owns right now, if its lease is still live.
    fn active(&mut self, now: Instant) -> Option<[f64; 3]> {
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
        since: Instant,
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
    fn homing_target(&self, now: Instant) -> Option<[f64; NUM_JOINTS]> {
        let Self::Homing { from, since, to } = self else {
            return None;
        };
        let t = now.duration_since(*since).as_secs_f64() / HOME_RAMP.as_secs_f64();
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

/// robotd's limp-fall sequence.
#[derive(Debug, Clone, Copy)]
enum LimpFall {
    Idle,
    /// Gains at `GAIN_LIMP`, joints commanded to wherever they already are.
    Limp { since: Instant, landing: Landing },
    /// Ramping from where the robot landed back to the standing pose.
    Posing { from: [f64; NUM_JOINTS], since: Instant },
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
    last_twist: Instant,
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
    idle_params: robotd_params::IdleParams,
    /// When the head was last commanded; idle glances start after `after_s`.
    last_head_command: Instant,
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
            last_twist: Instant::now(),
            twist_fresh: false,
            api_lease: ApiLease::default(),
            twist_ema: [0.0; 3],
            head_ema: [0.0; 4],
            body_ema: [0.0; 3],
            body_active: false,
            idle_head: crate::control::IdleHead::new(robotd_params::IdleParams::default(), 0xD0C),
            idle_params: robotd_params::IdleParams::default(),
            last_head_command: Instant::now(),
            dt: Duration::from_millis(20),
        }
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
        if at_home {
            self.bringup = Bringup::Ready;
            self.controller.reset();
            tracing::info!("bring-up: already at home; the policy has the robot");
        } else {
            self.bringup = Bringup::Homing {
                from: sensors.positions,
                since: Instant::now(),
                to: DEFAULT_POSITION,
            };
        }
        self.enabled = true;
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
        Ok(())
    }

    pub fn set_twist(&mut self, twist: [f64; 3]) {
        self.command.twist = twist;
        self.last_twist = Instant::now();
        self.twist_fresh = true;
    }

    /// A twist from the intent API. It takes the duck over from the
    /// on-screen joystick for [API_LEASE], so an external agent can drive it
    /// while it keeps sending twists.
    pub fn set_api_twist(&mut self, twist: [f64; 3]) {
        self.api_lease.take(twist);
    }

    pub fn set_head(&mut self, head: [f64; 4]) {
        self.command.head = head;
        // Any head command wakes the idle look-around.
        self.last_head_command = Instant::now();
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
        self.controller.skill_names()
    }

    pub fn trigger_skill(&mut self, name: &str) -> Result<bool, &'static str> {
        let index = self
            .controller
            .skill_names()
            .iter()
            .position(|candidate| candidate == name)
            .ok_or("no such skill")?;
        self.controller.start_skill(index)
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
        self.limp_fall = LimpFall::Idle;
    }

    /// Shake/shove input: an external trunk velocity impulse.
    pub fn push(&mut self, velocity: [f64; 3]) -> Result<(), RuntimeError> {
        self.safety.push(velocity)?;
        Ok(())
    }

    /// One 50 Hz tick. The order is `robotd`'s: read, observe, gate, policy, safety.
    pub fn tick(&mut self) -> Result<TickReport, RuntimeError> {
        let now = Instant::now();
        let sensors: Sensors = self.safety.read()?;
        self.safety.observe(&sensors, self.dt);
        let hold = sensors.positions;

        let mut report = TickReport {
            label: "held".to_owned(),
            targets: hold,
            gain: 200,
            fallen: self.safety.fallen(),
            bringup: self.bringup.state(),
        };

        match self.bringup {
            Bringup::Limp => {
                // Torque off, but still write and step: a limp duck has to
                // fall, not freeze in the pose it was standing in.
                self.safety.apply(hold, hold, 0)?;
                report.label = "limp".to_owned();
                report.gain = 0;
                report.targets = hold;
                return Ok(report);
            }
            Bringup::Homing { to, .. } => {
                let target = self.bringup.homing_target(now).unwrap_or(to);
                let done = self.bringup.homing_target(now).is_none();
                self.safety.apply(target, hold, 200)?;
                report.label = if done { "ready" } else { "homing" }.to_owned();
                report.targets = target;
                report.gain = 200;
                if done {
                    self.bringup = Bringup::Ready;
                    self.controller.reset();
                    tracing::info!("bring-up: at home pose; the policy has the robot");
                }
                return Ok(report);
            }
            Bringup::Ready => {}
        }

        // Deadman: a command stream that stops leaves the robot standing, not walking off.
        if self.twist_fresh && now.duration_since(self.last_twist) > DEADMAN {
            self.command.twist = [0.0; 3];
            self.twist_fresh = false;
            tracing::warn!("deadman: no twist for {DEADMAN:?}; standing still");
        }

        // Limp-fall, robotd's sequence: go soft before the landing, let the
        // robot arrive limp, put it back into the standing pose, and only then
        // hand it to the policy. Read from the fall predictor, not the `fallen`
        // verdict: a verdict that waits for the robot to be down cannot make
        // it soft before it lands.
        let was_driving = self.enabled && matches!(self.bringup, Bringup::Ready);

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
                let timed_out = now.duration_since(since) >= LIMP_FALL_MAX;
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
                if now.duration_since(since) >= LIMP_FALL_POSE {
                    tracing::warn!("posed - handing back to the standing policy");
                    self.limp_fall = LimpFall::Idle;
                    self.fall.reset();
                    self.controller.reset();
                }
            }
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
                let t = (now.duration_since(since).as_secs_f64()
                    / LIMP_FALL_POSE.as_secs_f64())
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

        // The idle look-around, robotd's: a still duck glances, tilts its head
        // and breathes, and the head command it feeds is an observation input
        // (the policy's head block), exactly as on the real robot.
        let still = self.idle_params.enabled_in(robotd_params::Mode::Walk)
            && was_driving
            && matches!(self.limp_fall, LimpFall::Idle)
            && command.twist.iter().all(|value| value.abs() < 1e-3)
            && now.duration_since(self.last_head_command) >= self.idle_head.after()
            && !self.controller.busy();
        let idle = self.idle_head.tick(now, still, self.dt);
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

        match self
            .controller
            .step(
                &sensors,
                &command,
                self.body_active,
                self.dt.as_secs_f64(),
                1.0,
            )
        {
            Ok(step) => {
                self.safety.apply(step.targets, hold, step.gain)?;
                report.label = step.label.to_string();
                report.targets = step.targets;
                report.gain = step.gain;
            }
            Err(error) => {
                self.safety.apply(hold, hold, 200)?;
                report.label = "held".to_owned();
                report.gain = 200;
                tracing::warn!(%error, "inference failed; holding the pose");
            }
        }
        report.fallen = self.safety.fallen();
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_api_lease_outlives_the_joystick_twist() {
        let mut lease = ApiLease::default();
        assert_eq!(lease.active(Instant::now()), None);

        lease.take([0.4, 0.0, 0.0]);
        let now = Instant::now();
        assert_eq!(lease.active(now), Some([0.4, 0.0, 0.0]));
        // The app pushes the joystick's twist every 20 ms; the lease must
        // survive that, otherwise an external agent could never drive.
        assert_eq!(lease.active(now + Duration::from_millis(500)), Some([0.4, 0.0, 0.0]));
    }

    #[test]
    fn the_lease_expires_and_gives_the_duck_back() {
        let mut lease = ApiLease::default();
        lease.take([0.4, 0.0, 0.0]);
        let now = Instant::now();
        assert_eq!(lease.active(now + API_LEASE + Duration::from_millis(1)), None);
        // And it stays given back.
        assert_eq!(lease.active(now + API_LEASE + Duration::from_millis(2)), None);
    }

    #[test]
    fn a_new_twist_extends_the_lease() {
        let mut lease = ApiLease::default();
        lease.take([0.4, 0.0, 0.0]);
        let now = Instant::now();
        assert_eq!(lease.active(now + Duration::from_millis(900)), Some([0.4, 0.0, 0.0]));
        lease.take([0.0, 0.0, 0.2]);
        assert_eq!(lease.active(now + Duration::from_millis(900)), Some([0.0, 0.0, 0.2]));
        // The new twist bought a fresh lease: another second of control.
        assert_eq!(lease.active(now + Duration::from_millis(1900)), None);
    }
}
