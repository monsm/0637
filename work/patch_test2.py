import io
import re

p = 'core/duck-mobile-core/src/runtime.rs'
s = io.open(p, encoding='utf-8').read()

# --- rewrite the three lease tests against the tick clock ---
start = s.index('    #[test]\n    fn the_api_lease_outlives_the_joystick_twist() {')
end = s.index('    /// A policy that fails every step')
new_tests = '''    #[test]
    fn the_api_lease_outlives_the_joystick_twist() {
        let mut lease = ApiLease::default();
        assert_eq!(lease.active(TickTime(0)), None);

        lease.take([0.4, 0.0, 0.0], TickTime(0));
        assert_eq!(lease.active(TickTime(0)), Some([0.4, 0.0, 0.0]));
        // The app pushes the joystick's twist every 20 ms; the lease must
        // survive that, otherwise an external agent could never drive.
        assert_eq!(lease.active(TickTime(10)), Some([0.4, 0.0, 0.0]));
    }

    #[test]
    fn the_lease_expires_and_gives_the_duck_back() {
        let mut lease = ApiLease::default();
        lease.take([0.4, 0.0, 0.0], TickTime(0));
        assert_eq!(lease.active(TickTime(25)), None);
        assert_eq!(lease.active(TickTime(26)), None);
    }

    #[test]
    fn a_new_twist_extends_the_lease() {
        let mut lease = ApiLease::default();
        lease.take([0.4, 0.0, 0.0], TickTime(0));
        assert_eq!(lease.active(TickTime(20)), Some([0.4, 0.0, 0.0]));
        lease.take([0.0, 0.0, 0.2], TickTime(20));
        assert_eq!(lease.active(TickTime(40)), Some([0.0, 0.0, 0.2]));
        assert_eq!(lease.active(TickTime(46)), None);
    }

'''
s = s[:start] + new_tests + s[end:]

# --- complete the Probe body ---
s = s.replace('''            fn measures_load(&self) -> bool {
                true
            }
            fn snapshot(&self) -> Option<Vec<f64>> {''', '''            fn measures_load(&self) -> bool {
                true
            }
            fn set_gain(&mut self, _: u16) -> Result<(), IoError> {
                Ok(())
            }
            fn set_torque(&mut self, _: bool) -> Result<(), IoError> {
                Ok(())
            }
            fn reboot(&mut self, _: u8) -> Result<(), IoError> {
                Ok(())
            }
            fn slow_sensors(&mut self) -> Result<duck_control::io::SlowSensors, IoError> {
                Ok(duck_control::io::SlowSensors::default())
            }
            fn snapshot(&self) -> Option<Vec<f64>> {''')

# --- fix the escaped path in the test policy ---
s = s.replace('r"D:\\\\AI\\\\Duck\\\\policies"', 'r"D:\\AI\\Duck\\policies"')
io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('ok')
