//! BAM M6 actuator model for the Dynamixel XL330, ported from the official
//! Python implementation (`bam` 1.0.1, `Model`/`VoltageControlledActuator`).
//!
//! The numbers are the identified XL330 M6 parameters; the equations are the
//! voltage control law, the DC-motor torque with back-EMF, and the
//! load-dependent Stribeck/directional/quadratic friction budget.

use serde_json::Value;

pub struct BamModel {
    pub kt: f64,
    pub r: f64,
    pub armature: f64,
    friction_base: f64,
    friction_stribeck: f64,
    load_motor: f64,
    load_external: f64,
    load_motor_stribeck: f64,
    load_external_stribeck: f64,
    load_motor_quad: f64,
    load_external_quad: f64,
    dtheta_stribeck: f64,
    alpha: f64,
    friction_viscous: f64,

    /// Firmware P gain (`--kp-fw`); 200 in training and on the robot.
    pub kp: f64,
    /// Supply voltage. Training DR is 6.5–8.2 V; 7.4 is the nominal 2S pack.
    pub vin: f64,
    pub vin_drop_gain: f64,
    pub vin_min: f64,
    pub max_pwm: f64,
    error_gain: f64,
}

impl BamModel {
    pub fn xl330_m6() -> Self {
        let value: Value = serde_json::from_str(include_str!("../params/xl330_m6.json"))
            .expect("bundled BAM parameters");
        let get = |name: &str| value[name].as_f64().unwrap_or_else(|| panic!("BAM parameter {name}"));
        Self {
            kt: get("kt"),
            r: get("R"),
            armature: get("armature"),
            friction_base: get("friction_base"),
            friction_stribeck: get("friction_stribeck"),
            load_motor: get("load_friction_motor"),
            load_external: get("load_friction_external"),
            load_motor_stribeck: get("load_friction_motor_stribeck"),
            load_external_stribeck: get("load_friction_external_stribeck"),
            load_motor_quad: get("load_friction_motor_quad"),
            load_external_quad: get("load_friction_external_quad"),
            dtheta_stribeck: get("dtheta_stribeck"),
            alpha: get("alpha"),
            friction_viscous: get("friction_viscous"),
            kp: 200.0,
            vin: 7.4,
            vin_drop_gain: 0.1,
            vin_min: 6.0,
            max_pwm: 1.0,
            // XL330: 4096 counts/rev, firmware kp divisor 256, PWM limit 885.
            error_gain: (4096.0 / (2.0 * std::f64::consts::PI)) / (256.0 * 885.0),
        }
    }

    /// The voltage the firmware would apply to the motor.
    pub fn voltage(&self, q_target: f64, q: f64, _dq: f64, vin_effective: f64) -> f64 {
        let duty = (q_target - q) * self.kp * self.error_gain;
        let duty = duty.clamp(-self.max_pwm, self.max_pwm);
        vin_effective * duty
    }

    /// DC motor torque with back-EMF.
    pub fn torque(&self, volts: f64, dq: f64) -> f64 {
        self.kt * volts / self.r - self.kt * self.kt * dq / self.r
    }

    /// The voltage after the previous tick's load-dependent sag.
    pub fn vin_effective(&self, previous_torques: &[f64]) -> f64 {
        let load: f64 = previous_torques.iter().map(|torque| torque.abs()).sum();
        (self.vin - self.vin_drop_gain * load).max(self.vin_min)
    }

