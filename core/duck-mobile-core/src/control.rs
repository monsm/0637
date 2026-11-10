//! Re-export of the official `robotd` skill scheduler.
//!
//! The file is compiled directly from the upstream checkout. `#[path]` is
//! relative to this file's directory (`src/`), so three levels up is the
//! workspace root (`D:\AI\Duck`).

#[path = "../../../upstream/microduck/robotd/src/control.rs"]
mod inner;

pub use inner::*;

/// The idle look-around: the real duck glances, tilts its head and breathes
/// when nothing is happening. `robotd` owns the behaviour; compiling the same
/// file keeps the app's duck doing what the physical one does.
#[path = "../../../upstream/microduck/robotd/src/idle_head.rs"]
mod idle_head;

pub use idle_head::{IdleHead, IdleOffset, IdleState};

/// Is the robot standing, sitting, or lying down — judged from where it
/// actually is. `robotd` asks once before a policy takes over at the home
/// pose: a seated duck must rise first, or the gait walks it over backwards.
#[path = "../../../upstream/microduck/robotd/src/posture.rs"]
pub mod posture;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A checkpoint must replay the same glances: RNG, phases and the spell's
    /// clock all live in IdleHead, and leaving any of them behind was shown to
    /// diverge in ~98% of replayed ticks.
    #[test]
    fn idle_state_roundtrips_the_spell() {
        let origin = Instant::now();
        let mut head = IdleHead::new(robotd_params::IdleParams::default(), 0xD0C);
        let dt = Duration::from_millis(20);
        // Tick well into an idle spell (after 2 s + 1.5 s fade).
        for tick in 0..400 {
            head.tick(origin + dt * tick, true, dt);
        }
        let state = head.snapshot(origin);
        let json = serde_json::to_string(&state).expect("idle state serializes");
        let restored: IdleState = serde_json::from_str(&json).expect("idle state parses");
        let mut replay = IdleHead::new(robotd_params::IdleParams::default(), 0xD0C);
        replay.restore_state(restored, origin);
        // Both continue from the same simulated instant and must agree exactly.
        for tick in 400..500 {
            let now = origin + dt * tick;
            let a = head.tick(now, true, dt);
            let b = replay.tick(now, true, dt);
            // Instant -> millis -> Instant costs one ULP in the spell clock,
            // so the replay agrees to floating-point tolerance rather than
            // bit-for-bit. A missing RNG/phase diverges by ~0.6 rad, which
            // this tolerance still catches by nine orders of magnitude.
            for axis in 0..4 {
                assert!(
                    (a.head[axis] - b.head[axis]).abs() < 1e-9,
                    "glance diverged at tick {tick} axis {axis}"
                );
            }
            assert!((a.takeover - b.takeover).abs() < 1e-9);
            assert!((a.mouth - b.mouth).abs() < 1e-9);
        }
    }
}

/// The controller's private state, reachable through the re-exported type.
impl Controller {
    /// The policy's recurrent memory, for a checkpoint.
    pub fn policy_state(&self) -> Vec<u8> {
        self.policy_ref().state_bytes()
    }

    /// Restore the driving network and its memory.
    pub fn restore_policy_state(&mut self, net: Option<duck_control::policy::Net>, bytes: &[u8]) -> bool {
        self.policy_mut().restore_state(net, bytes)
    }

    /// The network driving this tick.
    pub fn active_net(&self) -> Option<duck_control::policy::Net> {
        self.policy_ref().active_net()
    }
}
