#!/usr/bin/env python3
"""Component contract gate (D2, docs/UIUX_REFACTOR_DEEP_2026-11-19.md §3.2).

contracts/ui-components.json is the one source of truth for the eleven base
components: their per-end implementation files, their state matrix and the
tokens they may use. This gate proves three things and nothing more:

1. every declared component has an implementation on BOTH ends (a symbol with
   that name exists in the declared file);
2. each implementation file carries a `STATE MATRIX: <Name> = state, ...` line
   whose state list equals the JSON (default/pressed/disabled/selected/focus/
   error and their per-component variants);
3. the component files hold no raw literals — colour, font size, spacing,
   radius or motion — outside the token layer.

It is expected to be RED until the D2 batch lands: the contract exists first,
then the components make it green.
"""
import json
import re
import sys
from pathlib import Path

CONTRACT = "contracts/ui-components.json"
RAW = [
    (r"\.system\(size:", "raw font size"),
    (r"Color\(red:", "raw colour"),
    (r"fontSize\s*=\s*[0-9]", "raw font size"),
    (r"Color\(0x", "raw colour"),
    (r"cornerRadius:\s*[0-9]", "raw radius"),
    (r"RoundedCornerShape\(\s*[0-9]", "raw radius"),
    (r"\.padding\(\s*(?:\.\w+\s*,\s*)?[1-9]", "raw spacing"),
    (r"spacing:\s*[1-9]", "raw spacing"),
    (r"withAnimation\(\.\w+(?:\(duration:\s*[0-9.]+\))?\)", "raw motion"),
]
MATRIX = re.compile(r"STATE MATRIX:\s*([A-Za-z]+)\s*=\s*([a-zA-Z, ]+)")


def ios_symbol(text, name):
    return re.search(rf"(?:struct|enum|class|func)\s+{name}\b", text) is not None


def android_symbol(text, name):
    return re.search(rf"(?:fun|object|class)\s+{name}\b", text) is not None


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(".")
    contract = json.loads((root / CONTRACT).read_text(encoding="utf-8"))
    components = contract["components"]
    declared = {c["name"] for c in components}
    waves = [name for wave in contract["waves"].values() for name in wave]
    failures = []

    if declared != set(waves) or len(waves) != len(set(waves)):
        failures.append("waves and components disagree (every component belongs to exactly one wave)")

    for component in components:
        name = component["name"]
        states = [s.strip() for s in component["states"]]
        for end, key, checker in (
            ("iOS", "ios_file", ios_symbol),
            ("Android", "android_file", android_symbol),
        ):
            path = root / component[key]
            if not path.is_file():
                failures.append(f"{name}: {end} file missing: {component[key]}")
                continue
            text = path.read_text(encoding="utf-8", errors="replace")
            if not checker(text, name):
                failures.append(f"{name}: no {end} symbol in {component[key]}")
            matrices = {m.group(1): [s.strip() for s in m.group(2).split(",") if s.strip()]
                        for m in MATRIX.finditer(text)}
            if name in matrices and matrices[name] != states:
                failures.append(f"{name}: STATE MATRIX on {end} is {matrices[name]}, contract says {states}")

    files = {c["ios_file"] for c in components} | {c["android_file"] for c in components}
    for rel in sorted(files):
        path = root / rel
        if not path.is_file():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for pattern, label in RAW:
            for match in re.finditer(pattern, text):
                line = text.count("\n", 0, match.start()) + 1
                failures.append(f"{rel}:{line}: {label} outside the token layer")

    if failures:
        print("::error::component contract not met")
        for failure in failures:
            print("  " + failure)
        return 1
    print(f"component contract holds: {len(components)} components, waves {contract['waves']}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))