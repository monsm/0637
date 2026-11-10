//! The duck's lifetime record — the vet book, in a phone.
//!
//! `robotd/src/vetbook.rs` keeps a JSON file at `/var/lib/robot/vetbook.json`
//! with the robot's whole life in it: seconds powered, seconds walking, falls,
//! pickups, one-shots by name. A phone has no such path, but the *counters*
//! are the duck's history and they are what makes it feel like the same animal
//! over time rather than a fresh model each launch.
//!
//! This is the same set of facts, counted the same way — a tick adds to a
//! tracker the loop owns outright, never a lock — and published in `/state`
//! and over MCP. The upstream file's persistence, wear tables and servo
//! identity records are a board's business, not a phone's.
//!
//! The counts survive a restart of the app because the app saves them (the
//! shells keep them in their own storage); the core hands them out and
//! takes them back.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The lifetime totals, mirroring upstream's `duck_ipc_proto::vetbook::Life`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Life {
    /// The app running, as near to "switched on" as software can see.
    pub powered_s: f64,
    /// Torque on: the joints holding the duck up.
    pub torqued_s: f64,
    pub walking_s: f64,
    pub standing_s: f64,
    pub sitting_s: f64,
    /// Paused by the pickup detector: somebody is holding the duck.
    pub held_s: f64,
    /// Board boots and daemon starts: here, app launches.
    pub boots: u64,
    pub starts: u64,
    /// Falls: the fall verdict going up.
    pub falls: u64,
    /// Falls the limp-fall sequence rode down.
    pub soft_landings: u64,
    /// Times a hand picked the duck up.
    pub pickups: u64,
    pub ground_picks: u64,
    pub sit_downs: u64,
    pub stand_ups: u64,
    /// Bring-ups to the home pose.
    pub wake_ups: u64,
    /// Times somebody petted it.
    pub pets: u64,
    /// One-shot skills, by name: rolls, kicks, emotions.
    pub skills: BTreeMap<String, u64>,
    /// Emotions played, by name.
    pub emotions: BTreeMap<String, u64>,
    /// Metres walked, from the contact odometry.
    pub metres: f64,
    /// Sit-then-power-off sequences that ran to the end.
    pub power_offs: u64,
}

/// What the loop saw this tick, and what it adds to the book.
#[derive(Debug, Clone, Copy, Default)]
pub struct Facts {
    pub powered: bool,
    pub torqued: bool,
    pub walking: bool,
    pub standing: bool,
    pub sitting: bool,
    pub held: bool,
    pub dt: f64,
}

/// The book itself: the totals plus the events a client can watch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Book {
    pub life: Life,
    /// The last few things that happened, newest last. Capped, because a
    /// phone's storage is not a robot's.
    pub events: Vec<String>,
}

/// How many events are kept.
const MAX_EVENTS: usize = 40;

impl Book {
    /// One tick's facts. The loop calls this every tick: no lock, no
    /// allocation beyond the counters themselves.
    pub fn tick(&mut self, facts: Facts) {
        let dt = facts.dt;
        if facts.powered {
            self.life.powered_s += dt;
        }
        if facts.torqued {
            self.life.torqued_s += dt;
        }
        if facts.walking {
            self.life.walking_s += dt;
        }
        if facts.standing {
            self.life.standing_s += dt;
        }
        if facts.sitting {
            self.life.sitting_s += dt;
        }
        if facts.held {
            self.life.held_s += dt;
        }
    }

    /// Count a skill, by name.
    pub fn skill(&mut self, name: &str) {
        *self.life.skills.entry(name.to_owned()).or_insert(0) += 1;
        self.note(format!("skill:{name}"));
    }

    /// Count an emotion, by name.
    pub fn emotion(&mut self, name: &str) {
        *self.life.emotions.entry(name.to_owned()).or_insert(0) += 1;
        self.note(format!("emotion:{name}"));
    }

    /// A dated moment, capped so the list cannot grow without bound.
    pub fn note(&mut self, text: String) {
        self.events.push(text);
        if self.events.len() > MAX_EVENTS {
            let excess = self.events.len() - MAX_EVENTS;
            self.events.drain(0..excess);
        }
    }

    /// The top of the book, for a client that wants the story.
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "powered_s": self.life.powered_s,
            "torqued_s": self.life.torqued_s,
            "walking_s": self.life.walking_s,
            "standing_s": self.life.standing_s,
            "sitting_s": self.life.sitting_s,
            "held_s": self.life.held_s,
            "boots": self.life.boots,
            "falls": self.life.falls,
            "soft_landings": self.life.soft_landings,
            "pickups": self.life.pickups,
            "ground_picks": self.life.ground_picks,
            "sit_downs": self.life.sit_downs,
            "stand_ups": self.life.stand_ups,
            "wake_ups": self.life.wake_ups,
            "pets": self.life.pets,
            "metres": self.life.metres,
            "power_offs": self.life.power_offs,
            "skills": self.life.skills,
            "emotions": self.life.emotions,
            "events": self.events,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tick_adds_the_seconds_the_duck_was_doing_them() {
        let mut book = Book::default();
        book.tick(Facts {
            powered: true,
            torqued: true,
            walking: true,
            standing: false,
            sitting: false,
            held: false,
            dt: 0.02,
        });
        assert!((book.life.powered_s - 0.02).abs() < 1e-9);
        assert!((book.life.walking_s - 0.02).abs() < 1e-9);
        assert_eq!(book.life.standing_s, 0.0);
    }

    #[test]
    fn the_event_list_is_capped() {
        let mut book = Book::default();
        for _ in 0..(MAX_EVENTS + 10) {
            book.note("skill:roulade".to_owned());
        }
        assert_eq!(book.events.len(), MAX_EVENTS);
    }

    #[test]
    fn skills_are_counted_by_name() {
        let mut book = Book::default();
        book.skill("roulade");
        book.skill("roulade");
        book.emotion("curious");
        assert_eq!(book.life.skills.get("roulade"), Some(&2));
        assert_eq!(book.life.emotions.get("curious"), Some(&1));
    }
}
