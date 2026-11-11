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
            "last_completed_day": self.last_completed_day,
        })
    }

    /// A shell-saved board, clamped: a corrupt file cannot invent a streak.
    ///
    /// Shells save what /state publishes — the summary shape, without a
    /// quest's metric or baseline — so the summary is rehydrated: the metric
    /// comes from the quest's blueprint id and the baseline is rebuilt from
    /// the saved progress fraction against the live counters. A board that
    /// claims impossible goals (a target of zero completes itself) or
    /// bookkeeping that disagrees with itself is refused wholesale.
    pub fn restore(&mut self, saved: &Value, values: &Value) {
        let day = saved.get("day").and_then(Value::as_str).unwrap_or("");
        if day.len() != 10 || !is_plausible_day(day) {
            return;
        }
        let saved_quests = match saved.get("quests").and_then(Value::as_array) {
            Some(quests) if !quests.is_empty() && quests.len() <= 3 => quests,
            _ => return,
        };
        let mut quests = Vec::with_capacity(saved_quests.len());
        for saved in saved_quests {
            let id = saved.get("id").and_then(Value::as_str).unwrap_or("");
            let target = saved.get("target").and_then(Value::as_f64).unwrap_or(0.0);
            if !target.is_finite() || target <= 0.0 {
                return;
            }
            let Some((_, metric, _, title_en, title_zh)) =
                BLUEPRINTS.iter().find(|(blueprint_id, ..)| *blueprint_id == id)
            else {
                return;
            };
            // A full board carries its own baseline; a summary board is
            // rehydrated from the progress fraction the shell saw.
            let mut baseline = saved
                .get("baseline")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite() && *value >= 0.0);
            if baseline.is_none() {
                let progress = saved
                    .get("progress")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite())
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                baseline = Some((metric_value(*metric, values) - progress * target).max(0.0));
            }
            let done = saved.get("done").and_then(Value::as_bool).unwrap_or(false);
            quests.push(Quest {
                id: id.to_owned(),
                metric: *metric,
                target,
                title_en: saved
                    .get("title_en")
                    .and_then(Value::as_str)
                    .unwrap_or(title_en)
                    .to_owned(),
                title_zh: saved
                    .get("title_zh")
                    .and_then(Value::as_str)
                    .unwrap_or(title_zh)
                    .to_owned(),
                baseline: baseline.unwrap_or(0.0),
                done,
                celebrated: done,
            });
        }
        // The bookkeeping must agree with itself: completions come from the
        // quests, a claimed last-completed day is a real past date, and a
        // streak without one is nothing.
        let completed = quests.iter().filter(|quest| quest.done).count() as u32;
        let mut streak = saved
            .get("streak")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(3650) as u32;
        let mut last = saved
            .get("last_completed_day")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if (completed < 3 && last == day) || (!last.is_empty() && !is_plausible_day(&last))
            || last.as_str() > day
        {
            last.clear();
            streak = 0;
        }
        // Older saves (the summary shape) do not carry the last-completed
        // day. Rebuild the only value consistent with the rest: all three
        // finished means today, and a live streak was completed no earlier
        // than yesterday — which is exactly the day a next-day roll compares.
        if last.is_empty() && streak > 0 {
            last = if completed >= 3 {
                day.to_owned()
            } else {
                previous_day(day)
            };
        }
        let best_saved = saved
            .get("best_streak")
            .and_then(Value::as_u64)
            .unwrap_or(streak as u64)
            .min(3650) as u32;
        let best = best_saved.max(streak);
        self.day = day.to_owned();
        self.quests = quests;
        self.streak = streak;
        self.best_streak = best;
        self.last_completed_day = last;
        self.completed_today = completed.min(3);
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

fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_plausible_day(day: &str) -> bool {
    let parts: Vec<i32> = day.split('-').filter_map(|part| part.parse().ok()).collect();
    if parts.len() != 3 {
        return false;
    }
    let (year, month, day_num) = (parts[0], parts[1], parts[2]);
    day_num >= 1 && day_num <= days_in_month(year, month)
}

