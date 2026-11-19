//! The policy set, resolved the way `robotd` resolves it.
//!
//! The manifest types are the official ones (`robotd_params::SetManifest`), so a
//! set published for the robot is read the same way here: an episodic
//! constant-command entry is a skill, a phase-encoded one is the ground pick, a
//! posture-flag one is the sit/stand, and the walk policy is velstand.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use duck_control::policy::PolicyPaths;
use robotd_params::{Mode, SetManifest, SetPolicy, SkillDef, SkillOverrides};

use crate::control::{SkillTuning, Tuning};

#[derive(Debug)]
pub struct PolicyBundle {
    pub paths: PolicyPaths,
    pub skills: Vec<SkillDef>,
    pub skill_tuning: SkillTuning,
    /// The locomotion entry's own action scale, when the set declares one.
    pub action_scale: Option<f64>,
    /// Which bundle this is. The loop reads it for the idle behaviour and the
    /// shell publishes it so a client knows which duck it is talking to.
    pub mode: Mode,
    /// Roller has no standing network; see `Policy::set_standing_disabled`.
    pub standing_disabled: bool,
}

impl PolicyBundle {
    pub fn tuning(&self) -> Tuning {
        let mut tuning = Tuning::default();
        if let Some(scale) = self.action_scale {
            tuning.action_scale = scale;
        }
        tuning
    }
}

/// The networks this core can point at a file of a person's choosing: the
/// official `[policy]` keys it drives itself (`robotd-params`'
/// `PolicyParams`/`Slot`). The other three official slots — `kick_left`,
/// `kick_right`, `roulade` — are skills here, and a skill's identity on this
/// core is its manifest skill *name* (`skills_from`), not a slot key: the
/// file is whatever that entry says, so a slot override by file name could
/// name a file the skill list does not publish and make the two disagree.
/// They are deliberately not override targets, and the refusal says so by
/// listing the names that are.
const OVERRIDABLE_SLOTS: [&str; 4] = ["walk", "stand", "sitstand", "ground_pick"];

const WALK_SLOT: &str = "walk";

/// The official refusal for `walk = "none"`: the walking policy is the slot
/// every other one falls back to, so `robot.loadPolicy` will not switch it
/// off (`robotd/src/main.rs`' `load_policy_request`). `resolved_with` additionally
/// ignores a `"none"` already in the file, and `drop_unloadable_overrides`
/// clears it and reports degraded.
const WALK_NONE_REFUSAL: &str = "the walking policy cannot be switched off — it is what every \
     other slot falls back to. `policy reset walk` returns it to this robot's own";

/// The slot overrides: the phone's twin of the official `[policy] <slot>`
/// config keys, which resolve ahead of everything a set declares
/// (`robotd-params`' `PolicyParams::resolved_with`, config over default). A
/// value is a file name relative to the policies directory, or the official
/// `"none"` sentinel for a slot switched off. It is a process global because
/// the official keys are one machine-level config.
static SLOT_OVERRIDES: Mutex<BTreeMap<&'static str, String>> = Mutex::new(BTreeMap::new());

/// Overrides a load had to fall back from, for the health gate to take with
/// [`take_override_warnings`]. The official daemon records the same reasons
/// on its `SlotErrors` and reports degraded; the core has no runtime handle
/// here, so it leaves them in this queue for the shell's reload hook.
static OVERRIDE_WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The directory of the most recent policy load. The custom skill table has
/// no per-call directory of its own — `POST /config custom_skills` and
/// `robot.setSkill` edit a process global — but its entries' `file` fields
/// are names in a policies directory, so the set-time existence check has to
/// know which one. `load_mode` remembers the directory it was given, and the
/// set-time check resolves against it; the load-time check uses the actual
/// directory each load is given (which can differ after a `/reload`).
static POLICIES_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The custom skill table: the phone's twin of the official `[[policy.skill]]`
/// config entries. The official daemon edits a config file and reloads; a
/// phone edits this table (`/config custom_skills`, `robot.setSkill`,
/// `robot.removeSkill`) and reloads. It is a process global because the
/// official keys are one machine-level config, exactly like the slot table.
static SKILL_OVERRIDES: Mutex<BTreeMap<String, SkillDef>> = Mutex::new(BTreeMap::new());

/// The directory of the most recent load attempt, for the custom-skill
/// table's set-time file checks.
fn policies_dir() -> Option<PathBuf> {
    POLICIES_DIR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Record which directory the next load resolution runs against.
fn remember_policies_dir(dir: &Path) {
    *POLICIES_DIR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(dir.to_path_buf());
}

/// The canonical spelling of an overridable slot, or the official
/// `robot.loadPolicy` refusal: the names this core does know. Official
/// `Slot::parse` matches the wire spelling exactly, so this does too.
fn canonical_slot(slot: &str) -> Result<&'static str, String> {
    OVERRIDABLE_SLOTS
        .iter()
        .copied()
        .find(|known| *known == slot)
        .ok_or_else(|| {
            format!(
                "no policy slot named {slot:?}; expected one of {}",
                OVERRIDABLE_SLOTS.join(", ")
            )
        })
}

/// A name, not a route out of the policies directory — the same rule the C
/// ABI applied to the walk override.
fn validate_override_name(name: &str) -> Result<(), String> {
    if name.contains('\\') || name.contains('/') || name.contains("..") {
        return Err(format!(
            "{name:?} is not a file name in the policies directory"
        ));
    }
    Ok(())
}

/// Point one slot at a file in the policies directory, or clear/disable it —
/// `robotctl policy load <slot> <file|none>` has no robotctl on a phone, so
/// the shell sets the override here and reloads.
///
/// For `walk`, `None` clears the override and `"none"` is refused with the
/// official wording: a robot with no walking network has nothing to run. For
/// the other slots, `Some(name)` overrides the slot, `Some("none")` disables
/// it (the slot resolves to no path), and `None` clears the override so the
/// next reload resolves the set's own slot again.
pub fn set_slot_override(slot: &str, value: Option<String>) -> Result<(), String> {
    let slot = canonical_slot(slot)?;
    let mut overrides = SLOT_OVERRIDES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(value) = value else {
        overrides.remove(slot);
        return Ok(());
    };
    if value.is_empty() {
        return Err(format!(
            "empty path for slot {slot:?}; omit the override to clear it"
        ));
    }
    if is_none_sentinel(&value) {
        if slot == WALK_SLOT {
            return Err(WALK_NONE_REFUSAL.to_owned());
        }
        // The literal that turns a slot off, stored as the official config
        // file spells it. It names no file, so it is never validated.
        overrides.insert(slot, "none".to_owned());
        return Ok(());
    }
    validate_override_name(&value)?;
    overrides.insert(slot, value);
    Ok(())
}

/// The current override for one slot, `"none"` included so a reader can tell
/// a disabled slot from an unset one.
pub fn slot_override(slot: &str) -> Option<String> {
    let slot = canonical_slot(slot).ok()?;
    SLOT_OVERRIDES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(slot)
        .cloned()
}

