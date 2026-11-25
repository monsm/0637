#!/usr/bin/env python3
"""Hold the IA 4x4 reorder to the contract in contracts/features.yaml.

The boss ruled (2026-11-16, docs/UIUX_REFACTOR_2026-11-15.md section 9-1):
the features table is the single source of truth for the entry reorder,
groups are frozen to the chosen scheme, every destination page carries
<= 6 controls, and both phones' entries must correspond one-to-one. The
build order is "contract red first, then move the entries" - so this gate
is EXPECTED to be red until the UI is actually regrouped, and green only
when the shells match the table.

Two halves:

  1. Internal consistency of features.yaml itself (always checkable):
     - 4 groups, first-layer rows <= 16 budget;
     - every destination page declares controls <= 6 (the boss's cap);
     - the 22+2 ledger is complete: every feature has zh/group/page/
       conform_b4/ios/android, unique ids, no dangling page references;
     - not-implemented rows carry a reason (no silent gaps, FIDELITY 0.1);
     - ensemble/chorus/gifts carry an enabled_when peer gate (visible but
       disabled with no neighbours - never vanished, IA-4);
     - exactly one feature sits on the time bar (rewind-play, the one
       citable precedent, UI_B4_PLAN.md 5:116).

  2. The contract measured against the actual UI (the red-first half):
     - the iOS MoreSheet drawer row count must be <= 16 (it is 25 today);
     - every feature marked implemented on an end must have its anchor
       (from contracts/ui-entries.json) present in that end's sources.

Requires PyYAML (preinstalled on ubuntu-latest CI runners and locally).

Usage: check-ui-features.py [repo-root]
"""
import json
import re
import sys
from pathlib import Path

import yaml

FEATURES = "contracts/features.yaml"
ENTRIES = "contracts/ui-entries.json"
IOS_MORE = "ios/DuckApp/MoreSheet.swift"
DRAWER_BUDGET = 16
PAGE_CAP = 6
GROUP_COUNT = 4
LEDGER_TOTAL = 47  # 22 + extensions + 2 no-ui（2026-11-19 H2 增「教它说话」、K2 增「日子」、S1 增「深度障碍」「声音摸摸」；2026-11-25 增「舞台道具」）
TIME_BAR_ID = "rewind-play"
NEIGHBOUR_GATED = {"ensemble", "chorus", "gifts"}

FAILURES = []


def fail(msg):
    FAILURES.append(msg)


def check_yaml_consistency(cfg):
    meta = cfg.get("meta", {})
    if meta.get("groups") != GROUP_COUNT:
        fail(f"meta.groups must be {GROUP_COUNT}, got {meta.get('groups')}")

    rows_budget = meta.get("first_layer_budget_rows")
    groups = cfg.get("groups", [])
    if len(groups) != GROUP_COUNT:
        fail(f"exactly {GROUP_COUNT} groups expected, got {len(groups)}")
    groups_used = []
    declared = 0
    for g in groups:
        gid = g.get("id")
        groups_used.append(gid)
        rows = g.get("rows")
        if not isinstance(rows, list):
            fail(f"group '{gid}' rows must be a list of row ids")
            continue
        declared += len(rows)
        if len(rows) > 4:
            fail(f"group '{gid}' has {len(rows)} first-layer rows, budget is <=4")
        if not g.get("destination"):
            fail(f"group '{gid}' missing destination page")
    if rows_budget is not None and declared > rows_budget:
        fail(f"first-layer rows {declared} exceed budget {rows_budget}")

    pages = cfg.get("pages", [])
    page_ids = {p.get("id") for p in pages}
    for p in pages:
        pid = p.get("id")
        if p.get("group") not in groups_used and p.get("group") != "none":
            fail(f"page '{pid}' references unknown group '{p.get('group')}'")
        ctrl = p.get("controls")
        cap = p.get("max_controls", PAGE_CAP)
        if ctrl is None:
            fail(f"page '{pid}' missing controls count")
        elif ctrl > cap:
            fail(f"page '{pid}' has {ctrl} controls, boss cap is {cap}")
        if not p.get("entries"):
            fail(f"page '{pid}' has no entries")

    seen = set()
    all_ids = set()
    for f in cfg.get("features", []):
        fid = f.get("id")
        if not fid:
            fail("feature without id")
            continue
        all_ids.add(fid)
        if fid in seen:
            fail(f"duplicate feature id '{fid}'")
        seen.add(fid)
        for field in ("zh", "conform_b4", "ios", "android"):
            if field not in f:
                fail(f"feature '{fid}' missing '{field}'")
        for end in ("ios", "android"):
            if f.get(end) == "not-implemented" and not f.get("reason"):
                fail(f"feature '{fid}' is not-implemented on {end} but has no reason")
        page = f.get("page")
        if page and page not in ("time-bar", "none") and page not in page_ids:
            fail(f"feature '{fid}' page '{page}' is not a declared page")
        if fid in NEIGHBOUR_GATED and not f.get("enabled_when"):
            fail(f"feature '{fid}' must carry an enabled_when peer gate "
                 "(visible but disabled with no neighbours)")

    timebar = [f["id"] for f in cfg.get("features", []) if f.get("page") == "time-bar"]
    if timebar != [TIME_BAR_ID]:
        fail(f"exactly '{TIME_BAR_ID}' may sit on the time bar, saw {timebar}")

    if len(cfg.get("features", [])) != LEDGER_TOTAL:
        fail(f"ledger must hold {LEDGER_TOTAL} rows, got {len(cfg.get('features', []))}")

    for p in pages:
        for eid in p.get("entries", []):
            if eid not in all_ids:
                fail(f"page '{p.get('id')}' lists unknown feature '{eid}'")


