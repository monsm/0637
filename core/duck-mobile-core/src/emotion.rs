//! The duck's emotions: robotd's own keyframe timelines and their sounds.
//!
//! `robotd/emotions/` ships thirteen `.json` + `.wav` pairs. A `.json` is a
//! timeline of head (neck, pitch, yaw, roll), body pitch and mouth opening;
//! the `.wav` is the voice that starts with it. `robot.do curious` plays one,
//! so on the real robot an emotion is a skill like any other.
//!
//! We do not compile upstream `emotion.rs`: it speaks `robotd`'s `Snapshot`
//! and IPC types, which this crate does not have. We compile the *data* —
//! the same keyframes, the same samples — and apply them in `runtime::tick`
//! at the same point `Frame::apply` would: an emotion replaces the client's
//! head/body/mouth command for its own ticks and leaves nothing behind when
//! it ends, so a pad still sending commands cannot win half the frames.
//!
//! The wav files are 48 kHz mono 16-bit PCM, exactly the synth's native rate
//! (`duck_core_voice_rate`), so they reach the platform's speaker through the
//! same `duck_core_render_sound` channel as a synthesized chirp.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// One keyframe, in the wire format `robotd/emotions/*.json` uses.
#[derive(Debug, Clone, Deserialize)]
struct Key {
    t: f64,
    #[serde(default)]
    neck: f64,
    #[serde(default)]
    head_pitch: f64,
    #[serde(default)]
    head_yaw: f64,
    #[serde(default)]
    head_roll: f64,
    #[serde(default)]
    body_pitch: f64,
    /// Body height; `Frame::apply` puts this in the pose's z slot. All shipped
    /// emotions leave it at zero, but the field is part of the wire format.
    #[serde(default)]
    body_z: Option<f64>,
    /// An emotion holds the duck still: upstream's Frame::apply zeroes the
    /// twist. Every shipped timeline carries [0,0,0]; parse it so a future
    /// one that walks is not silently ignored.
    #[serde(default)]
    twist: Option<[f64; 3]>,
    #[serde(default)]
    mouth: f64,
    /// `devastated` sits the duck down partway through. The controller owns
    /// sit/stand, so this is a request the runtime acts on when it ticks past
    /// this keyframe, not a pose the keyframe itself describes.
    #[serde(default)]
    skill: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct File {
    keyframes: Vec<Key>,
}

/// What an emotion asks for at one instant: the same four head axes, three
/// body axes and mouth as `emotion.rs::Frame`, minus the IPC snapshot wrapper.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EmotionFrame {
    pub head: [f64; 4],
    pub body: [f64; 3],
    /// The twist this timeline asks for; `None` means "hold still", which is
    /// what `Frame::apply` does to the Snapshot.
    pub twist: Option<[f64; 3]>,
    pub mouth: f64,
}

/// One emotion, with its own voice.
#[derive(Debug, Clone)]
pub struct Emotion {
    pub name: String,
    keys: Vec<Key>,
    pcm: Vec<f32>,
    /// When the embedded `sit` fires, so a checkpoint does not fire it twice.
    sit_due: Option<f64>,
}

impl Emotion {
    pub fn duration(&self) -> f64 {
        self.keys.last().map_or(0.0, |key| key.t)
    }

    pub fn pcm(&self) -> &[f32] {
        &self.pcm
    }

    /// Linear interpolation between the two keyframes around `t`, exactly the
    /// shape `emotion.rs::frame_at` produces. Before the first and after the
    /// last key the nearest one holds.
    pub fn frame_at(&self, t: f64) -> EmotionFrame {
        if self.keys.is_empty() {
            return EmotionFrame::default();
        }
        let index = self.keys.partition_point(|key| key.t <= t);
        let (a, b) = if index == 0 {
            (self.keys[0].clone(), self.keys[0].clone())
        } else if index >= self.keys.len() {
            let last = self.keys[self.keys.len() - 1].clone();
            (last.clone(), last)
        } else {
            (self.keys[index - 1].clone(), self.keys[index].clone())
        };
        let span = b.t - a.t;
        let alpha = if span > 1e-9 { ((t - a.t) / span).clamp(0.0, 1.0) } else { 0.0 };
        let mix = |x: f64, y: f64| x + (y - x) * alpha;
        EmotionFrame {
            head: [
                mix(a.neck, b.neck),
                mix(a.head_pitch, b.head_pitch),
                mix(a.head_yaw, b.head_yaw),
                mix(a.head_roll, b.head_roll),
            ],
            body: [mix(a.body_z.unwrap_or(0.0), b.body_z.unwrap_or(0.0)), 0.0, mix(a.body_pitch, b.body_pitch)],
            twist: a.twist.map(|from| {
                let to = b.twist.unwrap_or(from);
                [
                    mix(from[0], to[0]),
                    mix(from[1], to[1]),
                    mix(from[2], to[2]),
                ]
            }),
            mouth: mix(a.mouth, b.mouth),
        }
    }

