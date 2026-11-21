#!/usr/bin/env python3
"""Hold both shells to the one route table (B7, meeting section 5).

After B7 there is exactly one vocabulary for "which sheet is up": the route
enum on each end, with the skill ring living in a separate overlay field.
This gate pins the shape the meeting approved:

  - the case sets match the registered lists (no route can sneak in or
    silently disappear);
  - no loose `show*` navigation boolean survives on either end (the
    fourteen Android sheet booleans, the five iOS adapter bools and the
    drawer / instrument enums are gone, not renamed);
  - Android keeps BackHandler and gains rememberSaveable;
  - every route carries the `route-` tag prefix;
  - the pure reducer has JVM tests on Android and unit tests on iOS.

Pure stdlib. Usage: check-routes.py [repo-root]
"""
import re
import sys
from pathlib import Path

IOS_ROUTE = "ios/DuckApp/DuckRoute.swift"
ANDROID_ROUTE = "android/app/src/main/java/ai/duck/microduck/DuckRoute.kt"
ANDROID_SHELL = "android/app/src/main/java/ai/duck/microduck/Shell.kt"
ANDROID_MORE = "android/app/src/main/java/ai/duck/microduck/MoreSheet.kt"
IOS_CONTENT = "ios/DuckApp/ContentView.swift"
IOS_MORE = "ios/DuckApp/MoreSheet.swift"
IOS_INSTRUMENT = "ios/DuckApp/InstrumentSheet.swift"
JVM_TEST = "android/app/src/test/java/ai/duck/microduck/DuckRouteTest.kt"
IOS_TEST = "ios/DuckTests/DuckRouteTests.swift"

# Registered case sets. iOS carries the destinations that only exist there
# (photo preview, backup and A/B sheets); Android reaches backup/AB as
# instrument toggles, not sheets, so its table is a subset by design.
REGISTERED_IOS = {
    "instrument", "photo", "rules", "peers", "ai", "dance", "progress",
    "more", "diary", "postcards", "dex", "modes", "big", "backup", "ab",
    "playMore", "duckPanel", "tools",
}
REGISTERED_ANDROID = {
    "instrument", "rules", "peers", "ai", "dance", "progress", "more",
    "diary", "postcards", "dex", "modes", "big", "play-more", "duck-panel",
    "tools",
}

FAILURES = []


def fail(msg):
    FAILURES.append(msg)


def read(root, rel):
    p = root / rel
    if not p.exists():
        fail(f"{rel} missing")
        return ""
    return p.read_text(encoding="utf-8", errors="replace")


def swift_cases(text):
    body = text.split("enum DuckRoute")[1].split("enum ShellOverlay")[0]
    return set(re.findall(r"^\s*case\s+([a-z][A-Za-z0-9_]*)\s*$", body, re.MULTILINE))


def kotlin_cases(text):
    body = text.split("enum class DuckRoute")[1].split(";")[0]
    return set(re.findall(r'^\s{4}[A-Z][A-Za-z0-9_]*\("([a-z0-9-]+)"\)', body, re.MULTILINE))


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(".")
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

    ios_text = read(root, IOS_ROUTE)
    kt_text = read(root, ANDROID_ROUTE)
    if ios_text:
        cases = swift_cases(ios_text)
        if cases != REGISTERED_IOS:
            fail(f"iOS route cases differ from the registered set: "
                 f"added={sorted(cases - REGISTERED_IOS)} "
                 f"missing={sorted(REGISTERED_IOS - cases)}")
        if '"route-' not in ios_text:
            fail("iOS routes lost the route- tag prefix")
    if kt_text:
        cases = kotlin_cases(kt_text)
        if cases != REGISTERED_ANDROID:
            fail(f"Android route cases differ from the registered set: "
                 f"added={sorted(cases - REGISTERED_ANDROID)} "
                 f"missing={sorted(REGISTERED_ANDROID - cases)}")
        if '"route-' not in kt_text:
            fail("Android routes lost the route- tag prefix")

    # Loose navigation booleans. Copy.showLess/showMore are strings, and
    # `showsChevron` is a row parameter — neither is navigation state.
    # showHint is a caption fade and showDetails a disclosure, not a route.
    allowed = {"showHint", "showDetails"}
    for rel in (ANDROID_SHELL, ANDROID_MORE, IOS_CONTENT, IOS_MORE, IOS_INSTRUMENT):
        text = read(root, rel)
        if not text:
            continue
        bad = [name for name in re.findall(
            r"\b(?:var|@State\s+private\s+var)\s+"
            r"(show[A-Z][A-Za-z0-9_]*|destination|modal)\b", text)
            if name not in allowed]
        if bad:
            fail(f"{rel} still declares loose navigation state: {sorted(set(bad))}")

    shell = read(root, ANDROID_SHELL)
    if shell:
        if "BackHandler(" not in shell:
            fail("Shell.kt lost BackHandler")
        if "rememberSaveable(" not in shell:
            fail("Shell.kt does not save the route across process death")
        if "ShellOverlayReducer.reduce" not in shell:
            fail("Shell.kt does not go through the pure reducer")

    jvm = read(root, JVM_TEST)
    if jvm and len(re.findall(r"@Test", jvm)) < 6:
        fail(f"{JVM_TEST} carries fewer than 6 reducer tests")
    ios_test = read(root, IOS_TEST)
    if ios_test and len(re.findall(r"func test", ios_test)) < 4:
        fail(f"{IOS_TEST} carries fewer than 4 reducer tests")

    if FAILURES:
        for m in FAILURES:
            print(f"ROUTES {m}")
        print(f"::error::{len(FAILURES)} route-contract violations "
              "(one route table, no loose boolean trees)")
        return 1
    print("route contract holds: registered case sets, no loose show* state, "
          "BackHandler + saveable, route- tags, reducer tests present")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
