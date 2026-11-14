//! Block rules for kids: "when I clap, roll over".
//!
//! A rule is one trigger and one action, both named with the duck's own
//! vocabulary. The loop executes them; nothing here reaches into a policy or
//! the 61-D observation. A programmed rule takes precedence over the duck's
//! default reaction to the same sound, so the child sees exactly what they
//! built.
//!
//! The set is small (eight rules) and stored as JSON so either shell can
//! render it as blocks and hand it back across launches.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What a child can listen for.
pub const TRIGGERS: [&str; 4] = ["clap", "whistle", "pet", "timer"];

/// What a block can make the duck do.
pub const ACTIONS: [&str; 4] = ["skill", "emotion", "sound", "light"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// "clap", "whistle" or "pet".
    pub trigger: String,
    /// "skill", "emotion" or "sound". The first action; kept for older
    /// saved programs that only know the one-action shape.
    pub action: String,
    /// The skill/emotion/sound name, e.g. "roulade".
    pub value: String,
    /// Optional ordered follow-up actions. A program that has a `then` list
    /// runs `action/value` first, then each entry in order. Absent means the
    /// old one-action rule.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub then: Vec<Action>,
}

/// One block after the first: same action/value pair as a classic rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub action: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Fired {
    pub trigger: String,
    pub action: String,
    pub value: String,
    pub tick: u64,
}

/// The programmed rules and the last one that fired.
#[derive(Debug, Clone, Default)]
pub struct Rules {
    pub rules: Vec<Rule>,
    pub last: Option<Fired>,
    /// Per-timer-rule tick of its last firing, keyed by rule index. The
    /// tick resets this when the program changes, so a new timer is due
    /// immediately rather than inheriting an old clock.
    pub timer_last: std::collections::HashMap<usize, u64>,
}

impl Rules {
    /// A timer rule's period in SIMULATED SECONDS, parsed from its value.
    /// The value is the duck's own clock (20 ms ticks), never a wall clock.
    pub fn timer_period_s(value: &str) -> Option<u32> {
        value.trim().parse::<u32>().ok().filter(|period| *period >= 5 && *period <= 3600)
    }

