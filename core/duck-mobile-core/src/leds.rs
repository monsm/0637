//! The face LEDs, as state rather than hardware.
//!
//! `robotd/src/leds.rs` owns a status LED, a four-light battery gauge and a
//! three-channel flashlight over I²C. A phone has neither bus nor LEDs, but
//! the *semantics* are part of the duck: `robot.health` reports the same
//! verdict, the gauge has the same five levels with the same 3% hysteresis,
//! and `robot.flashlight` cycles the same seven colours. Compiling the
//! upstream file would drag in `RobotState` and the sysfs `Led` type, so this
//! is the pure logic, with the thresholds and colour order copied verbatim.
//!
//! `/state.leds` publishes it; the shells draw a virtual face. Nothing here
//! touches the control loop.

/// The battery gauge, in the order it fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Gauge {
    Unknown,
    Critical,
    Low,
    One,
    Two,
    Three,
}

/// The percentages a gauge level starts above, from `Low` up.
const GAUGE_ABOVE: [f64; 4] = [5.0, 15.0, 40.0, 70.0];
const GAUGE_HYSTERESIS: f64 = 3.0;

impl Gauge {
    fn of(percent: f64) -> Gauge {
        match GAUGE_ABOVE.iter().filter(|&&above| percent > above).count() {
            0 => Gauge::Critical,
            1 => Gauge::Low,
            2 => Gauge::One,
            3 => Gauge::Two,
            _ => Gauge::Three,
        }
    }

    /// The gauge for `percent`, given what it showed before. The reading is
    /// already averaged; the band either side stops a walk dipping the pack
    /// across a boundary from flipping the top LED every few seconds.
    pub fn next(self, percent: Option<f64>) -> Gauge {
        let Some(percent) = percent else {
            return Gauge::Unknown;
        };
        let low = Gauge::of(percent - GAUGE_HYSTERESIS);
        let high = Gauge::of(percent + GAUGE_HYSTERESIS);
        if (low..=high).contains(&self) { self } else { Gauge::of(percent) }
    }
}

impl Default for Gauge {
    fn default() -> Self {
        Gauge::Unknown
    }
}

/// What the status LED says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Status {
    Starting,
    Healthy,
    Degraded,
    Fault,
}

impl Status {
    /// `starting` is "no tick yet and no failed attempt", which a health gate
    /// reports as unhealthy but a person sees as a duck waking up.
    pub fn of(healthy: bool, degraded: bool, starting: bool) -> Status {
        if healthy {
            Status::Healthy
        } else if starting {
            Status::Starting
        } else if degraded {
            Status::Degraded
        } else {
            Status::Fault
        }
    }
}

impl Default for Status {
    fn default() -> Self {
        Status::Starting
    }
}

/// The seven flashlight colours, in `robotd`'s cycling order.
pub const COLORS: [&str; 7] = ["white", "red", "green", "blue", "yellow", "cyan", "magenta"];

/// Zero is off; otherwise the colour's position in [`COLORS`] plus one.
pub type Flashlight = u8;

/// `robot.flashlight`: a press toggles, an explicit `on` sets it. Same shape
/// as `leds::flashlight_after`, without the IPC parameter struct.
pub fn flashlight_after(current: Flashlight, toggle: bool, on: bool, color: &str) -> Flashlight {
    let lit = if toggle { current == 0 } else { on };
    if lit {
        COLORS
            .iter()
            .position(|&c| c == color)
            .map_or(1, |i| i as u8 + 1)
    } else {
        0
    }
}

/// The whole virtual face, published once a second and on a flashlight press.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct Face {
    pub status: Status,
    pub gauge: Gauge,
    pub battery_percent: Option<f64>,
    pub flashlight: Flashlight,
}

impl Face {
    /// The colour name, or `None` when the flashlight is off.
    pub fn color(&self) -> Option<&'static str> {
        if self.flashlight == 0 {
            return None;
        }
        COLORS.get(usize::from(self.flashlight) - 1).copied()
    }

    /// How many of the four battery LEDs are lit, for a compact UI gauge.
    pub fn bars(&self) -> u8 {
        match self.gauge {
            Gauge::Unknown | Gauge::Critical => 0,
            Gauge::Low => 1,
            Gauge::One => 1,
            Gauge::Two => 2,
            Gauge::Three => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gauge_uses_the_upstream_thresholds_and_hysteresis() {
        assert_eq!(Gauge::Unknown.next(None), Gauge::Unknown);
        assert_eq!(Gauge::Unknown.next(Some(50.0)), Gauge::Two);
        assert_eq!(Gauge::Two.next(Some(41.0)), Gauge::Two, "3% band holds");
        // 38 still holds: 38+3 crosses 40, so Two is inside the band.
        assert_eq!(Gauge::Two.next(Some(38.0)), Gauge::Two);
        assert_eq!(Gauge::Two.next(Some(36.0)), Gauge::One);
        assert_eq!(Gauge::Unknown.next(Some(2.0)), Gauge::Critical);
    }

    #[test]
    fn the_flashlight_cycles_the_official_colours() {
        assert_eq!(flashlight_after(0, true, false, "white"), 1);
        assert_eq!(flashlight_after(1, true, false, "white"), 0);
        assert_eq!(flashlight_after(0, false, true, "cyan"), 6);
        assert_eq!(Face { flashlight: 6, ..Face::default() }.color(), Some("cyan"));
        assert_eq!(Face::default().color(), None);
    }
}
