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
pub mod emotion;
pub mod intent;
pub mod leds;

pub mod io;
pub mod manifest;
pub mod mcp;
pub mod params;
pub mod pickup;
pub mod provenance;
#[cfg(feature = "mujoco")]
pub mod ffi;
#[cfg(feature = "mujoco")]
pub mod mujoco;
pub mod runtime;
pub mod sensors;
pub mod speech;
pub mod voice;

// The robot's voice synth, compiled verbatim. `sounds/` is a crate of pure
// Rust modules that address each other as `crate::rng`, `crate::synth`, …,
// so they have to sit at *this* crate's root to link unchanged. Reusing the
// files (rather than copying the recipes) is what makes the digital duck's
// quack the same quack.
#[path = "../../../upstream/microduck/sounds/src/rng.rs"]
pub mod rng;
#[path = "../../../upstream/microduck/sounds/src/personality.rs"]
pub mod personality;
#[path = "../../../upstream/microduck/sounds/src/synth.rs"]
pub mod synth;
#[path = "../../../upstream/microduck/sounds/src/voices.rs"]
pub mod voices;

/// robotd/src/main.rs's `SEAT_POSITION`: the sitstand policy's own SIT
/// keyframe, quoted verbatim. `posture.rs` — compiled verbatim too — tests
/// against `crate::SEAT_POSITION`, and in this crate `crate::` is here. Keep
/// in step with `SITTING_TARGET_OVERRIDES` upstream.
pub const SEAT_POSITION: [f64; duck_control::model::NUM_JOINTS] = [
    0.0,     // left_hip_yaw
    0.0,     // left_hip_roll
    -0.4079, // left_hip_pitch
    1.35,    // left_knee
    0.0,     // left_ankle
    duck_control::model::DEFAULT_POSITION[5],
    duck_control::model::DEFAULT_POSITION[6],
    duck_control::model::DEFAULT_POSITION[7],
    duck_control::model::DEFAULT_POSITION[8],
    duck_control::model::DEFAULT_POSITION[9],
    0.0,    // right_hip_yaw
    0.0,    // right_hip_roll
    0.4079, // right_hip_pitch
    -1.35,  // right_knee
    0.0,    // right_ankle
];

/// Every voice tag and its recipe — `sounds/src/lib.rs`'s table, which the
/// verbatim `voices.rs` addresses as `crate::TAGS` from its own tests.
pub type Recipe = fn(&personality::Personality, u32) -> Vec<f32>;

pub const TAGS: [(&str, Recipe); 7] = [
    ("alarm", voices::alarm),
    ("greet", voices::greet),
    ("inquire", voices::inquire),
    ("peck", voices::peck),
    ("chirp", voices::chirp),
    ("coo", voices::coo),
    ("wheee", voices::wheee),
];

/// How many variants each tag gets, from `sounds::variant_count`: more
/// variants directly means a more organic-feeling duck.
pub fn variant_count(tag: &str) -> u32 {
    match tag {
        "greet" | "chirp" => 12,
        "wheee" => 6,
        _ => 10,
    }
}

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
