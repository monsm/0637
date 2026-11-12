//! The policy set, resolved the way `robotd` resolves it.
//!
//! The manifest types are the official ones (`robotd_params::SetManifest`), so a
//! set published for the robot is read the same way here: an episodic
//! constant-command entry is a skill, a phase-encoded one is the ground pick, a
//! posture-flag one is the sit/stand, and the walk policy is velstand.

use std::path::{Path, PathBuf};

use duck_control::policy::PolicyPaths;
use robotd_params::{Mode, SetManifest, SetPolicy, SkillDef, SkillOverrides};

use crate::control::{SkillTuning, Tuning};

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

/// Read `manifest.json`, accepting both shapes the ecosystem publishes.
///
/// The official set is one manifest with a `policies` array. A community
/// repo ships one policy and a *flat* manifest — the fields of a single
/// entry with no array — and `SetManifest`'s `#[serde(default)]` turns that
/// into an empty set, which used to leave the app with no one-shot skills at
/// all. Parse the flat shape too, taking the only `.onnx` beside it when the
/// entry does not name its file.
fn read_manifest(dir: &Path) -> Option<SetManifest> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    if let Ok(set) = serde_json::from_str::<SetManifest>(&text) {
        if !set.policies.is_empty() {
            return Some(set);
        }
    }
    let mut policy: SetPolicy = serde_json::from_str(&text).ok()?;
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
    Some(SetManifest {
        policies: vec![policy],
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

    // Walking keeps the app's long-standing preference: velstand is the
    // current gait (a walking set also ships alpha_walking, which is older).
    // A non-walking mode takes the manifest's perpetual entry tagged with it.
    let walk = match mode {
        Mode::Walk => ["velstand.onnx", "alpha_walking.onnx"]
            .iter()
            .map(|file| dir.join(file))
            .find(|path| path.exists()),
        Mode::Roller => manifest
            .as_ref()
            .and_then(|manifest| {
                manifest
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
            .policies
            .iter()
            .find(|policy| policy.file.as_str() == walk_file)
            .and_then(|policy| policy.action_scale)
    });

    let sitstand = manifest
        .as_ref()
        .and_then(|manifest| {
            manifest
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
            skills_from(manifest, dir),
            manifest.ground_pick(mode),
            manifest.sitstand(),
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

    #[test]
    fn official_manifest_resolves_walk_slots_and_skills() {
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

    fn walker_walk(bundle: &PolicyBundle) -> bool {
        bundle.paths.walk.ends_with("velstand.onnx")
    }
}