    /// Replace the set from the wire. Invalid blocks are refused by name
    /// rather than silently dropped: a child's program that did not load must
    /// not look like one that did.
    pub fn set(&mut self, value: &Value) -> Result<(), String> {
        let array = value
            .get("rules")
            .and_then(Value::as_array)
            .ok_or("missing rules")?;
        if array.len() > 8 {
            return Err("at most 8 rules".to_owned());
        }
        let mut rules = Vec::with_capacity(array.len());
        self.timer_last.clear();
        for (index, item) in array.iter().enumerate() {
            let trigger = item
                .get("trigger")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("rule {index}: missing trigger"))?;
            if !TRIGGERS.contains(&trigger) {
                return Err(format!("rule {index}: no such trigger: {trigger}"));
            }
            let action = item
                .get("action")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("rule {index}: missing action"))?;
            if !ACTIONS.contains(&action) {
                return Err(format!("rule {index}: no such action: {action}"));
            }
            let name = item
                .get("value")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("rule {index}: missing value"))?;
            if name.trim().is_empty() {
                return Err(format!("rule {index}: empty value"));
            }
            // A timer rule's value is SECONDS of the duck's own clock: a
            // junk number is refused here, at the door, not discovered by a
            // tick that never fires.
            if trigger == "timer" && Self::timer_period_s(name).is_none() {
                return Err(format!(
                    "rule {index}: a timer needs whole seconds between 5 and 3600"
                ));
            }
            let mut then = Vec::new();
            if let Some(list) = item.get("then") {
                let list = list
                    .as_array()
                    .ok_or_else(|| format!("rule {index}: then must be a list"))?;
                if list.len() > 4 {
                    return Err(format!("rule {index}: at most 4 follow-up actions"));
                }
                for (at, step) in list.iter().enumerate() {
                    let step_action = step
                        .get("action")
                        .and_then(Value::as_str)
                        .ok_or_else(|| format!("rule {index}.{at}: missing action"))?;
                    if !ACTIONS.contains(&step_action) {
                        return Err(format!(
                            "rule {index}.{at}: no such action: {step_action}"
                        ));
                    }
                    let step_value = step
                        .get("value")
                        .and_then(Value::as_str)
                        .ok_or_else(|| format!("rule {index}.{at}: missing value"))?;
                    if step_value.trim().is_empty() {
                        return Err(format!("rule {index}.{at}: empty value"));
                    }
                    then.push(Action {
                        action: step_action.to_owned(),
                        // Trimmed, so the value the core runs is the value
                        // the shells wrote: a program that says " laugh "
                        // must not sit in the book with spaces around it.
                        value: step_value.trim().to_owned(),
                    });
                }
            }
            rules.push(Rule {
                trigger: trigger.to_owned(),
                action: action.to_owned(),
                value: name.trim().to_owned(),
                then,
            });
        }
        self.rules = rules;
        Ok(())
    }

    pub fn matching(&self, trigger: &str) -> Vec<Rule> {
        self.rules
            .iter()
            .filter(|rule| rule.trigger == trigger)
            .cloned()
            .collect()
    }

    pub fn summary(&self) -> Value {
        serde_json::json!({
            "rules": self.rules,
            "triggers": TRIGGERS,
            "actions": ACTIONS,
            "last": self.last,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_step_keeps_the_official_flashlight_vocabulary() {
        let mut rules = Rules::default();
        rules.set(&serde_json::json!({
            "rules": [{
                "trigger": "clap",
                "action": "light",
                "value": "yellow",
                "then": [
                    { "action": "sound", "value": "chirp" },
                    { "action": "light", "value": "off" }
                ]
            }]
        })).expect("a light step with an official colour must load");
        // The word is validated by the runtime against COLORS, not here:
        // an invented colour is still a legal wire shape and must be refused
        // where the lamp lives, with no state changed.
        assert!(ACTIONS.contains(&"light"));
    }

    #[test]
    fn a_well_formed_rule_set_loads_and_matches() {
        let mut rules = Rules::default();
        rules
            .set(&serde_json::json!({
                "rules": [{ "trigger": "clap", "action": "skill", "value": "roulade" }]
            }))
            .unwrap();
        assert_eq!(rules.matching("clap").len(), 1);
        assert!(rules.matching("whistle").is_empty());
    }

    /// The value the core runs is the value the shells wrote.
    ///
    /// The core trims with Rust's `trim()`, which knows the whole Unicode
    /// White_Space set. The shells have to trim the same way: a value that
    /// looked fine on the phone and was empty here would refuse the whole
    /// program — and a refused program is one the duck is not running.
    #[test]
    fn a_value_is_trimmed_the_way_the_shells_trim_it() {
        let mut rules = Rules::default();
        rules
            .set(&serde_json::json!({
                "rules": [{ "trigger": "clap", "action": "skill", "value": "  roulade  " }]
            }))
            .unwrap();
        let matching = rules.matching("clap");
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].value, "roulade");

        // A no-break space is white space for the core, so it is empty.
        let mut rules = Rules::default();
        let error = rules
            .set(&serde_json::json!({
                "rules": [{ "trigger": "clap", "action": "skill", "value": "\u{00a0}" }]
            }))
            .unwrap_err();
        assert!(error.contains("empty value"), "{error}");

        // And so is a newline, which the iOS shell used to accept.
        let mut rules = Rules::default();
        let error = rules
            .set(&serde_json::json!({
                "rules": [{ "trigger": "clap", "action": "skill", "value": "\n" }]
            }))
            .unwrap_err();
        assert!(error.contains("empty value"), "{error}");
    }

    #[test]
    fn a_block_with_an_unknown_trigger_is_refused() {
        let mut rules = Rules::default();
        let error = rules
            .set(&serde_json::json!({
                "rules": [{ "trigger": "explode", "action": "skill", "value": "roulade" }]
            }))
            .unwrap_err();
        assert!(error.contains("no such trigger"));
    }

    #[test]
    fn a_rule_can_carry_a_short_ordered_chain() {
        let mut rules = Rules::default();
        rules
            .set(&serde_json::json!({
                "rules": [{
                    "trigger": "clap",
                    "action": "skill",
                    "value": "roulade",
                    "then": [
                        { "action": "emotion", "value": "laugh" },
                        { "action": "sound", "value": "chirp" }
                    ]
                }]
            }))
            .unwrap();
        let rule = &rules.rules[0];
        assert_eq!(rule.then.len(), 2);
        assert_eq!(rule.then[1].value, "chirp");
        // The old single-action payload still loads.
        rules
            .set(&serde_json::json!({
                "rules": [{ "trigger": "pet", "action": "sound", "value": "coo" }]
            }))
            .unwrap();
        assert!(rules.rules[0].then.is_empty());
    }
}