/// Clear every slot override, the official `policy reset` with no slot.
pub fn clear_slot_overrides() {
    SLOT_OVERRIDES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// Take the reasons a reload fell back from an override, for the shell's
/// reload hook to mark the health gate degraded with. Drains the queue: each
/// warning is reported once, like the official boot-time report.
pub fn take_override_warnings() -> Vec<String> {
    std::mem::take(
        &mut *OVERRIDE_WARNINGS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// The name checks every way of adding a skill goes through, in the official
/// `robot.setSkill`'s own order and words (`robotd/src/main.rs`'s
/// `set_skill_request`): a blank name is not something `robot.do` can be
/// asked for, and a name the daemon drives itself is unreachable as a table
/// entry (the ground pick and the sit toggle live in their own arm of the
/// cascade).
pub fn validate_skill_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("a skill needs a name; it is what `robot.do` asks for".to_owned());
    }
    if robotd_params::DAEMON_OWNED_SKILLS.contains(&name) {
        return Err(format!(
            "{name} is driven by the robot itself and cannot be a table entry"
        ));
    }
    // A skill's name is also its `<name>.onnx` file stem when the entry names
    // no file, so it obeys the same bare-file-name rule as a slot override: a
    // name like `../../evil` (or an absolute path) would otherwise resolve
    // outside the policies directory at the next load — and load a stranger's
    // `.onnx` under the duck's own name. A colon is refused with the
    // separators because on Windows `C:x.onnx` is a drive prefix, and
    // `Path::join` would replace the base with it.
    validate_override_name(name)?;
    if name.contains(':') {
        return Err(format!(
            "{name:?} is not a file name in the policies directory"
        ));
    }
    Ok(())
}

/// A skill's file: a bare name in the policies directory that directory
/// currently resolves against — the same rule the slot override uses (a name
/// is not a route out of the directory), plus the existence check the slot
/// override leaves to the load (`load_mode` warns and skips a file that
/// vanished after it was set, but a name that was never there is refused at
/// the door so nobody is told a skill was added and finds out by pressing the
/// button).
fn validate_skill_file(file: &str, dir: &Path) -> Result<(), String> {
    if !bare_file_name(file) {
        return Err(format!(
            "{file:?} is not a file name in the policies directory"
        ));
    }
    if !dir.join(file).is_file() {
        return Err(format!("no such file: {}", dir.join(file).display()));
    }
    Ok(())
}

/// Whether `file` is a bare file name in a policies directory: no
/// separators, no `..`, and no colon (on Windows `C:x.onnx` is a drive
/// prefix, and `Path::join` would replace the base with it). The load's
/// merge applies this to every derived `<name>.onnx` too, so a table written
/// by an older build cannot walk out either.
fn bare_file_name(file: &str) -> bool {
    validate_override_name(file).is_ok() && !file.contains(':')
}

/// Parse and validate one `custom_skills` entry — the official manifest
/// policy-entry shape, read the same way [`skills_from`] reads it. Returns
/// the skill's name and the entry as a `SkillDef` whose `path` is the bare
/// file name (the load resolves it against its own directory). A refusal
/// names the entry and the reason, in the official words where the official
/// code has words.
fn parse_custom_skill_entry(
    entry: &serde_json::Value,
    index: usize,
) -> Result<(String, SkillDef), String> {
    let identify = || {
        entry
            .get("name")
            .and_then(serde_json::Value::as_str)
            .or_else(|| entry.get("file").and_then(serde_json::Value::as_str))
            .map(str::to_owned)
    };
    let wrap = |reason: String| match identify() {
        Some(name) => format!("custom_skills[{index}] ({name:?}): {reason}"),
        None => format!("custom_skills[{index}]: {reason}"),
    };
    let policy: SetPolicy = serde_json::from_value(entry.clone())
        .map_err(|error| wrap(format!("not a manifest policy entry: {error}")))?;
    // A custom entry must name itself: the table is keyed by name, and unlike
    // the set's own entries there is no file-stem fallback a client asked for.
    let Some(name) = policy
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
    else {
        return Err(wrap(
            "a skill needs a name; it is what `robot.do` asks for".to_owned(),
        ));
    };
    validate_skill_name(name).map_err(wrap)?;
    let daemon_driven = policy.is_phase() || policy.is_posture_flag();
    if !policy.is_episodic() {
        return Err(wrap(
            "not a skill: `kind` must be \"episodic\" — a perpetual entry has no \
             length of its own, and a scripted one is the sit↔stand"
                .to_owned(),
        ));
    }
    if daemon_driven {
        return Err(wrap(
            "not a skill: a phase command is the ground pick and a posture flag \
             is the sit↔stand, which the daemon drives itself"
                .to_owned(),
        ));
    }
    let dir = policies_dir().ok_or_else(|| {
        wrap("the policies directory is not known yet; load a policy set first".to_owned())
    })?;
    if policy.file.is_empty() {
        return Err(wrap(
            "a skill needs a file — the `.onnx`, named in the policies directory".to_owned(),
        ));
    }
    validate_skill_file(&policy.file, &dir).map_err(wrap)?;
    let duration = match policy.duration_s {
        Some(duration) if duration > 0.0 => duration,
        Some(_) => {
            return Err(wrap("a skill's duration must be more than zero".to_owned()));
        }
        None => {
            return Err(wrap(format!(
                "{name:?} is new here, so it needs a duration — how long it runs, \
                 in seconds"
            )));
        }
    };
    let def = SkillDef {
        name: name.to_owned(),
        // Bare: the entry's file is relative to the policies directory, and a
        // load's own directory is where it is resolved (and where a file that
        // went away since is caught).
        path: Some(PathBuf::from(&policy.file)),
        duration,
        chain: policy.chain,
        unwind: policy
            .command
            .as_ref()
            .and_then(|command| command.idle)
            .unwrap_or_default(),
        unwind_s: policy.unwind_s.unwrap_or(0.0),
        params: SkillOverrides {
            action_scale: policy.action_scale,
            ..Default::default()
        },
        ..Default::default()
    };
    Ok((name.to_owned(), def))
}

/// Replace the whole custom skill table — `POST /config custom_skills`.
/// An empty array clears it. Each entry is a manifest policy entry; a parse
/// failure or an invalid shape refuses the **whole** request, with the entry
/// and the reason, before anything is stored (a table with a bad row is not
/// half applied). A later entry with the same name overwrites an earlier one.
pub fn set_custom_skills(entries: Vec<serde_json::Value>) -> Result<(), String> {
    let mut table = BTreeMap::new();
    for (index, entry) in entries.iter().enumerate() {
        let (name, def) = parse_custom_skill_entry(entry, index)?;
        table.insert(name, def);
    }
    *SKILL_OVERRIDES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = table;
    Ok(())
}

/// Add or replace one custom skill — `robot.setSkill`'s half. The IPC
/// handler does the official per-field merge against whatever the duck
/// already had; this validates the merged entry and stores it. A `path`
/// the caller did not name resolves to `<name>.onnx` by convention at the
/// next load (the wire's absent `path`), so only a path that *is* named is
/// checked here — the exact line `robotd`'s `set_skill_request` draws. The
/// official `"none"` sentinel is the exception on that line: it names no
/// file (it switches the same-named skill off, `SkillDef::resolved_path`
/// resolving it to nothing), so it is stored as-is and never checked.
pub fn upsert_custom_skill(def: SkillDef) -> Result<(), String> {
    validate_skill_name(&def.name)?;
    if def.duration <= 0.0 {
        return Err("a skill's duration must be more than zero".to_owned());
    }
    if let Some(path) = &def.path {
        let file = path.to_string_lossy().into_owned();
        if !is_none_sentinel(&file) {
            let dir = policies_dir().ok_or_else(|| {
                "the policies directory is not known yet; load a policy set first".to_owned()
            })?;
            validate_skill_file(&file, &dir)?;
        }
    }
    SKILL_OVERRIDES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(def.name.clone(), def);
    Ok(())
}

/// Take one custom skill out — `robot.removeSkill`'s half. False when the
/// table has no such entry; the caller answers that "already", the official
/// `edit::remove_skill`'s `Ok(false)`.
pub fn remove_custom_skill(name: &str) -> bool {
    SKILL_OVERRIDES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(name)
        .is_some()
}

/// The whole custom table, for readers (echoing a table back over `robot.*`,
/// the IPC merge's existing-entry lookup) and for tests.
pub fn custom_skills() -> BTreeMap<String, SkillDef> {
    SKILL_OVERRIDES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Clear every custom skill, the official `policy reset` with no slot.
pub fn clear_custom_skills() {
    SKILL_OVERRIDES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// The override table is a process global. Tests that resolve a slot or
/// change an override take this lock so a parallel test never reads another
/// test's state. The intent tests' `/config` route shares the table, so the
/// lock is crate-visible for them too.
#[cfg(test)]
pub(crate) static OVERRIDE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Hold [`OVERRIDE_TEST_LOCK`] with the table clear and no warnings left
/// over — the state every override test assumes. The custom-skill table and
/// the remembered policies directory are part of the same global state, so
/// they are cleared here too: a parallel test must not resolve a load
/// through another test's custom skill.
#[cfg(test)]
pub(crate) fn override_test_lock() -> std::sync::MutexGuard<'static, ()> {
    let guard = OVERRIDE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    clear_slot_overrides();
    clear_custom_skills();
    *POLICIES_DIR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    let _ = take_override_warnings();
    guard
}

/// Point the walk slot at one file — the pre-slot-table signature, kept for
/// the C ABI and existing callers. It has no error channel, so its historic
/// reading of empty and the official `"none"` sentinel stands: both clear the
/// override. [`set_slot_override`] is the API that carries the official walk
/// refusal; this wrapper cannot report it.
pub fn set_walk_override(name: Option<String>) {
    let name = name.filter(|name| !name.is_empty() && !is_none_sentinel(name));
    // The name still goes through the table's validation; a rejected name
    // leaves the previous override in place, as the C ABI's own gate does.
    let _ = set_slot_override(WALK_SLOT, name);
}

/// The current walk-slot override, if one is set.
pub fn walk_override() -> Option<String> {
    slot_override(WALK_SLOT)
}

/// The official sentinel: a config path of `"none"` means "no policy here",
/// case-insensitively (`robotd-params`' `is_none_sentinel`).
pub fn is_none_sentinel(name: &str) -> bool {
    name.eq_ignore_ascii_case("none")
}

/// The manifest's top-level declaration claims, read straight from the text
/// beside `SetManifest` because the set vocabulary does not carry them: these
/// are the fields `docs/policy-manifest.md` §Fields lists under "refused if
/// they disagree", and the official updater gates a fetch on them
/// (`updater/src/policy.rs`' `PolicyManifest`). Everything is optional because
/// a set is under no obligation to carry any of it.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct ManifestDeclarations {
    /// Which revision of the convention wrote this. Schema 2 is a superset of
    /// 1 and nothing gates on it — display only.
    #[allow(dead_code)]
    schema_version: Option<u32>,
    obs_len: Option<usize>,
    action_len: Option<usize>,
    model_api: Option<u32>,
    robot: Option<ManifestRobot>,
}

/// The `robot` block, as the published convention describes it.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct ManifestRobot {
    model: Option<String>,
}

/// The model API this daemon speaks. The official `robotd` reports 2
/// (`robotd/src/main.rs`' `MODEL_API`) — the API the recurrent LSTM exports
/// need — and this build compiles the same `duck-control`, whose `Policy`
/// carries LSTM state. A manifest asking for more than this is refused; one
/// asking for less (the official set's 1, feed-forward) still runs.
const EXPECTED_MODEL_API: u32 = 2;

