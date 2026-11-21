#!/usr/bin/env python3
"""Check the UI entry ledger (contracts/ui-entries.json) against source.

The ledger is the meeting's feature-preservation map: every feature the
seven batches shipped gets a row, and every row must either carry a
verified source anchor per implemented platform or an explicit
not-implemented note — no silent gaps. Anchors are (file, regex) pairs; a
row whose anchor stops matching means an entry disappeared from the code.

Known limit (documented): source-level anchors cannot prove runtime
reachability — the iOS pad-send chip carries a valid id inside a closure
that never renders. Reachability is the UI tests' job (B2/B3 batches add
those assertions); this guard is the map, not the proof.

Usage: check-ui-entries.py [repo-root]
"""
import json
import re
import sys
from pathlib import Path

LEDGER = "contracts/ui-entries.json"
CONFORM = {"ok", "extension", "violation"}


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(".")
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    data = json.loads((root / LEDGER).read_text(encoding="utf-8"))
    rows = data["rows"]
    failures = []
    cache = {}

    def source(rel):
        if rel not in cache:
            cache[rel] = (root / rel).read_text(encoding="utf-8", errors="replace")
        return cache[rel]

    seen = set()
    ok_rows = 0
    for row in rows:
        rid = row["id"]
        if rid in seen:
            failures.append(f"{rid}: duplicate id")
        seen.add(rid)
        if row.get("conform_b4") not in CONFORM:
            failures.append(f"{rid}: conform_b4 must be one of {sorted(CONFORM)}")
        for platform in ("ios", "android"):
            side = row.get(platform)
            if side is None:
                failures.append(f"{rid}: missing {platform} side")
                continue
            if side.get("implemented"):
                anchor = side.get("anchor")
                if not anchor:
                    failures.append(f"{rid}/{platform}: implemented but no anchor")
                    continue
                rel, pattern = anchor[0], anchor[1]
                try:
                    text = source(rel)
                except OSError:
                    failures.append(f"{rid}/{platform}: anchor file missing: {rel}")
                    continue
                if not re.search(pattern, text):
                    failures.append(f"{rid}/{platform}: anchor '{pattern}' not found in {rel} "
                                    "— did an entry disappear from the code?")
                else:
                    ok_rows += 1
            else:
                if not side.get("note"):
                    failures.append(f"{rid}/{platform}: not implemented but no note — "
                                    "a silent gap is how features get lost")

    total = len(rows)
    print(f"{total} features on the ledger, {ok_rows} source anchors verified")
    if failures:
        print()
        for line in failures:
            print(f"::error::{line}")
        return 1
    print("entry ledger agrees with source")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
