"""Temporarily append observation tests, run only them, restore exact bytes.

Run from the repository root with no concurrent source edits. No GUI, network,
real user settings, or real clipboard mutation is used. Passing tests confirm
the current defects; these are evidence probes, not regression expectations.
"""
from pathlib import Path
import hashlib
import subprocess

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
PAIRS = [
    (ROOT / "apps/ugurugu/src/files.rs", HERE / "files-probes.rs"),
    (ROOT / "crates/ugu-session/src/lib.rs", HERE / "session-probes.rs"),
    (ROOT / "apps/ugurugu/src/update.rs", HERE / "update-probes.rs"),
    (ROOT / "apps/ugurugu/src/ui/window.rs", HERE / "ui-probes.rs"),
    (ROOT / "apps/ugurugu/src/settings.rs", HERE / "settings-probes.rs"),
]
originals = {}
injected = {}
try:
    for target, probe in PAIRS:
        if subprocess.run(["git", "diff", "--quiet", "HEAD", "--", str(target)], cwd=ROOT).returncode:
            raise RuntimeError(f"Source already modified: {target}")
        original = target.read_bytes()
        originals[target] = original
        content = original + b"\n" + probe.read_bytes()
        injected[target] = content
        target.write_bytes(content)
    with (HERE / "review-probes.log").open("wb") as log:
        result = subprocess.run(
            ["cargo", "test", "--release", "--locked", "--offline", "-p", "ugurugu",
             "-p", "ugu-session", "review_audit", "--", "--nocapture", "--test-threads=1"],
            cwd=ROOT, stdout=log, stderr=subprocess.STDOUT,
        )
    print(f"probe exit code: {result.returncode}")
finally:
    conflicts = []
    for target, original in originals.items():
        if target.read_bytes() != injected[target]:
            backup = HERE / (target.parent.parent.name + "-" + target.name + ".original")
            backup.write_bytes(original)
            conflicts.append(str(backup))
            continue
        target.write_bytes(original)
        print(f"restored {target.relative_to(ROOT)} sha256={hashlib.sha256(original).hexdigest()}")
    if conflicts:
        raise RuntimeError(f"Concurrent edit detected; originals kept at: {', '.join(conflicts)}")

raise SystemExit(result.returncode)
