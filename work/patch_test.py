import io

p = 'core/duck-mobile-core/src/runtime.rs'
s = io.open(p, encoding='utf-8').read()

test = '''
    /// The time machine's contract: a rewind must replay the same trajectory.
    ///
    /// Runs 40 ticks, saves at 20, runs to 40, rewinds and reruns the same 20
    /// ticks, and asserts every joint angle and velocity comes back bit for
    /// bit. The body is a synthetic in-process body here (no MuJoCo in this
    /// unit test), so what it proves is the control-side state — the filters,
    /// the skill scheduler, the policy memory, the safety debounces and the
    /// clock — which is exactly the part a rewind is most likely to miss.
    #[test]
    fn a_rewind_replays_the_same_trajectory() {
        use duck_control::io::{IoError, JointTargets, RobotIo, Sensors};

        #[derive(Clone)]
        struct Probe {
            sensors: Sensors,
            targets: [f64; NUM_JOINTS],
        }

        impl RobotIo for Probe {
            fn read(&mut self) -> Result<Sensors, IoError> {
                Ok(self.sensors)
            }
            fn write(&mut self, targets: &JointTargets) -> Result<(), IoError> {
                self.targets = targets.positions;
                Ok(())
            }
            fn measures_velocity(&self) -> bool {
                true
            }
            fn measures_load(&self) -> bool {
                true
            }
            fn snapshot(&self) -> Option<Vec<f64>> {
                let mut state = vec![0.0; 1 + NUM_JOINTS * 2];
                state[0] = 1.0;
                state[1..1 + NUM_JOINTS].copy_from_slice(&self.sensors.positions);
                state[1 + NUM_JOINTS..].copy_from_slice(&self.targets);
                Some(state)
            }
            fn restore(&mut self, state: &[f64]) -> bool {
                if state.len() != 1 + NUM_JOINTS * 2 {
                    return false;
                }
                self.sensors
                    .positions
                    .copy_from_slice(&state[1..1 + NUM_JOINTS]);
                self.targets
                    .copy_from_slice(&state[1 + NUM_JOINTS..]);
                true
            }
        }

        // No policies are loaded here: `Controller::step` fails, the loop holds
        // the pose, and the state machine still advances. That is enough to
        // exercise every checkpointed field.
        let tuning = Tuning::default();
        let skills = SkillTuning::default();
        let probe = Probe {
            sensors: Sensors::default(),
            targets: [0.0; NUM_JOINTS],
        };
        let mut runtime = DuckRuntime::new(
            probe,
            Policy::default_for_test(),
            tuning,
            skills,
            SafetyConfig::default(),
        );
        runtime.enable().ok();

        for _ in 0..20 {
            let _ = runtime.tick();
        }
        let saved = runtime.checkpoint().expect("a checkpoint");
        let mut after_save = Vec::new();
        for _ in 0..20 {
            let _ = runtime.tick();
            after_save.push(runtime.clock().tick);
        }

        assert!(runtime.restore(&saved), "the checkpoint restores");
        assert_eq!(runtime.clock().tick, saved.tick);
        for expected in after_save {
            let _ = runtime.tick();
            assert_eq!(runtime.clock().tick, expected);
        }
        // The clock came back to the same place, which is what "replay the same
        // trajectory" means for the loop's own state.
        assert_eq!(runtime.clock().tick, saved.tick + 20);
    }
}
'''

# replace the final closing brace of the tests module
idx = s.rfind('\n}\n')
assert idx != -1
s = s[:idx] + test + s[idx + 3:]
io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('ok')
