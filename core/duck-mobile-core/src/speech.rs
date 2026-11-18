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
//! Matching is deliberately small and predictable: a keyword table where
//! the longest contained phrase wins, so `"快速点头"` beats `"点头"` and
//! `"不要"` does not fire `"要"`. It is a voice remote, not a language
//! model — an LLM client that wants to understand sentences should call
//! `/skill` itself.
//!
//! A person can also teach the duck their own words ([`VoiceAliases`]): a
//! phrase maps to a verb the duck already has. Matching is one global
//! longest-phrase contest across both tables: a taught word wins when its
//! phrase is at least as long as the longest built-in phrase the text also
//! contains, and a tie goes to the taught word — so a short alias like
//! 「站」 cannot hijack the built-in 「站起来」. Taught words only affect
//! *hearing*: what a word does still runs through the same execution path
//! as the built-in phrases.

use std::collections::BTreeMap;

/// How many words a person may teach at once. One page of them, not a
/// dictionary: the table travels in one `/config` object.
pub const MAX_ALIASES: usize = 64;

/// The longest taught phrase: a word or a short call, not a sentence.
///
/// The contract is **24 Unicode scalar values** — Rust `chars().count()`,
/// the unit this core counts — not bytes and not grapheme clusters: a ZWJ
/// emoji counts once per scalar, and the iOS and Android shells count the
/// same unit, so a phrase one end accepts the other cannot refuse.
pub const MAX_ALIAS_CHARS: usize = 24;

/// The words a person taught the duck: phrase -> the verb it names.
///
/// The table is the client's whole list, replaced as one object on every
/// save (an empty object clears it). Phrases are trimmed and lowercased on
/// insert — matching is case-insensitive, the same reading [`interpret`]
/// gives the built-in table. The queue that stores it checks every target
/// with [`target_is_valid`] before the table goes in, so a taught word can
/// never invent a move this duck does not have.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoiceAliases {
    map: BTreeMap<String, String>,
}

impl VoiceAliases {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a table from a client's pairs.
    ///
    /// A phrase that is empty after trimming, longer than
    /// [`MAX_ALIAS_CHARS`] (Unicode scalars, the unit that constant
    /// promises), or one of more than [`MAX_ALIASES`] distinct phrases
    /// refuses the whole table with the phrase named. When the same
    /// (trimmed, lowercased) phrase appears twice, the later spelling
    /// replaces the earlier: the client's list is whole state, not a patch.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (String, String)>) -> Result<Self, String> {
        let mut map = BTreeMap::new();
        for (phrase, target) in pairs {
            let phrase = phrase.trim().to_lowercase();
            if phrase.is_empty() {
                return Err("a voice alias phrase must not be empty".to_owned());
            }
            let length = phrase.chars().count();
            if length > MAX_ALIAS_CHARS {
                return Err(format!(
                    "voice alias {phrase:?} is {length} characters; the limit is {MAX_ALIAS_CHARS}"
                ));
            }
            map.insert(phrase, target.trim().to_owned());
        }
        if map.len() > MAX_ALIASES {
            return Err(format!(
                "at most {MAX_ALIASES} voice aliases; got {}",
                map.len()
            ));
        }
        Ok(Self { map })
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// The target taught for `phrase` (already trimmed and lowercased).
    pub fn get(&self, phrase: &str) -> Option<&str> {
        self.map.get(phrase).map(String::as_str)
    }

    /// `(phrase, target)` as stored, in the map's own order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.map
            .iter()
            .map(|(phrase, target)| (phrase.as_str(), target.as_str()))
    }

    /// The longest taught phrase contained in `lowered`, with its target.
    ///
    /// `BTreeMap` order is lexicographic, not longest-first, so the length
    /// comparison is explicit. A tie keeps the lexicographically first
    /// phrase: the answer never depends on insertion history.
    fn longest(&self, lowered: &str) -> Option<(&str, &str)> {
        let mut best: Option<(&str, &str)> = None;
        for (phrase, target) in self.iter() {
            if !lowered.contains(phrase) {
                continue;
            }
            if best.is_none_or(|(found, _)| phrase.chars().count() > found.chars().count()) {
                best = Some((phrase, target));
            }
        }
        best
    }
}