fn previous_day(day: &str) -> String {
    // The day before an ISO yyyy-mm-dd: real month lengths, leap years and
    // year rolls. A streak must survive the first of every month.
    let parts: Vec<i32> = day.split('-').filter_map(|part| part.parse().ok()).collect();
    if parts.len() != 3 || !is_plausible_day(day) {
        return String::new();
    }
    let (mut year, mut month, day_num) = (parts[0], parts[1], parts[2]);
    let mut previous = day_num - 1;
    if previous < 1 {
        month -= 1;
        if month < 1 {
            month = 12;
            year -= 1;
        }
        previous = days_in_month(year, month);
    }
    format!("{:04}-{:02}-{:02}", year, month, previous)
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

    #[test]
    fn yesterday_is_the_real_calendar_day() {
        assert_eq!(previous_day("2026-11-12"), "2026-11-11");
        assert_eq!(previous_day("2026-03-01"), "2026-02-28");
        assert_eq!(previous_day("2026-01-01"), "2025-12-31");
        assert_eq!(previous_day("2026-04-01"), "2026-03-31");
        assert_eq!(previous_day("2026-08-01"), "2026-07-31");
        assert_eq!(previous_day("2024-03-01"), "2024-02-29", "leap February");
        assert_eq!(previous_day("2026-02-30"), "", "impossible day");
    }

    #[test]
    fn a_streak_survives_a_month_boundary() {
        let mut board = QuestBoard::default();
        board.day = "2026-02-28".to_owned();
        board.last_completed_day = "2026-02-28".to_owned();
        board.streak = 3;
        board.best_streak = 3;
        // The next calendar day is the first of March, not February 30th.
        board.update("2026-03-01", &state(0, 0, 0.0));
        assert_eq!(board.day, "2026-03-01");
        assert_eq!(board.streak, 3, "February 28th's work carries over");
        // Finishing the new day extends the streak.
        for quest in &mut board.quests {
            quest.metric = Metric::Pets;
            quest.target = 1.0;
            quest.baseline = 0.0;
        }
        board.update("2026-03-01", &state(3, 0, 0.0));
        assert_eq!(board.streak, 4);
    }

    #[test]
    fn a_summary_board_rehydrates_with_its_progress() {
        let mut board = QuestBoard::default();
        // What a shell saves: /state.quests' summary, no metric or baseline.
        let summary = json!({
            "day": "2026-11-12",
            "quests": [
                { "id": "pet", "title_en": "Pet the duck 3 times", "title_zh": "摸摸鸭子 3 次",
                  "target": 3.0, "done": false, "progress": 2.0 / 3.0 },
                { "id": "roll", "title_en": "Roll over twice", "title_zh": "翻两个滚",
                  "target": 2.0, "done": true, "progress": 1.0 },
            ],
            "streak": 2,
            "best_streak": 4,
            "completed_today": 0,
        });
        let values = state(2, 0, 0.0); // two pets done so far today
        board.restore(&summary, &values);
        assert_eq!(board.day, "2026-11-12");
        assert_eq!(board.quests.len(), 2);
        assert_eq!(board.streak, 2);
        assert_eq!(board.best_streak, 4);
        let pet = board.quests.iter().find(|quest| quest.id == "pet").unwrap();
        assert_eq!(pet.metric, Metric::Pets);
        assert!((pet.baseline - 0.0).abs() < 1e-9, "progress 2/3 of 3 = the 2 pets already done");
        let roll = board.quests.iter().find(|quest| quest.id == "roll").unwrap();
        assert!(roll.done && roll.celebrated, "a done quest does not celebrate twice");
    }

    #[test]
    fn a_corrupt_board_cannot_invent_a_streak() {
        // A target of zero would complete itself on the next tick.
        let zero = json!({
            "day": "2026-11-12",
            "quests": [
                { "id": "pet", "target": 0.0, "done": false, "progress": 0.0 },
                { "id": "roll", "target": 2.0, "done": false, "progress": 0.0 },
            ],
            "streak": 0,
        });
        let mut board = QuestBoard::default();
        board.restore(&zero, &state(0, 0, 0.0));
        assert!(board.quests.is_empty(), "a zero target is not a goal");
        // Completions the bookkeeping disagrees with.
        let liar = json!({
            "day": "2026-11-12",
            "quests": [
                { "id": "pet", "target": 3.0, "done": false, "progress": 0.5 },
                { "id": "roll", "target": 2.0, "done": false, "progress": 0.0 },
            ],
            "streak": 9,
            "last_completed_day": "2026-11-12",
            "completed_today": 3,
        });
        let mut board = QuestBoard::default();
        board.restore(&liar, &state(0, 0, 0.0));
        assert_eq!(board.streak, 0, "two unfinished quests cannot claim today");
        assert_eq!(board.completed_today, 0);
        // A last-completed day that never existed.
        let phantom = json!({
            "day": "2026-11-12",
            "quests": [{ "id": "pet", "target": 3.0, "done": false, "progress": 0.0 }],
            "streak": 5,
            "last_completed_day": "2026-02-30",
        });
        let mut board = QuestBoard::default();
        board.restore(&phantom, &state(0, 0, 0.0));
        assert_eq!(board.streak, 0);
        assert!(board.last_completed_day.is_empty());
        // An unknown quest id cannot be measured.
        let stranger = json!({
            "day": "2026-11-12",
            "quests": [{ "id": "fly", "target": 3.0, "done": false, "progress": 0.0 }],
        });
        let mut board = QuestBoard::default();
        board.restore(&stranger, &state(0, 0, 0.0));
        assert!(board.quests.is_empty());
    }
}
