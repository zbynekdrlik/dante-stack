#!/usr/bin/env python3
"""On a PR, the workspace version must be higher than the base branch's."""
import re
import subprocess
import sys


def version(text: str) -> tuple[int, ...]:
    m = re.search(r'\[workspace\.package\][^\[]*?version\s*=\s*"(\d+)\.(\d+)\.(\d+)', text, re.S)
    if not m:
        sys.exit("no [workspace.package] version")
    return tuple(int(x) for x in m.groups())


def main() -> None:
    base = sys.argv[1] if len(sys.argv) > 1 else "origin/main"
    head = version(open("Cargo.toml", encoding="utf-8").read())
    shown = subprocess.run(["git", "show", f"{base}:Cargo.toml"], capture_output=True, text=True)
    if shown.returncode != 0:
        listed = subprocess.run(["git", "ls-tree", "--name-only", base, "Cargo.toml"],
                                check=True, capture_output=True, text=True).stdout
        if listed.strip():
            sys.exit(f"cannot read {base}:Cargo.toml: {shown.stderr.strip()}")
        print(f"version {head}: {base} has no Cargo.toml yet (first release)")
        return
    base_text = shown.stdout
    if head <= version(base_text):
        sys.exit(f"version {head} is not above {base}'s {version(base_text)}")
    print(f"version {head} above {base}")


if __name__ == "__main__":
    main()