/// Whether `target` names something this duck really has: one of the loop's
/// own verbs (`sit`, `stand`, `ground_pick`), a built-in sound tag, a loaded
/// skill, or a loaded emotion.
///
/// The check runs when the table is set, against the snapshots the core last
/// published, so a taught word is refused at the door rather than failing
/// later as an invented move.
pub fn target_is_valid(target: &str, skills: &[String], emotions: &[String]) -> bool {
    matches!(target, "sit" | "stand" | "ground_pick")
        || crate::voice::TAGS.contains(&target)
        || skills.iter().any(|name| name == target)
        || emotions.iter().any(|name| name == target)
}

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

/// A heard verb that owns its name, so a taught word can point at a skill
/// or emotion that is not in the static table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeardWord {
    Skill(String),
    /// A sound tag rather than a move, for greetings.
    Sound(String),
    Sit,
    Stand,
    GroundPick,
}

impl From<Heard> for HeardWord {
    fn from(heard: Heard) -> Self {
        match heard {
            Heard::Skill(name) => Self::Skill(name.to_owned()),
            Heard::Sound(tag) => Self::Sound(tag.to_owned()),
            Heard::Sit => Self::Sit,
            Heard::Stand => Self::Stand,
            Heard::GroundPick => Self::GroundPick,
        }
    }
}

impl HeardWord {
    /// Whether the verb moves the body: the verbs the gates refuse — a
    /// power-off, frozen time, no driving policy — because they would change
    /// what the body is doing. A sound moves nothing and stays available
    /// through every gate.
    pub fn moves_body(&self) -> bool {
        matches!(
            self,
            Self::Skill(_) | Self::Sit | Self::Stand | Self::GroundPick
        )
    }
}

/// `target` as the verb it names.
///
/// Only a target [`target_is_valid`] allowed can be stored, so anything not
/// one of the loop's own verbs or a sound tag is a skill or an emotion —
/// exactly how `trigger_skill` reads a name too.
fn word_for_target(target: &str) -> HeardWord {
    match target {
        "sit" => HeardWord::Sit,
        "stand" => HeardWord::Stand,
        "ground_pick" => HeardWord::GroundPick,
        tag if crate::voice::TAGS.contains(&tag) => HeardWord::Sound(tag.to_owned()),
        name => HeardWord::Skill(name.to_owned()),
    }
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
/// entries; Chinese has no case. The longest contained phrase wins, ties
/// keeping the table's earlier entry.
pub fn interpret(text: &str) -> Option<(&'static str, Heard)> {
    interpret_builtin(&text.to_lowercase())
}

/// The verb `text` asks for, with the person's own words in the contest.
///
/// Both tables are matched with the same substring rule [`interpret`]
/// applies, and one longest phrase wins the lot: a taught word takes the
/// answer when its phrase is at least as long as the longest built-in
/// phrase the text also contains — a tie goes to the taught word — and a
/// shorter alias loses to a longer built-in phrase. So teaching 「站」 to
/// a kick cannot hijack the built-in 「站起来」, and teaching 「好」
/// cannot hijack the greeting 「你好」. The phrase borrows from whichever
/// table won.
pub fn interpret_with<'a>(aliases: &'a VoiceAliases, text: &str) -> Option<(&'a str, HeardWord)> {
    let lowered = text.to_lowercase();
    let builtin = interpret_builtin(&lowered);
    let Some((phrase, target)) = aliases.longest(&lowered) else {
        return builtin.map(|(phrase, heard)| (phrase, HeardWord::from(heard)));
    };
    if builtin.is_none_or(|(built, _)| phrase.chars().count() >= built.chars().count()) {
        return Some((phrase, word_for_target(target)));
    }
    builtin.map(|(phrase, heard)| (phrase, HeardWord::from(heard)))
}

