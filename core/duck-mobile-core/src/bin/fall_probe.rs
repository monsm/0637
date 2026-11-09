//! Temporary probe: topple the duck while a twist is held and watch the
//! limp-fall hand-back. Prints label/gain/twist_ema per tick around the sequence.
//!
//!   fall_probe <hold|release|release_at_fall> [push_vy]

use std::path::Path;

use duck_control::policy::Policy;
use duck_control::safety::SafetyConfig;
use duck_mobile_core::manifest;
use duck_mobile_core::mujoco::MujocoIo;
use duck_mobile_core::runtime::DuckRuntime;

const POLICIES: &str = r"D:\AI\Duck\policies";
const SCENE: &str =
    r"D:\AI\Duck\upstream\microduck_rl\src\mjlab_microduck\robot\microduck\scene_backlash.xml";

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "hold".to_owned());
    let push_vy: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1.5);

    let bundle = manifest::load(Path::new(POLICIES)).expect("policy manifest");
    let policy = Policy::load(&bundle.paths, 0.05).expect("load official policy set");
    let tuning = bundle.tuning();
    let body = MujocoIo::load(SCENE).expect("load MuJoCo scene");
    let mut runtime = DuckRuntime::new(
        body,
        policy,
        tuning,
        bundle.skill_tuning,
        SafetyConfig::default(),
    );
    runtime.enable().expect("enable");

    let mut in_fall = false;
    let mut hand_back = None;
    let mut fall_start = None;

    for tick in 0..400u64 {
        let t = tick as f64 * 0.02;
        let release_now = match mode.as_str() {
            "release" => t >= 2.0,
            "release_at_fall" => fall_start.is_some(),
            _ => false,
        };
        runtime.set_twist(if release_now { [0.0; 3] } else { [0.4, 0.0, 0.0] });
        if tick == 100 {
            runtime.push([0.0, push_vy, 0.0]).expect("push");
        }
        let report = runtime.tick().expect("tick");
        let cp = runtime.checkpoint().expect("checkpoint");
        let label = report.label.as_str();
        if label == "limp_fall" && !in_fall {
            in_fall = true;
            fall_start = Some(tick);
            println!("t={t:.2} FALL START ema={:?}", cp.twist_ema);
        }
        if in_fall && label == "pose" {
            println!("t={t:.2} LANDED ema={:?}", cp.twist_ema);
        }
        if in_fall && label != "limp_fall" && label != "pose" {
            if hand_back.is_none() {
                hand_back = Some(tick);
                println!("t={t:.2} HAND BACK label={label} ema={:?}", cp.twist_ema);
            }
        }
        if hand_back.is_some_and(|h| tick > h + 10) {
            println!("t={t:.2} after10 label={label} ema={:?}", cp.twist_ema);
            break;
        }
    }
}
