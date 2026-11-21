#!/usr/bin/env python3
"""
Exception classes:

  platform_facts — the two phones genuinely say different true things
      (iOS has no ambient-light API; the other does). These may differ
      until a meeting decides otherwise.
  pending        — real drift, queued for the copy batch. Listed here
      so the gate is green today without pretending the drift away.
      Removing an entry requires actually fixing the value on one side.

The platform_facts list is FROZEN by the 2026-11-16 boss ruling to exactly
{roomLampNote, rewindPlay, netPad} — each side tells its own truth, no
unified wording. The script asserts the list equals this set: growing or
shrinking it needs a meeting, not an edit to silence the gate.

Known parsing limits (documented, not hidden): String(format:) wrappers and
switch/when-shaped functions are compared by scanning every pick("…","…")
inside the member, in order, so shaped functions ARE compared; but a value
assembled without a literal pick call is invisible to this guard.

Usage: check-copy-values.py [repo-root] [--update-exceptions-draft]
"""
import json
import re
import sys
from pathlib import Path

IOS = "ios/DuckApp/Copy.swift"
ANDROID = "android/app/src/main/java/ai/duck/microduck/Copy.kt"
EXCEPTIONS = "contracts/copy-exceptions.json"


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


def members(body, pattern):
    """Split a dictionary body into (name, segment) members."""
    found = list(pattern.finditer(body))
    out = {}
    for index, match in enumerate(found):
        end = found[index + 1].start() if index + 1 < len(found) else len(body)
        out.setdefault(match.group(1), body[match.start():end])
    return out


SWIFT_MEMBER = re.compile(r"^ {4}static\s+(?:var|func)\s+(\w+)", re.MULTILINE)
KT_MEMBER = re.compile(r"^ {4}(?:val|fun)\s+(\w+)", re.MULTILINE)

def _string_at(s, i):
    """s[i] == '"'. Return (content, index_after_closing_quote), handling
    escapes and interpolation regions (Swift \\(...), Kotlin ${...}) whose
    bodies may themselves contain string literals."""
    i += 1
    buf = []
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s):
            nxt = s[i + 1]
            if nxt == "(":
                end = _balanced(s, i + 1, "(", ")")
                buf.append(s[i:end])
                i = end
                continue
            buf.append(s[i:i + 2])
            i += 2
            continue
        if c == "$" and i + 1 < len(s) and s[i + 1] == "{":
            end = _balanced(s, i + 1, "{", "}")
            buf.append(s[i:end])
            i = end
            continue
        if c == '"':
            i += 1
            break
        buf.append(c)
        i += 1
    return "".join(buf), i


def pick_calls(segment):
    """Every pick("en", "zh") in the segment as (en, zh) raw strings, using
    the interpolation-aware string scanner (nested quotes survive)."""
    out = []
    for m in re.finditer(r"\bpick\(", segment):
        i = m.end()
        args = []
        while i < len(segment):
            while i < len(segment) and segment[i] in " \t\r\n":
                i += 1
            if i >= len(segment):
                break
            if segment[i] == '"':
                text, i = _string_at(segment, i)
                args.append(text)
                continue
            if segment[i] == ",":
                i += 1
                continue
            if segment[i] == ")":
                break
            i += 1  # anything else (rare), step over
        if len(args) >= 2:
            out.append((args[0], args[1]))
    return out


def _balanced(s, start, open_ch, close_ch):
    """Index just past the balanced bracket pair opened at `start` (which
    points at open_ch), skipping string literals inside."""
    depth = 0
    i = start
    while i < len(s):
        c = s[i]
        if c == '"':
            i += 1
            while i < len(s):
                if s[i] == "\\":
                    i += 2
                    continue
                if s[i] == '"':
                    i += 1
                    break
                i += 1
            continue
        if c == open_ch:
            depth += 1
        elif c == close_ch:
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return len(s)


