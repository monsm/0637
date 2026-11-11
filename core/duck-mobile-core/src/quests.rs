//! Daily quests: three tiny goals from the duck's real counters.
//!
//! Nothing here adds a fake number. A quest measures a delta of the vet
//! book's own counts (or the care state), from the moment the day started.
//! The day is the *phone's* calendar day: a companion ritual belongs to the
//! person, not to the simulated clock. Completion is an event for the loop to
//! celebrate with one of its official emotions.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// What a quest measures. The counter names are the vet book's wire names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Metric {
    Metres,
    Rolls,
    Pets,
    Pickups,
    Fed,
    Claps,
    Whistles,
    Emotions,
    Skills,
    GroundPicks,
    Bond,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quest {
    pub id: String,
    pub metric: Metric,
    pub target: f64,
    pub title_en: String,
    pub title_zh: String,
    /// Counter value when this day's quests were chosen.
    pub baseline: f64,
    pub done: bool,
    pub celebrated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct QuestBoard {
    pub day: String,
    pub quests: Vec<Quest>,
    pub streak: u32,
    pub last_completed_day: String,
    pub best_streak: u32,
    pub completed_today: u32,
}

const BLUEPRINTS: &[(&str, Metric, f64, &str, &str)] = &[
    ("walk", Metric::Metres, 2.0, "Walk 2 m together", "一起走 2 米"),
    ("roll", Metric::Rolls, 2.0, "Roll over twice", "翻两个滚"),
    ("pet", Metric::Pets, 3.0, "Pet the duck 3 times", "摸摸鸭子 3 次"),
    ("hold", Metric::Pickups, 1.0, "Pick the duck up", "把鸭子抱起来"),
    ("feed", Metric::Fed, 1.0, "Give one treat", "喂一次零食"),
    ("clap", Metric::Claps, 2.0, "Clap twice", "拍手两次"),
    ("whistle", Metric::Whistles, 1.0, "Whistle once", "吹一次口哨"),
    ("moods", Metric::Emotions, 2.0, "Show 2 moods", "做 2 个表情"),
    ("skills", Metric::Skills, 2.0, "Do 2 moves", "做 2 个动作"),
    ("peck", Metric::GroundPicks, 2.0, "Peck twice", "贴地啄两次"),
    ("bond", Metric::Bond, 0.05, "Grow the bond", "亲密值涨一点"),
];

impl QuestBoard {
    /// Choose three quests deterministically for a calendar day, using the
    /// day's digits as a stable rotation. Same day, same quests after a
    /// restart; a new day gets a new set.
    pub fn roll_day(&mut self, day: &str, values: &Value) {
        if self.day == day && !self.quests.is_empty() {
            return;
        }
        // A streak only survives consecutive days with all three done. It is
        // a gentle nudge, not a punishment: a missed day simply starts at 1.
        let seed: u64 = day
            .bytes()
            .fold(0u64, |acc, byte| acc.wrapping_mul(31).wrapping_add(byte as u64));
        let mut offset = (seed % BLUEPRINTS.len() as u64) as usize;
        let mut ids = Vec::with_capacity(3);
        let mut quests = Vec::with_capacity(3);
        while quests.len() < 3 {
            let (id, metric, target, en, zh) = BLUEPRINTS[offset % BLUEPRINTS.len()];
            offset += 1;
            if ids.contains(&id) {
                continue;
            }
            ids.push(id);
            let baseline = metric_value(metric, values);
            quests.push(Quest {
                id: (*id).to_owned(),
                metric,
                target,
                title_en: (*en).to_owned(),
                title_zh: (*zh).to_owned(),
                baseline,
                done: false,
                celebrated: false,
            });
        }
        if self.last_completed_day != previous_day(day) {
            self.streak = 0;
        }
        self.day = day.to_owned();
        self.quests = quests;
        self.completed_today = 0;
    }

    /// Update from the published /state. Returns the ids that just finished.
    pub fn update(&mut self, day: &str, values: &Value) -> Vec<String> {
        if self.day != day || self.quests.is_empty() {
            if !day.is_empty() && self.last_completed_day != day {
                let previous = &self.day;
                if !previous.is_empty() && previous != day && previous_day(day) != *previous {
                    self.streak = 0;
                }
            }
            self.roll_day(day, values);
        }
        let mut finished = Vec::new();
        for quest in &mut self.quests {
            if quest.done {
                continue;
            }
            let now = if quest.metric == Metric::Bond {
                values.get("care").and_then(|c| c.get("bond")).and_then(Value::as_f64).unwrap_or(0.0)
            } else {
                metric_value(quest.metric, values)
            };
            if now < quest.baseline {
                // A restored life or a new branch can move a counter back.
                // Start the day's measurement from the new value rather than
                // showing a negative or a stale goal.
                quest.baseline = now;
            }
            if now - quest.baseline >= quest.target {
                quest.done = true;
                quest.celebrated = true;
                self.completed_today = self.completed_today.saturating_add(1);
                finished.push(quest.id.clone());
            }
        }
        if self.completed_today >= 3 && self.last_completed_day != day {
            self.last_completed_day = day.to_owned();
            self.streak = self.streak.saturating_add(1);
            self.best_streak = self.best_streak.max(self.streak);
        }
        finished
    }

    pub fn progress(&self, values: &Value) -> Vec<f64> {
        self.quests
            .iter()
            .map(|quest| {
                if quest.done {
                    return 1.0;
                }
                let now = if quest.metric == Metric::Bond {
                    values.get("care").and_then(|c| c.get("bond")).and_then(Value::as_f64).unwrap_or(0.0)
                } else {
                    metric_value(quest.metric, values)
                };
                ((now - quest.baseline) / quest.target).clamp(0.0, 1.0)
            })
            .collect()
    }

    pub fn summary(&self, values: &Value) -> Value {
        let progress = self.progress(values);
        let quests: Vec<Value> = self
            .quests
            .iter()
            .zip(progress.iter())
            .map(|(quest, fraction)| {
                json!({
                    "id": quest.id,
                    "title_en": quest.title_en,
                    "title_zh": quest.title_zh,
                    "target": quest.target,
                    "done": quest.done,
                    "progress": fraction,
                })
            })
            .collect();
        json!({
            "day": self.day,
            "quests": quests,
            "streak": self.streak,
            "best_streak": self.best_streak,
            "completed_today": self.completed_today,
        })
    }

    /// A shell-saved board, clamped: a corrupt file cannot invent a streak.
    pub fn restore(&mut self, saved: &Value) {
        if let Ok(board) = serde_json::from_value::<QuestBoard>(saved.clone()) {
            if board.day.len() == 10 && board.quests.len() <= 3 {
                *self = board;
                self.streak = self.streak.min(3650);
                self.best_streak = self.best_streak.min(3650);
                self.completed_today = self.completed_today.min(3);
            }
        }
    }

    pub fn json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }

    pub fn completed(&self) -> bool {
        self.completed_today >= 3
    }
}