    /// `(frictionloss, damping)` for MuJoCo's friction budget.
    pub fn frictions(
        &self,
        motor_torque: f64,
        external_torque: f64,
        dq: f64,
    ) -> (f64, f64) {
        let gearbox = (external_torque * self.load_external
            - motor_torque * self.load_motor)
            .abs();
        let gearbox_stribeck = (external_torque * self.load_external_stribeck
            - motor_torque * self.load_motor_stribeck)
            .abs();
        let stribeck = (-((dq.abs() / self.dtheta_stribeck).powf(self.alpha))).exp();

        let mut frictionloss = self.friction_base
            + gearbox
            + stribeck * self.friction_stribeck
            + gearbox_stribeck * stribeck;

        // Quadratic load coupling, only when the motor and the external load push
        // against each other.
        let sign_external = sign(external_torque);
        let sign_motor = sign(motor_torque);
        let enable = sign_external != sign_motor;
        let direction_motor = external_torque.abs() < motor_torque.abs();
        let direction_external = external_torque.abs() > motor_torque.abs();
        let gearbox2_motor = self.load_external_quad * external_torque.abs().powi(2);
        let gearbox2_external = self.load_motor_quad * motor_torque.abs().powi(2);
        if enable {
            frictionloss += stribeck
                * (if direction_motor { gearbox2_motor } else { 0.0 }
                    + if direction_external { gearbox2_external } else { 0.0 });
        }

        (frictionloss, self.friction_viscous)
    }

    pub fn force_limit(&self) -> f64 {
        self.vin * self.kt / self.r
    }
}

fn sign(value: f64) -> i8 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voltage_control_law_clamps_to_the_battery() {
        let model = BamModel::xl330_m6();
        // 2 rad of error at kp=200 is past the PWM limit; 0.1 rad is not.
        let volts = model.voltage(2.0, 0.0, 0.0, 7.4);
        assert!((volts - 7.4).abs() < 1e-12, "saturated at +vin, got {volts}");
        let volts = model.voltage(-2.0, 0.0, 0.0, 7.4);
        assert!((volts + 7.4).abs() < 1e-12, "saturated at -vin, got {volts}");

        let expected = 7.4 * (0.1 * 200.0 * model.error_gain);
        let volts = model.voltage(0.1, 0.0, 0.0, 7.4);
        assert!((volts - expected).abs() < 1e-12, "unsaturated law: {volts} vs {expected}");
    }

    #[test]
    fn torque_matches_the_dc_motor_equation() {
        let model = BamModel::xl330_m6();
        let volts = 2.0;
        let dq = 1.0;
        let expected = model.kt * volts / model.r - model.kt * model.kt * dq / model.r;
        assert!((model.torque(volts, dq) - expected).abs() < 1e-12);
    }

    #[test]
    fn friction_is_a_budget_and_positive() {
        let model = BamModel::xl330_m6();
        let (frictionloss, damping) = model.frictions(0.1, -0.2, 0.5);
        assert!(frictionloss >= model.friction_base);
        assert_eq!(damping, model.friction_viscous);
    }

    /// Values from the official Python `bam` implementation for the same states.
    #[test]
    fn matches_the_python_reference() {
        let model = BamModel::xl330_m6();
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;

        let volts = model.voltage(0.1, 0.0, 0.5, 7.4);
        assert!(close(volts, 0.425851870330), "{volts}");
        assert!(close(model.torque(volts, 0.5), 0.031615861849));
        let (fl, damping) = model.frictions(0.1, -0.2, 0.5);
        assert!(close(fl, 0.052384491582), "{fl}");
        assert!(close(damping, 0.005359668275), "{damping}");

        let volts = model.voltage(2.0, 0.0, 0.0, 7.4);
        assert!(close(volts, 7.4));
        assert!(close(model.torque(volts, 0.0), 0.963401594660));
        let (fl, _) = model.frictions(0.3, 0.1, 0.0);
        assert!(close(fl, 0.097557218110), "{fl}");

        let volts = model.voltage(0.0, 0.2, -1.0, 7.4);
        assert!(close(volts, -0.851703740659), "{volts}");
        assert!(close(model.torque(volts, -1.0), -0.063231723697));
        let (fl, _) = model.frictions(-0.15, 0.05, -1.0);
        assert!(close(fl, 0.053517830713), "{fl}");
    }
}
