//! What the phone senses, and how it reaches the duck.
//!
//! The duck's own 61-D observation has no phone sensor in it. Feeding a phone
//! camera or microphone straight into the policy would make it a different
//! robot, so the boundary here is the one FIDELITY.md draws:
//!
//! * **IMU** — the phone's accelerometer is an *external disturbance*. A shake
//!   is a shove, which is what a hand would do to the real duck too.
//! * **Camera** — the world the duck stands in. Frames are available to the
//!   shell for AR compositing and to a client for a "what do you see" answer;
//!   they never enter the observation.
//! * **Microphone** — a sound event. The duck can be made to react (a quack, a
//!   startle), and the level is published; it is not a sense the real duck's
//!   policy was trained on either.
//! * **LiDAR/depth** — where the phone has it, a distance field for the AR
//!   ground plane. Down-sampled to the duck's 8×8 ToF grid when a client asks
//!   for what the duck would see.
//!
//! Everything is reported with its mode on the wire, so a client can tell what
//! the duck actually perceived from what the phone merely noticed.

use serde_json::{json, Value};

/// The three declared modes. A sensor's reading is tagged with the one it was
/// taken in, so a client never mistakes an AR observation for the duck's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    /// 1:1. The duck's proprioception is MuJoCo's; phone sensors are external.
    Twin,
    /// Body 1:1, environment real. Camera/depth describe the room.
    Ar,
    /// The phone is the duck's body. Explicitly not 1:1.
    Possession,
    /// Benchmark-only senses, never in a normal run.
    Eval,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Twin => "twin",
            Mode::Ar => "ar",
            Mode::Possession => "possession",
            Mode::Eval => "eval-only",
        }
    }
}

/// One reading, with the sensor it came from and the mode it was taken in.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Reading {
    pub sensor: String,
    pub mode: String,
    /// Sensor-specific payload.
    pub value: Value,
    /// Milliseconds on the simulation clock when it arrived.
    pub tick_ms: u64,
    /// Whether this reading can reach the duck's 61-D observation. Always
    /// false except for the IMU, which reaches it as a disturbance.
    pub in_observation: bool,
}

/// The latest reading from each sensor, as a client would poll them.
#[derive(Debug, Default)]
pub struct Sensors {
    pub imu: Option<Reading>,
    pub camera: Option<Reading>,
    pub microphone: Option<Reading>,
    pub depth: Option<Reading>,
    pub mode: Option<Mode>,
}

impl Sensors {
    /// The IMU as a disturbance impulse, which is the one sensor that acts on
    /// the duck. Scaled the way the app has always scaled a shake.
    pub fn imu_push(&self) -> Option<[f64; 3]> {
        let reading = self.imu.as_ref()?;
        let acceleration = reading.value.get("acceleration")?.as_array()?;
        if acceleration.len() != 3 {
            return None;
        }
        let values: Vec<f64> = acceleration
            .iter()
            .filter_map(|value| value.as_f64())
            .collect();
        if values.len() != 3 {
            return None;
        }
        let magnitude = values.iter().map(|value| value * value).sum::<f64>().sqrt();
        if magnitude <= 1.2 {
            return None;
        }
        Some([values[0] * 0.8, values[1] * 0.8, values[2] * 0.8])
    }

    /// [`Self::imu_push`], consumed: a reading is one shove, not a force that
    /// keeps applying until the next post.
    pub fn take_imu_push(&mut self) -> Option<[f64; 3]> {
        let push = self.imu_push()?;
        // Mark the reading as delivered by clearing the acceleration; the
        // value itself stays readable in `/sensors`.
        if let Some(value) = self.imu.as_mut().and_then(|reading| reading.value.as_object_mut()) {
            value.insert("acceleration".to_owned(), serde_json::json!([0.0, 0.0, 0.0]));
        }
        Some(push)
    }

    /// A sudden loud sound in the room, as an external event. The duck's own
    /// 61-D observation never sees the microphone; this is the sanctioned
    /// boundary (FIDELITY §4): a startle, not a sense.
    pub fn loud(&self) -> bool {
        let Some(reading) = &self.microphone else { return false };
        let rms = reading.value.get("rms").and_then(Value::as_f64).unwrap_or(0.0);
        rms > 0.35
    }

    pub fn to_json(&self) -> Value {
        // The 8x8 grid the duck's head would see, computed from whatever
        // depth the phone published, by the OFFICIAL resampler. The array
        // shape the client sends is its own choice; anything that does not
        // parse publishes an empty grid rather than an invented one.
        let grid: Vec<f64> = self
            .depth
            .as_ref()
            .and_then(|reading| {
                reading
                    .value
                    .get("values")
                    .and_then(Value::as_array)
                    .map(|rows| {
                        rows.iter()
                            .filter_map(Value::as_f64)
                            .collect::<Vec<f64>>()
                    })
            })
            .map(|values| to_tof_grid(&values, 8, 8))
            .unwrap_or_default();
        json!({
            "mode": self.mode.unwrap_or(Mode::Twin).as_str(),
            "imu": self.imu,
            "camera": self.camera,
            "microphone": self.microphone,
            "depth": self.depth,
            "tof_grid": grid,
            "note": "phone sensors never enter the 61-D observation; the IMU acts as an external disturbance",
        })
    }
}

/// Down-sample a depth field to the duck's head ToF grid.
///
/// The real duck has an 8×8 time-of-flight sensor with a 45° field of view and
/// a 4 m range. A phone's LiDAR is denser and wider, so a client asking what
/// the duck would see gets the grid resampled and clipped to that envelope —
/// an approximation, labelled as one.
/// A sound the room made that the duck can answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundKind {
    Clap,
    Whistle,
}

