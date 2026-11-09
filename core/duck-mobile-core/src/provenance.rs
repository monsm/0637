//! What this build actually is.
//!
//! A physical duck cannot prove which weights it is running. The app can:
//! every file that decides behaviour — the scene, the meshes, the policies,
//! the manifest — is hashed into one fingerprint at startup, logged once, and
//! served on `GET /fingerprint`. Two builds with the same fingerprint run the
//! same duck.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[cfg(feature = "mujoco")]
use crate::mujoco;

/// One file's contribution, in sorted order.
#[derive(Debug, Clone)]
pub struct Part {
    pub name: String,
    pub digest: String,
}

#[derive(Debug, Clone)]
pub struct Provenance {
    pub fingerprint: String,
    pub mujoco: String,
    pub parts: Vec<Part>,
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hash_file(path: &Path) -> Option<[u8; 32]> {
    let bytes = std::fs::read(path).ok()?;
    Some(Sha256::digest(&bytes).into())
}

fn collect_files(scene: &Path, policies: &Path) -> Vec<PathBuf> {
    let mut files = vec![scene.to_path_buf()];
    let manifest = policies.join("manifest.json");
    if manifest.exists() {
        files.push(manifest);
    }
    if let Ok(entries) = std::fs::read_dir(policies) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("onnx") {
                files.push(path);
            }
        }
    }
    // The scene references its meshes from a sibling `assets` directory.
    if let Some(dir) = scene.parent().map(|dir| dir.join("assets")) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) == Some("stl") {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files
}

/// Hash everything that decides behaviour into one fingerprint.
pub fn collect(scene: &Path, policies: &Path) -> Provenance {
    #[cfg(feature = "mujoco")]
    let mujoco_version = mujoco::version();
    #[cfg(not(feature = "mujoco"))]
    let mujoco_version = "unlinked".to_owned();
    let mut overall = Sha256::new();
    overall.update(b"duck-provenance-v1");
    overall.update(mujoco_version.as_bytes());

    let mut parts = Vec::new();
    for path in collect_files(scene, policies) {
        let Some(digest) = hash_file(&path) else { continue };
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_owned();
        overall.update(name.as_bytes());
        overall.update(digest);
        parts.push(Part {
            name,
            digest: hex(&digest),
        });
    }

    Provenance {
        fingerprint: hex(&overall.finalize()),
        mujoco: mujoco_version,
        parts,
    }
}

impl Provenance {
    /// A JSON document, for `GET /fingerprint`.
    pub fn to_json(&self) -> String {
        let parts: Vec<String> = self
            .parts
            .iter()
            .map(|part| format!("{{\"file\":{},\"sha256\":\"{}\"}}", json_string(&part.name), part.digest))
            .collect();
        format!(
            "{{\"schema\":\"duck-provenance-v1\",\"fingerprint\":\"{}\",\"mujoco\":{},\"files\":[{}]}}",
            self.fingerprint,
            json_string(&self.mujoco),
            parts.join(",")
        )
    }
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", control as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_files_give_the_same_fingerprint() {
        let dir = Path::new(r"D:\AI\Duck\policies");
        if !dir.join("manifest.json").exists() {
            return;
        }
        let scene = Path::new(
            r"D:\AI\Duck\upstream\microduck_rl\src\mjlab_microduck\robot\microduck\scene_backlash.xml",
        );
        let first = collect(scene, dir);
        let second = collect(scene, dir);
        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(first.fingerprint.len(), 64);
        assert!(first.parts.len() >= 10, "manifest + policies");
        assert!(first.to_json().contains("\"fingerprint\""));
    }
}