impl ManifestDeclarations {
    /// Refuse a policy the manifest itself says will not work here.
    ///
    /// The official wording and priority, verbatim: only refuses on a claim
    /// that is *present and wrong* — a manifest with no `obs_len` is not
    /// evidence of anything, and refusing on absence would reject every repo
    /// that does not follow a convention nobody has published.
    fn incompatibility(&self) -> Option<String> {
        if let Some(obs) = self.obs_len
            && obs != duck_control::obs::OBS_LEN
        {
            return Some(format!(
                "its manifest says observation width {obs}, and this robot builds {}",
                duck_control::obs::OBS_LEN
            ));
        }
        if let Some(actions) = self.action_len
            && actions != duck_control::obs::ACTION_LEN
        {
            return Some(format!(
                "its manifest says {actions} actions, and this robot has {}",
                duck_control::obs::ACTION_LEN
            ));
        }
        if let Some(api) = self.model_api
            && api > EXPECTED_MODEL_API
        {
            return Some(format!(
                "it needs model API {api} and this daemon speaks {EXPECTED_MODEL_API} — update the daemon first"
            ));
        }
        if let Some(model) = self.robot.as_ref().and_then(|r| r.model.as_deref())
            && !model.eq_ignore_ascii_case(duck_ipc_proto::ROBOT_MODEL)
        {
            return Some(format!(
                "it is for a {model}, and this is a {}",
                duck_ipc_proto::ROBOT_MODEL
            ));
        }
        None
    }
}

/// What `read_manifest` saw: the set's own vocabulary, each entry's `slot`
/// beside it, and the declaration claims at the top level.
struct Manifest {
    set: SetManifest,
    /// Each entry's `slot`, parallel to `set.policies`. Upstream reads the
    /// field for display only — `policy load <slot> <repo>` is the install
    /// line it names (`docs/policy-manifest.md` §Fields) — because the daemon
    /// has `robotctl` to write the config. A phone's Reload AI is the install
    /// line, so the slot here is what a declared walk slot rides on.
    slots: Vec<Option<String>>,
    declarations: ManifestDeclarations,
}

impl Manifest {
    /// Entry `index`'s `slot`, when it names one.
    fn slot_of(&self, index: usize) -> Option<String> {
        self.slots
            .get(index)
            .cloned()
            .unwrap_or_default()
            .filter(|slot| !slot.is_empty())
            .map(|slot| slot.to_ascii_lowercase())
    }
}

/// The `slot` field alone, read per entry from the set shape. `SetPolicy`
/// does not carry it, and re-parsing the same array keeps the slots parallel
/// to `set.policies` by construction.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct SlotProbe {
    policies: Vec<SlotOnly>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct SlotOnly {
    slot: Option<String>,
}

/// The flat shape's `slot`, at the top level beside the single policy's other
/// fields.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct SlotFlat {
    slot: Option<String>,
}

/// Read `manifest.json`, accepting both shapes the ecosystem publishes.
///
/// The official set is one manifest with a `policies` array. A community
/// repo ships one policy and a *flat* manifest — the fields of a single
/// entry with no array — and `SetManifest`'s `#[serde(default)]` turns that
/// into an empty set, which used to leave the app with no one-shot skills at
/// all. Parse the flat shape too, taking the only `.onnx` beside it when the
/// entry does not name its file.
fn read_manifest(dir: &Path) -> Option<Manifest> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    if let Ok(set) = serde_json::from_str::<SetManifest>(&text) {
        if !set.policies.is_empty() {
            // The same array, read for the one field `SetPolicy` does not
            // carry. A slot of the wrong shape is a display field: none of
            // the slots, not a dead manifest.
            let slots = serde_json::from_str::<SlotProbe>(&text)
                .map(|probe| probe.policies.into_iter().map(|entry| entry.slot).collect())
                .unwrap_or_else(|_| vec![None; set.policies.len()]);
            return Some(Manifest {
                set,
                slots,
                declarations: serde_json::from_str(&text).unwrap_or_default(),
            });
        }
    }
    let mut policy: SetPolicy = serde_json::from_str(&text).ok()?;
    let slot = serde_json::from_str::<SlotFlat>(&text)
        .ok()
        .and_then(|flat| flat.slot);
    if policy.file.is_empty() {
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("onnx"))
            .collect();
        if found.len() != 1 {
            return None;
        }
        policy.file = found.pop()?.file_name()?.to_str()?.to_owned();
    }
    Some(Manifest {
        set: SetManifest {
            policies: vec![policy],
        },
        slots: vec![slot],
        declarations: serde_json::from_str(&text).unwrap_or_default(),
    })
}

/// The walking set. Kept as the name every caller had before modes existed.
pub fn load(dir: &Path) -> Result<PolicyBundle, String> {
    load_mode(dir, Mode::Walk)
}

/// The set for one drive mode, resolved the way `robotd` resolves it.
///
/// A mode is a whole-bundle switch, not a filter: roller keeps the same skills
/// and sit/stand, swaps the locomotion network for `roller.onnx`, drops the
/// standing network, and drives the crouch where the walking set has the
/// ground pick.
pub fn load_mode(dir: &Path, mode: Mode) -> Result<PolicyBundle, String> {
    // The directory this load runs against is what the custom-skill table's
    // set-time file checks resolve against (`set_custom_skills`,
    // `upsert_custom_skill`); remember it before anything can fail.
    remember_policies_dir(dir);
    let manifest = read_manifest(dir);

    // The set's own claims about itself, before any file is opened: a policy
    // whose manifest says it is for another robot, or for an observation
    // layout this build does not build, is refused here — with the reason,
    // the way the official updater refuses a fetch — rather than as a tensor
    // shape error the shell would show as a bare failure. A manifest that
    // does not parse its claims is not evidence of anything: no gate.
    if let Some(manifest) = manifest.as_ref() {
        if let Some(why) = manifest.declarations.incompatibility() {
            return Err(format!(
                "{} will not run on this robot: {why}",
                dir.join("manifest.json").display()
            ));
        }
    }

    // An override whose file is gone costs its slot and nothing else, which
    // is the official `drop_unloadable_overrides` rule for a config key left
    // behind by a deleted file: drop it, record why, and let the set resolve
    // as if it had never been set. Here the question is only whether the file
    // exists; the full ONNX validation is the reload's own job.
    if let Some(name) = slot_override(WALK_SLOT)
        && !is_none_sentinel(&name)
    {
        let path = dir.join(&name);
        if !path.is_file() {
            warn_override_unloadable(WALK_SLOT, &path);
            let _ = set_slot_override(WALK_SLOT, None);
        }
    }

    // Walking keeps the app's long-standing preference: velstand is the
    // current gait (a walking set also ships alpha_walking, which is older).
    // A non-walking mode takes the manifest's perpetual entry tagged with it.
    let walk = match mode {
        Mode::Walk => walk_gait(dir, manifest.as_ref()),
        Mode::Roller => manifest
            .as_ref()
            .and_then(|manifest| {
                manifest
                    .set
                    .policies
                    .iter()
                    .find(|policy| {
                        policy.mode() == mode
                            && !policy.is_episodic()
                            && !policy.is_scripted()
                            && dir.join(&policy.file).exists()
                    })
                    .map(|policy| dir.join(&policy.file))
            })
            .or_else(|| existing(dir.join("roller.onnx"))),
    }
    .ok_or_else(|| format!("no {:?} locomotion policy in {}", mode, dir.display()))?;

    let walk_file = walk
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let action_scale = manifest.as_ref().and_then(|manifest| {
        manifest
            .set
            .policies
            .iter()
            .find(|policy| policy.file.as_str() == walk_file)
            .and_then(|policy| policy.action_scale)
    });

    let sitstand = manifest
        .as_ref()
        .and_then(|manifest| {
            manifest
                .set
                .policies
                .iter()
                .find(|policy| policy.sitstand_timing().is_some())
        })
        .map(|policy| dir.join(&policy.file))
        .filter(|path| path.exists())
        .or_else(|| existing(dir.join("alpha_sitstand.onnx")));
    let ground_pick = manifest
        .as_ref()
        .and_then(|manifest| {
            manifest
                .set
                .policies
                .iter()
                .find(|policy| policy.mode() == mode && policy.phase_timing().is_some())
        })
        .map(|policy| dir.join(&policy.file))
        .filter(|path| path.exists())
        .or_else(|| {
            let fallback = match mode {
                Mode::Walk => "alpha_ground_pick.onnx",
                Mode::Roller => "roller_crouch.onnx",
            };
            existing(dir.join(fallback))
        });

    // The `[policy]` overrides for the three slots whose resolution is not the
    // walk gait's own (walk is applied in `walk_gait`): the named file wins
    // over the set, `"none"` leaves the slot with nothing, and a file that is
    // gone falls back to what the set resolved and is reported.
    let stand = apply_slot_override(dir, "stand", None);
    let sitstand = apply_slot_override(dir, "sitstand", sitstand);
    let ground_pick = apply_slot_override(dir, "ground_pick", ground_pick);

    let (skills, pick_timing, sit_timing) = match manifest.as_ref() {
        Some(manifest) => (
            // Skills are deliberately not filtered by mode, exactly as
            // upstream: the roller keeps its roulade and kicks.
            skills_from(&manifest.set, dir),
            manifest.set.ground_pick(mode),
            manifest.set.sitstand(),
        ),
        None => (fallback_skills(dir), None, None),
    };
    // The person's own skill table rides over the set's: a same-named entry
    // replaces it in place, a new name joins at the end, and an entry whose
    // file is gone is dropped with the same unloadable warning a slot
    // override leaves. `/skills`, MCP and the skill trigger all read the
    // merged table after this.
    let skills = merge_custom_skills(dir, skills);

    // Per-mode fallbacks, as `ResolvedPolicy` resolves them: the roller runs
    // at a smaller action scale and its crouch is a different length.
    let fallback_period = match mode {
        Mode::Walk => 4.0,
        Mode::Roller => 3.0,
    };
    let fallback_scale = match mode {
        Mode::Walk => 1.0,
        Mode::Roller => 0.8,
    };
    let fallback_action_scale = match mode {
        Mode::Walk => 0.9,
        Mode::Roller => 0.8,
    };
    let action_scale = action_scale.or(Some(fallback_action_scale));

    let skill_tuning = SkillTuning {
        ground_pick_period: pick_timing.map_or(fallback_period, |timing| timing.period_s),
        ground_pick_end_phase: pick_timing
            .map_or(robotd_params::DEFAULT_GROUND_PICK_END_PHASE, |timing| {
                timing.end_phase
            }),
        ground_pick_action_scale: pick_timing
            .and_then(|timing| timing.action_scale)
            .unwrap_or(fallback_scale),
        ground_pick_gain_ratio: 1.0,
        sitstand_rise_s: sit_timing.map_or(robotd_params::DEFAULT_SITSTAND_RISE_S, |timing| {
            timing.rise_s
        }),
        sitstand_ramp_s: sit_timing.map_or(robotd_params::DEFAULT_SITSTAND_RAMP_S, |timing| {
            timing.ramp_s
        }),
        skills: skills.clone(),
    };

    let paths = PolicyPaths {
        walk,
        // **No standing network by default in either mode**, which is the
        // robot's own default: velstand (set v5) stands still at zero command
        // on its own, and `[policy] stand = "none"` in the shipped config.
        // `alpha_stand` ships in the set for sets that predate velstand, not
        // for this one. An explicit `stand` override is the way a person
        // loads one back (`apply_slot_override` above).
        stand,
        sitstand,
        ground_pick,
        skills: skills
            .iter()
            .filter_map(|skill| skill.path.clone())
            .collect(),
    };

    Ok(PolicyBundle {
        paths,
        skills,
        skill_tuning,
        action_scale,
        mode,
        standing_disabled: mode == Mode::Roller,
    })
}

