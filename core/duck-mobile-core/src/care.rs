//! The duck's care loop: hunger, mood and bond.
//!
//! The physical duck has no such state — this is the companion layer a phone
//! can add without lying about the robot. It changes *when* the duck chooses
//! to show an emotion (sad when neglected, a doze when left alone) and how it
//! answers a hand; it never touches the 61-D observation, the policies or the
//! safety layer.
//!
//! Time is simulated: rates below are per simulated second, so a rewound duck
//! has the care state it had at that tick, and CI can run the clock faster.

use serde::{Deserialize, Serialize};

/// Hours of simulated time from fed to starving.
pub const HUNGER_SECONDS: f64 = 45.0 * 60.0;
/// How long without any touch before the duck starts feeling neglected.
pub const NEGLECT_AFTER_S: f64 = 6.0 * 60.0;
/// How long after that it curls up and dozes.
pub const DOZE_AFTER_S: f64 = 12.0 * 60.0;
/// A pet, a feed and a ride move the bond by these amounts.
const BOND_PER_PET: f64 = 0.02;
const BOND_PER_FEED: f64 = 0.03;
const BOND_DECAY_PER_S: f64 = 1.0 / (24.0 * 3600.0);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Care {
    /// 0 fed … 1 starving.
    pub hunger: f64,
    /// -1 sad … 1 happy.
    pub mood: f64,
    /// 0 stranger … 1 bonded.
    pub bond: f64,
    /// Curled up after a long neglect; any interaction wakes it.
    pub asleep: bool,
    /// Simulated tick of the last interaction (pet, feed, carry, drive).
    pub last_touch: u64,
}

impl Default for Care {
    fn default() -> Self {
        Self {
            hunger: 0.15,
            mood: 0.35,
            bond: 0.1,
            asleep: false,
            last_touch: 0,
        }
    }
}

impl Care {
    /// One simulated tick. `now` is the simulated tick (20 ms each).
    pub fn tick(&mut self, now: u64, dt: f64, scale: f64) {
        let dt = dt * scale;
        self.hunger = (self.hunger + dt / HUNGER_SECONDS).min(1.0);

        let idle_s = now.saturating_sub(self.last_touch) as f64 * 0.02 * scale;
        let neglected = (idle_s - NEGLECT_AFTER_S).max(0.0);
        // Hunger drags the mood down; neglect drags it further; a bond lifts
        // the floor. The mood follows its target over about two minutes.
        let target = 0.55 - 0.9 * self.hunger - (neglected / 300.0).min(0.5)
            + 0.25 * self.bond;
        self.mood += (target.clamp(-1.0, 1.0) - self.mood) * (dt / 120.0).min(1.0);

        self.bond = (self.bond - BOND_DECAY_PER_S * dt).max(0.0);

        if !self.asleep && idle_s >= DOZE_AFTER_S {
            self.asleep = true;
        }
        if self.asleep && idle_s < 1.0 {
            // Any touch (recorded this tick) wakes it.
            self.asleep = false;
        }
    }

    /// A hand: pets are the strongest bond signal.
    pub fn pet(&mut self, now: u64) {
        self.last_touch = now;
        self.asleep = false;
        self.bond = (self.bond + BOND_PER_PET).min(1.0);
        self.mood = (self.mood + 0.12).min(1.0);
    }

    /// A treat: the only thing that lowers hunger.
    pub fn feed(&mut self, now: u64) {
        self.last_touch = now;
        self.asleep = false;
        self.hunger = (self.hunger - 0.45).max(0.0);
        self.bond = (self.bond + BOND_PER_FEED).min(1.0);
        self.mood = (self.mood + 0.1).min(1.0);
    }

    /// Driving, carrying, a skill: attention, but weaker than a pet.
    pub fn touch(&mut self, now: u64) {
        self.last_touch = now;
        self.asleep = false;
    }

    /// Whether it is sad enough to show it, given the cooldown.
    pub fn sad(&self) -> bool {
        self.mood < -0.35
    }

    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "hunger": self.hunger,
            "mood": self.mood,
            "bond": self.bond,
            "asleep": self.asleep,
            "last_touch": self.last_touch,
        })
    }

    /// Restore the numbers a shell saved. Unknown fields are ignored; absent
    /// ones keep the defaults.
    pub fn restore(&mut self, value: &serde_json::Value) {
        if let Some(v) = value.get("hunger").and_then(serde_json::Value::as_f64) {
            self.hunger = v.clamp(0.0, 1.0);
        }
        if let Some(v) = value.get("mood").and_then(serde_json::Value::as_f64) {
            self.mood = v.clamp(-1.0, 1.0);
        }
        if let Some(v) = value.get("bond").and_then(serde_json::Value::as_f64) {
            self.bond = v.clamp(0.0, 1.0);
        }
        if let Some(v) = value.get("asleep").and_then(serde_json::Value::as_bool) {
            self.asleep = v;
        }
        if let Some(v) = value.get("last_touch").and_then(serde_json::Value::as_u64) {
            self.last_touch = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feeding_lowers_hunger_and_petting_lifts_the_mood() {
        let mut care = Care::default();
        let before = care.hunger;
        care.feed(0);
        assert!(care.hunger < before);
        let mood = care.mood;
        care.pet(0);
        assert!(care.mood > mood);
    }

    #[test]
    fn neglect_makes_it_sad_and_then_dozy() {
        let mut care = Care::default();
        care.last_touch = 0;
        // 30 simulated minutes at normal speed: hungry, neglected, sad.
        let ticks = 30 * 60 * 50;
        for tick in 0..ticks {
            care.tick(tick as u64, 0.02, 1.0);
        }
        assert!(care.mood < 0.0, "mood {}", care.mood);
        assert!(care.asleep, "it should doze after a long neglect");
        care.pet(ticks as u64);
        assert!(!care.asleep, "a touch wakes it");
    }

    #[test]
    fn a_saved_state_restores() {
        let mut care = Care::default();
        care.feed(12);
        let json = care.summary();
        let mut back = Care::default();
        back.restore(&json);
        assert!((back.hunger - care.hunger).abs() < 1e-9);
        assert_eq!(back.last_touch, 12);
    }
}