/// The longest built-in phrase contained in `lowered`, with its verb.
///
/// The table is read whole, not first-found: entries like `("捡",
/// ..)` before `("捡起来", ..)` would otherwise name the shorter phrase a
/// person did not say. An equal length keeps the table's earlier entry, so
/// the answer never depends on scan order.
fn interpret_builtin(lowered: &str) -> Option<(&'static str, Heard)> {
    let mut best: Option<(&'static str, Heard)> = None;
    for &(phrase, heard) in PHRASES {
        if lowered.contains(phrase)
            && best.is_none_or(|(found, _)| phrase.chars().count() > found.chars().count())
        {
            best = Some((phrase, heard));
        }
    }
    best
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

    fn taught(pairs: &[(&str, &str)]) -> VoiceAliases {
        VoiceAliases::from_pairs(
            pairs
                .iter()
                .map(|(phrase, target)| ((*phrase).to_owned(), (*target).to_owned())),
        )
        .expect("a valid table")
    }

    #[test]
    fn a_taught_word_reaches_a_loaded_skill() {
        let aliases = taught(&[("转圈圈", "roulade")]);
        let (phrase, heard) = interpret_with(&aliases, "转圈圈！").expect("the word was taught");
        assert_eq!(phrase, "转圈圈");
        assert_eq!(heard, HeardWord::Skill("roulade".to_owned()));
    }

    #[test]
    fn taught_words_beat_the_built_in_table() {
        // The same phrase the built-in table knows: the person's meaning
        // wins. Without the word the table still reads it as a sit.
        let aliases = taught(&[("坐下", "kick_left")]);
        let (phrase, heard) = interpret_with(&aliases, "坐下").expect("taught");
        assert_eq!(phrase, "坐下");
        assert_eq!(heard, HeardWord::Skill("kick_left".to_owned()));
        assert_eq!(interpret("坐下").unwrap().1, Heard::Sit);
    }

    #[test]
    fn the_longest_taught_phrase_wins() {
        let aliases = taught(&[("圈", "roulade"), ("转圈圈", "kick_left")]);
        let (phrase, heard) = interpret_with(&aliases, "转圈圈").expect("taught");
        assert_eq!(phrase, "转圈圈");
        assert_eq!(heard, HeardWord::Skill("kick_left".to_owned()));
    }

    /// A short alias must not hijack the longer built-in phrases the text
    /// also contains: the QA trap where 「站」 taught to a kick made
    /// 「站起来」 kick, and 「好」 hijacked 「你好」 and 「好的」.
    #[test]
    fn a_short_alias_cannot_hijack_a_longer_builtin_phrase() {
        let aliases = taught(&[("站", "kick_left")]);
        assert_eq!(
            interpret_with(&aliases, "站起来").unwrap().1,
            HeardWord::Stand
        );
        assert_eq!(
            interpret_with(&aliases, "起来").unwrap().1,
            HeardWord::Stand
        );
        // 「好」 inside the greeting and inside 「好的」: both built-in
        // phrases are longer than the taught word.
        let aliases = taught(&[("好", "roulade")]);
        assert_eq!(
            interpret_with(&aliases, "你好").unwrap().1,
            HeardWord::Sound("greet".to_owned())
        );
        assert_eq!(
            interpret_with(&aliases, "好的").unwrap().1,
            HeardWord::Skill("yes".to_owned())
        );
    }

    #[test]
    fn a_long_taught_word_beats_a_shorter_builtin_phrase() {
        // Five taught characters against the four-character built-in
        // 「快速点头」 the text also contains: the person's word wins.
        let aliases = taught(&[("快速点头啦", "roulade")]);
        let (phrase, heard) = interpret_with(&aliases, "快速点头啦").expect("taught");
        assert_eq!(phrase, "快速点头啦");
        assert_eq!(heard, HeardWord::Skill("roulade".to_owned()));
    }

    #[test]
    fn a_tie_between_the_tables_goes_to_the_taught_word() {
        // 「的呀」 and the built-in 「好的」 are both two characters and
        // both live in "好的呀": an equal match is the person's meaning.
        let aliases = taught(&[("的呀", "roulade")]);
        let (phrase, heard) = interpret_with(&aliases, "好的呀").expect("taught");
        assert_eq!(phrase, "的呀");
        assert_eq!(heard, HeardWord::Skill("roulade".to_owned()));
    }

    #[test]
    fn the_builtin_table_reports_the_longest_phrase_it_matched() {
        // 「捡起来」 contains both entries; the verb is the same either
        // way, but the phrase named is the one a person actually said.
        let (phrase, heard) = interpret("捡起来").unwrap();
        assert_eq!(phrase, "捡起来");
        assert_eq!(heard, Heard::GroundPick);
        let (phrase, heard) = interpret("大笑").unwrap();
        assert_eq!(phrase, "大笑");
        assert_eq!(heard, Heard::Skill("laugh"));
    }

    #[test]
    fn a_taught_target_can_name_any_of_the_ducks_own_verbs() {
        let aliases = taught(&[
            ("蹲下", "sit"),
            ("起立", "stand"),
            ("捡宝贝", "ground_pick"),
            ("打招呼", "greet"),
        ]);
        assert_eq!(interpret_with(&aliases, "蹲下").unwrap().1, HeardWord::Sit);
        assert_eq!(
            interpret_with(&aliases, "起立").unwrap().1,
            HeardWord::Stand
        );
        assert_eq!(
            interpret_with(&aliases, "捡宝贝").unwrap().1,
            HeardWord::GroundPick
        );
        assert_eq!(
            interpret_with(&aliases, "打招呼").unwrap().1,
            HeardWord::Sound("greet".to_owned())
        );
    }

    #[test]
    fn an_empty_table_falls_back_to_the_built_in_words() {
        let aliases = VoiceAliases::new();
        assert!(aliases.is_empty());
        assert_eq!(
            interpret_with(&aliases, "翻滚").unwrap().1,
            HeardWord::Skill("roulade".to_owned())
        );
        assert_eq!(interpret_with(&aliases, "坐下").unwrap().1, HeardWord::Sit);
        assert!(interpret_with(&aliases, "qwertyuiop").is_none());
    }

    #[test]
    fn phrases_are_trimmed_and_case_folded() {
        // English is matched the way the built-in table matches it, and the
        // stored phrase is the one that actually matched.
        let aliases = taught(&[("  Roll Over  ", "roulade")]);
        let (phrase, heard) = interpret_with(&aliases, "ROLL OVER now").expect("taught");
        assert_eq!(phrase, "roll over");
        assert_eq!(heard, HeardWord::Skill("roulade".to_owned()));
    }

    #[test]
    fn a_repeated_phrase_keeps_the_later_meaning() {
        let aliases = VoiceAliases::from_pairs([
            (" 转圈圈 ".to_owned(), "roulade".to_owned()),
            ("转圈圈".to_owned(), "kick_left".to_owned()),
        ])
        .expect("a valid table");
        assert_eq!(aliases.len(), 1, "one phrase, one meaning");
        assert_eq!(aliases.get("转圈圈"), Some("kick_left"));
    }

    #[test]
    fn bad_phrases_are_refused_whole() {
        let pairs = |phrase: &str, target: &str| [(phrase.to_owned(), target.to_owned())];
        assert!(VoiceAliases::from_pairs(pairs("", "roulade")).is_err());
        assert!(VoiceAliases::from_pairs(pairs("   ", "roulade")).is_err());

        // The limit is characters, not bytes: 24 CJK characters are 72 bytes.
        let longest = "圈".repeat(MAX_ALIAS_CHARS);
        assert!(VoiceAliases::from_pairs(pairs(&longest, "roulade")).is_ok());
        let overlong = "圈".repeat(MAX_ALIAS_CHARS + 1);
        assert!(VoiceAliases::from_pairs(pairs(&overlong, "roulade")).is_err());

        let many: Vec<(String, String)> = (0..=MAX_ALIASES)
            .map(|index| (format!("词{index}"), "roulade".to_owned()))
            .collect();
        assert!(VoiceAliases::from_pairs(many).is_err());
    }

    /// The limit's unit is Unicode scalar values, not grapheme clusters: the
    /// ZWJ family emoji is one cluster but seven scalars, and a phrase of 24
    /// scalars fits where a cluster count would say there is room to spare.
    #[test]
    fn the_phrase_limit_is_unicode_scalars_not_graphemes() {
        let family = "👨‍👩‍👧‍👦";
        assert_eq!(family.chars().count(), 7, "one cluster, seven scalars");
        let fits = format!("{family}{}", "圈".repeat(MAX_ALIAS_CHARS - 7));
        assert_eq!(fits.chars().count(), MAX_ALIAS_CHARS);
        assert!(VoiceAliases::from_pairs([(fits, "roulade".to_owned())]).is_ok());
        let over = format!("{family}{}", "圈".repeat(MAX_ALIAS_CHARS - 6));
        assert_eq!(over.chars().count(), MAX_ALIAS_CHARS + 1);
        assert!(VoiceAliases::from_pairs([(over, "roulade".to_owned())]).is_err());
    }

    #[test]
    fn a_target_must_name_something_the_duck_has() {
        let skills = vec!["roulade".to_owned(), "sit".to_owned()];
        let emotions = vec!["excited".to_owned()];
        for target in [
            "roulade",
            "excited",
            "sit",
            "stand",
            "ground_pick",
            "greet",
            "chirp",
        ] {
            assert!(target_is_valid(target, &skills, &emotions), "{target}");
        }
        for target in ["backflip", "", "emotion:excited"] {
            assert!(!target_is_valid(target, &skills, &emotions), "{target}");
        }
    }
}
