#!/usr/bin/env python3
"""Refresh only the rendered diagram snapshots affected by connector repair.

Usage: python3 tools/refresh_connector_snapshots.py /path/to/current/vizir
Published HIR/MIR and chart-only byte expectations remain unchanged.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "crates/vizir-cli/tests/fixtures"
BIN = str(Path(sys.argv[1]).resolve())
SOURCES = {
    "examples/composition/service-grid.compose.yaml",
    "examples/diagram/agent-runtime.viz.yaml",
    "examples/diagram/data-platform.viz.yaml",
    "examples/diagram/dialect-lowering.viz.yaml",
    "examples/interaction/linked-services.viz.yaml",
    "examples/mixed/capacity-planning.viz.yaml",
    "examples/mixed/reliability-brief.viz.yaml",
    "examples/mixed/theme-defaults.viz.yaml",
}


def run(*args):
    return subprocess.run([BIN, *map(str, args)], cwd=ROOT, check=True, capture_output=True).stdout


def digest(data):
    return hashlib.sha256(data).hexdigest()


with tempfile.TemporaryDirectory() as directory:
    scratch = Path(directory)
    for name in ("plot-alignment-legacy-bytes.json", "shared-legend-legacy-bytes.json"):
        path = FIXTURES / name
        fixture = json.loads(path.read_text())
        for case in fixture["cases"]:
            if case["source"] not in SOURCES:
                continue
            assert not case.get("profile") and not case.get("theme")
            source = case["source"]
            if "hir_sha256" in case:
                hir = run("compose", source)
                assert digest(hir) == case["hir_sha256"], "HIR contract drift"
                source = scratch / "composed.json"
                source.write_bytes(hir)
            normalized = run("normalize", source)
            assert digest(normalized) == case["normalize_sha256"], "MIR contract drift"
            scene = run("lower", source)
            svg = scratch / "diagram.svg"
            run("render", source, "--format", "svg", "-o", svg)
            case["lower_sha256"] = digest(scene)
            case["svg_sha256"] = digest(svg.read_bytes())
            if "replay_lower_sha256" in case:
                persisted = json.loads(normalized)
                if "format" not in persisted:
                    persisted = {"format": "vizir-compiled-mir/1", "context": {}, "mir": persisted}
                replay = scratch / "replay.json"
                replay.write_text(json.dumps(persisted))
                assert digest(run("normalize", replay)) == case["replay_normalize_sha256"]
                case["replay_lower_sha256"] = digest(run("lower", replay))
                run("render", replay, "--format", "svg", "-o", svg)
                case["replay_svg_sha256"] = digest(svg.read_bytes())
        fixture["render_revision"] = {
            "date": "2026-10-11",
            "reason": "Distinct diagram ports, external self-loops and fixed-size visible arrowheads",
            "sources": sorted(SOURCES),
            "unchanged": "All HIR/MIR, schema and chart-only byte expectations",
        }
        path.write_text(json.dumps(fixture, indent=2) + "\n")
        print(f"refreshed diagram geometry in {name}")
    source = FIXTURES / "legacy-theme-source.json"
    mir = scratch / "legacy.mir.json"
    run("normalize", source, "-o", mir)
    assert mir.read_bytes() == (FIXTURES / "legacy-theme.mir.json").read_bytes()
    run("lower", source, "-o", FIXTURES / "legacy-theme.scene.json")
    run("render", source, "--format", "svg", "-o", FIXTURES / "legacy-theme.svg")