def norm_interp(text):
    """Every placeholder becomes a numbered hole «1» «2» … in appearance
    order, so Swift's positional String(format:) style and Kotlin's named
    $-style compare equal when the wording matches. A literal percent —
    Swift writes %% in format strings, Kotlin writes % in templates — and
    an escaped %% both become the same «pct» token; literal parentheses
    without a backslash are untouched."""
    out = []
    count = 0

    def hole():
        nonlocal count
        count += 1
        return f"«{count}»"

    i = 0
    while i < len(text):
        c = text[i]
        if c == "%":
            nxt = text[i + 1] if i + 1 < len(text) else ""
            if nxt == "%":
                out.append("«pct»")
                i += 2
                continue
            if nxt and nxt in "@dsf":
                out.append(hole())
                i += 2
                continue
            m = re.match(r"%\.(\d+)f", text[i:])
            if m:
                out.append(hole())
                i += m.end()
                continue
            out.append("«pct»")
            i += 1
            continue
        if c == "\\" and i + 1 < len(text) and text[i + 1] == "(":
            i = _balanced(text, i + 1, "(", ")")
            out.append(hole())
            continue
        if c == "\\" and i + 1 < len(text):
            out.append(text[i:i + 2])
            i += 2
            continue
        if c == "$":
            if i + 1 < len(text) and text[i + 1] == "{":
                i = _balanced(text, i + 1, "{", "}")
                out.append(hole())
                continue
            m = re.match(r"\$(\w+)", text[i:])
            if m:
                out.append(hole())
                i += m.end()
                continue
        out.append(c)
        i += 1
    return "".join(out)


def pairs(segment):
    """Every pick("en", "zh") in the segment, interpolation-normalised."""
    return [(norm_interp(a), norm_interp(b)) for a, b in pick_calls(segment)]


def drifted(ios_set, kt_set):
    """True when the two wordings genuinely disagree.

    One side being a strict subset of the other is a *shape* difference —
    one platform hangs extra strings (a when-branch, a count variant) under
    the same name — and is allowed. Any overlap that is not a subset means
    the shared semantic changed wording on one phone: drift.
    """
    if not ios_set or not kt_set:
        return False
    if ios_set <= kt_set or kt_set <= ios_set:
        return False
    return True


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(".")
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    ios_body = block((root / IOS).read_text(encoding="utf-8"), "enum Copy {")
    kt_body = block((root / ANDROID).read_text(encoding="utf-8"), "object Copy {")
    ios_map = members(ios_body, SWIFT_MEMBER)
    kt_map = members(kt_body, KT_MEMBER)

    exceptions_path = root / EXCEPTIONS
    exceptions = {"platform_facts": [], "pending": []}
    if exceptions_path.exists():
        exceptions = json.loads(exceptions_path.read_text(encoding="utf-8"))

    # Frozen platform-fact set (boss ruling, 2026-11-16): each side tells its
    # own truth for exactly these three keys. Growing or shrinking the list
    # needs a meeting, not an edit to silence the gate.
    FROZEN_PLATFORM_FACTS = {"roomLampNote", "rewindPlay", "netPad"}
    frozen_list = set(exceptions["platform_facts"])
    if frozen_list != FROZEN_PLATFORM_FACTS:
        print(f"::error::copy-exceptions platform_facts is {sorted(frozen_list)}; "
              f"the 2026-11-16 boss ruling froze it to {sorted(FROZEN_PLATFORM_FACTS)}. "
              "A meeting must change it, not this file.")
        return 1

    exempt = frozen_list | set(exceptions["pending"])

    shared = sorted((set(ios_map) & set(kt_map)) - exempt)
    diffs = [name for name in shared if drifted(set(pairs(ios_map[name])), set(pairs(kt_map[name])))]

    print(f"compared keys: {len(shared)} shared members, "
          f"{len(FROZEN_PLATFORM_FACTS)} frozen platform facts + "
          f"{len(exceptions['pending'])} pending exceptions")
    if diffs:
        for name in diffs:
            print(f"VALUE DIFF {name}")
            print(f"   ios     = {pairs(ios_map[name])[:3]}")
            print(f"   android = {pairs(kt_map[name])[:3]}")
        print("::error::Copy values drift between the two phones; "
              "fix the values or move a real platform fact into "
              f"{EXCEPTIONS} (the list may only shrink)")
        return 1
    print("Copy values agree (beyond declared exceptions)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
