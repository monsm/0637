#!/usr/bin/env python3
"""UI-scope guard (D0): a commit that touches the phone shells must not also
touch the protected core, upstream sources or the policy patch. That is the
mechanical reading of the red line "the duck's 61-dim observation and safety
layer does not change for UI work".

A commit that touches only core/ is fine (core work is allowed); the guard
fires only when UI paths and protected paths appear in the same commit.
Exemption: the commit message contains "[scope-exempt: <reason>]".

Usage: check-ui-scope.py [repo-root]
"""
import subprocess
import sys
from pathlib import Path

UI_PREFIXES = ("ios/DuckApp/", "ios/DuckUITests/", "ios/DuckTests/",
               "android/app/src/main/java/ai/duck/microduck/",
               "android/app/src/test/java/ai/duck/microduck/")
PROTECTED_PREFIXES = ("core/", "upstream/", "patches/", "policies/")


def git(root, *args):
    return subprocess.run(["git", "-C", str(root), *args],
                          capture_output=True, text=True)


def main(argv):
    root = Path(argv[1]) if len(argv) > 1 else Path(".")
    diff = git(root, "diff", "--name-only", "HEAD^", "HEAD")
    if diff.returncode != 0:
        print("check-ui-scope: no parent commit (first commit) — pass")
        return 0
    changed = [line.strip().replace("\\", "/") for line in diff.stdout.splitlines() if line.strip()]
    ui = [f for f in changed if f.startswith(UI_PREFIXES)]
    protected = [f for f in changed if f.startswith(PROTECTED_PREFIXES)]
    if not ui or not protected:
        print(f"check-ui-scope: {len(ui)} UI file(s), {len(protected)} protected file(s) — pass")
        return 0
    message = git(root, "log", "-1", "--format=%B").stdout
    if "[scope-exempt:" in message:
        print("check-ui-scope: protected files carried by an explicit scope exemption — pass")
        return 0
    print("::error::this commit touches both the phone shells and protected paths:")
    for f in protected:
        print(f"  protected: {f}")
    print("Split the batch, or add [scope-exempt: <reason>] to the commit message.")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))