//! Mobile control core for the MicroDuck digital twin.
//!
//! The body simulation (MuJoCo) plugs in behind [`duck_control::io::RobotIo`],
//! exactly as the official `robotd` twin does. Everything above that seam is the
//! official control core, so observation layout, policy selection, safety and the
//! skill scheduler cannot drift from the robot.

pub use duck_control;

/// Official `robotd` skill scheduler, compiled verbatim into this crate.
///
/// `robotd/src/control.rs` is pure computation over `duck-control` types: no
/// tokio, no sockets, no filesystem. Reusing the file directly (instead of
/// copying it) is what keeps the skill priority, timings and low-pass behaviour
/// identical to the robot.
pub mod bam;
pub mod control;
pub mod intent;

pub mod io;
pub mod manifest;
pub mod mcp;
pub mod provenance;
#[cfg(feature = "mujoco")]
pub mod ffi;
#[cfg(feature = "mujoco")]
pub mod mujoco;
pub mod runtime;
pub mod sensors;

#[cfg(all(target_os = "android", feature = "mujoco"))]
pub mod jni;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_skill_scheduler_is_reused() {
        let tuning = control::Tuning::default();
        assert_eq!(tuning.gain, 200);
        assert_eq!(tuning.action_scale, 0.9);
        assert_eq!(tuning.standing_action_scale, 1.0);
        assert_eq!(tuning.head_lowpass, Some(0.5));
        assert_eq!(tuning.legs_lowpass, Some(0.7));

        let skills = control::SkillTuning::default();
        assert_eq!(skills.ground_pick_period, 4.0);
        assert_eq!(skills.sitstand_rise_s, 1.0);
    }

    #[test]
    fn null_io_implements_the_official_seam() {
        use duck_control::io::RobotIo;

        let mut io = io::NullIo::new();
        assert!(io.read().is_ok());
        assert!(io.slow_sensors().is_ok());
        assert!(!io.measures_velocity());
    }
}
