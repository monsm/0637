#!/usr/bin/env bash
# Fetch the official policy set into policies/.
#
# The files are not in the repository: they are the released MicroDuck
# policies, published by Pollen/Hugging Face. The manifest is the download
# list — its `policies[].file` entries name every network in the set, so a
# policy added upstream is fetched without editing this script. The whole set
# is fetched every time, with the manifest moved into place last so a failed
# download never leaves a manifest claiming a complete set.
#
# The base URL names a ref, not necessarily a branch: point DUCK_POLICY_BASE
# at a commit or tag to pin the set (the default is `main`).
#
# Usage: scripts/fetch-policies.sh [destination]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${1:-$ROOT/policies}"
BASE="${DUCK_POLICY_BASE:-https://huggingface.co/pollen-robotics/microduck-policies/resolve/main}"
BASE="${BASE%/}"

mkdir -p "$DEST"

MANIFEST_TMP="$(mktemp "$DEST/manifest.json.XXXXXX")"
trap 'rm -f "$MANIFEST_TMP"' EXIT

curl -L --fail --retry 3 -o "$MANIFEST_TMP" "$BASE/manifest.json"

# The manifest is somebody else's document, and DUCK_POLICY_BASE is
# configurable, so a `file` naming `../../etc/anything` would otherwise choose
# where the download lands. Take only plain file names — the same rule the
# official updater applies in `files_in_manifest` (updater/src/policy.rs): no
# `/`, no `\`, nothing starting with `.` (which also rejects `..`).
FILES="$(python3 - "$MANIFEST_TMP" <<'PY'
import json
import sys

path = sys.argv[1]
try:
    with open(path, encoding="utf-8") as fh:
        manifest = json.load(fh)
except (OSError, ValueError) as error:
    raise SystemExit(f"fetch-policies: cannot read {path}: {error}")

policies = manifest.get("policies")
if not isinstance(policies, list):
    raise SystemExit("fetch-policies: manifest has no policies array")

files = []
for policy in policies:
    if not isinstance(policy, dict):
        continue
    name = policy.get("file")
    if not isinstance(name, str) or not name:
        continue
    if (
        "/" in name
        or "\\" in name
        or name.startswith(".")
        or "\n" in name
        or "\r" in name
    ):
        raise SystemExit(f"fetch-policies: refusing policy file name {name!r}")
    if name not in files:
        files.append(name)

if not files:
    raise SystemExit("fetch-policies: manifest lists no policy files")

# Write LF endings even when this runs on Windows: each line is one file
# name, and a stray CR would become part of the URL.
sys.stdout.buffer.write(("\n".join(files) + "\n").encode("utf-8"))
PY
)"
while IFS= read -r f; do
  [ -n "$f" ] || continue
  if [ -s "$DEST/$f" ]; then
    continue
  fi
  # Download beside the target and rename, so a truncated transfer is never
  # mistaken for the cached file on the next run.
  curl -L --fail --retry 3 -o "$DEST/$f.part" "$BASE/$f"
  if [ ! -s "$DEST/$f.part" ]; then
    echo "fetch-policies: $f downloaded empty" >&2
    exit 1
  fi
  mv "$DEST/$f.part" "$DEST/$f"
done <<< "$FILES"

# Last, so a failed transfer above leaves no manifest claiming a complete set.
mv "$MANIFEST_TMP" "$DEST/manifest.json"
trap - EXIT

echo "official policies in $DEST:"
ls -la "$DEST"
