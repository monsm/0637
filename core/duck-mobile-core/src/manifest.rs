//! The policy set, resolved the way `robotd` resolves it.
//!
//! The manifest types are the official ones (`robotd_params::SetManifest`), so a
//! set published for the robot is read the same way here: an episodic
//! constant-command entry is a skill, a phase-encoded one is the ground pick, a
//! posture-flag one is the sit/stand, and the walk policy is velstand.

use std::path::{Path, PathBuf};

use duck_control::policy::PolicyPaths;
use robotd_params::{Mode, SetManifest, SkillDef, SkillOverrides};

use crate::control::{SkillTuning, Tuning};

pub struct PolicyBundle {
    pub paths: PolicyPaths,
    pub skills: Vec<SkillDef>,
    pub skill_tuning: SkillTuning,
    /// The walk entry's own action scale, when the set declares one.
    pub action_scale: Option<f64>,
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

pub fn load(dir: &Path) -> Result<PolicyBundle, String> {
    let manifest: Option<SetManifest> = std::fs::read_to_string(dir.join("manifest.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());

    let walk = ["velstand.onnx", "alpha_walking.onnx"]
        .iter()
        .map(|file| dir.join(file))
        .find(|path| path.exists())
        .ok_or_else(|| format!("no walk policy in {}", dir.display()))?;

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
                .find(|policy| policy.mode() == Mode::Walk && policy.phase_timing().is_some())
        })
        .map(|policy| dir.join(&policy.file))
        .filter(|path| path.exists())
        .or_else(|| existing(dir.join("alpha_ground_pick.onnx")));

    let (skills, pick_timing, sit_timing) = match manifest.as_ref() {
        Some(manifest) => (
            skills_from(manifest, dir),
            manifest.ground_pick(Mode::Walk),
            manifest.sitstand(),
        ),
        None => (fallback_skills(dir), None, None),
    };

    let skill_tuning = SkillTuning {
        ground_pick_period: pick_timing.map_or(4.0, |timing| timing.period_s),
        ground_pick_end_phase: pick_timing
            .map_or(robotd_params::DEFAULT_GROUND_PICK_END_PHASE, |timing| {
                timing.end_phase
            }),
        ground_pick_action_scale: pick_timing
            .and_then(|timing| timing.action_scale)
            .unwrap_or(1.0),
        ground_pick_gain_ratio: 1.0,
        sitstand_rise_s: sit_timing
            .map_or(robotd_params::DEFAULT_SITSTAND_RISE_S, |timing| timing.rise_s),
        sitstand_ramp_s: sit_timing
            .map_or(robotd_params::DEFAULT_SITSTAND_RAMP_S, |timing| timing.ramp_s),
        skills: skills.clone(),
    };

    let paths = PolicyPaths {
        walk,
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
    })
}

fn existing(path: PathBuf) -> Option<PathBuf> {
    path.exists().then_some(path)
}

fn skills_from(manifest: &SetManifest, dir: &Path) -> Vec<SkillDef> {
    manifest
        .skills()
        .filter(|policy| policy.mode() == Mode::Walk)
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
        let dir = Path::new(r"D:\AI\Duck\policies");
        if !dir.join("manifest.json").exists() {
            return;
        }
        let bundle = load(dir).expect("manifest");
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
}
