#!/usr/bin/env python3
"""Warn when a push touches the duck's fidelity-carrying files.

The 61-dim observation, the actuator model and the safety logic live in a
small set of core files plus the vendored upstream. A UI/UX batch must
never change their *semantics*; a legit core bug fix (there is a history of
them) is allowed. This guard is deliberately WARNING-level: it names the
touched files for human review instead of failing the run — failing here
would block honest core fixes, and staying silent would let a UI batch
sneak a semantics change past review. The next step up (a required check
with an explicit allowlist) is planned once the ratchet has run a while.

On push events the workflow passes the previous commit; with no diff
available (new branch, force push, dispatch) it reports what it can.

Usage: check-protected-paths.py [repo-root] [before-sha]
"""
import subprocess
import sys
from pathlib import Path

# Files carrying the 61-dim observation, MuJoCo physics, the BAM actuator
# model, or the safety state machine. ui/ does NOT belong here (shader
# uniforms and look flags are presentation).
PROTECTED = [
    "core/duck-mobile-core/src/sensors.rs",
    "core/duck-mobile-core/src/mujoco.rs",
    "core/duck-mobile-core/src/runtime.rs",
    "core/duck-mobile-core/src/control.rs",
    "core/duck-mobile-core/src/bam.rs",
    "core/duck-mobile-core/src/pickup.rs",
    "upstream/",
    "policies/",
]


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(".")
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    before = argv[2] if len(argv) > 2 else None
    if not before or set(before) == {"0"} or len(before) != 40:
        print("protected-paths: no before-sha on this event; nothing to diff")
        return 0
    diff = subprocess.run(
        ["git", "diff", "--name-only", before, "HEAD"],
        cwd=root, capture_output=True, text=True,
    )
    if diff.returncode != 0:
        print("protected-paths: diff unavailable (shallow clone?)")
        return 0
    touched = [line.strip() for line in diff.stdout.splitlines() if line.strip()]
    hits = [f for f in touched for p in PROTECTED if f.startswith(p) or f == p]
    if not hits:
        print(f"protected-paths: {len(touched)} files changed, none in protected territory")
        return 0
    print(f"::warning::protected-paths: {len(hits)} fidelity-carrying files changed in this push — "
          "a human must review that observation/physics/safety semantics are unchanged:")
    for f in hits:
        print(f"::warning::protected-paths: {f}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
