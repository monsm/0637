//! A [`RobotIo`] with no body, for compile-time and unit tests.
//!
//! The real mobile backend will be MuJoCo; this one proves the official control
//! core builds without any hardware and gives the loop tests a body that stands
//! still.

use duck_control::io::{JointTargets, Result, RobotIo, Sensors, SlowSensors};
use duck_control::model::NUM_JOINTS;

#[derive(Debug)]
pub struct NullIo {
    pub sensors: Sensors,
    pub volts: f64,
    pub kp: u16,
    pub torque_on: bool,
}

impl Default for NullIo {
    fn default() -> Self {
        Self {
            sensors: Sensors::default(),
            volts: 7.4,
            kp: 200,
            torque_on: false,
        }
    }
}

impl NullIo {
    pub fn new() -> Self {
        Self::default()
    }
}

impl RobotIo for NullIo {
    fn read(&mut self) -> Result<Sensors> {
        Ok(self.sensors)
    }

    fn write(&mut self, _targets: &JointTargets) -> Result<()> {
        Ok(())
    }

    fn measures_velocity(&self) -> bool {
        false
    }

    fn measures_load(&self) -> bool {
        false
    }

    fn set_gain(&mut self, kp: u16) -> Result<()> {
        self.kp = kp;
        Ok(())
    }

    fn set_torque(&mut self, on: bool) -> Result<()> {
        self.torque_on = on;
        Ok(())
    }

    fn reboot(&mut self, _id: u8) -> Result<()> {
        Ok(())
    }

    fn slow_sensors(&mut self) -> Result<SlowSensors> {
        Ok(SlowSensors {
            volts: self.volts,
            temps_c: [25.0; NUM_JOINTS],
        })
    }
}
