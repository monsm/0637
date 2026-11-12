//! Full mobile runtime on the desktop harness: bring-up, walking, sit/stand,
//! roulade, kick, ground pick — all through the official scheduler.
//!
//!   sim_runtime [addr] [seconds]
//!
//! Set ORT_DYLIB_PATH first. The body is the BAM server (`work/body_bam.py`) or
//! any protocol-1 server.

use std::time::{Duration, Instant};

use duck_control::policy::Policy;
use duck_control::safety::SafetyConfig;
use duck_control::sim::RemoteIo;
use duck_mobile_core::manifest;
use duck_mobile_core::runtime::DuckRuntime;

fn main() {
    let mut args = std::env::args().skip(1);
    let addr = args.next().unwrap_or_else(|| "127.0.0.1:7801".to_owned());
    let seconds: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(26.0);

    let policies = duck_mobile_core::fixtures::policies_dir().expect("the official policy bundle");
    let bundle = manifest::load(&policies).expect("policy manifest");
    let policy = Policy::load(&bundle.paths, 0.05).expect("load official policy set");
    let tuning = bundle.tuning();
    let mut runtime = DuckRuntime::new(
        RemoteIo::at(addr),
        policy,
        tuning,
        bundle.skill_tuning,
        SafetyConfig::default(),
    );

    runtime.enable().expect("enable: torque on and home ramp");
    println!(
        "runtime up: skills={:?}, action_scale={:.2}, gain={}",
        runtime.skill_names(),
        tuning.action_scale,
        tuning.gain
    );

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

    while start.elapsed().as_secs_f64() < seconds {
        let t = start.elapsed().as_secs_f64();

        // 2 s homing, then walk. The twist is re-sent every tick because the
        // runtime has robotd's 500 ms deadman: a command that stops arriving
        // means stop, which is the robot's behaviour, not a harness detail.
        if t >= 2.0 && t < 10.0 {
            if !walked {
                println!("t={t:.1}s command: walk 0.4 m/s");
                walked = true;
            }
            runtime.set_twist([0.4, 0.0, 0.0]);
        }
        if t >= 10.0 && !stopped {
            runtime.set_twist([0.0; 3]);
            stopped = true;
            println!("t={t:.1}s command: stand");
        }
        if t >= 12.0 && !sat {
            match runtime.sit_toggle() {
                Ok(action) => println!("t={t:.1}s command: sit ({action})"),
                Err(error) => println!("t={t:.1}s sit refused: {error}"),
            }
            sat = true;
        }
        if t >= 16.0 && !stood {
            match runtime.sit_toggle() {
                Ok(action) => println!("t={t:.1}s command: stand ({action})"),
                Err(error) => println!("t={t:.1}s stand refused: {error}"),
            }
            stood = true;
        }
        if t >= 18.5 && !rolled {
            match runtime.trigger_skill("roulade") {
                Ok(started) => println!("t={t:.1}s command: roulade (started={started})"),
                Err(error) => println!("t={t:.1}s roulade refused: {error}"),
            }
            rolled = true;
        }
        if t >= 21.0 && !kicked {
            match runtime.trigger_skill("kick_left") {
                Ok(started) => println!("t={t:.1}s command: kick left (started={started})"),
                Err(error) => println!("t={t:.1}s kick refused: {error}"),
            }
            kicked = true;
        }
        if t >= 23.0 && !picked {
            match runtime.start_ground_pick() {
                Ok(()) => println!("t={t:.1}s command: ground pick"),
                Err(error) => println!("t={t:.1}s ground pick refused: {error}"),
            }
            picked = true;
        }

        match runtime.tick() {
            Ok(report) => {
                ticks += 1;
                if last_report.elapsed() >= Duration::from_secs(1) {
                    println!(
                        "t={t:.1}s label={} gain={} bringup={} fallen={}",
                        report.label,
                        report.gain,
                        report.bringup.label(),
                        report.fallen
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
        "runtime done: {ticks} ticks in {:.2}s",
        start.elapsed().as_secs_f64()
    );
}
