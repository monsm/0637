#!/usr/bin/env python3
"""D4 hit-target audit: iOS 44 pt / Android 48 dp floors.

The UI_TOKENS §6 rule is a floor on every interactive control, not on the
glyph inside it. Rather than pretend a grep can see every SwiftUI/Compose
modifier chain, this gate does the two things a static pass CAN do honestly:

1. It audits every explicit *minimum height* in the phone shells. A minimum
   below the platform floor is a violation unless the line is a known
   non-interactive decoration (a progress bar, a status dot) listed below.
2. It pins the shared interactive controls (close glyph, chips, rows,
   segmented options, stage buttons) to their contracted floors, so a later
   edit cannot quietly shrink one back.

Runtime checks live where an element's real frame is knowable: the dense-sheet
XCUITest asserts its key controls are tappable, and the Android smoke asserts
clickable node bounds at font_scale 2.0.

Usage: check-hit-targets.py [repo-root]
"""
import re
import sys
from pathlib import Path

IOS_DIR = "ios/DuckApp"
KT_DIR = "android/app/src/main/java/ai/duck/microduck"
IOS_FLOOR = 44
ANDROID_FLOOR = 48

# Explicit minimums that are decorations, not tap targets. Each entry is
# (file suffix, exact snippet) so a moved or edited line stops matching and
# the exception cannot silently cover something else.
IOS_DECORATIVE = [
    ("DexSheet.swift", ".frame(minHeight: 32)"),  # static dex row, not a Button
]
ANDROID_DECORATIVE = [
    ("DuckComponents.kt", ".heightIn(min = 8.dp)"),  # DuckProgress bar
]

# The shared controls and the minimum they must keep.
IOS_CONTROLS = [
    ("Design/DuckSurfaces.swift", r"\.frame\(width: 48, height: 48\)", "close glyph"),
    ("Design/DuckButton.swift", r"\.frame\(minHeight: 48\)", "DuckButton/DuckChip"),
    ("Design/DuckSurfaces.swift", r"\.frame\(minHeight: 56\)", "DuckListRow"),
    ("Design/DuckControls.swift", r"\.frame\(maxWidth: \.infinity, minHeight: 44\)", "DuckSegmented option"),
    ("Design/DuckStageButtons.swift", r"\.frame\(width: 52, height: 52\)", "primary stage button"),
    ("Design/DuckStageButtons.swift", r"\.frame\(width: 48, height: 48\)", "secondary stage button"),
]
ANDROID_CONTROLS = [
    ("Shell.kt", r"\.size\(size\.coerceAtLeast\(48\.dp\)\)", "CloseChip/RoundChip floor"),
    ("Shell.kt", r"\.sizeIn\(minWidth = 48\.dp, minHeight = 48\.dp\)", "ButtonChip/PathChip floor"),
    ("DuckComponents.kt", r"\.heightIn\(min = 48\.dp\)", "shared row/segmented floor"),
]


def read_all(root, directory):
    out = {}
    for path in sorted((root / directory).rglob("*")):
        if path.suffix in (".swift", ".kt"):
            out[str(path.relative_to(root)).replace("\\", "/")] = path.read_text(
                encoding="utf-8", errors="replace"
            )
    return out


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(".")
    failures = []

    ios = read_all(root, IOS_DIR)
    for path, text in ios.items():
        if path.endswith("Design/DuckTheme.swift"):
            continue
        for number, line in enumerate(text.splitlines(), start=1):
            m = re.search(r"\.frame\([^)]*minHeight:\s*([0-9.]+)", line)
            if not m:
                continue
            value = float(m.group(1))
            if value >= IOS_FLOOR:
                continue
            if any(path.endswith(suffix) and snippet in line for suffix, snippet in IOS_DECORATIVE):
                continue
            failures.append(f"{path}:{number}: iOS minimum {value:g} pt < {IOS_FLOOR} pt: {line.strip()}")

    kt = read_all(root, KT_DIR)
    for path, text in kt.items():
        for number, line in enumerate(text.splitlines(), start=1):
            m = re.search(r"heightIn\(min\s*=\s*([0-9.]+)\.dp", line)
            if not m:
                continue
            value = float(m.group(1))
            if value >= ANDROID_FLOOR:
                continue
            if any(path.endswith(suffix) and snippet in line for suffix, snippet in ANDROID_DECORATIVE):
                continue
            failures.append(
                f"{path}:{number}: Android minimum {value:g} dp < {ANDROID_FLOOR} dp: {line.strip()}"
            )

    for suffix, pattern, name in IOS_CONTROLS:
        matches = [text for path, text in ios.items() if path.endswith(suffix)]
        if not any(re.search(pattern, text) for text in matches):
            failures.append(f"iOS control floor missing: {name} ({suffix} / {pattern})")
    for suffix, pattern, name in ANDROID_CONTROLS:
        matches = [text for path, text in kt.items() if path.endswith(suffix)]
        if not any(re.search(pattern, text) for text in matches):
            failures.append(f"Android control floor missing: {name} ({suffix} / {pattern})")

    if failures:
        for line in failures:
            print(f"::error::{line}")
        return 1
    print(
        f"hit targets hold: iOS >= {IOS_FLOOR} pt, Android >= {ANDROID_FLOOR} dp "
        f"({len(ios)} Swift files, {len(kt)} Kotlin files)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))