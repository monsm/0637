#!/usr/bin/env python3
"""UI debt ratchet: the counts below are the 2026-11-15 baseline (2458dec),
computed from the code by --update, not copied from prose. A metric with
direction "max" may only stay equal or go DOWN (debt shrinking); "min" may
only stay equal or go UP (guards accumulating). The numbers are honest:
they record today's debt, they do not bless it. docs/UIUX_REFACTOR_2026-11-15.md
owns the plan that drives each metric down.

Usage: check-ui-baseline.py [repo-root] [--update]
"""
import json
import re
import sys
from pathlib import Path

IOS_DIR = "ios/DuckApp"
KT_DIR = "android/app/src/main/java/ai/duck/microduck"
BASELINE = "contracts/ui-baseline.json"


def read_all(root, directory, suffix):
    out = {}
    for path in sorted((root / directory).rglob(f"*{suffix}")):
        out[str(path.relative_to(root)).replace("\\", "/")] = path.read_text(encoding="utf-8", errors="replace")
    return out


def collect(root):
    swift = read_all(root, IOS_DIR, ".swift")
    kt = read_all(root, KT_DIR, ".kt")

    def total(files, pattern, flags=0):
        rx = re.compile(pattern, flags)
        return sum(len(rx.findall(text)) for text in files.values())

    def count_files(files, pattern):
        rx = re.compile(pattern)
        return sum(1 for text in files.values() if rx.search(text))

    shell = kt.get(f"{KT_DIR}/Shell.kt", "")
    more_kt = kt.get(f"{KT_DIR}/MoreSheet.kt", "")
    controller = swift.get(f"{IOS_DIR}/DuckController.swift", "")

    return {
        # Fixed-size font literals (dynamic type still broken outside first screen)
        "ios_font_literals": (total(swift, r"\.system\(size:"), "max"),
        "ios_missing_rounded": (sum(1 for t in swift.values()
                                    for line in t.splitlines()
                                    if ".system(size:" in line and "design:" not in line), "max"),
        "android_font_literals": (total(kt, r"fontSize\s*="), "max"),
        # Dark mode declared per-view instead of once
        "ios_dark_hardwrites": (total(swift, r"\.preferredColorScheme\(\.dark\)"), "max"),
        # Press feedback suppressed
        "android_press_no_feedback": (total(kt, r"indication\s*=\s*null"), "max"),
        # Scattered modal booleans (Android), roll-call scope: shell + more drawer
        "android_modal_bools": (len(re.findall(r"var\s+show\w+\s+by\s+remember", shell)
                                    + re.findall(r"var\s+show\w+\s+by\s+remember", more_kt)), "max"),
        # God files
        "shell_kt_lines": (shell.count("\n"), "max"),
        "content_view_lines": (swift.get(f"{IOS_DIR}/ContentView.swift", "").count("\n"), "max"),
        "controller_published": (len(re.findall(r"@Published", controller)), "max"),
        # Hand-copied close buttons (iOS only; Android has shared CloseChip)
        "ios_close_button_files": (count_files(swift, r'Image\(systemName: "xmark"\)'), "max"),
        # Colour literals outside a theme file
        "android_color_literals": (total(kt, r"Color\(0x"), "max"),
        "ios_color_literals": (total(swift, r"Color\(red:"), "max"),
        # Guard metrics: these may only grow (a11y, state restoration, back handling)
        "android_remember_saveable": (total(kt, r"rememberSaveable"), "min"),
        "android_back_handler": (total(kt, r"BackHandler"), "min"),
        "a11y_custom_actions": (total(swift, r"accessibilityCustomActions")
                                + total(kt, r"customActions"), "min"),
        "a11y_announcements": (total(swift, r"UIAccessibility\.post")
                               + total(kt, r"\.liveRegion"), "min"),
        "ios_reduce_motion": (total(swift, r"accessibilityReduceMotion"), "min"),
    }


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(".")
    update = "--update" in argv
    current = collect(root)
    baseline_path = root / BASELINE
    if update:
        baseline = {name: {"count": value, "direction": direction}
                    for name, (value, direction) in current.items()}
        baseline_path.write_text(json.dumps(baseline, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"baseline written to {BASELINE} ({len(baseline)} metrics)")
        return 0
    baseline = json.loads(baseline_path.read_text(encoding="utf-8"))
    failures = []
    for name, (value, direction) in sorted(current.items()):
        base = baseline.get(name)
        if base is None:
            failures.append(f"{name}: not in baseline — re-run --update deliberately and commit")
            continue
        limit, want = base["count"], base["direction"]
        bad = value > limit if want == "max" else value < limit
        mark = "FAIL" if bad else "ok  "
        arrow = "≤" if want == "max" else "≥"
        print(f"{mark} {name}: {value} {arrow} {limit}")
        if bad:
            failures.append(f"{name} grew to {value} (baseline {limit}) — "
                            "this is a debt ratchet; shrink it or re-baseline deliberately")
    missing = sorted(set(baseline) - set(current))
    for name in missing:
        print(f"WARN {name}: metric no longer computed (script changed?)")
    if failures:
        print()
        for line in failures:
            print(f"::error::{line}")
        return 1
    print("UI debt ratchet holds")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