impl SoundKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SoundKind::Clap => "clap",
            SoundKind::Whistle => "whistle",
        }
    }
}

/// Clap and whistle, from the same RMS/zero-crossing channel the shell already
/// posts. The microphone is the room, not a sense: this only decides which of
/// the duck's own reactions to play, and what a kid's rule can listen for.
///
/// The classifier uses what this channel can actually see: a clap is a loud
/// broadband transient whose zero-crossing peak is high; a whistle is a
/// sustained tone with a stable peak in the musical band. A shout is neither
/// and keeps the old startle path.
#[derive(Debug, Default)]
pub struct SoundWatch {
    last_sequence: u64,
    loud_since: Option<u64>,
    peak_max: f64,
    emitted: bool,
    seen_reading: bool,
}

impl SoundWatch {
    /// One observation, at most one event. `sequence` changes only when a new
    /// reading arrives, so a reading held across ticks is not heard twice;
    /// `now` is its wall clock in milliseconds, for the spell's duration.
    pub fn observe(
        &mut self,
        reading: Option<&Reading>,
        now: u64,
        sequence: u64,
    ) -> Option<SoundKind> {
        let reading = reading?;
        if self.seen_reading && sequence == self.last_sequence {
            return None;
        }
        self.seen_reading = true;
        self.last_sequence = sequence;
        let stamp = if reading.tick_ms > 0 { reading.tick_ms } else { now };
        let rms = reading.value.get("rms").and_then(Value::as_f64).unwrap_or(0.0);
        let peak = reading
            .value
            .get("peak_hz")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        if rms > 0.30 {
            let since = *self.loud_since.get_or_insert(stamp);
            self.peak_max = self.peak_max.max(peak);
            let duration = stamp.saturating_sub(since);
            if !self.emitted {
                // A sustained pure tone: whistle.
                if duration >= 200
                    && (400.0..=1800.0).contains(&self.peak_max)
                {
                    self.emitted = true;
                    return Some(SoundKind::Whistle);
                }
                // A clap's energy is high and broad; waiting for it to end
                // would miss the moment the duck should answer.
                if peak >= 2200.0 {
                    self.emitted = true;
                    return Some(SoundKind::Clap);
                }
            }
        } else {
            // A short loud spell that ended is a clap even if its peak was
            // middling; a long, low-frequency spell was speech.
            let clap = self
                .loud_since
                .map(|since| stamp.saturating_sub(since) <= 350 && self.peak_max >= 900.0)
                .unwrap_or(false)
                && !self.emitted;
            self.loud_since = None;
            self.peak_max = 0.0;
            self.emitted = false;
            if clap {
                return Some(SoundKind::Clap);
            }
        }
        None
    }
}

pub fn to_tof_grid(depth: &[f64], rows: usize, cols: usize) -> Vec<f64> {
    if depth.is_empty() || rows == 0 || cols == 0 {
        return Vec::new();
    }
    let mut grid = Vec::with_capacity(rows * cols);
    let step = (depth.len() as f64 / (rows * cols) as f64).max(1.0);
    for cell in 0..rows * cols {
        let index = (cell as f64 * step) as usize;
        let value = depth[index.min(depth.len() - 1)];
        grid.push(value.clamp(0.0, 4.0));
    }
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shake_becomes_a_push_and_a_still_phone_does_not() {
        let mut sensors = Sensors::default();
        sensors.imu = Some(Reading {
            sensor: "imu".to_owned(),
            mode: "twin".to_owned(),
            value: json!({ "acceleration": [0.1, 0.0, 0.0] }),
            tick_ms: 0,
            in_observation: false,
        });
        assert_eq!(sensors.imu_push(), None);

        sensors.imu = Some(Reading {
            sensor: "imu".to_owned(),
            mode: "twin".to_owned(),
            value: json!({ "acceleration": [2.0, 0.0, 0.0] }),
            tick_ms: 0,
            in_observation: false,
        });
        let push = sensors.imu_push().expect("a shake is a push");
        assert!((push[0] - 1.6).abs() < 1e-9);
    }

    #[test]
    fn a_short_loud_high_transient_is_a_clap_and_a_tone_is_a_whistle() {
        let mic = |rms: f64, peak: f64, tick: u64| Reading {
            sensor: "microphone".to_owned(),
            mode: "twin".to_owned(),
            value: json!({ "rms": rms, "peak_hz": peak }),
            tick_ms: tick,
            in_observation: false,
        };
        let mut watch = SoundWatch::default();
        assert_eq!(watch.observe(Some(&mic(0.02, 300.0, 1000)), 1000, 1), None);
        assert_eq!(
            watch.observe(Some(&mic(0.6, 2800.0, 1050)), 1050, 2),
            Some(SoundKind::Clap)
        );
        // The same reading held across ticks is not a second clap.
        assert_eq!(watch.observe(Some(&mic(0.6, 2800.0, 1050)), 1100, 2), None);

        let mut watch = SoundWatch::default();
        assert_eq!(watch.observe(Some(&mic(0.4, 900.0, 2000)), 2000, 1), None);
        assert_eq!(
            watch.observe(Some(&mic(0.4, 950.0, 2400)), 2400, 2),
            Some(SoundKind::Whistle)
        );
    }

    #[test]
    fn the_tof_grid_is_eight_by_eight_and_clipped() {
        let depth: Vec<f64> = (0..1000).map(|value| value as f64 * 0.01).collect();
        let grid = to_tof_grid(&depth, 8, 8);
        assert_eq!(grid.len(), 64);
        assert!(grid.iter().all(|value| (0.0..=4.0).contains(value)));
    }
}