fn existing(path: PathBuf) -> Option<PathBuf> {
    path.exists().then_some(path)
}

/// Apply one slot override to a resolved path, the way the official config
/// key applies to a slot: the named file wins, `"none"` resolves to nothing,
/// and no override leaves the set's own value. An override whose file is gone
/// falls back to `resolved`, keeps its slot out of the failure (the set still
/// runs), is dropped from the table, and leaves a warning for
/// [`take_override_warnings`] — `drop_unloadable_overrides`' rule.
fn apply_slot_override(dir: &Path, slot: &str, resolved: Option<PathBuf>) -> Option<PathBuf> {
    let Some(name) = slot_override(slot) else {
        return resolved;
    };
    if is_none_sentinel(&name) {
        return None;
    }
    let path = dir.join(&name);
    if path.is_file() {
        return Some(path);
    }
    warn_override_unloadable(slot, &path);
    let _ = set_slot_override(slot, None);
    resolved
}

/// Record why a slot fell back, in the official daemon's words: the override
/// will not load and the slot's default stands.
fn warn_override_unloadable(slot: &str, path: &Path) {
    OVERRIDE_WARNINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(format!(
            "the {slot} policy override {} will not load; falling back to this slot's default",
            path.display()
        ));
}

/// The walk slot, resolved the way the official daemon resolves it, with the
/// manifest as the extra voice a phone can hear. `robotd` takes the config
/// `[policy] walk` override and otherwise the mode's default file, and reads
/// the manifest's `slot` field for display only — because `robotctl policy
/// load` is what writes the config. A phone has no robotctl: Reload AI *is*
/// the install line, so a set that declares its walk slot gets it.
///
/// Candidate order: the override (the config twin — it wins over everything,
/// and is taken as named, so a file that is not there fails the load with
/// that file's own error rather than a silent fallback); then the set's
/// `slot = "walk"` entries in declaration order; then the traditional file
/// names; then the set's remaining walk-mode perpetual entries. The
/// traditional names stay ahead of the slot-less entries so the official set
/// — whose `alpha_walking` and `alpha_stand` are perpetual and slot-less,
/// older than velstand — keeps resolving to velstand exactly as it always
/// has.
fn walk_gait(dir: &Path, manifest: Option<&Manifest>) -> Option<PathBuf> {
    if let Some(name) = walk_override()
        && !is_none_sentinel(&name)
    {
        return Some(dir.join(name));
    }
    let Some(manifest) = manifest else {
        return existing(dir.join("velstand.onnx"))
            .or_else(|| existing(dir.join("alpha_walking.onnx")));
    };
    let declared = |slot: Option<&str>| -> Option<PathBuf> {
        manifest
            .set
            .policies
            .iter()
            .enumerate()
            .filter(|(index, policy)| {
                // A gait runs until told otherwise; an episodic or scripted
                // entry is a skill or the sit↔stand, and a `mode: "roller"`
                // entry is the roller's. The same reading the roller branch
                // makes of the manifest.
                policy.mode() != Mode::Roller
                    && !policy.is_episodic()
                    && !policy.is_scripted()
                    && manifest.slot_of(*index).as_deref() == slot
                    && dir.join(&policy.file).exists()
            })
            .map(|(_, policy)| dir.join(&policy.file))
            .next()
    };
    declared(Some("walk"))
        .or_else(|| existing(dir.join("velstand.onnx")))
        .or_else(|| existing(dir.join("alpha_walking.onnx")))
        .or_else(|| declared(None))
}

fn skills_from(manifest: &SetManifest, dir: &Path) -> Vec<SkillDef> {
    manifest
        .skills()
        .filter_map(|policy| {
            let path = dir.join(&policy.file);
            if !path.exists() {
                return None;
            }
            Some(SkillDef {
                name: policy.skill_name(),
                path: Some(path),
                duration: policy.duration_s?,
                chain: policy.chain,
                unwind: policy
                    .command
                    .as_ref()
                    .and_then(|command| command.idle)
                    .unwrap_or_default(),
                unwind_s: policy.unwind_s.unwrap_or(0.0),
                params: SkillOverrides {
                    action_scale: policy.action_scale,
                    ..Default::default()
                },
                ..Default::default()
            })
        })
        .collect()
}

/// Merge the custom skill table into a resolved skill list, the way the
/// official config's `[[policy.skill]]` entries ride over the set's own
/// (`robotd-params`' `builtin_skills` + config overlay): a same-named entry
/// replaces the set's in place, a new name joins at the end (`robot.do`
/// tries skills in order, and the set's own priority stands), and a mode
/// filter there is none — skills are deliberately not filtered by mode.
///
/// An entry whose file is gone costs itself and nothing else, **for this load
/// only**: it is skipped, a warning names it for [`take_override_warnings`],
/// and the table keeps it — a load from a directory that still has the file
/// brings the skill back, and a directory switch cannot destroy a
/// registration the person made. (The official `drop_unloadable_overrides`
/// rule drops a *config key* because a file left behind by a deleted policy is
/// stale config; a custom skill is a live request, so here it is only the load
/// that skips it.)
///
/// The `"none"` path is the official switch-off (`SkillDef::resolved_path`
/// resolving it to no file): the entry removes the same-named skill —
/// built-in or set-declared — from the table and adds nothing itself, which is
/// how a person takes a shipped skill out without a second mechanism.
///
/// **Order caveat, recorded honestly.** The official table is the manifest's
/// entry order followed by config entries in file order; this table is a
/// `BTreeMap`, so custom skills are appended in *name* order. `/skills` (and
/// `robot.skills`) therefore lists custom skills in a different order than a
/// `robotd` would, and `robot.do` tries them in that order. `robot.do` is
/// addressed by exact name, so a request is unaffected; only a client that
/// reads priority off the listing would be.
fn merge_custom_skills(dir: &Path, mut skills: Vec<SkillDef>) -> Vec<SkillDef> {
    let mut switched_off: Vec<String> = Vec::new();
    let mut merged: Vec<(String, SkillDef)> = Vec::new();
    let mut warnings: Vec<(String, PathBuf)> = Vec::new();
    {
        // One critical section for the whole read, and **no write**: a
        // concurrent `setSkill` must never have an entry this load could
        // remove from under it (the two-phase version did exactly that).
        let table = SKILL_OVERRIDES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (name, def) in table.iter() {
            if def
                .path
                .as_ref()
                .is_some_and(|path| is_none_sentinel(&path.to_string_lossy()))
            {
                switched_off.push(name.clone());
                continue;
            }
            // A path the entry did not name is the wire's absent `path`: the
            // file this convention ships under `<name>.onnx`.
            let file = def
                .path
                .clone()
                .unwrap_or_else(|| PathBuf::from(format!("{name}.onnx")));
            // Both doors validate a name as a file stem (`validate_skill_name`);
            // this is the same rule as defence in depth for a table written by
            // an older build — the load is where a derived `<name>.onnx` would
            // actually be opened.
            if !bare_file_name(&file.to_string_lossy()) {
                warnings.push((name.clone(), dir.join(&file)));
                continue;
            }
            let path = dir.join(&file);
            if !path.is_file() {
                warnings.push((name.clone(), path));
                continue;
            }
            let resolved = SkillDef {
                path: Some(path),
                ..def.clone()
            };
            merged.push((name.clone(), resolved));
        }
    }
    for (name, path) in warnings {
        warn_skill_override_unloadable(&name, &path);
    }
    // The switch-offs first: an entry that turns a skill off must win over a
    // set entry of that name even when the set is what is in `skills` here.
    for name in switched_off {
        skills.retain(|skill| skill.name != name);
    }
    for (name, def) in merged {
        match skills.iter().position(|skill| skill.name == name) {
            Some(index) => skills[index] = def,
            None => skills.push(def),
        }
    }
    skills
}

