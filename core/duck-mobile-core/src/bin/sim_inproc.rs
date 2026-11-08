//! Full runtime with an in-process MuJoCo body — the path the iOS app uses.
//!
//!   sim_inproc [seconds]
//!
//! Requires `--features mujoco`, MUJOCO_INCLUDE_DIR / MUJOCO_LIB_DIR at build
//! time, and `mujoco.dll` on PATH (desktop) or a static MuJoCo lib (iOS).

use std::path::Path;
use std::time::{Duration, Instant};

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
    let seconds: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(26.0);

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

    runtime.enable().expect("enable: torque on and home ramp");
    println!("in-process runtime up; scene={SCENE}");

    let start = Instant::now();
    let mut next = Instant::now() + Duration::from_millis(20);
    let mut last_report = Instant::now();
    let mut ticks: u64 = 0;

    let mut walked = false;
    let mut stopped = false;
    let mut sat = false;
    let mut stood = false;
    let mut rolled = false;
    let mut kicked = false;
    let mut picked = false;
    let mut pushed = false;

    while start.elapsed().as_secs_f64() < seconds {
        let t = start.elapsed().as_secs_f64();

        if t >= 2.0 && t < 10.0 {
            if !walked {
                println!("t={t:.1}s command: walk 0.4 m/s");
                walked = true;
            }
            runtime.set_twist([0.4, 0.0, 0.0]);
        }
        if t >= 6.0 && !pushed {
            let _ = runtime.push([0.6, 0.0, 0.0]);
            pushed = true;
            println!("t={t:.1}s sensor: push 0.6 m/s on the trunk");
        }
        if t >= 10.0 && !stopped {
            runtime.set_twist([0.0; 3]);
            stopped = true;
            println!("t={t:.1}s command: stand");
        }
        if t >= 12.0 && !sat {
            let _ = runtime.sit_toggle();
            sat = true;
            println!("t={t:.1}s command: sit");
        }
        if t >= 16.0 && !stood {
            let _ = runtime.sit_toggle();
            stood = true;
            println!("t={t:.1}s command: stand");
        }
        if t >= 18.5 && !rolled {
            let _ = runtime.trigger_skill("roulade");
            rolled = true;
            println!("t={t:.1}s command: roulade");
        }
        if t >= 21.0 && !kicked {
            let _ = runtime.trigger_skill("kick_left");
            kicked = true;
            println!("t={t:.1}s command: kick left");
        }
        if t >= 23.0 && !picked {
            let _ = runtime.start_ground_pick();
            picked = true;
            println!("t={t:.1}s command: ground pick");
        }

        match runtime.tick() {
            Ok(report) => {
                ticks += 1;
                if last_report.elapsed() >= Duration::from_secs(1) {
                    let body = runtime.io();
                    let trunk = body.trunk();
                    let gravity = body.gravity();
                    println!(
                        "t={t:.1}s label={} gain={} bringup={} fallen={} trunk=({:+.3},{:+.3},{:.3}) gravity=({:+.2},{:+.2},{:+.2})",
                        report.label,
                        report.gain,
                        report.bringup.label(),
                        report.fallen,
                        trunk[0],
                        trunk[1],
                        trunk[2],
                        gravity[0],
                        gravity[1],
                        gravity[2],
                    );
                    last_report = Instant::now();
                }
            }
            Err(error) => {
                eprintln!("tick: {error}");
                std::thread::sleep(Duration::from_millis(20));
                next = Instant::now() + Duration::from_millis(20);
                continue;
            }
        }

        next += Duration::from_millis(20);
        let slack = next.saturating_duration_since(Instant::now());
        if slack > Duration::from_millis(2) {
            std::thread::sleep(slack);
        }
    }

    let _ = runtime.disable();
    println!(
        "in-process runtime done: {ticks} ticks in {:.2}s",
        start.elapsed().as_secs_f64()
    );
}
