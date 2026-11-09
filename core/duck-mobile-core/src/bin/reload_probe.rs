//! TEMPORARY probe (delete after): reload during limp-fall, measure displacement.

use std::path::Path;

use duck_control::policy::Policy;
use duck_control::safety::SafetyConfig;
use duck_mobile_core::manifest;
use duck_mobile_core::mujoco::MujocoIo;
use duck_mobile_core::runtime::DuckRuntime;

const POLICIES: &str = r"D:\AI\Duck\policies";
const SCENE: &str =
    r"D:\AI\Duck\upstream\microduck_rl\src\mjlab_microduck\robot\microduck\scene_backlash.xml";

fn load() -> (Policy, duck_mobile_core::control::Tuning, duck_mobile_core::control::SkillTuning) {
    let bundle = manifest::load(Path::new(POLICIES)).expect("manifest");
    let policy = Policy::load(&bundle.paths, 0.05).expect("policy");
    (policy, bundle.tuning(), bundle.skill_tuning)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "reload".to_owned());

    let (policy, tuning, skills) = load();
    let body = MujocoIo::load(SCENE).expect("scene");
    let mut rt = DuckRuntime::new(body, policy, tuning, skills, SafetyConfig::default());
    rt.enable().expect("enable");

    let mut reloaded = false;
    let mut fall_tick = None;
    let mut x_at_fall = 0.0;
    let mut max_down_ticks = 0;
    for tick in 0..260u64 {
        rt.set_twist([0.4, 0.0, 0.0]);
        if tick == 100 {
            rt.push([0.0, 1.5, 0.0]).expect("push");
        }
        let report = rt.tick().expect("tick");
        let trunk = rt.io().trunk();

        if report.label == "limp_fall" && fall_tick.is_none() {
            fall_tick = Some(tick);
            x_at_fall = trunk[0];
            println!("tick {tick}: FALL START trunk=({:.4},{:.4},{:.4})", trunk[0], trunk[1], trunk[2]);
        }

        if mode == "reload" && !reloaded && fall_tick.is_some_and(|f| tick == f + 1) {
            let (p2, t2, s2) = load();
            rt.replace_policy(p2, t2, s2);
            reloaded = true;
            println!("tick {tick}: RELOAD during label={} gain={}", report.label, report.gain);
        }

        if fall_tick.is_some() {
            if report.fallen {
                max_down_ticks += 1;
            }
            if tick % 10 == 0 || tick == fall_tick.unwrap() {
                println!(
                    "tick {tick}: label={} gain={} fallen={} trunk=({:.4},{:.4},{:.4}) dx={:.4}",
                    report.label,
                    report.gain,
                    report.fallen,
                    trunk[0],
                    trunk[1],
                    trunk[2],
                    trunk[0] - x_at_fall
                );
            }
        }
        if fall_tick.is_some() && tick > fall_tick.unwrap() + 140 {
            break;
        }
    }
}