/// Record why a custom skill was skipped for one load, in the same spirit as
/// a slot's: the override will not load and the set's own skills stand.
fn warn_skill_override_unloadable(name: &str, path: &Path) {
    OVERRIDE_WARNINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(format!(
            "the {name} skill override {} will not load; the set's own skills stand",
            path.display()
        ));
}

/// The three skills every robot has had since the prototype, for a set with no
/// manifest.
fn fallback_skills(dir: &Path) -> Vec<SkillDef> {
    [
        ("roulade", "roulade.onnx", 1.0, true),
        ("kick_left", "ball_kick_left.onnx", 0.5, false),
        ("kick_right", "ball_kick_right.onnx", 0.5, false),
    ]
    .into_iter()
    .filter_map(|(name, file, duration, chain)| {
        let path = dir.join(file);
        if !path.exists() {
            return None;
        }
        Some(SkillDef {
            name: name.to_owned(),
            path: Some(path),
            duration,
            chain,
            ..Default::default()
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;

    /// Hold the lock for the duration of an override test, with the table
    /// clear and no warnings left over — the state every other test assumes.
    fn walk_test_lock() -> MutexGuard<'static, ()> {
        override_test_lock()
    }

    /// A temp policies directory with stub files — the resolver only checks
    /// that a candidate exists, so the contents do not have to be ONNX.
    fn write_set(name: &str, files: &[&str], manifest_text: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("duck-manifest-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp policies dir");
        for file in files {
            std::fs::write(dir.join(file), b"stub").expect("temp policy file");
        }
        if let Some(text) = manifest_text {
            std::fs::write(dir.join("manifest.json"), text).expect("temp manifest");
        }
        dir
    }

    #[test]
    fn official_manifest_resolves_walk_slots_and_skills() {
        let _lock = walk_test_lock();
        let Some(dir) = crate::fixtures::policies_dir() else {
            return;
        };
        let bundle = load(&dir).expect("manifest");
        assert!(bundle.paths.walk.ends_with("velstand.onnx"));
        assert!(
            bundle
                .paths
                .sitstand
                .as_ref()
                .is_some_and(|path| path.ends_with("alpha_sitstand.onnx"))
        );
        assert!(
            bundle
                .paths
                .ground_pick
                .as_ref()
                .is_some_and(|path| path.ends_with("alpha_ground_pick.onnx"))
        );
        assert_eq!(bundle.skill_tuning.ground_pick_period, 4.0);
        assert_eq!(bundle.skill_tuning.ground_pick_end_phase, 0.7);
        assert_eq!(bundle.skill_tuning.sitstand_rise_s, 1.0);
        assert_eq!(bundle.skill_tuning.sitstand_ramp_s, 2.0);

        let names: Vec<&str> = bundle
            .skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect();
        assert!(names.contains(&"roulade"));
        assert!(names.contains(&"kick_left"));
        assert!(names.contains(&"kick_right"));
        for skill in &bundle.skills {
            match skill.name.as_str() {
                "roulade" => {
                    assert_eq!(skill.duration, 1.0);
                    assert!(skill.chain);
                }
                "kick_left" | "kick_right" => assert_eq!(skill.duration, 0.5),
                _ => {}
            }
        }
    }

    /// Roller is a whole-bundle switch: a different locomotion network, no
    /// standing network, the crouch where walking has the ground pick, and
    /// the same skills — upstream deliberately does not filter skills by mode.
    #[test]
    fn the_roller_manifest_swaps_the_bundle_and_keeps_the_skills() {
        let _lock = walk_test_lock();
        let Some(dir) = crate::fixtures::policies_dir() else {
            return;
        };
        if !crate::fixtures::has_all(&dir, &["roller.onnx", "roller_crouch.onnx"]) {
            return;
        }
        let walk = load_mode(&dir, Mode::Walk).expect("walk");
        let roller = load_mode(&dir, Mode::Roller).expect("roller");

        assert!(
            walker_walk(&walk),
            "walking keeps velstand: {:?}",
            walk.paths.walk
        );
        assert!(roller.paths.walk.ends_with("roller.onnx"));
        assert!(
            roller.standing_disabled,
            "the roller has no standing network"
        );
        assert_eq!(roller.paths.stand, None);
        assert!(
            roller
                .paths
                .ground_pick
                .as_ref()
                .is_some_and(|path| path.ends_with("roller_crouch.onnx"))
        );
        // The crouch's own timing comes from the manifest, not the walking pick.
        assert_eq!(roller.skill_tuning.ground_pick_period, 5.0);
        assert_eq!(roller.skill_tuning.ground_pick_end_phase, 0.7);

        let walk_names: Vec<&str> = walk.skills.iter().map(|s| s.name.as_str()).collect();
        let roller_names: Vec<&str> = roller.skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(walk_names, roller_names, "skills are not mode-filtered");
        assert!(roller_names.contains(&"roulade"));

        // The same seed of tuning except the locomotion scale: the roller set
        // declares action_scale 0.8.
        assert_eq!(roller.action_scale, Some(0.8));
    }

    /// The official set's own declaration claims must pass the gate the same
    /// loader applies to a stranger's set: model_api 1 under a daemon that
    /// speaks 2, the contract's observation and action widths, this robot's
    /// model. (`DUCK_REQUIRE_FIXTURES=1` turns a missing bundle into a
    /// failure — CI fetches it, so there the official file is always here.)
    #[test]
    fn the_official_manifest_passes_the_declaration_gate() {
        let _lock = walk_test_lock();
        let Some(dir) = crate::fixtures::policies_dir() else {
            return;
        };
        let text =
            std::fs::read_to_string(dir.join("manifest.json")).expect("official manifest.json");
        let declarations: ManifestDeclarations =
            serde_json::from_str(&text).expect("official declarations parse");
        assert!(
            declarations.incompatibility().is_none(),
            "the official set must pass its own gate"
        );
        load(&dir).expect("the official set loads");
    }

    /// Refusals carry the official updater's reason, word for word: the
    /// manifest's claim and what this robot actually is.
    #[test]
    fn a_manifest_lying_about_the_model_api_is_refused() {
        // The load writes the process-global policies directory, so this
        // test takes the same lock every other load test does.
        let _lock = walk_test_lock();
        let dir = write_set(
            "model-api",
            &["velstand.onnx"],
            Some(
                r#"{ "model_api": 3, "policies": [ { "file": "velstand.onnx", "kind": "perpetual" } ] }"#,
            ),
        );
        let error = load(&dir).expect_err("model API 3 is newer than the daemon");
        assert!(error.contains("model API 3"), "{error}");
        assert!(error.contains("update the daemon first"), "{error}");
    }

    #[test]
    fn a_manifest_for_another_robot_is_refused() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "robot-model",
            &["velstand.onnx"],
            Some(
                r#"{ "robot": { "model": "reachy" }, "policies": [ { "file": "velstand.onnx", "kind": "perpetual" } ] }"#,
            ),
        );
        let error = load(&dir).expect_err("reachy is not this robot");
        assert!(error.contains("reachy"), "{error}");
        assert!(error.contains("microduck"), "{error}");
    }

    #[test]
    fn a_manifest_lying_about_the_observation_width_is_refused() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "obs-len",
            &["velstand.onnx"],
            Some(
                r#"{ "obs_len": 51, "policies": [ { "file": "velstand.onnx", "kind": "perpetual" } ] }"#,
            ),
        );
        let error = load(&dir).expect_err("51 is not what this core builds");
        assert!(error.contains("observation width 51"), "{error}");
        // The action length reads the same way.
        let dir = write_set(
            "action-len",
            &["velstand.onnx"],
            Some(
                r#"{ "action_len": 12, "policies": [ { "file": "velstand.onnx", "kind": "perpetual" } ] }"#,
            ),
        );
        let error = load(&dir).expect_err("12 is not this robot's action count");
        assert!(error.contains("12 actions"), "{error}");
    }

    /// Absent claims are not evidence: the flat community shape with no
    /// declaration fields at all loads as it always has.
    #[test]
    fn a_manifest_with_no_declarations_gates_nothing() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "no-declarations",
            &["velstand.onnx"],
            Some(r#"{ "file": "velstand.onnx", "kind": "perpetual" }"#),
        );
        load(&dir).expect("a sparse manifest is no refusal");
    }

    /// The gap the walk slot existed to close: a set that names its own gait
    /// loads it, instead of failing because the file is not called
    /// velstand.onnx.
    #[test]
    fn the_manifest_walk_slot_resolves_a_declared_gait() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "slot-walk",
            &["my_gait.onnx"],
            Some(
                r#"{ "policies": [ { "file": "my_gait.onnx", "kind": "perpetual", "slot": "walk" } ] }"#,
            ),
        );
        let bundle = load(&dir).expect("the declared gait loads");
        assert!(
            bundle.paths.walk.ends_with("my_gait.onnx"),
            "{:?}",
            bundle.paths.walk
        );
    }

    /// A slot-less perpetual entry is the main gait when the set ships no
    /// traditional name to fall back to — the shape `publish` writes for a
    /// single-policy repo.
    #[test]
    fn a_slot_less_gait_resolves_when_no_traditional_name_exists() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "slot-less",
            &["my_gait.onnx"],
            Some(r#"{ "policies": [ { "file": "my_gait.onnx", "kind": "perpetual" } ] }"#),
        );
        let bundle = load(&dir).expect("the declared gait loads");
        assert!(
            bundle.paths.walk.ends_with("my_gait.onnx"),
            "{:?}",
            bundle.paths.walk
        );
    }

    /// The override is the config twin: it wins over what the set declares,
    /// exactly as `[policy] walk` wins over the default on the robot.
    #[test]
    fn the_walk_override_wins_over_the_set() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "override",
            &["chosen.onnx", "my_gait.onnx"],
            Some(
                r#"{ "policies": [ { "file": "my_gait.onnx", "kind": "perpetual", "slot": "walk" } ] }"#,
            ),
        );
        set_walk_override(Some("chosen.onnx".to_owned()));
        let bundle = load(&dir).expect("the override loads");
        assert!(
            bundle.paths.walk.ends_with("chosen.onnx"),
            "{:?}",
            bundle.paths.walk
        );
        // The legacy wrapper keeps its old "none"/empty-clears reading — the
        // C ABI maps its sentinel there before calling it — while the slot
        // API refuses walk = "none" (see the test below).
        set_walk_override(Some("none".to_owned()));
        let bundle = load(&dir).expect("the sentinel clears the override");
        assert!(
            bundle.paths.walk.ends_with("my_gait.onnx"),
            "{:?}",
            bundle.paths.walk
        );
        set_walk_override(None);
    }

    /// The official `robot.loadPolicy` refuses `walk = "none"`: the walking
    /// policy is what every other slot falls back to. Other slots can be
    /// switched off, and the sentinel is reported as the override.
    #[test]
    fn walk_cannot_be_switched_off_and_other_slots_can() {
        let _lock = walk_test_lock();
        let error =
            set_slot_override("walk", Some("none".to_owned())).expect_err("walk is required");
        assert!(error.contains("cannot be switched off"), "{error}");
        assert!(error.contains("policy reset walk"), "{error}");
        assert_eq!(slot_override("walk"), None);

        set_slot_override("sitstand", Some("none".to_owned())).expect("a slot to disable");
        assert_eq!(slot_override("sitstand").as_deref(), Some("none"));
        set_slot_override("sitstand", None).expect("reset");
        assert_eq!(slot_override("sitstand"), None);
    }

    /// `sitstand = "none"` leaves the slot empty after a load — the local
    /// twin of the official key disabling the commanded sit↔stand.
    #[test]
    fn a_disabled_sitstand_slot_has_no_path() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "sitstand-none",
            &["velstand.onnx", "alpha_sitstand.onnx"],
            None,
        );
        let bundle = load(&dir).expect("the set loads");
        assert!(bundle.paths.sitstand.is_some());
        set_slot_override("sitstand", Some("none".to_owned())).expect("disable");
        let bundle = load(&dir).expect("the disabled set loads");
        assert_eq!(bundle.paths.sitstand, None);
    }

    /// A slot override wins over the set's own entry, exactly as `[policy]
    /// ground_pick` wins over the default on the robot.
    #[test]
    fn a_ground_pick_override_wins_over_the_set() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "ground-pick-override",
            &[
                "velstand.onnx",
                "alpha_ground_pick.onnx",
                "custom_pick.onnx",
            ],
            None,
        );
        let bundle = load(&dir).expect("the set loads");
        assert!(
            bundle
                .paths
                .ground_pick
                .as_ref()
                .is_some_and(|path| path.ends_with("alpha_ground_pick.onnx")),
            "{:?}",
            bundle.paths.ground_pick
        );
        set_slot_override("ground_pick", Some("custom_pick.onnx".to_owned())).expect("override");
        let bundle = load(&dir).expect("the override loads");
        assert!(
            bundle
                .paths
                .ground_pick
                .as_ref()
                .is_some_and(|path| path.ends_with("custom_pick.onnx")),
            "{:?}",
            bundle.paths.ground_pick
        );
    }

    /// The walking set ships no standing network; an override is how a person
    /// points the slot at one without editing the manifest.
    #[test]
    fn a_stand_override_loads_a_network_the_set_does_not_ship() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "stand-override",
            &["velstand.onnx", "stand_custom.onnx"],
            None,
        );
        let bundle = load(&dir).expect("the set loads");
        assert_eq!(bundle.paths.stand, None);
        set_slot_override("stand", Some("stand_custom.onnx".to_owned())).expect("override");
        let bundle = load(&dir).expect("the override loads");
        assert!(
            bundle
                .paths
                .stand
                .as_ref()
                .is_some_and(|path| path.ends_with("stand_custom.onnx")),
            "{:?}",
            bundle.paths.stand
        );
    }

    /// An override whose file is gone costs its slot and nothing else — the
    /// official `drop_unloadable_overrides` rule. The slot falls back to the
    /// set's value, the override is dropped, and the reason is left for the
    /// health gate to take (`take_override_warnings`).
    #[test]
    fn an_unloadable_override_falls_back_and_is_reported() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "override-fallback",
            &[
                "velstand.onnx",
                "alpha_sitstand.onnx",
                "alpha_ground_pick.onnx",
            ],
            None,
        );
        set_slot_override("walk", Some("gone_walk.onnx".to_owned())).expect("walk override");
        set_slot_override("sitstand", Some("gone_sit.onnx".to_owned())).expect("sitstand override");
        set_slot_override("ground_pick", Some("gone_pick.onnx".to_owned())).expect("pick override");

        let bundle = load(&dir).expect("the set still loads");
        assert!(
            bundle.paths.walk.ends_with("velstand.onnx"),
            "{:?}",
            bundle.paths.walk
        );
        assert!(
            bundle
                .paths
                .sitstand
                .as_ref()
                .is_some_and(|path| path.ends_with("alpha_sitstand.onnx")),
            "{:?}",
            bundle.paths.sitstand
        );
        assert!(
            bundle
                .paths
                .ground_pick
                .as_ref()
                .is_some_and(|path| path.ends_with("alpha_ground_pick.onnx")),
            "{:?}",
            bundle.paths.ground_pick
        );

        // Dropped, as the config key would be on the robot: a later reload
        // must not warn about it again.
        assert_eq!(slot_override("walk"), None);
        assert_eq!(slot_override("sitstand"), None);
        assert_eq!(slot_override("ground_pick"), None);

        let warnings = take_override_warnings();
        for (slot, file) in [
            ("walk", "gone_walk.onnx"),
            ("sitstand", "gone_sit.onnx"),
            ("ground_pick", "gone_pick.onnx"),
        ] {
            assert!(
                warnings
                    .iter()
                    .any(|warning| warning.contains(slot) && warning.contains(file)),
                "no {slot} warning in {warnings:?}"
            );
        }
        assert!(
            take_override_warnings().is_empty(),
            "each warning is reported once"
        );
    }

    /// A name that could walk out of the policies directory is refused before
    /// it is stored, the same rule the C ABI applied to the walk override.
    #[test]
    fn an_override_may_not_leave_the_policies_directory() {
        let _lock = walk_test_lock();
        for bad in [
            "sub/gait.onnx",
            "sub\\gait.onnx",
            "../gait.onnx",
            "..",
            "ga..it.onnx",
        ] {
            let error = set_slot_override("ground_pick", Some(bad.to_owned())).expect_err(bad);
            assert!(error.contains("not a file name"), "{bad}: {error}");
        }
        assert_eq!(slot_override("ground_pick"), None);
    }

    /// Official `Slot::parse` names seven slots; this core's skill slots are
    /// keyed by manifest skill name instead, so they are not override targets
    /// and an unknown name is refused with the list of names it does know.
    #[test]
    fn unknown_and_skill_slots_are_refused() {
        let _lock = walk_test_lock();
        for slot in ["kick_left", "kick_right", "roulade", "walk_policy"] {
            let error = set_slot_override(slot, Some("x.onnx".to_owned())).expect_err(slot);
            assert!(error.contains("no policy slot named"), "{slot}: {error}");
        }
        let error = set_slot_override("kick_left", None).expect_err("unknown even when clearing");
        assert!(
            error.contains("expected one of walk, stand, sitstand, ground_pick"),
            "{error}"
        );
        assert_eq!(slot_override("kick_left"), None);
    }

    /// No manifest and no override is the behaviour the app has always had:
    /// the traditional file name.
    #[test]
    fn without_a_manifest_the_traditional_name_stands() {
        let _lock = walk_test_lock();
        let dir = write_set("no-manifest", &["velstand.onnx"], None);
        let bundle = load(&dir).expect("velstand loads");
        assert!(
            bundle.paths.walk.ends_with("velstand.onnx"),
            "{:?}",
            bundle.paths.walk
        );
    }

    fn walker_walk(bundle: &PolicyBundle) -> bool {
        bundle.paths.walk.ends_with("velstand.onnx")
    }

    /// The person's own skill: an episodic entry pointed at a file in the
    /// policies directory rides the next load — in the bundle's skill list
    /// (what `/skills` and MCP publish) and in the paths the policy actually
    /// loads (what the trigger runs).
    #[test]
    fn a_custom_skill_rides_the_next_load() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "custom-skill",
            &["velstand.onnx", "spin.onnx"],
            Some(
                r#"{ "policies": [ { "file": "velstand.onnx", "kind": "perpetual", "slot": "walk" } ] }"#,
            ),
        );
        load_mode(&dir, Mode::Walk).expect("the set loads");

        set_custom_skills(vec![serde_json::json!({
            "file": "spin.onnx",
            "name": "spin_left",
            "kind": "episodic",
            "duration_s": 1.5,
            "chain": true,
            "action_scale": 0.9,
            "command": { "idle": [0.0, 0.0, 0.0] },
        })])
        .expect("the entry is a skill");

        let bundle = load_mode(&dir, Mode::Walk).expect("the set loads");
        let skill = bundle
            .skills
            .iter()
            .find(|skill| skill.name == "spin_left")
            .expect("the custom skill is in the merged table");
        assert_eq!(skill.duration, 1.5);
        assert!(skill.chain);
        assert_eq!(skill.params.action_scale, Some(0.9));
        assert!(
            skill
                .path
                .as_ref()
                .is_some_and(|path| path.ends_with("spin.onnx")),
            "{:?}",
            skill.path
        );
        assert!(
            bundle
                .paths
                .skills
                .iter()
                .any(|path| path.ends_with("spin.onnx")),
            "the policy loads the file too: {:?}",
            bundle.paths.skills
        );
        clear_custom_skills();
    }

    /// A custom entry with the same name as a set skill replaces it in place
    /// — the config overlay's rule, and the way a person re-times a built-in
    /// one-shot without renaming anything.
    #[test]
    fn a_custom_skill_overrides_a_same_named_manifest_skill() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "custom-override",
            &["velstand.onnx", "roulade.onnx"],
            Some(
                r#"{ "policies": [
                    { "file": "velstand.onnx", "kind": "perpetual", "slot": "walk" },
                    { "file": "roulade.onnx", "kind": "episodic", "duration_s": 1.0, "chain": true }
                ] }"#,
            ),
        );
        load_mode(&dir, Mode::Walk).expect("the set loads");
        let bundle = load_mode(&dir, Mode::Walk).expect("the set loads");
        let roulade = bundle
            .skills
            .iter()
            .find(|skill| skill.name == "roulade")
            .expect("the set's roulade");
        assert_eq!(roulade.duration, 1.0);

        set_custom_skills(vec![serde_json::json!({
            "file": "roulade.onnx",
            "name": "roulade",
            "kind": "episodic",
            "duration_s": 2.5,
        })])
        .expect("the re-timed entry");
        let bundle = load_mode(&dir, Mode::Walk).expect("the set loads");
        let roulade = bundle
            .skills
            .iter()
            .find(|skill| skill.name == "roulade")
            .expect("roulade is still there");
        assert_eq!(roulade.duration, 2.5, "the custom timing wins");
        assert_eq!(
            bundle
                .skills
                .iter()
                .filter(|skill| skill.name == "roulade")
                .count(),
            1,
            "replaced in place, not added a second time"
        );
        clear_custom_skills();
    }

    /// A file that was never there is refused at the door (the same
    /// existence check the slot override defers to the load — deferred
    /// there because the override rides an existing config; a skill
    /// someone is adding right now must not be answered "accepted" and
    /// then silently skipped). A file that *went away* after being set is
    /// skipped **for that load only**, with a warning: the table keeps the
    /// entry, so a directory that still has the file (or the file coming
    /// back) restores the skill instead of losing the registration.
    #[test]
    fn an_unknown_file_is_refused_and_a_vanished_one_is_skipped_but_kept() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "custom-vanish",
            &["velstand.onnx", "spin.onnx"],
            Some(
                r#"{ "policies": [ { "file": "velstand.onnx", "kind": "perpetual", "slot": "walk" } ] }"#,
            ),
        );
        load_mode(&dir, Mode::Walk).expect("the set loads");

        let error = set_custom_skills(vec![serde_json::json!({
            "file": "gone.onnx",
            "name": "phantom",
            "kind": "episodic",
            "duration_s": 1.0,
        })])
        .expect_err("gone.onnx is not in the directory");
        assert!(error.contains("no such file"), "{error}");
        assert!(
            error.contains("phantom"),
            "the refusal names the entry: {error}"
        );
        assert!(
            custom_skills().is_empty(),
            "the refused table is not stored"
        );

        // The entry that was really there rides the load, then its file is
        // deleted: the next load skips it with a warning, and the table
        // keeps it — a registration is not destroyed by a directory that
        // happened to be mid-copy.
        set_custom_skills(vec![serde_json::json!({
            "file": "spin.onnx",
            "name": "spin_left",
            "kind": "episodic",
            "duration_s": 1.0,
        })])
        .expect("the entry is a skill");
        std::fs::remove_file(dir.join("spin.onnx")).expect("the file goes away");
        let bundle = load_mode(&dir, Mode::Walk).expect("the set still loads");
        assert!(
            !bundle.skills.iter().any(|skill| skill.name == "spin_left"),
            "the vanished skill is skipped"
        );
        let warnings = take_override_warnings();
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("spin_left") && warning.contains("spin.onnx")),
            "no spin_left warning in {warnings:?}"
        );
        assert!(
            custom_skills().contains_key("spin_left"),
            "the entry survives the load that could not use it"
        );

        // The file comes back (a copy finishing, or the same directory next
        // launch): so does the skill, with no re-registration.
        std::fs::write(dir.join("spin.onnx"), b"stub").expect("the file returns");
        let bundle = load_mode(&dir, Mode::Walk).expect("the set still loads");
        assert!(
            bundle.skills.iter().any(|skill| skill.name == "spin_left"),
            "the kept entry loads again: {:?}",
            bundle.skills
        );
        assert!(take_override_warnings().is_empty());
    }

    /// A skill's name doubles as its `<name>.onnx` file stem when the entry
    /// names no file. A name that could walk out of the policies directory —
    /// `..`, an absolute path, either separator — is refused at every door,
    /// and the load never brings in an `.onnx` from outside the directory.
    #[test]
    fn a_skill_name_that_could_leave_the_directory_is_refused() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "custom-name",
            &["velstand.onnx"],
            Some(
                r#"{ "policies": [ { "file": "velstand.onnx", "kind": "perpetual", "slot": "walk" } ] }"#,
            ),
        );
        load_mode(&dir, Mode::Walk).expect("the set loads");

        // A real `.onnx` one level above the policies directory, so "nothing
        // outside was loaded" is an assertion about a file that exists.
        let outside = dir
            .parent()
            .expect("a temp parent")
            .join(format!("outside-{}.onnx", std::process::id()));
        std::fs::write(&outside, b"stub").expect("a file outside the directory");
        let escape_name = format!("../{}", outside.file_stem().unwrap().to_string_lossy());
        for bad in [
            escape_name.as_str(),
            "../../evil",
            "../evil",
            "sub/evil",
            "sub\\evil",
            "/abs/evil",
            "C:evil",
            "..",
        ] {
            // `robot.setSkill`'s door (a new skill with no path: `<name>.onnx`).
            let error = upsert_custom_skill(SkillDef {
                name: bad.to_owned(),
                path: None,
                duration: 1.0,
                ..SkillDef::default()
            })
            .expect_err(bad);
            assert!(
                error.contains("not a file name"),
                "{bad:?} must be refused as a file name: {error}"
            );
            // `/config`'s door.
            let error = set_custom_skills(vec![serde_json::json!({
                "file": "velstand.onnx",
                "name": bad,
                "kind": "episodic",
                "duration_s": 1.0,
            })])
            .expect_err(bad);
            assert!(
                error.contains("not a file name"),
                "{bad:?} must be refused as a file name: {error}"
            );
            assert!(custom_skills().is_empty(), "{bad:?} was stored: {error}");
        }

        let bundle = load_mode(&dir, Mode::Walk).expect("the set loads");
        assert!(
            bundle.skills.iter().all(|skill| skill
                .path
                .as_ref()
                .is_none_or(|path| path.starts_with(&dir))),
            "nothing outside the policies directory was loaded: {:?}",
            bundle.skills
        );
        let _ = std::fs::remove_file(&outside);
    }

    /// `path: "none"` is the official switch-off: the entry names no file, so
    /// it is stored without a file check, and the next load removes the
    /// same-named skill from the table instead of adding one. Removing the
    /// table entry brings the set's own skill back.
    #[test]
    fn the_none_path_switches_a_set_skill_off_and_back() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "custom-off",
            &["velstand.onnx", "roulade.onnx"],
            Some(
                r#"{ "policies": [
                    { "file": "velstand.onnx", "kind": "perpetual", "slot": "walk" },
                    { "file": "roulade.onnx", "kind": "episodic", "duration_s": 1.0 }
                ] }"#,
            ),
        );
        load_mode(&dir, Mode::Walk).expect("the set loads");
        let bundle = load_mode(&dir, Mode::Walk).expect("the set loads");
        assert!(
            bundle.skills.iter().any(|skill| skill.name == "roulade"),
            "the set has roulade to switch off"
        );

        upsert_custom_skill(SkillDef {
            name: "roulade".to_owned(),
            path: Some(PathBuf::from("none")),
            duration: 1.0,
            ..SkillDef::default()
        })
        .expect("the sentinel names no file, so there is nothing to check");

        let bundle = load_mode(&dir, Mode::Walk).expect("the set loads");
        assert!(
            !bundle.skills.iter().any(|skill| skill.name == "roulade"),
            "the switched-off skill is out of the table: {:?}",
            bundle.skills
        );
        assert!(
            !bundle
                .paths
                .skills
                .iter()
                .any(|path| path.ends_with("roulade.onnx")),
            "and the policy does not load its file: {:?}",
            bundle.paths.skills
        );

        // The official rule: a skill the release ships comes back when the
        // override is removed.
        assert!(remove_custom_skill("roulade"));
        let bundle = load_mode(&dir, Mode::Walk).expect("the set loads");
        assert!(
            bundle.skills.iter().any(|skill| skill.name == "roulade"),
            "removing the switch-off restores the set's skill"
        );
    }

    /// The whole-table semantics: an empty array clears, a later entry with
    /// the same name overwrites an earlier one, and a rejected table leaves
    /// the previous one in force.
    #[test]
    fn the_custom_skill_table_replaces_and_clears_whole() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "custom-replace",
            &["velstand.onnx", "spin.onnx", "bow.onnx"],
            Some(
                r#"{ "policies": [ { "file": "velstand.onnx", "kind": "perpetual", "slot": "walk" } ] }"#,
            ),
        );
        load_mode(&dir, Mode::Walk).expect("the set loads");

        set_custom_skills(vec![
            serde_json::json!({
                "file": "spin.onnx", "name": "spin_left",
                "kind": "episodic", "duration_s": 1.0,
            }),
            serde_json::json!({
                "file": "bow.onnx", "name": "polite_bow",
                "kind": "episodic", "duration_s": 4.0,
            }),
        ])
        .expect("two entries");
        assert_eq!(custom_skills().len(), 2);

        // Same name, new numbers: the later entry wins.
        set_custom_skills(vec![serde_json::json!({
            "file": "spin.onnx", "name": "spin_left",
            "kind": "episodic", "duration_s": 2.0,
        })])
        .expect("the replaced entry");
        let table = custom_skills();
        assert_eq!(table.len(), 1, "the table is replaced, not appended to");
        assert_eq!(table["spin_left"].duration, 2.0);

        // An empty array is the official reset.
        set_custom_skills(Vec::new()).expect("the clear");
        assert!(custom_skills().is_empty());

        // One bad row refuses the whole request, and the table already in
        // force stays.
        set_custom_skills(vec![serde_json::json!({
            "file": "bow.onnx", "name": "polite_bow",
            "kind": "episodic", "duration_s": 4.0,
        })])
        .expect("back in force");
        let error = set_custom_skills(vec![
            serde_json::json!({
                "file": "bow.onnx", "name": "polite_bow",
                "kind": "episodic", "duration_s": 4.0,
            }),
            serde_json::json!({
                "file": "spin.onnx", "name": "spin_left",
                "kind": "perpetual",
            }),
        ])
        .expect_err("a perpetual entry is not a skill");
        assert!(error.contains("spin_left"), "{error}");
        assert_eq!(custom_skills().len(), 1, "the old table stands");
        clear_custom_skills();
    }

    /// Every shape the door refuses, each with the entry named and the
    /// reason said: the daemon-driven names, a non-episodic kind, a phase
    /// command, a missing name, a missing file, a path out of the
    /// directory, a missing and a non-positive duration, and an entry that
    /// is not an object at all.
    #[test]
    fn bad_custom_skill_entries_are_refused_with_entry_and_reason() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "custom-bad",
            &["velstand.onnx", "spin.onnx"],
            Some(
                r#"{ "policies": [ { "file": "velstand.onnx", "kind": "perpetual", "slot": "walk" } ] }"#,
            ),
        );
        load_mode(&dir, Mode::Walk).expect("the set loads");

        for (entry, expected) in [
            (
                serde_json::json!({
                    "file": "spin.onnx", "name": "ground_pick",
                    "kind": "episodic", "duration_s": 1.0,
                }),
                "driven by the robot itself",
            ),
            (
                serde_json::json!({
                    "file": "spin.onnx", "name": "sit_toggle",
                    "kind": "episodic", "duration_s": 1.0,
                }),
                "driven by the robot itself",
            ),
            (
                serde_json::json!({
                    "file": "spin.onnx", "name": "hold_still",
                    "kind": "perpetual", "duration_s": 1.0,
                }),
                "`kind` must be \"episodic\"",
            ),
            (
                serde_json::json!({
                    "file": "spin.onnx", "name": "crouch",
                    "kind": "episodic", "duration_s": 3.5,
                    "command": { "encoding": "phase", "period_s": 4.0, "end_phase": 0.7 },
                }),
                "the daemon drives itself",
            ),
            (
                serde_json::json!({
                    "file": "spin.onnx", "kind": "episodic", "duration_s": 1.0,
                }),
                "a skill needs a name",
            ),
            (
                serde_json::json!({
                    "file": "spin.onnx", "name": "   ",
                    "kind": "episodic", "duration_s": 1.0,
                }),
                "a skill needs a name",
            ),
            (
                serde_json::json!({
                    "name": "no_file", "kind": "episodic", "duration_s": 1.0,
                }),
                "a skill needs a file",
            ),
            (
                serde_json::json!({
                    "file": "gone.onnx", "name": "phantom",
                    "kind": "episodic", "duration_s": 1.0,
                }),
                "no such file",
            ),
            (
                serde_json::json!({
                    "file": "../escape.onnx", "name": "escape",
                    "kind": "episodic", "duration_s": 1.0,
                }),
                "not a file name",
            ),
            (
                serde_json::json!({
                    "file": "spin.onnx", "name": "lengthless", "kind": "episodic",
                }),
                "needs a duration",
            ),
            (
                serde_json::json!({
                    "file": "spin.onnx", "name": "instant",
                    "kind": "episodic", "duration_s": 0.0,
                }),
                "must be more than zero",
            ),
            (serde_json::json!("a bow"), "not a manifest policy entry"),
        ] {
            let error = set_custom_skills(vec![entry]).expect_err("refused");
            assert!(
                error.contains("custom_skills[0]"),
                "the refusal names the entry: {error}"
            );
            assert!(error.contains(expected), "{expected}: {error}");
            assert!(custom_skills().is_empty(), "nothing was stored: {error}");
        }
    }

    /// `robot.setSkill`'s single-entry half: a merged `SkillDef` upserts by
    /// name, and a missing file or a zero duration is refused with the
    /// official sentences.
    #[test]
    fn upsert_adds_and_replaces_one_custom_skill() {
        let _lock = walk_test_lock();
        let dir = write_set(
            "custom-upsert",
            &["velstand.onnx", "spin.onnx"],
            Some(
                r#"{ "policies": [ { "file": "velstand.onnx", "kind": "perpetual", "slot": "walk" } ] }"#,
            ),
        );
        load_mode(&dir, Mode::Walk).expect("the set loads");

        upsert_custom_skill(SkillDef {
            name: "spin_left".to_owned(),
            path: Some(PathBuf::from("spin.onnx")),
            duration: 1.5,
            chain: true,
            ..SkillDef::default()
        })
        .expect("the entry upserts");
        assert_eq!(custom_skills()["spin_left"].duration, 1.5);
        upsert_custom_skill(SkillDef {
            name: "spin_left".to_owned(),
            path: Some(PathBuf::from("spin.onnx")),
            duration: 2.5,
            ..SkillDef::default()
        })
        .expect("the entry replaces");
        let table = custom_skills();
        assert_eq!(table.len(), 1);
        assert_eq!(table["spin_left"].duration, 2.5);
        assert!(!table["spin_left"].chain, "the replacement is whole");

        let error = upsert_custom_skill(SkillDef {
            name: "phantom".to_owned(),
            path: Some(PathBuf::from("gone.onnx")),
            duration: 1.0,
            ..SkillDef::default()
        })
        .expect_err("the file is not there");
        assert!(error.contains("no such file"), "{error}");
        let error = upsert_custom_skill(SkillDef {
            name: "instant".to_owned(),
            path: Some(PathBuf::from("spin.onnx")),
            duration: 0.0,
            ..SkillDef::default()
        })
        .expect_err("no duration, no skill");
        assert!(error.contains("more than zero"), "{error}");

        assert!(remove_custom_skill("spin_left"), "the entry came out");
        assert!(!remove_custom_skill("spin_left"), "and only once");
        assert!(custom_skills().is_empty());
    }
}
