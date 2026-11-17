//! The policy set, resolved the way `robotd` resolves it.
//!
//! The manifest types are the official ones (`robotd_params::SetManifest`), so a
//! set published for the robot is read the same way here: an episodic
//! constant-command entry is a skill, a phase-encoded one is the ground pick, a
//! posture-flag one is the sit/stand, and the walk policy is velstand.

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

/// The walk-slot override: the phone's twin of the official `[policy] walk`
/// config key, which resolves ahead of everything a set declares
/// (`robotd-params`' `ResolvedPolicy.walk`, config over default). The name is
/// relative to the policies directory; the official `"none"` sentinel clears
/// it. It is a process global because the official config key is one.
static WALK_OVERRIDE: Mutex<Option<String>> = Mutex::new(None);

/// Point the walk slot at one file in the policies directory — `robotctl
/// policy load walk <file>` has no robotctl on a phone, so the shell sets the
/// override here and reloads. `None` clears the override so the next reload
/// resolves the set's own walk slot again.
pub fn set_walk_override(name: Option<String>) {
    *WALK_OVERRIDE.lock().unwrap() = name.filter(|name| !name.is_empty());
}

/// The current walk-slot override, if one is set.
pub fn walk_override() -> Option<String> {
    WALK_OVERRIDE.lock().unwrap().clone()
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
    let slot = serde_json::from_str::<SlotFlat>(&text).ok().and_then(|flat| flat.slot);
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

    let walk_file = walk.file_name().and_then(|name| name.to_str()).unwrap_or("");
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
        sitstand_rise_s: sit_timing
            .map_or(robotd_params::DEFAULT_SITSTAND_RISE_S, |timing| timing.rise_s),
        sitstand_ramp_s: sit_timing
            .map_or(robotd_params::DEFAULT_SITSTAND_RAMP_S, |timing| timing.ramp_s),
        skills: skills.clone(),
    };

    let paths = PolicyPaths {
        walk,
        // **No standing network in either mode**, which is the robot's own
        // default: velstand (set v5) stands still at zero command on its own,
        // and `[policy] stand = "none"` in the shipped config. `alpha_stand`
        // ships in the set for sets that predate velstand, not for this one.
        stand: None,
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
    use std::sync::{Mutex, MutexGuard};

    /// The walk override is a process global. Tests that resolve a walk slot
    /// take this lock so a parallel test never reads another test's override.
    static WALK_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Hold the lock for the duration of a walk-resolution test, with the
    /// override guaranteed clear — the state every other test assumes.
    fn walk_test_lock() -> MutexGuard<'static, ()> {
        let guard = WALK_TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        set_walk_override(None);
        guard
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
        assert!(bundle
            .paths
            .sitstand
            .as_ref()
            .is_some_and(|path| path.ends_with("alpha_sitstand.onnx")));
        assert!(bundle
            .paths
            .ground_pick
            .as_ref()
            .is_some_and(|path| path.ends_with("alpha_ground_pick.onnx")));
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

        assert!(walker_walk(&walk), "walking keeps velstand: {:?}", walk.paths.walk);
        assert!(roller.paths.walk.ends_with("roller.onnx"));
        assert!(roller.standing_disabled, "the roller has no standing network");
        assert_eq!(roller.paths.stand, None);
        assert!(roller
            .paths
            .ground_pick
            .as_ref()
            .is_some_and(|path| path.ends_with("roller_crouch.onnx")));
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
        let dir = write_set(
            "model-api",
            &["velstand.onnx"],
            Some(r#"{ "model_api": 3, "policies": [ { "file": "velstand.onnx", "kind": "perpetual" } ] }"#),
        );
        let error = load(&dir).expect_err("model API 3 is newer than the daemon");
        assert!(error.contains("model API 3"), "{error}");
        assert!(error.contains("update the daemon first"), "{error}");
    }

    #[test]
    fn a_manifest_for_another_robot_is_refused() {
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
        assert!(bundle.paths.walk.ends_with("my_gait.onnx"), "{:?}", bundle.paths.walk);
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
        assert!(bundle.paths.walk.ends_with("my_gait.onnx"), "{:?}", bundle.paths.walk);
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
        assert!(bundle.paths.walk.ends_with("chosen.onnx"), "{:?}", bundle.paths.walk);
        // And the official `"none"` sentinel clears it again.
        set_walk_override(Some("none".to_owned()));
        let bundle = load(&dir).expect("the sentinel clears the override");
        assert!(bundle.paths.walk.ends_with("my_gait.onnx"), "{:?}", bundle.paths.walk);
        set_walk_override(None);
    }

    /// No manifest and no override is the behaviour the app has always had:
    /// the traditional file name.
    #[test]
    fn without_a_manifest_the_traditional_name_stands() {
        let _lock = walk_test_lock();
        let dir = write_set("no-manifest", &["velstand.onnx"], None);
        let bundle = load(&dir).expect("velstand loads");
        assert!(bundle.paths.walk.ends_with("velstand.onnx"), "{:?}", bundle.paths.walk);
    }

    fn walker_walk(bundle: &PolicyBundle) -> bool {
        bundle.paths.walk.ends_with("velstand.onnx")
    }
}