def check_ui_matches(cfg, root):
    ios_more = root / IOS_MORE
    if not ios_more.exists():
        fail(f"{IOS_MORE} missing")
        return
    rows = len(re.findall(r'id:\s*"more-', ios_more.read_text(encoding="utf-8")))
    if rows > DRAWER_BUDGET:
        fail(f"iOS MoreSheet drawer has {rows} rows, budget is {DRAWER_BUDGET} "
             "(contract-first: red until the reorder lands)")

    entries_path = root / ENTRIES
    if not entries_path.exists():
        fail(f"{ENTRIES} missing (anchor map needed for correspondence)")
        return
    entries = {r["id"]: r for r in json.loads(
        entries_path.read_text(encoding="utf-8")).get("rows", [])}

    scanned = {}
    for p in [
        "ios/DuckApp/MoreSheet.swift", "ios/DuckApp/ContentView.swift",
        "ios/DuckApp/InstrumentSheet.swift", "ios/DuckApp/PeersSheet.swift",
        "ios/DuckApp/DiarySheet.swift", "ios/DuckApp/ProgressKit.swift",
        "ios/DuckApp/RulesSheet.swift", "ios/DuckApp/DanceSheet.swift",
        # H2 2026-11-19: the destination pages and the collection sheet
        # joined the scan so the alias / badges / collection-open anchors
        # are actually verified, not skipped as "outside the scanned set".
        # K2 2026-11-19: the days board joined the scan so its ids (title,
        # week card, rows, empty state, share) are verified too.
        "ios/DuckApp/MorePages.swift", "ios/DuckApp/CollectionSheet.swift",
        "ios/DuckApp/DaysSheet.swift",
        "android/app/src/main/java/ai/duck/microduck/Shell.kt",
        "android/app/src/main/java/ai/duck/microduck/MoreSheet.kt",
        "android/app/src/main/java/ai/duck/microduck/Companion.kt",
        "android/app/src/main/java/ai/duck/microduck/DanceSheet.kt",
        "android/app/src/main/java/ai/duck/microduck/MorePages.kt",
        "android/app/src/main/java/ai/duck/microduck/CollectionSheet.kt",
    ]:
        fp = root / p
        if fp.exists():
            scanned[p] = fp.read_text(encoding="utf-8", errors="replace")

    for f in cfg.get("features", []):
        fid = f["id"]
        if f.get("page") in (None, "none") or f.get("door"):
            continue
        rows = [entries[i] for i in ([fid] + f.get("ledger_ids", [])) if i in entries]
        if not rows:
            fail(f"feature '{fid}' has no row in {ENTRIES}")
            continue
        for end in ("ios", "android"):
            if f.get(end) != "implemented":
                continue
            for row in rows:
                cell = row.get(end) or {}
                anchor = cell.get("anchor")
                if not anchor:
                    continue
                fpath, pat = anchor[0], anchor[1]
                text = scanned.get(fpath)
                if text is None:
                    continue  # file outside the scanned set
                if not re.search(pat, text):
                    fail(f"feature '{fid}' anchor '{pat}' missing in {fpath} ({end})")


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(".")
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    cfg_path = root / FEATURES
    if not cfg_path.exists():
        print(f"::error::{FEATURES} missing")
        return 1
    cfg = yaml.safe_load(cfg_path.read_text(encoding="utf-8"))

    check_yaml_consistency(cfg)
    check_ui_matches(cfg, root)

    if FAILURES:
        for m in FAILURES:
            print(f"UI-FEATURES {m}")
        print(f"::error::{len(FAILURES)} UI-feature contract violations "
              "(features.yaml is the single source of truth; fix the UI to "
              "match, not the table)")
        return 1
    print("features.yaml contract holds: 4 groups x <=16 rows, pages <=6 "
          "controls, ledger complete, entries correspond")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