fn metric_value(metric: Metric, values: &Value) -> f64 {
    let life = values.get("life").unwrap_or(&Value::Null);
    let number = |key: &str| -> f64 { life.get(key).and_then(Value::as_f64).unwrap_or(0.0) };
    let skills = || -> f64 {
        life.get("skills")
            .and_then(Value::as_object)
            .map(|map| map.values().filter_map(Value::as_f64).sum())
            .unwrap_or(0.0)
    };
    let emotions = || -> f64 {
        life.get("emotions")
            .and_then(Value::as_object)
            .map(|map| map.values().filter_map(Value::as_f64).sum())
            .unwrap_or(0.0)
    };
    match metric {
        Metric::Metres => number("metres"),
        Metric::Rolls => life.get("skills").and_then(|s| s.get("roulade")).and_then(Value::as_f64).unwrap_or(0.0),
        Metric::Pets => number("pets"),
        Metric::Pickups => number("pickups"),
        Metric::Fed => number("fed"),
        Metric::Claps => number("claps"),
        Metric::Whistles => number("whistles"),
        Metric::Emotions => emotions(),
        Metric::Skills => skills(),
        Metric::GroundPicks => number("ground_picks"),
        Metric::Bond => values.get("care").and_then(|c| c.get("bond")).and_then(Value::as_f64).unwrap_or(0.0),
    }
}

fn previous_day(day: &str) -> String {
    // A conservative approximation: any board whose last completed day is
    // not exactly yesterday loses its streak. Date arithmetic is the shell's
    // calendar's job; it passes ISO yyyy-mm-dd here.
    let mut parts: Vec<i32> = day
        .split('-')
        .filter_map(|part| part.parse().ok())
        .collect();
    if parts.len() != 3 {
        return String::new();
    }
    parts[2] -= 1;
    if parts[2] < 1 {
        parts[1] -= 1;
        if parts[1] < 1 {
            parts[0] -= 1;
            parts[1] = 12;
        }
        parts[2] = 30;
    }
    format!("{:04}-{:02}-{:02}", parts[0], parts[1], parts[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(pets: u64, rolls: u64, metres: f64) -> Value {
        json!({
            "care": { "bond": 0.1 },
            "life": {
                "pets": pets, "metres": metres,
                "skills": { "roulade": rolls },
            },
        })
    }

    #[test]
    fn a_day_picks_three_distinct_quests_and_is_stable() {
        let mut board = QuestBoard::default();
        board.roll_day("2026-11-11", &state(0, 0, 0.0));
        assert_eq!(board.quests.len(), 3);
        let ids: Vec<&str> = board.quests.iter().map(|quest| quest.id.as_str()).collect();
        assert_eq!(ids.iter().collect::<std::collections::HashSet<_>>().len(), 3);
        let clone = board.quests.clone();
        board.roll_day("2026-11-11", &state(5, 5, 10.0));
        assert_eq!(board.quests, clone, "a restart keeps the same goals");
    }

    #[test]
    fn a_completed_quest_fires_once_and_celebrates_all_three() {
        let mut board = QuestBoard::default();
        board.roll_day("2026-11-11", &state(0, 0, 0.0));
        // Force all three quests to use pets so one value can finish them.
        for quest in &mut board.quests {
            quest.metric = Metric::Pets;
            quest.target = 1.0;
            quest.baseline = 0.0;
        }
        let finished = board.update("2026-11-11", &state(3, 0, 0.0));
        assert_eq!(finished.len(), 3);
        assert_eq!(board.completed_today, 3);
        assert_eq!(board.streak, 1);
        assert!(board.update("2026-11-11", &state(9, 0, 0.0)).is_empty());
    }
}