    /// The first embedded skill not yet delivered by `played_skills`.
    pub fn pending_skill(&self, t: f64) -> Option<&str> {
        if t < self.sit_due? {
            return None;
        }
        self.keys.iter().find_map(|key| key.skill.as_deref())
    }
}

/// Every emotion on this duck, by name. `BTreeMap` keeps `/skills` ordered
/// and the ring's list stable across a checkpoint/restore.
#[derive(Debug, Clone, Default)]
pub struct Library(pub BTreeMap<String, Emotion>);

impl Library {
    /// Load `<dir>/*.json` and the matching `.wav`. A missing or malformed
    /// pair is skipped rather than failing the whole bundle: one bad emotion
    /// must not cost the duck its other twelve.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let mut out = BTreeMap::new();
        let entries = std::fs::read_dir(dir).map_err(|error| format!("{dir:?}: {error}"))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(_) => continue,
            };
            let Ok(file) = serde_json::from_str::<File>(&text) else {
                continue;
            };
            if file.keyframes.is_empty() {
                continue;
            }
            let wav = dir.join(format!("{name}.wav"));
            let pcm = decode_wav(&wav).unwrap_or_default();
            let sit_due = file
                .keyframes
                .iter()
                .find(|key| key.skill.as_deref() == Some("sit"))
                .map(|key| key.t);
            out.insert(
                name.to_owned(),
                Emotion {
                    name: name.to_owned(),
                    keys: file.keyframes,
                    pcm,
                    sit_due,
                },
            );
        }
        Ok(Self(out))
    }

    pub fn names(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<&Emotion> {
        self.0.get(name)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// One emotion playing right now, on the simulated clock.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Playing {
    pub name: String,
    /// Seconds since the emotion started.
    pub elapsed: f64,
    /// The embedded `sit` was handed to the controller already.
    pub skill_delivered: bool,
}

/// Decode the one format robotd ships: 48 kHz mono 16-bit PCM WAV. Anything
/// else is an empty voice, never a crash: the pose timeline still plays.
fn decode_wav(path: &Path) -> Option<Vec<f32>> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut offset = 12;
    let mut channels = 0u16;
    let mut rate = 0u32;
    let mut bits = 0u16;
    let mut data: &[u8] = &[];
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        let start = offset + 8;
        let end = (start + size).min(bytes.len());
        match id {
            b"fmt " if size >= 16 => {
                channels = u16::from_le_bytes(bytes[start..start + 2].try_into().ok()?);
                rate = u32::from_le_bytes(bytes[start + 4..start + 8].try_into().ok()?);
                bits = u16::from_le_bytes(bytes[start + 14..start + 16].try_into().ok()?);
            }
            b"data" => data = &bytes[start..end],
            _ => {}
        }
        offset = start + size + (size % 2);
    }
    if channels != 1 || rate != crate::synth::SR as u32 || bits != 16 || data.is_empty() {
        return None;
    }
    let mut pcm = Vec::with_capacity(data.len() / 2);
    for pair in data.chunks_exact(2) {
        let sample = i16::from_le_bytes([pair[0], pair[1]]);
        pcm.push(f32::from(sample) / 32768.0);
    }
    Some(pcm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_interpolates_between_keys() {
        let emotion = Emotion {
            name: "test".into(),
            keys: vec![
                Key { t: 0.0, neck: 0.0, head_pitch: 0.0, head_yaw: 0.0, head_roll: 0.0, body_pitch: 0.0, body_z: None, twist: None, mouth: 0.0, skill: None },
                Key { t: 1.0, neck: 0.0, head_pitch: 0.0, head_yaw: 1.0, head_roll: 0.0, body_pitch: 0.0, body_z: None, twist: None, mouth: 0.0, skill: None },
            ],
            pcm: vec![],
            sit_due: None,
        };
        assert_eq!(emotion.duration(), 1.0);
        assert!((emotion.frame_at(0.25).head[2] - 0.25).abs() < 1e-9);
        assert!((emotion.frame_at(2.0).head[2] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_official_emotions_load() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../upstream/microduck/robotd/emotions");
        let library = Library::load(&dir).expect("upstream emotions");
        assert!(!library.is_empty());
        assert!(library.get("curious").is_some());
        assert!(library.get("curious").unwrap().duration() > 2.0);
        assert!(!library.get("curious").unwrap().pcm().is_empty());
    }
}
