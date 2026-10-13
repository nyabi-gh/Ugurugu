# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Nyabi (nyabi-gh)
"""Fails when the docs point at something that is not there.

- A relative link or image in a tracked Markdown file must name a tracked
  file or folder.
- Every backlog ID (B3-nn) mentioned anywhere in the tree must be a row of
  docs/rust/backlog.md, and every row must be there once.

Release notes are left alone: they describe the tree as it was.
"""

import posixpath
import re
import subprocess
import sys
from pathlib import Path, PurePosixPath

BACKLOG = "docs/rust/backlog.md"
LINK = re.compile(r"\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)|(?:href|src)=\"([^\"]+)\"")
ID = re.compile(r"\bB3-\d+\b")
ROW = re.compile(r"^\|\s*(B3-\d+)\s*\|", re.MULTILINE)


def tracked(root: Path) -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "-z"], cwd=root, check=True, capture_output=True
    ).stdout
    return [p for p in out.decode("utf-8").split("\0") if p]


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    files = tracked(root)
    known = set(files)
    for path in files:
        parent = PurePosixPath(path).parent
        while parent != PurePosixPath("."):
            known.add(str(parent))
            parent = parent.parent

    problems = []
    texts = {}
    for path in files:
        if path.startswith("release-notes/"):
            continue
        try:
            texts[path] = (root / path).read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue

    for path, text in texts.items():
        if not path.endswith(".md"):
            continue
        for number, line in enumerate(text.splitlines(), 1):
            for match in LINK.finditer(line):
                target = match.group(1) or match.group(2)
                if re.match(r"^[a-z][a-z0-9+.-]*:", target) or target.startswith("#"):
                    continue
                target = target.split("#", 1)[0]
                resolved = posixpath.normpath(posixpath.join(posixpath.dirname(path), target))
                if resolved not in known:
                    problems.append(f"{path}:{number}: no such file: {target}")

    rows = ROW.findall(texts.get(BACKLOG, ""))
    for duplicate in sorted({r for r in rows if rows.count(r) > 1}):
        problems.append(f"{BACKLOG}: {duplicate} has more than one row")
    for path, text in texts.items():
        for number, line in enumerate(text.splitlines(), 1):
            for found in ID.findall(line):
                if found not in rows:
                    problems.append(f"{path}:{number}: {found} is not a row of {BACKLOG}")

    for problem in problems:
        print(problem)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
