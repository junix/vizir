#!/usr/bin/env python3
"""Whole-tree fingerprint manifest (2026-09-07 audit hardening, class
fingerprint-gaps). Closes the §12.4 gap where data/*.json "未变" was a
claim, not a fingerprint: every file in the tree — data (frozen evidence),
tools (generators + audit machinery), render (bitmaps), docs (README /
VERIFICATION / contract) — is hashed with sha256.

Single named exemption: data/audit/ — the run-record location (pills.json,
this manifest, detached-verify records). Run records must stay outside the
fingerprint or every gate re-run would invalidate the manifest and force a
new commit (fixpoint rule, audit-batteries §7). Justified in VERIFICATION
§13; adding any further exemption requires a new dated VERIFICATION entry.

Stable fields only: relative posix paths, sorted; no timestamps, no git
HEAD, no machine paths — the manifest is idempotent (re-running changes
nothing) and path-independent (verify from a copied tree with --check).

Usage:
  python3 tools/fingerprint_tree.py           # write data/audit/fingerprints.json
  python3 tools/fingerprint_tree.py --check   # verify tree against the
                                             # manifest; exit 1 on any
                                             # changed/missing/extra file
"""
from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

TREE = Path(__file__).resolve().parent.parent
MANIFEST = TREE / "data" / "audit" / "fingerprints.json"
EXEMPTIONS = ["data/audit/"]
EXEMPTION_RATIONALE = (
    "data/audit/ 是门禁运行记录区（毒丸记录、本指纹清单、detached 复核"
    "记录）：运行记录若入指纹，每次门禁重跑都会使清单失效并触发新的提交"
    "（fixpoint 规则，audit-batteries §7）；冻结证据 data/*.json 本身全部"
    "在册，不受该豁免影响。"
)


def is_exempt(rel: str) -> bool:
    return any(rel.startswith(e) for e in EXEMPTIONS)


def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 16), b""):
            h.update(chunk)
    return h.hexdigest()


def scan() -> dict[str, str]:
    files: dict[str, str] = {}
    for p in sorted(TREE.rglob("*")):
        if not p.is_file():
            continue
        rel = p.relative_to(TREE).as_posix()
        if is_exempt(rel):
            continue
        files[rel] = sha256_file(p)
    return files


def render(files: dict[str, str]) -> str:
    doc = {
        "algorithm": "sha256",
        "exemptions": EXEMPTIONS,
        "exemption_rationale": EXEMPTION_RATIONALE,
        "file_count": len(files),
        "files": {k: files[k] for k in sorted(files)},
    }
    return json.dumps(doc, ensure_ascii=False, indent=1) + "\n"


def main() -> int:
    if "--check" in sys.argv[1:]:
        if not MANIFEST.exists():
            print("FAIL: manifest missing, run without --check first",
                  file=sys.stderr)
            return 1
        recorded = json.loads(MANIFEST.read_text())["files"]
        current = scan()
        bad = 0
        for rel in sorted(set(recorded) | set(current)):
            if rel in recorded and rel not in current:
                print(f"FAIL: recorded file missing: {rel}", file=sys.stderr)
                bad += 1
            elif rel not in recorded and rel in current:
                print(f"FAIL: unrecorded file present: {rel}",
                      file=sys.stderr)
                bad += 1
            elif recorded[rel] != current[rel]:
                print(f"FAIL: hash mismatch: {rel} "
                      f"{recorded[rel][:12]}… != {current[rel][:12]}…",
                      file=sys.stderr)
                bad += 1
        if bad:
            print(f"fingerprint check: {bad} problem(s)", file=sys.stderr)
            return 1
        print(f"fingerprint check: {len(current)} files all match "
              f"({MANIFEST.relative_to(TREE)} exempted: {EXEMPTIONS})")
        return 0

    files = scan()
    MANIFEST.parent.mkdir(exist_ok=True)
    out = render(files)
    if MANIFEST.exists() and MANIFEST.read_text() == out:
        print(f"fingerprints unchanged: {len(files)} files "
              f"(idempotent re-run)")
        return 0
    MANIFEST.write_text(out)
    print(f"wrote {MANIFEST.relative_to(TREE)}: {len(files)} files, "
          f"exempt {EXEMPTIONS}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
