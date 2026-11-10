//! The duck's voice, from the robot's own synth.
//!
//! `sounds/` is the release's voice generator: a seedable formant synth with
//! seven tags and a dozen variants each, ported from the Python prototype so a
//! board carries its own voice. It is pure Rust apart from `hardware_seed`
//! (which reads the SoC serial), so the modules that make the sound compile
//! here verbatim and the digital duck quacks with the same recipes as the
//! physical one.
//!
//! The robot's *decisions* are `robotd`'s too: a greet when the loop comes up,
//! a chirp when asked, a low peck before the servos go quiet.

use crate::personality::Personality;

/// The tags `robotd` plays, in its own spelling — all seven of them.
/// `wheee` is the held joy-ride: upstream streams start/loop/end, and the
/// recipe also renders a finite one-shot (start + two loops + end). A phone
/// has no pad button held down, so the finite form is what `/sound` plays;
/// it is still the official recipe, not a stand-in.
pub const TAGS: [&str; 7] = ["greet", "chirp", "coo", "peck", "inquire", "alarm", "wheee"];

/// The recipe for a tag, from the shared table.
fn recipe(tag: &str) -> Option<fn(&Personality, u32) -> Vec<f32>> {
    crate::TAGS
        .iter()
        .find(|(name, _)| *name == tag)
        .map(|(_, recipe)| *recipe)
}

/// The duck's voice: one personality, derived once from a stable seed.
///
/// The robot seeds from the SoC's efuse serial — a per-chip identity. A phone
/// has no such number, so the seed is the build's provenance fingerprint
/// instead: stable across launches, and different between builds that are
/// different robots. It is not the same duck as a particular physical unit,
/// which is the one thing a digital twin cannot be (FIDELITY.md, L4).
pub struct Voice {
    personality: Personality,
    /// Per-tag rotation through that tag's variants, so the duck does not
    /// sound like a stuck recording. The robot picks at random; counters are
    /// the same idea without a source of entropy a replayed tick would diverge
    /// on. Per tag, because one shared counter would make a chirp after a greet
    /// start at variant 2 rather than at the tag's own first.
    plays: [u64; 7],
}

impl Voice {
    pub fn new(seed: u32) -> Self {
        Self {
            personality: Personality::from_seed(seed),
            plays: [0; 7],
        }
    }

    pub fn seed(&self) -> u32 {
        self.personality.seed
    }

    pub fn personality(&self) -> Personality {
        self.personality
    }

    /// Render one sound, choosing the next variant of its tag.
    pub fn render(&mut self, tag: &str) -> Option<(String, Vec<f32>)> {
        let recipe = recipe(tag)?;
        let count = crate::variant_count(tag).max(1);
        let slot = TAGS.iter().position(|name| *name == tag).unwrap_or(0);
        let variant = (self.plays[slot] % u64::from(count)) as u32;
        self.plays[slot] = self.plays[slot].wrapping_add(1);
        Some((tag.to_owned(), recipe(&self.personality, variant)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tag_renders_audible_pcm_at_the_robots_rate() {
        let mut voice = Voice::new(0xD0C);
        for tag in TAGS {
            let (name, pcm) = voice.render(tag).expect(tag);
            assert_eq!(name, tag);
            assert!(
                pcm.len() > crate::synth::SR as usize / 20,
                "{tag} is too short: {} samples",
                pcm.len()
            );
            let rms = (pcm.iter().map(|s| s * s).sum::<f32>() / pcm.len() as f32).sqrt();
            assert!(rms > 0.01, "{tag} is silent (rms {rms})");
            assert!(pcm.iter().all(|s| s.is_finite()), "{tag} has a non-finite sample");
        }
    }

    #[test]
    fn the_same_seed_is_the_same_voice_and_a_different_one_is_not() {
        let a = Voice::new(1).render("greet").expect("greet").1;
        let b = Voice::new(1).render("greet").expect("greet").1;
        let c = Voice::new(2).render("greet").expect("greet").1;
        assert_eq!(a, b, "the same seed must be the same duck");
        assert_ne!(a, c, "two seeds must sound like two ducks");
    }

    #[test]
    fn variants_rotate_rather_than_repeat() {
        let mut voice = Voice::new(7);
        let first = voice.render("chirp").expect("chirp").1;
        let second = voice.render("chirp").expect("chirp").1;
        assert_ne!(first, second, "a second chirp must not be the same file");
    }

    #[test]
    fn an_unknown_tag_is_refused() {
        assert!(Voice::new(1).render("bark").is_none());
    }
}
