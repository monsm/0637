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

use kinematics::tof::{COLS as TOF_COLS, Posture, ROWS as TOF_ROWS, Reprojector, Zone};
use serde_json::{Value, json};

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

/// Pull the microphone's `pcm16` payload out of a `/sensor` reading.
///
/// The wire format is the shell's: base64 of mono little-endian i16 samples,
/// with an optional `"rate"` field (Hz, 16 000 when absent). The fields are
/// **removed from the reading** whether or not this build can use them:
/// `/sensors` publishes the registry, and echoing a megabyte of base64 audio
/// back to every poller would be neither useful nor private. A payload that
/// does not decode is dropped the same way — nothing is invented, nothing is
/// stored. So is a malformed one: a capture rate outside 8 000..=48 000 Hz
/// (a 1 Hz post would resample one second into 160 million samples) or a
/// decoded array longer than two seconds at the detector's 16 kHz.
pub fn take_pcm16(value: &mut Value) -> Option<(Vec<i16>, u32)> {
    let object = value.as_object_mut()?;
    let encoded = object.remove("pcm16")?.as_str().map(str::to_owned)?;
    let rate = object
        .remove("rate")
        .as_ref()
        .and_then(Value::as_u64)
        .unwrap_or(crate::petdetect::SAMPLE_RATE as u64);
    // The shells capture at 16 kHz, 44.1 kHz or 48 kHz. A rate outside
    // 8 kHz..=48 kHz is a bad payload, not a capture setting: 1 Hz would make
    // the resampler turn a second of audio into 160 million samples, and the
    // app would be killed for memory. Out of range means dropped.
    let rate = u32::try_from(rate)
        .ok()
        .filter(|rate| (8_000..=48_000).contains(rate))?;
    let samples = crate::petdetect::decode_pcm16_base64(&encoded)?;
    // Two seconds at the detector's 16 kHz is one post's worth of evidence. A
    // longer array is a malformed upload and would only inflate memory before
    // the detector's own ring caps it.
    if samples.len() > 2 * crate::petdetect::SAMPLE_RATE {
        return None;
    }
    Some((samples, rate))
}

