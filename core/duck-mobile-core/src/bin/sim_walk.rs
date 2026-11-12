//! Minimal desktop loop: official control core + official skill scheduler + MuJoCo body.
//!
//! The body is the official `duck-body` TCP server (microduck_rl), or any server
//! speaking protocol 1. This binary is the mobile core's desktop harness:
//!
//!   sim_walk [addr] [seconds] [vx] [walk.onnx]
//!
//! Set ORT_DYLIB_PATH to an ONNX Runtime >= 1.23 dylib/dll before running.

use std::time::{Duration, Instant};

use duck_control::obs::Command;
use duck_control::policy::{Policy, PolicyPaths};
use duck_control::safety::{Safety, SafetyConfig};
use duck_control::sim::RemoteIo;
use duck_mobile_core::control::{Controller, SkillTuning, Tuning};

fn main() {
    let mut args = std::env::args().skip(1);
    let addr = args
        .next()
        .unwrap_or_else(|| "127.0.0.1:7801".to_owned());
    let seconds: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let vx: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(0.4);
    let walk = args.next().unwrap_or_else(|| {
        duck_mobile_core::fixtures::policies_dir()
            .expect("the official policy bundle")
            .join("velstand.onnx")
            .display()
            .to_string()
    });

    let paths = PolicyPaths {
        walk: walk.into(),
        ..Default::default()
    };
    let policy = Policy::load(&paths, 0.05).expect("load walking policy");
    let mut controller = Controller::new(policy, Tuning::default(), SkillTuning::default());
    let mut safety = Safety::new(RemoteIo::at(addr), SafetyConfig::default());

    safety.set_torque(true).expect("torque on");

    let mut command = Command::default();
    command.twist = [vx, 0.0, 0.0];

    println!(
        "loop start: vx={vx:.2} m/s, {seconds:.1}s, standing_threshold=0.05, action_scale={:.2}",
        Tuning::default().action_scale
    );

    let start = Instant::now();
    let mut next = Instant::now() + Duration::from_millis(20);
    let mut last_report = Instant::now();
    let mut ticks: u64 = 0;
    let mut last_label = String::from("walk");

    while start.elapsed().as_secs_f64() < seconds {
        let sensors = match safety.read() {
            Ok(sensors) => sensors,
            Err(error) => {
                eprintln!("read: {error}");
                std::thread::sleep(Duration::from_millis(20));
                next = Instant::now() + Duration::from_millis(20);
                continue;
            }
        };
        let hold = sensors.positions;

        match controller.step(&sensors, &command, false, 0.02, 1.0) {
            Ok(step) => {
                last_label = step.label.to_string();
                if let Err(error) = safety.apply(step.targets, hold, step.gain) {
                    eprintln!("write: {error}");
                }
            }
            Err(error) => {
                eprintln!("inference: {error}");
                let _ = safety.apply(hold, hold, 200);
            }
        }
        ticks += 1;

        if last_report.elapsed() >= Duration::from_secs(1) {
            println!(
                "t={:.1}s ticks={ticks} label={last_label} hip_pitch=({:+.3},{:+.3}) ankle=({:+.3},{:+.3})",
                start.elapsed().as_secs_f64(),
                sensors.positions[2],
                sensors.positions[11],
                sensors.positions[4],
                sensors.positions[13],
            );
            last_report = Instant::now();
        }

        // 50 Hz wall clock, the official loop's rate. The body server steps in its
        // own thread; read/write are snapshots, exactly like the servo bus.
        next += Duration::from_millis(20);
        let slack = next.saturating_duration_since(Instant::now());
        if slack > Duration::from_millis(2) {
            std::thread::sleep(slack);
        }
    }

    // Leave torque on for the validation query that follows; the body server keeps
    // holding the last targets. A production loop cuts torque on shutdown.
    println!("loop done: {ticks} ticks in {:.2}s", start.elapsed().as_secs_f64());
}
