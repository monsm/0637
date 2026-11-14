//! The duck's care loop: hunger, mood, bond and the night.
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

/// A night: the duck was put to bed on purpose rather than dozing off from
/// neglect. Only the start is stored; "how quiet" is always derived from the
/// tick delta, so a client cannot claim a longer night than the duck slept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Night {
    pub started_tick: u64,
}

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
    /// An interaction just woke the duck; the loop reads this once to give it
    /// a short break before a low mood can play the sad timeline.
    #[serde(default)]
    pub woke: bool,
    /// A night the person put the duck to bed for. The same doze a long
    /// neglect reaches, chosen: a night keeps `asleep` true until a hand or
    /// the morning button ends it.
    #[serde(default)]
    pub night: Option<Night>,
    /// A hand just ended a night. The loop reads this once to write the vet
    /// book's honest note, the way `woke` marks the wake for the sad grace.
    #[serde(skip)]
    pub woken_by_touch: bool,
}

impl Default for Care {
    fn default() -> Self {
        Self {
            hunger: 0.15,
            mood: 0.35,
            bond: 0.1,
            asleep: false,
            last_touch: 0,
            woke: false,
            night: None,
            woken_by_touch: false,
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

        // A night is a sleep by construction: no ordinary rule may stir it,
        // and a restored payload that forgot `asleep` is corrected here.
        if self.night.is_some() {
            self.asleep = true;
        }
        if !self.asleep && idle_s >= DOZE_AFTER_S {
            self.asleep = true;
        }
        // Waking is an interaction (pet/feed/touch) or the morning button and
        // is handled there. Deciding it from the idle span would make a
        // restored nap look like a fresh touch, because a new process's tick
        // starts at zero.
    }

    /// Put the duck to bed on purpose: the doze a neglected duck reaches,
    /// chosen. Idempotent — a second good night does not restart the clock,
    /// so the quiet span still begins at the first.
    pub fn enter_night(&mut self, now: u64) {
        if self.night.is_none() {
            self.night = Some(Night { started_tick: now });
        }
        self.asleep = true;
    }

    /// Morning: end the night and wake the duck. `Some(quiet_s)` when a night
    /// was running, `None` when it was not — a duck that is merely dozing on
    /// its own is not in night, and the morning button does not stir it.
    ///
    /// The span counts simulated ticks at 20 ms each, the clock every shell
    /// reads, deliberately not the care time scale: "quiet for ten minutes"
    /// must mean the same ten minutes whatever a demo sped the tummy clock
    /// up to.
    pub fn end_night(&mut self, now: u64) -> Option<f64> {
        let quiet_s = self.quiet_s(now);
        self.night.take()?;
        self.wake_up(now);
        Some(quiet_s)
    }

    /// The night so far, in simulated seconds. Zero when no night is running.
    pub fn quiet_s(&self, now: u64) -> f64 {
        self.night
            .map_or(0.0, |night| now.saturating_sub(night.started_tick) as f64 * 0.02)
    }

    /// The shared tail of every wake: the duck is up, the idle clock restarts
    /// (so it cannot fall straight back into the doze it just left), and the
    /// loop knows to give the moment a short grace before the sad timeline.
    fn wake_up(&mut self, now: u64) {
        self.woke = self.woke || self.asleep;
        self.last_touch = now;
        self.asleep = false;
    }

    /// A hand: it wakes the duck and, when it was in a night, ends the night
    /// and flags the reason. The morning button has its own path so the vet
    /// book can tell "morning" from "woken_by_touch".
    fn wake(&mut self, now: u64) {
        self.woken_by_touch = self.woken_by_touch || self.night.take().is_some();
        self.wake_up(now);
    }

    /// A hand: pets are the strongest bond signal.
    pub fn pet(&mut self, now: u64) {
        self.wake(now);
        self.bond = (self.bond + BOND_PER_PET).min(1.0);
        self.mood = (self.mood + 0.12).min(1.0);
    }

    /// A treat: the only thing that lowers hunger.
    pub fn feed(&mut self, now: u64) {
        self.wake(now);
        self.hunger = (self.hunger - 0.45).max(0.0);
        self.bond = (self.bond + BOND_PER_FEED).min(1.0);
        self.mood = (self.mood + 0.1).min(1.0);
    }

    /// A treat that arrived from somewhere else — a neighbour's gift.
    ///
    /// The duck is less hungry and in a better mood, because it really ate.
    /// But nobody's hand was here: the night stays as it was, and the bond
    /// does not move, because the bond is between the duck and the person
    /// holding the phone.
    pub fn feed_from_away(&mut self) {
        self.hunger = (self.hunger - 0.45).max(0.0);
        self.mood = (self.mood + 0.1).min(1.0);
    }

    /// Driving, carrying, a skill: attention, but weaker than a pet.
    pub fn touch(&mut self, now: u64) {
        self.wake(now);
    }

    /// Whether it is sad enough to show it, given the cooldown.
    pub fn sad(&self) -> bool {
        self.mood < -0.35
    }

    /// The heart rate a hand would feel through the shell, in beats per
    /// minute. A pure function of this Care — every input is a number the
    /// loop already keeps, and nothing here mutates anything: it is a
    /// reading, not an event. A sleeping duck's heart slows like a resting
    /// animal's; a happy bonded one is quick; hunger races it a little.
    pub fn heart_bpm(&self) -> f64 {
        if self.asleep {
            return 46.0;
        }
        let mood_beats = self.mood * 12.0;
        let hunger_beats = self.hunger * 14.0;
        let bond_beats = self.bond * -8.0;
        (72.0 + mood_beats + hunger_beats + bond_beats).clamp(42.0, 118.0)
    }

    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "hunger": self.hunger,
            "mood": self.mood,
            "bond": self.bond,
            "asleep": self.asleep,
            // The hand's reading, not a new fact: derived here so every
            // client feels the same heart without re-deriving it.
            "heart_bpm": (self.heart_bpm() * 10.0).round() / 10.0,
            "last_touch": self.last_touch,
            // The night travels with the three numbers: the shells save this
            // object wholesale and hand it back, and the quiet span is always
            // recomputed from the tick delta rather than trusted.
            "night": {
                "active": self.night.is_some(),
                "started_tick": self.night.map_or(0, |night| {
                    night.started_tick.min(u32::MAX as u64) as u32
                }),
            },
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
        if let Some(night) = value.get("night") {
            // Only a well-formed night starts one; a malformed row is
            // ignored rather than putting the duck to bed on a guess.
            match night.get("active").and_then(serde_json::Value::as_bool) {
                Some(true) => {
                    if let Some(started_tick) = night
                        .get("started_tick")
                        .and_then(serde_json::Value::as_u64)
                    {
                        self.night = Some(Night { started_tick });
                    }
                }
                Some(false) => self.night = None,
                None => {}
            }
        }
        // A night is a sleep by construction: no saved payload may leave a
        // night duck awake.
        if self.night.is_some() {
            self.asleep = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heart_is_a_pure_reading_of_the_care_numbers() {
        let care = Care::default();
        let rest = care.heart_bpm();
        assert!((55.0..90.0).contains(&rest), "rest {rest}");
        let mut happy = care;
        happy.mood = 1.0; happy.bond = 1.0; happy.hunger = 0.0;
        let mut sad = care;
        sad.mood = -1.0; sad.bond = 0.0; sad.hunger = 1.0;
        assert!(happy.heart_bpm() > sad.heart_bpm());
        let mut sleeper = happy;
        sleeper.asleep = true;
        assert_eq!(sleeper.heart_bpm(), 46.0);
        let summary = sleeper.summary();
        assert_eq!(
            summary.get("heart_bpm").and_then(serde_json::Value::as_f64),
            Some(46.0)
        );
        let mut restored = Care::default();
        restored.restore(&summary);
        assert!(restored.heart_bpm() > 0.0);
    }

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
    fn a_fast_care_clock_brings_the_doze_sooner() {
        let mut care = Care::default();
        care.last_touch = 0;
        // 200 ticks = 4 simulated seconds; at 200x that is 800 care-seconds,
        // past the twelve-minute doze threshold, without waiting.
        care.tick(200, 0.02, 200.0);
        assert!(care.asleep, "a sped-up care clock must age the idle spell too");
    }

    #[test]
    fn a_restored_old_touch_and_a_fast_clock_doze() {
        let mut care = Care::default();
        care.restore(&serde_json::json!({
            "hunger": 0.9,
            "mood": 0.2,
            "bond": 0.4,
            "asleep": false,
            "last_touch": 300,
        }));
        // 400 ticks of neglect at 200x = 1600 care-seconds, past 12 minutes.
        care.tick(700, 0.02, 200.0);
        assert!(care.asleep, "restore + scale must doze, got {:?}", care);
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

    #[test]
    fn a_good_night_holds_the_duck_asleep_until_the_morning() {
        let mut care = Care::default();
        care.enter_night(100);
        assert!(care.asleep);
        // A second good night is idempotent: the quiet clock begins at the
        // first press, not the last.
        care.enter_night(400);
        assert_eq!(care.night.map(|night| night.started_tick), Some(100));
        // Ten simulated minutes of ticks go by: a night duck is asleep by
        // construction and no ordinary rule may stir it.
        let ticks = 10 * 60 * 50;
        for tick in 0..=ticks {
            care.tick(tick, 0.02, 1.0);
        }
        assert!(care.asleep, "a night duck must stay asleep across ticks");
        let quiet = care.end_night(ticks + 100).expect("a night was running");
        // 30 000 ticks at 20 ms each, measured from the first press.
        assert!((quiet - 600.0).abs() < 1e-9, "quiet {quiet}");
        assert!(!care.asleep);
        assert!(care.night.is_none());
        assert_eq!(care.last_touch, ticks + 100, "the morning is a hand");
        // Morning again is a no-op, not a second wake.
        assert!(care.end_night(40_000).is_none());

        // A demo's sped-up care clock must not age a night duck awake either.
        let mut fast = Care::default();
        fast.enter_night(0);
        fast.tick(200, 0.02, 200.0);
        assert!(fast.asleep);
    }

    #[test]
    fn a_hand_ends_the_night_and_flags_the_honest_note() {
        for hand in [Care::pet as fn(&mut Care, u64), Care::feed, Care::touch] {
            let mut care = Care::default();
            care.enter_night(10);
            hand(&mut care, 60);
            assert!(care.night.is_none(), "a hand must end the night");
            assert!(!care.asleep);
            assert!(
                care.woken_by_touch,
                "the loop must note a hand, not the morning"
            );
        }
        // A hand that wakes an ordinary doze is not a night wake: there is
        // no "woken_by_touch" to note when no night was running.
        let mut dozy = Care::default();
        dozy.asleep = true;
        dozy.pet(5);
        assert!(!dozy.woken_by_touch);
    }

    #[test]
    fn the_night_round_trips_and_quiet_comes_from_the_tick_delta() {
        let mut care = Care::default();
        care.enter_night(321);
        let saved = care.summary();
        assert_eq!(saved["night"]["active"], serde_json::json!(true));
        assert_eq!(saved["night"]["started_tick"], serde_json::json!(321));
        let mut back = Care::default();
        back.restore(&saved);
        assert_eq!(back.night.map(|night| night.started_tick), Some(321));
        assert!(back.asleep, "a restored night is asleep by construction");
        // The span comes from the tick delta, never from a saved number.
        let quiet = back.end_night(321 + 50).expect("the restored night");
        assert!((quiet - 1.0).abs() < 1e-9);
        // An inactive night in the payload clears one the duck had.
        let mut woken = Care::default();
        woken.enter_night(10);
        woken.restore(&serde_json::json!({
            "night": { "active": false, "started_tick": 0 },
        }));
        assert!(woken.night.is_none());
        // A payload that forgot `asleep` cannot leave a night duck awake.
        let mut forgot = Care::default();
        forgot.restore(&serde_json::json!({
            "asleep": false,
            "night": { "active": true, "started_tick": 7 },
        }));
        assert!(forgot.asleep);
    }

    /// A neighbour's treat fills the duck, and nothing else.
    ///
    /// The night stays: a gift is not a hand, and a hand is what ends a
    /// night. The bond stays: the intimacy is between the duck and the
    /// person holding the phone, not between the duck and a neighbour.
    #[test]
    fn a_treat_from_away_fills_the_duck_without_waking_it() {
        let mut care = Care::default();
        care.enter_night(100);
        let bond = care.bond;
        let hunger = care.hunger;

        care.feed_from_away();

        assert!(care.asleep, "a gift must not end the night");
        assert!(!care.woken_by_touch, "a gift is not a touch");
        assert_eq!(care.bond, bond, "the bond did not move");
        assert!(
            care.hunger < hunger,
            "the duck really ate: {} < {hunger}",
            care.hunger
        );
    }

    /// The same for a hand: a treat in the room is a touch, and it ends the
    /// night. The two paths have to stay apart.
    #[test]
    fn a_treat_from_a_hand_wakes_the_duck_and_raises_the_bond() {
        let mut care = Care::default();
        care.enter_night(100);
        let bond = care.bond;

        care.feed(101);

        assert!(!care.asleep, "a hand ends the night");
        assert!(care.woken_by_touch, "a hand is a touch");
        assert!(care.bond > bond, "the bond moved");
    }
}
