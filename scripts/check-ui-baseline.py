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
    # The theme files are where literals are allowed to live (UI_TOKENS §7).
    swift_ui = {p: t for p, t in swift.items()
                if not p.endswith(f"{IOS_DIR}/Design/DuckTheme.swift")}
    kt_ui = {p: t for p, t in kt.items()
             if not p.endswith(f"{KT_DIR}/DuckTheme.kt")
             # DuckComponents.kt is where the shared press feedback is
             # IMPLEMENTED: its `indication = null` sits beside a
             # graphicsLayer scale/dim, so it is not missing feedback.
             and not p.endswith(f"{KT_DIR}/DuckComponents.kt")}

    def total(files, pattern, flags=0):
        rx = re.compile(pattern, flags)
        return sum(len(rx.findall(text)) for text in files.values())

    def count_files(files, pattern):
        rx = re.compile(pattern)
        return sum(1 for text in files.values() if rx.search(text))

    def count_lines(files, pattern):
        rx = re.compile(pattern)
        return sum(1 for text in files.values() for line in text.splitlines() if rx.search(line))

    shell = kt.get(f"{KT_DIR}/Shell.kt", "")
    more_kt = kt.get(f"{KT_DIR}/MoreSheet.kt", "")
    controller = swift.get(f"{IOS_DIR}/DuckController.swift", "")

    return {
        # Fixed-size font literals (dynamic type still broken outside first screen)
        "ios_font_literals": (total(swift_ui, r"\.system\(size:"), "max"),
        "ios_missing_rounded": (sum(1 for t in swift_ui.values()
                                    for line in t.splitlines()
                                    if ".system(size:" in line and "design:" not in line), "max"),
        "android_font_literals": (total(kt_ui, r"fontSize\s*=\s*[0-9]"), "max"),
        # Space and radius literals outside the token layer (0 is a layout
        # decision, not a hand-tuned value).
        "ios_offgrid_spacing": (count_lines(swift_ui, r"\.padding\(\s*(?:\.\w+\s*,\s*)?[1-9]")
                                + count_lines(swift_ui, r"spacing:\s*[1-9]")
                                + count_lines(swift_ui, r"Spacer\(minLength:\s*[1-9]"), "max"),
        "android_offgrid_spacing": (count_lines(kt_ui, r"padding\([^)]*[1-9][0-9]*\.dp")
                                    + count_lines(kt_ui, r"spacedBy\([^)]*[1-9][0-9]*\.dp")
                                    + count_lines(kt_ui, r"Spacer\(Modifier\.height\(\s*[1-9][0-9]*\.dp"), "max"),
        "ios_radius_literals": (count_lines(swift_ui, r"cornerRadius:\s*[0-9]"), "max"),
        "android_radius_literals": (count_lines(kt_ui, r"RoundedCornerShape\(\s*[0-9]"), "max"),
        # Dark mode declared per-view instead of once
        "ios_dark_hardwrites": (total(swift_ui, r"\.preferredColorScheme\(\.dark\)"), "max"),
        # Press feedback suppressed
        "android_press_no_feedback": (total(kt_ui, r"indication\s*=\s*null"), "max"),
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
        "android_color_literals": (total(kt_ui, r"Color\(0x"), "max"),
        "ios_color_literals": (total(swift_ui, r"Color\(red:"), "max"),
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
