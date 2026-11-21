#!/usr/bin/env python3
"""Adaptive + platform-difference contract (D6).

Two files make a platform difference or a layout band legal:
contracts/adaptive.json (three width bands, one IA) and
contracts/platform-differences.json (a fact + a reason per entry). This gate
checks the files are honest and that the shells' width predicate still uses
the documented 600 boundary — a new breakpoint must be written down here
first.

Usage: check-ui-parity.py [repo-root]
"""
import json
import re
import sys
from pathlib import Path

ROOT = "."
ADAPTIVE = "contracts/adaptive.json"
DIFFS = "contracts/platform-differences.json"
IOS_SHELL = "ios/DuckApp/ContentView.swift"
KT_SHELL = "android/app/src/main/java/ai/duck/microduck/Shell.kt"


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(ROOT)
    failures = []
    adaptive = json.loads((root / ADAPTIVE).read_text(encoding="utf-8"))
    bands = adaptive.get("bands", [])
    ids = [b.get("id") for b in bands]
    if ids != ["compact", "medium", "regular"]:
        failures.append(f"adaptive bands must be compact/medium/regular, got {ids}")
    if not any("< 600" in b.get("width", "") for b in bands):
        failures.append("adaptive.json must document the 600 boundary")

    diffs = json.loads((root / DIFFS).read_text(encoding="utf-8")).get("differences", [])
    seen = set()
    for entry in diffs:
        for field in ("id", "feature", "ios", "android", "reason"):
            if not entry.get(field):
                failures.append(f"platform difference {entry.get('id', '?')} is missing {field}")
        if entry.get("id") in seen:
            failures.append(f"duplicate platform difference id: {entry.get('id')}")
        seen.add(entry.get("id"))
        if entry.get("ios", "") == entry.get("android", ""):
            failures.append(f"{entry.get('id')}: two ends listed identically — not a difference")

    for path, label in ((IOS_SHELL, "iOS"), (KT_SHELL, "Android")):
        text = (root / path).read_text(encoding="utf-8", errors="replace")
        if not re.search(r"[Ww]idth\s*<\s*600", text):
            failures.append(f"{label} shell no longer branches at width < 600 (adaptive.json owns that number)")
        if re.search(r"[Ww]idth\s*<\s*(?!600)\d{2,4}", text):
            failures.append(f"{label} shell introduced a second width breakpoint — document it in adaptive.json")

    if failures:
        print("::error::adaptive/platform contract not met")
        for failure in failures:
            print("  " + failure)
        return 1
    print(f"adaptive/platform contract holds: {len(bands)} bands, {len(diffs)} platform facts")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))