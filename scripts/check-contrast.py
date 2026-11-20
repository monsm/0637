#!/usr/bin/env python3
"""Contrast gate (D4): the documented token pairs, recomputed from the real
theme files at HEAD, at WCAG AA (text >= 4.5:1, graphics >= 3:1).

Values come from ios/DuckApp/Design/DuckTheme.swift (Color(red:...)) and
android/.../DuckTheme.kt (Color(0xAARRGGBB)); both ends must agree per token.
The two brand constants (the 3D beak and the icon canvas) are not UI tokens —
they are pinned here with their sources, so the pair the icon uses is checked
as well.

Usage: check-contrast.py [repo-root]
"""
import re
import sys
from pathlib import Path


def parse_ios(root):
    text = (root / "ios/DuckApp/Design/DuckTheme.swift").read_text(encoding="utf-8")
    out = {}
    for match in re.findall(
        r"static let (\w+) = Color\(red: ([0-9.]+), green: ([0-9.]+), blue: ([0-9.]+)\)", text
    ):
        name, *vals = match
        out[name] = tuple(float(v) for v in vals)
    return out


def parse_kt(root):
    text = (root / "android/app/src/main/java/ai/duck/microduck/DuckTheme.kt").read_text(encoding="utf-8")
    out = {}
    for name, hexval in re.findall(r"val (\w+) = Color\(0xFF([0-9A-Fa-f]{6})\)", text):
        out[name] = tuple(int(hexval[i:i + 2], 16) / 255.0 for i in (0, 2, 4))
    return out


def luminance(rgb):
    def channel(c):
        return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4
    r, g, b = (channel(c) for c in rgb)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def composite(fg, bg, alpha):
    return tuple(fg[i] * alpha + bg[i] * (1 - alpha) for i in range(3))


def ratio(a, b):
    la, lb = luminance(a), luminance(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


IOS = {"root": "duckBgRoot", "surface": "duckBgSurface", "primary": "duckTextPrimary",
       "onAccent": "duckOnAccent", "action": "duckAction", "data": "duckData",
       "info": "duckInfo", "good": "duckGood", "warn": "duckWarn", "bad": "duckBad"}
KT = {"root": "BgRoot", "surface": "BgSurface", "primary": "TextPrimary",
      "onAccent": "TextOnAccent", "action": "AccentAction", "data": "AccentData",
      "info": "AccentInfo", "good": "StateGood", "warn": "StateWarn", "bad": "StateBad"}

PAIRS = [
    # (name, fg token, fg alpha, bg token, minimum)
    ("text.primary on bg.root", "primary", 1.0, "root", 4.5),
    ("text.secondary@70 on bg.root", "primary", 0.70, "root", 4.5),
    ("text.tertiary@50 on bg.root", "primary", 0.50, "root", 4.5),
    ("accent.action on bg.root", "action", 1.0, "root", 4.5),
    ("text.onAccent on accent.action", "onAccent", 1.0, "action", 4.5),
    ("accent.data on bg.root", "data", 1.0, "root", 4.5),
    ("accent.action on bg.surface", "action", 1.0, "surface", 4.5),
    ("state.good on bg.root", "good", 1.0, "root", 4.5),
    ("state.warn on bg.root", "warn", 1.0, "root", 4.5),
    ("state.bad on bg.root", "bad", 1.0, "root", 4.5),
    # Graphics floor: the info accent as a line/glyph.
    ("accent.info on bg.root (graphic)", "info", 1.0, "root", 3.0),
]

BRAND = {"beak": (0xFF / 255, 0x67 / 255, 0x1F / 255), "canvas": (0x12 / 255, 0x11 / 255, 0x10 / 255)}


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(".")
    ios, kt = parse_ios(root), parse_kt(root)
    failures = []
    for key, ios_name in IOS.items():
        kt_name = KT[key]
        if ios_name not in ios:
            failures.append(f"iOS token missing: {ios_name}")
            continue
        if kt_name not in kt:
            failures.append(f"Android token missing: {kt_name}")
            continue
        a, b = ios[ios_name], kt[kt_name]
        if any(abs(a[i] - b[i]) > 1 / 255 for i in range(3)):
            failures.append(f"token {key} drifts between ends: {a} vs {b}")
    for label, fg_key, alpha, bg_key, minimum in PAIRS:
        fg = ios.get(IOS[fg_key])
        bg = ios.get(IOS[bg_key])
        if fg is None or bg is None:
            failures.append(f"pair unmeasurable: {label}")
            continue
        effective = composite(fg, bg, alpha) if alpha < 1 else fg
        value = ratio(effective, bg)
        verdict = "ok" if value >= minimum else "FAIL"
        print(f"{verdict:4} {label}: {value:.2f}:1 (floor {minimum})")
        if value < minimum:
            failures.append(f"{label} is {value:.2f}:1, floor {minimum}")
    brand_ratio = ratio(BRAND["beak"], BRAND["canvas"])
    print(f"{'ok' if brand_ratio >= 4.5 else 'FAIL':4} brand.beak on brand.canvas: {brand_ratio:.2f}:1 (floor 4.5)")
    if brand_ratio < 4.5:
        failures.append(f"brand beak/canvas is {brand_ratio:.2f}:1")
    if failures:
        print("::error::contrast gate failed")
        for failure in failures:
            print("  " + failure)
        return 1
    print("contrast: every documented pair holds")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))