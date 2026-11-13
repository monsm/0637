#!/usr/bin/env python3
"""Fail when the two shells' Copy dictionaries do not name the same keys.

B4 §6.4: every word the shell says lives in one dictionary per platform, and
the two dictionaries must carry the same keys so the same feature cannot be
called by different names on the two phones. The values may differ (each
language has its own wording); the key sets may not.

Internal helpers (language picking, preference loading) are not copy keys.
Usage: check-copy-keys.py [repo-root]
"""
import re
import sys
from pathlib import Path

# Helpers, not user-visible copy keys.
EXCLUDE = {
    "pick", "load", "save", "language", "cached", "PREFS", "KEY",
}

IOS = "ios/DuckApp/Copy.swift"
ANDROID = "android/app/src/main/java/ai/duck/microduck/Copy.kt"


def block(text, opener):
    """The text inside the balanced braces after `opener`."""
    start = text.index(opener) + len(opener)
    depth = 1
    for index in range(start, len(text)):
        if text[index] == "{":
            depth += 1
        elif text[index] == "}":
            depth -= 1
            if depth == 0:
                return text[start:index]
    raise ValueError(f"unbalanced block after {opener}")


def ios_keys(text):
    # Inside enum Copy: `static var name: ...` and `static func name(...)`.
    body = block(text, "enum Copy {")
    return {
        name
        for name in re.findall(r"\bstatic\s+(?:var|func)\s+(\w+)", body)
        if name not in EXCLUDE
    }


def android_keys(text):
    # Inside object Copy the dictionary entries are `val <name> get() = ...`
    # and `fun <name>(`. Local vals have no getter and are not keys.
    body = block(text, "object Copy {")
    vals = re.findall(r"^\s*val\s+(\w+)\s+get\(\)", body, re.MULTILINE)
    funs = re.findall(r"^\s*fun\s+(\w+)\s*\(", body, re.MULTILINE)
    return {name for name in vals + funs if name not in EXCLUDE}


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(".")
    ios = ios_keys((root / IOS).read_text(encoding="utf-8"))
    android = android_keys((root / ANDROID).read_text(encoding="utf-8"))
    only_ios = sorted(ios - android)
    only_android = sorted(android - ios)
    print(f"iOS keys: {len(ios)}  Android keys: {len(android)}")
    if not only_ios and not only_android:
        print("Copy key sets match")
        return 0
    if only_ios:
        print(f"Only in iOS ({len(only_ios)}): {', '.join(only_ios)}")
    if only_android:
        print(f"Only in Android ({len(only_android)}): {', '.join(only_android)}")
    print("::error::the two Copy dictionaries do not name the same keys")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
