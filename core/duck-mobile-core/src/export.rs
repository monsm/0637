//! The vet book as a document a person can keep.
//!
//! `/life` publishes the counters as JSON for clients. This module turns the
//! same facts into the two shapes a share sheet needs: a canonical JSON
//! document and a short bilingual Markdown page. It invents no number: every
//! line here comes from [`crate::vetbook::Life`] or [`crate::care::Care`].

use crate::care::Care;
use crate::vetbook::Life;

/// The version of the exported document shape. A later reader can tell an
/// old file from a new one without guessing.
pub const FORMAT: &str = "microduck-vetbook/1";

fn rolls(life: &Life) -> u64 {
    // A roll is the official `roulade` skill. Older books counted it under
    // the same wire name, so one place, no alias table.
    life.skills.get("roulade").copied().unwrap_or(0)
}

/// The canonical document. `fingerprint` is the build's provenance hash, so
/// a shared record can be tied to the exact policies and scene that made it.
pub fn json(life: &Life, care: &Care, fingerprint: &str) -> serde_json::Value {
    serde_json::json!({
        "format": FORMAT,
        "build": { "fingerprint": fingerprint },
        "care": care.summary(),
        "life": life,
        "highlights": {
            "metres": life.metres,
            "rolls": rolls(life),
            "pickups": life.pickups,
            "falls": life.falls,
            "pets": life.pets,
            "fed": life.fed,
            "claps": life.claps,
            "whistles": life.whistles,
        },
    })
}

/// A short page a human reads. Both languages on purpose: the duck is the
/// same animal whichever language the app was left in.
pub fn markdown(life: &Life, care: &Care, fingerprint: &str) -> String {
    let hours = life.powered_s / 3600.0;
    let fullness = ((1.0 - care.hunger) * 100.0).clamp(0.0, 100.0);
    let mood = ((care.mood + 1.0) / 2.0 * 100.0).clamp(0.0, 100.0);
    let mut out = String::new();
    out.push_str("# MicroDuck · 终身记录 / Vet book\n\n");
    out.push_str(&format!(
        "走过 / walked **{:.1} m** · 翻滚 / rolls **{}** · 被抱起 / picked up **{}**\n\n",
        life.metres,
        rolls(life),
        life.pickups,
    ));
    out.push_str("| | |\n|---|---|\n");
    out.push_str(&format!("| 陪伴 / powered | {:.2} h |\n", hours));
    out.push_str(&format!("| 行走 / walking | {:.0} s |\n", life.walking_s));
    out.push_str(&format!("| 站着 / standing | {:.0} s |\n", life.standing_s));
    out.push_str(&format!("| 坐下 / sitting | {:.0} s |\n", life.sitting_s));
    out.push_str(&format!("| 在手里 / held | {:.0} s |\n", life.held_s));
    out.push_str(&format!("| 启动 / launches | {} |\n", life.boots));
    out.push_str(&format!("| 摔倒 / falls | {} |\n", life.falls));
    out.push_str(&format!("| 贴地啄 / ground picks | {} |\n", life.ground_picks));
    out.push_str(&format!("| 摸 / petted | {} |\n", life.pets));
    out.push_str(&format!("| 喂食 / fed | {} |\n", life.fed));
    out.push_str(&format!("| 拍手 / claps | {} |\n", life.claps));
    out.push_str(&format!("| 口哨 / whistles | {} |\n", life.whistles));
    out.push_str(&format!("| 关机 / power offs | {} |\n", life.power_offs));
    out.push_str(&format!(
        "| 饱腹 / fullness | {:.0}% |\n",
        fullness
    ));
    out.push_str(&format!("| 心情 / mood | {:.0}% |\n", mood));
    out.push_str(&format!("| 亲密 / bond | {:.0}% |\n", care.bond * 100.0));
    if care.asleep {
        out.push_str("| 状态 / state | 打盹中 / dozing |\n");
    }
    if !life.skills.is_empty() {
        out.push_str("\n## 技能 / skills\n\n");
        for (name, count) in &life.skills {
            out.push_str(&format!("- `{name}` × {count}\n"));
        }
    }
    if !life.emotions.is_empty() {
        out.push_str("\n## 情绪 / emotions\n\n");
        for (name, count) in &life.emotions {
            out.push_str(&format!("- `{name}` × {count}\n"));
        }
    }
    out.push_str(&format!(
        "\n---\nbuild: `{}`\n",
        if fingerprint.is_empty() { "unknown" } else { fingerprint }
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn life() -> Life {
        let mut life = Life::default();
        life.metres = 123.4;
        life.pickups = 5;
        life.skills.insert("roulade".to_owned(), 3);
        life.emotions.insert("sad".to_owned(), 1);
        life
    }

    #[test]
    fn the_document_states_the_three_headline_numbers() {
        let care = Care::default();
        let text = markdown(&life(), &care, "abc");
        assert!(text.contains("123.4 m"));
        assert!(text.contains("rolls **3**"));
        assert!(text.contains("picked up **5**"));
        assert!(text.contains("build: `abc`"));
    }

    #[test]
    fn the_json_names_the_format_and_the_same_numbers() {
        let care = Care::default();
        let value = json(&life(), &care, "abc");
        assert_eq!(value["format"], FORMAT);
        assert_eq!(value["highlights"]["rolls"], 3);
        assert_eq!(value["highlights"]["pickups"], 5);
        assert_eq!(value["care"]["bond"], care.bond);
    }
}