impl Sensors {
    /// [`take_pcm16`] on the latest microphone reading. `None` means this
    /// post carried no audio (the RMS/zero-crossing summary is all it had).
    pub fn take_pcm16(&mut self) -> Option<(Vec<i16>, u32)> {
        self.microphone
            .as_mut()
            .and_then(|reading| take_pcm16(&mut reading.value))
    }
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
        if let Some(value) = self
            .imu
            .as_mut()
            .and_then(|reading| reading.value.as_object_mut())
        {
            value.insert(
                "acceleration".to_owned(),
                serde_json::json!([0.0, 0.0, 0.0]),
            );
        }
        Some(push)
    }

    /// A sudden loud sound in the room, as an external event. The duck's own
    /// 61-D observation never sees the microphone; this is the sanctioned
    /// boundary (FIDELITY §4): a startle, not a sense.
    pub fn loud(&self) -> bool {
        let Some(reading) = &self.microphone else {
            return false;
        };
        let rms = reading
            .value
            .get("rms")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        rms > 0.35
    }

    pub fn to_json(&self) -> Value {
        // The 8x8 grid the duck's head would see, from whatever depth the
        // phone published. The client's array is reduced to the sensor's 64
        // zones and run through the official `Reprojector`; `to_tof_grid`
        // documents the resting-pose degradation. Anything that does not
        // parse publishes an empty grid rather than an invented one.
        let grid: Vec<f64> = self
            .depth
            .as_ref()
            .and_then(|reading| {
                reading
                    .value
                    .get("values")
                    .and_then(Value::as_array)
                    .map(|rows| rows.iter().filter_map(Value::as_f64).collect::<Vec<f64>>())
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
        let stamp = if reading.tick_ms > 0 {
            reading.tick_ms
        } else {
            now
        };
        let rms = reading
            .value
            .get("rms")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
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
                if duration >= 200 && (400.0..=1800.0).contains(&self.peak_max) {
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

/// The duck's ToF sensor: an 8×8 grid over a 45° field, 4 m range.
pub const TOF_MAX_RANGE_M: f64 = 4.0;

/// Reproject a depth field into the duck's head ToF grid.
///
/// The duck's sensor is an 8×8 VL53L5CX/L8CX: 64 slant ranges over a 45°
/// field, 4 m range. The geometry is the official
/// [`kinematics::tof::Reprojector`]: each slant range is placed in the trunk
/// frame, a return that reaches the floor (within the official 0.85 safety
/// factor) is not an obstacle, and a horizontal range under 10 cm is discarded
/// as the sensor's own crosstalk. The returned grid holds each `Hit`'s
/// horizontal range; `0.0` means no obstacle — an empty zone, the floor, or a
/// return too close to trust. That is the encoding the shells' 8-row strips
/// already read (`0` = nothing, larger = farther).
///
/// The client's array is reduced to 64 row-major slant ranges before
/// projection, because the wire carries a flat depth field rather than the
/// sensor's own frame. Values outside `(0, 4] m` (and NaN) are "no return" —
/// the envelope the real sensor can report.
///
/// **Degraded pose.** `/sensors` carries no measured head-joint angles and no
/// trunk gravity, so this assumes the head at its rest joints and the trunk
/// upright at its model standing height. The official reprojector takes both;
/// a caller that has them uses [`to_tof_grid_with_pose`]. This is the official
/// geometry with a defaulted pose, not a claim that a phone depth frame is the
/// duck's sensor.
pub fn to_tof_grid(depth: &[f64], rows: usize, cols: usize) -> Vec<f64> {
    to_tof_grid_with_pose(depth, rows, cols, [0.0; 4], &Posture::default())
}

/// [`to_tof_grid`] with the pose the official reprojector expects.
///
/// `head_joints` is `[neck_pitch, head_pitch, head_yaw, head_roll]` in
/// radians; `posture` is the trunk's measured gravity and height. See
/// [`to_tof_grid`] for the output encoding.
pub fn to_tof_grid_with_pose(
    depth: &[f64],
    rows: usize,
    cols: usize,
    head_joints: [f64; 4],
    posture: &Posture,
) -> Vec<f64> {
    if depth.is_empty() || rows != TOF_ROWS || cols != TOF_COLS {
        return Vec::new();
    }
    let mut ranges = [None; TOF_ROWS * TOF_COLS];
    let step = (depth.len() as f64 / (TOF_ROWS * TOF_COLS) as f64).max(1.0);
    for (cell, range) in ranges.iter_mut().enumerate() {
        let index = (cell as f64 * step) as usize;
        let value = depth[index.min(depth.len() - 1)];
        if value.is_finite() && value > 0.0 && value <= TOF_MAX_RANGE_M {
            *range = Some(value);
        }
    }
    let zones = Reprojector::alpha().project(&ranges, head_joints, posture);
    zones
        .iter()
        .map(|zone| match zone {
            Zone::Hit { range, .. } => *range,
            // Empty, too close to trust, or confirmed floor: no obstacle.
            Zone::Empty | Zone::TooClose | Zone::Floor { .. } => 0.0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shake_becomes_a_push_and_a_still_phone_does_not() {
        let mut sensors = Sensors {
            imu: Some(Reading {
                sensor: "imu".to_owned(),
                mode: "twin".to_owned(),
                value: json!({ "acceleration": [0.1, 0.0, 0.0] }),
                tick_ms: 0,
                in_observation: false,
            }),
            ..Sensors::default()
        };
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
    fn a_microphone_post_can_carry_pcm_and_the_registry_does_not_echo_it() {
        let mut reading = Reading {
            sensor: "microphone".to_owned(),
            mode: "twin".to_owned(),
            value: json!({
                "rms": 0.12,
                "peak_hz": 900.0,
                "pcm16": "AQD+/w==",
                "rate": 16000,
            }),
            tick_ms: 0,
            in_observation: false,
        };
        let (samples, rate) = take_pcm16(&mut reading.value).expect("PCM decodes");
        assert_eq!(samples, vec![1, -2]);
        assert_eq!(rate, 16_000);
        assert!(reading.value.get("pcm16").is_none(), "audio is not stored");
        assert!(reading.value.get("rate").is_none());
        assert_eq!(reading.value["rms"], json!(0.12), "the summary survives");

        // A rate the shell did not send is the 16 kHz contract.
        let mut bare = json!({ "pcm16": "AQD+/w==" });
        assert_eq!(take_pcm16(&mut bare).map(|(_, rate)| rate), Some(16_000));
        // Malformed or odd-length payloads are removed and refused, not kept.
        let mut broken = json!({ "pcm16": "AQA", "rate": 16000 });
        assert_eq!(take_pcm16(&mut broken), None);
        assert!(broken.get("pcm16").is_none());
        // Something that is not a microphone payload is untouched.
        let mut other = json!({ "acceleration": [1.0, 0.0, 0.0] });
        assert_eq!(take_pcm16(&mut other), None);
        assert!(other.get("acceleration").is_some());
    }

    /// Base64 for `bytes` zero bytes: the mic payload is little-endian i16
    /// samples, and zero bytes decode to zero samples.
    fn pcm_base64(bytes: usize) -> String {
        let mut out = "AAAA".repeat(bytes / 3);
        match bytes % 3 {
            1 => out.push_str("AA=="),
            2 => out.push_str("AAA="),
            _ => {}
        }
        out
    }

    /// A rate is a capture rate the shell really uses (8 kHz … 48 kHz). A
    /// rate of 1 Hz would make the resampler turn one second of audio into
    /// 160 million samples, and a megabyte post is not this second's sound.
    /// Both are bad payloads: stripped from the registry and dropped.
    #[test]
    fn a_pcm_post_is_bounded_to_real_rates_and_two_seconds() {
        for rate in [0, 1, 7_999, 48_001] {
            let mut value = json!({ "pcm16": pcm_base64(4), "rate": rate });
            assert_eq!(take_pcm16(&mut value), None, "rate {rate} must be refused");
            assert!(
                value.get("pcm16").is_none(),
                "rate {rate}: the audio is stripped, not stored"
            );
            assert!(value.get("rate").is_none(), "rate {rate}");
        }
        for rate in [8_000u64, 16_000, 44_100, 48_000] {
            let mut value = json!({ "pcm16": pcm_base64(4), "rate": rate });
            assert_eq!(
                take_pcm16(&mut value).map(|(_, accepted)| accepted),
                Some(rate as u32),
                "rate {rate} is a real capture rate"
            );
        }

        // Two seconds at the model's 16 kHz is exactly the cap.
        let two_seconds = pcm_base64(2 * crate::petdetect::SAMPLE_RATE * 2);
        let mut value = json!({ "pcm16": two_seconds, "rate": 16_000 });
        let (samples, _) = take_pcm16(&mut value).expect("two seconds is accepted");
        assert_eq!(samples.len(), 2 * crate::petdetect::SAMPLE_RATE);

        let over = pcm_base64(2 * (crate::petdetect::SAMPLE_RATE + 1) * 2);
        let mut value = json!({ "pcm16": over, "rate": 16_000 });
        assert_eq!(take_pcm16(&mut value), None, "over two seconds is refused");
        assert!(
            value.get("pcm16").is_none(),
            "the oversized payload is stripped too"
        );
    }

    #[test]
    fn a_forward_return_projects_to_its_horizontal_range() {
        // A metre along the centre zone's beam, everything else empty. The
        // official reprojection reports horizontal range, not the slant.
        let mut depth = vec![0.0; 64];
        let centre = 3 * 8 + 3;
        depth[centre] = 1.0;
        let grid = to_tof_grid(&depth, 8, 8);
        assert_eq!(grid.len(), 64);
        assert!(
            (0.8..=1.05).contains(&grid[centre]),
            "range: {}",
            grid[centre]
        );
        assert!(
            grid.iter()
                .enumerate()
                .all(|(index, value)| index == centre || *value == 0.0)
        );
    }

    #[test]
    fn the_floor_is_not_an_obstacle_and_a_return_before_it_is() {
        // The duck's official test fixture: a low, centre-column beam with the
        // head pitched down. Four metres reaches the floor; 20 cm, still over
        // the 10 cm noise band, is something standing in front of it.
        let looking_down = [0.0, 0.3, 0.0, 0.0];
        let zone = 6 * 8 + 3;
        let mut depth = vec![0.0; 64];
        depth[zone] = 4.0;
        let floor = to_tof_grid_with_pose(&depth, 8, 8, looking_down, &Posture::default());
        assert_eq!(
            floor[zone], 0.0,
            "a floor return must not read as an obstacle"
        );

        depth[zone] = 0.2;
        let hit = to_tof_grid_with_pose(&depth, 8, 8, looking_down, &Posture::default());
        assert!(hit[zone] > 0.0, "an obstacle before the floor is a hit");
    }

    #[test]
    fn a_return_closer_than_ten_centimetres_is_discarded() {
        let mut depth = vec![0.0; 64];
        let centre = 3 * 8 + 3;
        depth[centre] = 0.05;
        let grid = to_tof_grid(&depth, 8, 8);
        assert_eq!(grid[centre], 0.0, "crosstalk under 10 cm is not a return");
    }

    #[test]
    fn the_grid_is_the_duck_eight_by_eight_and_out_of_envelope_is_empty() {
        let mut depth = vec![0.0; 64];
        let centre = 3 * 8 + 3;
        depth[centre] = TOF_MAX_RANGE_M + 0.5;
        let grid = to_tof_grid(&depth, 8, 8);
        assert_eq!(grid.len(), 64);
        assert!(grid.iter().all(|value| *value == 0.0));
        // Other shapes have no official beam table to project through.
        assert!(to_tof_grid(&depth, 4, 4).is_empty());
    }
}
