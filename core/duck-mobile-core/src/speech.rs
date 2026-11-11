//! What the duck hears, as verbs.
//!
//! The microphone itself only gives loudness and a pitch estimate (a room
//! reading, never a sense). Speech recognition is the platform's job — iOS
//! `SFSpeechRecognizer` and Android `SpeechRecognizer` both do on-device
//! Chinese — and what they hand back is text. This module turns that text
//! into one of the duck's own verbs, so `"坐下"` and `"sit"` mean the same
//! thing, and the same words work whether they were typed, recognized, or
//! sent by an AI client over `POST /heard`.
//!
//! Matching is deliberately small and predictable: a keyword table, longest
//! phrase first, so `"快速点头"` beats `"点头"` and `"不要"` does not fire
//! `"要"`. It is a voice remote, not a language model — an LLM client that
//! wants to understand sentences should call `/skill` itself.

/// One thing the duck can be asked to do by voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heard {
    Skill(&'static str),
    /// A sound tag rather than a move, for greetings.
    Sound(&'static str),
    Sit,
    Stand,
    GroundPick,
}

/// Longest phrases first: `("快速点头", ...)` must be tested before
/// `("点头", ...)`, and every entry is a phrase a person actually says.
const PHRASES: &[(&str, Heard)] = &[
    // Greetings: the duck answers with its own voice, no move.
    ("你好", Heard::Sound("greet")),
    ("哈喽", Heard::Sound("greet")),
    ("hello", Heard::Sound("greet")),
    ("hi", Heard::Sound("greet")),
    // Emotions, the ones a person would say to a duck.
    ("开心", Heard::Skill("excited")),
    ("高兴", Heard::Skill("excited")),
    ("兴奋", Heard::Skill("excited")),
    ("难过", Heard::Skill("sad")),
    ("伤心", Heard::Skill("sad")),
    ("生气", Heard::Skill("angry")),
    ("愤怒", Heard::Skill("angry")),
    ("好奇", Heard::Skill("curious")),
    ("笑", Heard::Skill("laugh")),
    ("大笑", Heard::Skill("laugh")),
    ("不服", Heard::Skill("defiant")),
    ("崩溃", Heard::Skill("devastated")),
    ("不耐烦", Heard::Skill("impatient")),
    ("嘲弄", Heard::Skill("mock")),
    ("快速点头", Heard::Skill("yes_fast")),
    ("点头", Heard::Skill("yes")),
    ("是的", Heard::Skill("yes")),
    ("好的", Heard::Skill("yes")),
    ("摇头", Heard::Skill("no")),
    ("不要", Heard::Skill("no")),
    ("不是", Heard::Skill("no")),
    ("嗯", Heard::Skill("mmh")),
    // Actions.
    ("翻滚", Heard::Skill("roulade")),
    ("滚", Heard::Skill("roulade")),
    ("左踢", Heard::Skill("kick_left")),
    ("右踢", Heard::Skill("kick_right")),
    ("踢", Heard::Skill("kick_left")),
    ("坐下", Heard::Sit),
    ("坐", Heard::Sit),
    ("站起来", Heard::Stand),
    ("起来", Heard::Stand),
    ("站", Heard::Stand),
    ("捡", Heard::GroundPick),
    ("捡起来", Heard::GroundPick),
    ("抓", Heard::GroundPick),
    // English, because the same words should work either way.
    ("excited", Heard::Skill("excited")),
    ("happy", Heard::Skill("excited")),
    ("sad", Heard::Skill("sad")),
    ("angry", Heard::Skill("angry")),
    ("curious", Heard::Skill("curious")),
    ("laugh", Heard::Skill("laugh")),
    ("yes", Heard::Skill("yes")),
    ("no", Heard::Skill("no")),
    ("roll", Heard::Skill("roulade")),
    ("kick", Heard::Skill("kick_left")),
    ("sit", Heard::Sit),
    ("stand", Heard::Stand),
    ("pick", Heard::GroundPick),
];

/// The verb `text` asks for, if any. Case-insensitive for the English
/// entries; Chinese has no case. The first (longest) match wins.
pub fn interpret(text: &str) -> Option<(&'static str, Heard)> {
    let lowered = text.to_lowercase();
    PHRASES
        .iter()
        .find(|(phrase, _)| lowered.contains(phrase))
        .map(|(phrase, heard)| (*phrase, *heard))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_and_english_name_the_same_verbs() {
        assert_eq!(interpret("坐下").unwrap().1, Heard::Sit);
        assert_eq!(interpret("sit").unwrap().1, Heard::Sit);
        assert_eq!(interpret("翻滚吧").unwrap().1, Heard::Skill("roulade"));
        assert_eq!(interpret("我好开心").unwrap().1, Heard::Skill("excited"));
        assert_eq!(interpret("excited").unwrap().1, Heard::Skill("excited"));
        assert_eq!(interpret("你好呀").unwrap().1, Heard::Sound("greet"));
        assert_eq!(interpret("hello").unwrap().1, Heard::Sound("greet"));
        // Standing up is its own verb: it must never toggle a standing duck
        // back down into the seat.
        assert_eq!(interpret("站起来").unwrap().1, Heard::Stand);
        assert_eq!(interpret("stand").unwrap().1, Heard::Stand);
    }

    #[test]
    fn the_longest_phrase_wins() {
        // "快速点头" must not be read as "点头".
        assert_eq!(interpret("快速点头").unwrap().1, Heard::Skill("yes_fast"));
        // "不要" must not be read as a bare "要" (which is not a phrase).
        assert_eq!(interpret("不要").unwrap().1, Heard::Skill("no"));
    }

    #[test]
    fn silence_and_noise_ask_for_nothing() {
        assert!(interpret("").is_none());
        assert!(interpret("嗯嗯嗯嗯").is_some(), "a hum is mmh, not silence");
        assert!(interpret("qwertyuiop").is_none());
    }
}
