//! The repo's real assets, found without hard-coding one machine's paths.
//!
//! The core's tests read the *official* files — the policy bundle and the
//! training scene — because a manifest test that runs against a made-up
//! bundle proves nothing. Those files are not in the repository: they are
//! fetched (`scripts/fetch-policies.sh`, `upstream/microduck_rl`), so a test
//! has to find them wherever the checkout happens to live — a developer's
//! machine, a CI runner, a build agent that has never seen this project.
//!
//! Two rules keep that honest:
//!
//! * **Resolve, don't assume.** The location comes from the workspace this
//!   crate was built in (`CARGO_MANIFEST_DIR`), or from `DUCK_POLICIES_DIR`
//!   when the assets live somewhere else. No `D:\AI\Duck` anywhere: that
//!   path only ever existed on one machine, and a test that can only run
//!   there is a test nobody runs.
//! * **Never skip silently.** A test that quietly returns when the assets
//!   are missing reports "ok" while checking nothing — the worst kind of
//!   green. So the skip says so, and `DUCK_REQUIRE_FIXTURES=1` (what CI
//!   sets) turns a missing asset into a failure instead: CI always fetches
//!   them, so there a missing asset is a broken build, not a developer
//!   without a download.

use std::path::{Path, PathBuf};

/// The repository root, derived from where this crate was built.
///
/// `CARGO_MANIFEST_DIR` is `<repo>/core/duck-mobile-core`, so two levels up
/// is the workspace that holds `policies/` and `upstream/`.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Whether a missing asset should fail the test rather than skip it.
///
/// CI sets `DUCK_REQUIRE_FIXTURES=1`: the workflow always fetches the
/// official files, so a bundle that is not there is a broken build — not a
/// developer who has not downloaded anything yet.
pub fn required() -> bool {
    std::env::var_os("DUCK_REQUIRE_FIXTURES").is_some_and(|value| value != "0")
}

/// The places the official policy bundle is looked for, in order.
fn policy_candidates(explicit: Option<PathBuf>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(explicit) = explicit {
        candidates.push(explicit);
    }
    candidates.push(workspace_root().join("policies"));
    candidates
}

/// The first candidate that really holds the bundle.
fn find_policy_dir(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|dir| dir.join("manifest.json").is_file())
        .cloned()
}

/// Say that an asset is not here, and decide whether that is fatal.
///
/// A developer without the assets gets an honest "skipped". CI, which always
/// fetches them, gets a failure that names what was missing and where it
/// was looked for — because a green test that checked nothing is the
/// failure mode this replaces.
fn missing(what: &str, searched: &[PathBuf]) {
    let list = searched
        .iter()
        .map(|dir| dir.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if required() {
        panic!("{what} is missing (searched: {list})");
    }
    eprintln!("skipping: {what} is not here (searched: {list})");
}

/// The official policy bundle, when it is here.
///
/// `None` means the assets are not in this workspace: the caller should skip
/// rather than assert, unless [`required`] says otherwise.
pub fn policies_dir() -> Option<PathBuf> {
    let candidates = policy_candidates(std::env::var_os("DUCK_POLICIES_DIR").map(PathBuf::from));
    match find_policy_dir(&candidates) {
        Some(dir) => Some(dir),
        None => {
            missing("the official policy bundle", &candidates);
            None
        }
    }
}

/// Whether the bundle really carries these files.
///
/// A bundle that exists but is missing `roller.onnx` is not the official
/// bundle, and a test that read it would be checking a fiction.
pub fn has_all(dir: &Path, names: &[&str]) -> bool {
    let absent: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| !dir.join(name).is_file())
        .collect();
    if absent.is_empty() {
        return true;
    }
    if required() {
        panic!(
            "the official policy bundle is missing {} (in {})",
            absent.join(", "),
            dir.display()
        );
    }
    eprintln!(
        "skipping: the policy bundle has no {} (in {})",
        absent.join(", "),
        dir.display()
    );
    false
}

/// The training scene the fingerprint is taken over.
///
/// It lives in `upstream/microduck_rl`, which is fetched like the policies.
pub fn scene_backlash() -> Option<PathBuf> {
    let scene = workspace_root().join(
        "upstream/microduck_rl/src/mjlab_microduck/robot/microduck/scene_backlash.xml",
    );
    if scene.is_file() {
        return Some(scene);
    }
    missing("the training scene", &[scene.clone()]);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_workspace_root_is_the_repository_root() {
        let root = workspace_root();
        assert!(
            root.join("core").join("duck-mobile-core").is_dir(),
            "the workspace root must be the repository root, got {}",
            root.display()
        );
    }

    /// The bundle is looked for where it can actually be, and an explicit
    /// directory comes first so a checkout that keeps its assets elsewhere
    /// still finds them.
    #[test]
    fn the_bundle_is_looked_for_where_it_can_be() {
        let candidates = policy_candidates(Some(PathBuf::from("/nowhere")));
        assert_eq!(candidates[0], PathBuf::from("/nowhere"));
        assert!(
            candidates[1].ends_with("policies"),
            "the workspace's own policies directory is the fallback, got {}",
            candidates[1].display()
        );
    }

    /// A directory without a manifest is not the bundle. Saying so is what
    /// keeps a half-fetched bundle from being read as the official one.
    #[test]
    fn a_directory_without_a_manifest_is_not_the_bundle() {
        let empty = std::env::temp_dir().join("duck-empty-policy-dir");
        std::fs::create_dir_all(&empty).expect("temp dir");
        assert!(find_policy_dir(&[empty]).is_none());
    }
}
