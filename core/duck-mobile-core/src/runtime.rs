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
const LIMP_SETTLE: Duration = Duration::from_millis(500);
/// After a limp-fall, drive zero command for this long so velstand can get up.
const RECOVERY: Duration = Duration::from_secs(2);
/// `SafetyConfig::default().deadman`.
const DEADMAN: Duration = Duration::from_millis(500);

/// How long a twist from the intent API keeps control after it arrives.
///
/// The app pushes the on-screen joystick's twist into the runtime every
/// frame, so an API twist without a lease would be overwritten 20 ms later
/// and an external agent could not drive the duck at all. With the lease the
/// API owns the duck while it keeps talking, and control goes back to the
/// screen when it stops.
const API_LEASE: Duration = Duration::from_millis(1000);

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

/// The runtime. `T` is the body: MuJoCo in the app, `RemoteIo` on the desktop harness.
pub struct DuckRuntime<T: RobotIo> {
    safety: Safety<T>,
    controller: Controller,
    fall: FallPredictor,
    bringup: Bringup,
    command: Command,
    enabled: bool,
    limp_until: Option<Instant>,
    limp_hold: Option<[f64; NUM_JOINTS]>,
    recover_until: Option<Instant>,
    last_twist: Instant,
    twist_fresh: bool,
    /// The twist from the intent API, and the lease it holds on the duck.
    api_lease: ApiLease,
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
            limp_until: None,
            limp_hold: None,
            recover_until: None,
            last_twist: Instant::now(),
            twist_fresh: false,
            api_lease: ApiLease::default(),
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
        Ok(())
    }

    /// Torque off and forget the seat/move state.
    pub fn disable(&mut self) -> Result<(), RuntimeError> {
        self.safety.set_torque(false)?;
        self.enabled = false;
        self.bringup = Bringup::Limp;
        self.controller.reset();
        self.limp_until = None;
        self.limp_hold = None;
        self.recover_until = None;
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
    }

    pub fn set_body(&mut self, z: f64, roll: f64, pitch: f64) {
        self.command.body.z = z;
        self.command.body.roll = roll;
        self.command.body.pitch = pitch;
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

    /// Swap the policy set at rest. The body and bring-up state stay; the
    /// controller (skills, seat, filters) starts fresh.
    pub fn replace_policy(&mut self, policy: Policy, tuning: Tuning, skills: SkillTuning) {
        self.controller = Controller::new(policy, tuning, skills);
        self.limp_until = None;
        self.limp_hold = None;
        self.recover_until = None;
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
                report.label = "limp".to_owned();
                report.gain = 0;
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

        // Limp-fall reflex: drop the gain and ride the fall out, then let velstand get up.
        if let Some(until) = self.limp_until {
            if now < until {
                let target = self.limp_hold.unwrap_or(hold);
                self.safety.apply(target, hold, GAIN_LIMP)?;
                report.label = "limp_fall".to_owned();
                report.targets = target;
                report.gain = GAIN_LIMP;
                return Ok(report);
            }
            self.limp_until = None;
            self.limp_hold = None;
            self.recover_until = Some(now + RECOVERY);
            self.controller.reset();
            tracing::info!("limp-fall: settle done; handing back to the policy to get up");
        }

        let recovering = self.recover_until.is_some_and(|until| now < until);
        if self.recover_until.is_some_and(|until| now >= until) {
            self.recover_until = None;
        }

        if !recovering
            && !self.controller.busy()
            && self.fall.observe(&sensors.imu, self.dt)
        {
            self.limp_until = Some(now + LIMP_SETTLE);
            self.limp_hold = Some(hold);
            self.safety.apply(hold, hold, GAIN_LIMP)?;
            report.label = "limp_fall".to_owned();
            report.targets = hold;
            report.gain = GAIN_LIMP;
            return Ok(report);
        }

        let mut command = self.command;
        // An API twist owns the duck while its lease is live; the on-screen
        // joystick gets it back when the API goes quiet.
        if let Some(twist) = self.api_lease.active(now) {
            command.twist = twist;
        }
        if recovering {
            command.twist = [0.0; 3];
        }

        match self
            .controller
            .step(&sensors, &command, false, self.dt.as_secs_f64(), 1.0)
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
