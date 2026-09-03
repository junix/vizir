#!/usr/bin/env python3
"""One-shot evidence freezer for the vizir explainer infographic.

Runs the real vizir CLI inside a /tmp sandbox and exports every number that
appears on the page into data/*.json. Run ONCE; afterwards data/ is frozen
truth and this script is never re-run inside the delivery directory.

Engine repo is treated as strictly read-only: the only writes are /tmp
sandbox artifacts and this delivery directory. cargo test is invoked once to
record the live test count (writes go to the engine's target/ only).
"""
from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

ENGINE = Path.home() / "projects/plot/vizir"
VIZIR = ENGINE / "target/release/vizir"
SANDBOX = Path("/tmp/vizir-explainer-freeze")
EXAMPLE = ENGINE / "examples/chart/service-health.viz.yaml"
DELIV = Path(__file__).resolve().parent
DATA = DELIV / "data"

CRATE_TEST_RE = re.compile(r"Running (.+) \(")
RESULT_RE = re.compile(r"test result: ok\. (\d+) passed")


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(argv: list[str], **kw) -> subprocess.CompletedProcess:
    return subprocess.run(argv, capture_output=True, text=True, **kw)


def out(proc: subprocess.CompletedProcess) -> dict:
    return {"exit": proc.returncode, "stdout": proc.stdout, "stderr": proc.stderr}


def write(name: str, payload: dict) -> None:
    (DATA / name).write_text(
        json.dumps(payload, ensure_ascii=False, indent=1, sort_keys=False) + "\n"
    )
    print(f"wrote data/{name}")


def parse_explain(text: str) -> dict:
    fields = {}
    for line in text.strip().splitlines():
        if ":" in line:
            k, v = line.split(":", 1)
            fields[k.strip()] = v.strip()
    return fields


def main() -> None:
    if DATA.exists() and any(DATA.glob("*.json")):
        sys.exit("data/ already frozen — prep_data.py must never be re-run")
    DATA.mkdir(parents=True, exist_ok=True)
    SANDBOX.mkdir(parents=True, exist_ok=True)

    # ---- engine identity --------------------------------------------------
    commit = run(["git", "-C", str(ENGINE), "rev-parse", "HEAD"]).stdout.strip()
    dirty = run(["git", "-C", str(ENGINE), "status", "--porcelain"]).stdout
    version = out(run([str(VIZIR), "--version"]))
    rustc = run(["rustc", "--version"]).stdout.strip()
    write("engine.json", {
        "commit": commit,
        "dirty_status_baseline": dirty,
        "dirty_note": (
            "README.md/justfile 修改与 examples/mixed/capacity-planning.viz.yaml、"
            "gallery*、tools/ 未跟踪文件是构建长图之前就存在的用户工作树状态，"
            "非本次交付产生；本交付只新增 docs/infographics/vizir-explainer/。"
        ),
        "vizir_version": version["stdout"].strip(),
        "rustc": rustc,
        "example_input": str(EXAMPLE.relative_to(ENGINE)),
        "example_sha256": sha256(EXAMPLE),
    })

    # ---- CLI surface ------------------------------------------------------
    help_text = run([str(VIZIR), "--help"]).stdout
    subcommands = re.findall(
        r"^  (\w+)\s{2,}(.+)$", help_text, re.MULTILINE
    )
    write("cli.json", {
        "subcommands": [
            {"name": n, "about": a.strip()} for n, a in subcommands
            if n not in ("help",)
        ],
        "subcommand_count": len([n for n, _ in subcommands if n != "help"]),
    })

    # ---- explain decision tree -------------------------------------------
    nodes = [
        ("latency-risk/point/gateway", True),
        ("latency-risk/point/inference", True),
        ("latency-risk/axis/x", True),
        ("availability-ranking/bar/billing", True),
        ("availability-ranking/axis/x/category/3", True),
        ("does/not/exist", False),
    ]
    samples = []
    for node, ok in nodes:
        proc = run([str(VIZIR), "explain", str(EXAMPLE), "--node", node])
        samples.append({
            "queried": node,
            "expected_ok": ok,
            "exit": proc.returncode,
            "fields": parse_explain(proc.stdout) if proc.returncode == 0 else {},
            "stderr": proc.stderr.strip(),
        })
    write("explain_samples.json", {"samples": samples})

    # ---- render SVG twice (different outputs) + fixed-path manifest pair --
    svg_a = SANDBOX / "run-a.svg"
    svg_b = SANDBOX / "run-b.svg"
    man_a = SANDBOX / "run-a.manifest.json"
    man_fixed1 = SANDBOX / "fixed.manifest.json"
    man_fixed2 = SANDBOX / "fixed2.manifest.json"
    png_a = SANDBOX / "run-a.png"
    png_man = SANDBOX / "run-a.png.manifest.json"
    r1 = run([str(VIZIR), "render", str(EXAMPLE), "--format", "svg",
              "--output", str(svg_a), "--manifest", str(man_a)])
    r2 = run([str(VIZIR), "render", str(EXAMPLE), "--format", "svg",
              "--output", str(svg_b)])
    r3 = run([str(VIZIR), "render", str(EXAMPLE), "--format", "svg",
              "--output", str(svg_a), "--manifest", str(man_fixed1)])
    r4 = run([str(VIZIR), "render", str(EXAMPLE), "--format", "svg",
              "--output", str(svg_a), "--manifest", str(man_fixed2)])
    assert r1.returncode == r2.returncode == r3.returncode == r4.returncode == 0

    mir_a = SANDBOX / "run-a.mir.json"
    mir_b = SANDBOX / "run-b.mir.json"
    scene_a = SANDBOX / "run-a.scene.json"
    scene_b = SANDBOX / "run-b.scene.json"
    for path in (mir_a, mir_b):
        run([str(VIZIR), "normalize", str(EXAMPLE), "-o", str(path)])
    for path in (scene_a, scene_b):
        run([str(VIZIR), "lower", str(EXAMPLE), "-o", str(path)])

    fixed_identical = man_fixed1.read_bytes() == man_fixed2.read_bytes()
    man_pair = json.loads(man_a.read_text())
    man_b = SANDBOX / "run-b.manifest.json"
    r_b = run([str(VIZIR), "render", str(EXAMPLE), "--format", "svg",
               "--output", str(svg_b), "--manifest", str(man_b)])
    assert r_b.returncode == 0
    man_other = json.loads(man_b.read_text())
    diff_keys = sorted(
        k for k in man_pair if json.dumps(man_pair[k], sort_keys=True)
        != json.dumps(man_other[k], sort_keys=True)
    )
    write("determinism.json", {
        "pairs": [
            {"artifact": "VizMIR (normalize)", "file": "run-?.mir.json",
             "run_a": sha256(mir_a), "run_b": sha256(mir_b),
             "identical": sha256(mir_a) == sha256(mir_b)},
            {"artifact": "Scene2D (lower)", "file": "run-?.scene.json",
             "run_a": sha256(scene_a), "run_b": sha256(scene_b),
             "identical": sha256(scene_a) == sha256(scene_b)},
            {"artifact": "SVG (render)", "file": "run-?.svg",
             "run_a": sha256(svg_a), "run_b": sha256(svg_b),
             "identical": sha256(svg_a) == sha256(svg_b)},
        ],
        "manifest_noise": {
            "same_output_path_reruns_byte_identical": fixed_identical,
            "differing_keys_when_output_path_differs": diff_keys,
        },
        "note": "同一进程外因只允许 manifest 的 output 路径一行变化；产物字节不变。",
    })

    # ---- capability negotiation ------------------------------------------
    cap_svg = json.loads(run([str(VIZIR), "capabilities", "svg"]).stdout)
    cap_png = json.loads(run([str(VIZIR), "capabilities", "png"]).stdout)
    decisions = man_pair["capability_report"]["decisions"]
    png_proc = run([str(VIZIR), "render", str(EXAMPLE), "--format", "png",
                    "--output", str(png_a), "--manifest", str(png_man)])
    assert png_proc.returncode == 0
    png_manifest = json.loads(png_man.read_text())
    png_decisions = png_manifest["capability_report"]["decisions"]
    write("capability.json", {
        "svg_backend": cap_svg,
        "png_backend": cap_png,
        "svg_manifest_decisions": {
            "total": len(decisions),
            "by_status": dict(Counter(d["status"] for d in decisions)),
            "by_feature": dict(Counter(d["feature"] for d in decisions)
                               .most_common()),
            "sample": decisions[:3],
        },
        "png_manifest_decisions": {
            "total": len(png_decisions),
            "by_status": dict(Counter(d["status"] for d in png_decisions)),
            "sample_rasterized": next(
                d for d in png_decisions if d["status"] == "rasterized"),
        },
        "manifest_keys": sorted(man_pair.keys()),
        "compiler": man_pair["compiler"],
        "source_ir_version": man_pair["source_ir_version"],
        "accepted_ir": man_pair["capability_report"]["accepted_ir"],
        "png_losses": png_manifest["losses"],
        "png_rasterizer": png_manifest["rasterizer"],
    })

    # ---- fail-loud experiments -------------------------------------------
    empty_bin = SANDBOX / "emptybin"
    empty_bin.mkdir(exist_ok=True)
    doomed_out = SANDBOX / "should-not-exist.png"
    doomed_man = SANDBOX / "should-not-exist.manifest.json"
    fail = subprocess.run(
        [str(VIZIR), "render", str(EXAMPLE), "--format", "png",
         "--output", str(doomed_out), "--manifest", str(doomed_man)],
        capture_output=True, text=True, env={"PATH": str(empty_bin)},
    )
    write("fail_loud.json", {
        "cap0001": {
            "command": "env PATH=<empty> vizir render service-health.viz.yaml "
                       "--format png --output should-not-exist.png",
            "exit": fail.returncode,
            "stderr": fail.stderr.strip(),
            "output_file_exists": doomed_out.exists(),
            "manifest_file_exists": doomed_man.exists(),
        },
        "explain0001": {
            "command": "vizir explain service-health.viz.yaml --node does/not/exist",
            "exit": samples[-1]["exit"],
            "stderr": samples[-1]["stderr"],
        },
    })

    # ---- Scene2D accountability surface ----------------------------------
    scene = json.loads(scene_a.read_text())

    def walk(nodes, acc):
        for n in nodes:
            acc.append(n)
            kids = n.get("children")
            if isinstance(kids, list):
                walk(kids, acc)
        return acc

    flat = walk(scene["nodes"], [])
    gb = Counter((n.get("origin") or {}).get("generated_by", "(none)")
                 for n in flat)
    gateway = next(n for n in flat if n.get("id") == "latency-risk/point/gateway")
    write("scene_nodes.json", {
        "total_nodes": len(flat),
        "view_groups": [n["id"] for n in scene["nodes"]],
        "canvas": {"width": scene["width"], "height": scene["height"],
                   "background": scene["background"]},
        "losses": scene["losses"],
        "generated_by_histogram": dict(gb.most_common()),
        "origin_example": gateway["origin"],
        "origin_example_node_id": gateway["id"],
    })

    # ---- MIR provenance / nice-domain ------------------------------------
    mir = json.loads(mir_a.read_text())

    def find_provenance(obj, path=""):
        found = []
        if isinstance(obj, dict):
            if obj.get("provenance"):
                found.append({"id": obj.get("id", path),
                              "provenance": obj["provenance"],
                              "scales": [
                                  {"id": s["id"], "domain": s.get("domain"),
                                   "range": s.get("range")}
                                  for s in obj.get("scales", [])
                              ]})
            for k, v in obj.items():
                found.extend(find_provenance(v, f"{path}/{k}"))
        elif isinstance(obj, list):
            for i, v in enumerate(obj):
                found.extend(find_provenance(v, path))
        return found

    write("mir_provenance.json", {"views": find_provenance(mir)})

    # ---- diagnostic code inventory (81 codes / 14 families) --------------
    grep = run(["grep", "-rn", "-o", r"VIZ-[A-Z]*-[0-9]\{4\}",
                str(ENGINE / "crates"), "--include=*.rs"])
    occurrences = []
    for line in grep.stdout.splitlines():
        m = re.match(r"(.+?):(\d+):(VIZ-[A-Z]+-\d{4})", line)
        if m:
            occurrences.append((m.group(1), int(m.group(2)), m.group(3)))
    by_code: dict[str, dict] = {}
    for f, ln, code in occurrences:
        rec = by_code.setdefault(code, {"first": None, "src_hits": 0,
                                        "test_hits": 0})
        # path-based split: /tests/ = integration test file, /src/ = library
        # (unit tests inside #[cfg(test)] modules count as src by path)
        rec["src_hits" if "/tests/" not in f else "test_hits"] += 1
        if rec["first"] is None:
            rec["first"] = {
                "file": str(Path(f).relative_to(ENGINE)),
                "line": ln,
            }
    families = Counter(code.rsplit("-", 1)[0] for code in by_code)
    write("diag_codes.json", {
        "total_codes": len(by_code),
        "family_count": len(families),
        "families": dict(families.most_common()),
        "codes": {code: by_code[code] for code in sorted(by_code)},
    })

    # ---- patch contract ----------------------------------------------------
    patch_src = (ENGINE / "crates/vizir-core/src/patch.rs").read_text()
    test_fns = []
    for m in re.finditer(r"#\[test\]\s*(?:#\[[^\]]*\]\s*)*fn (\w+)",
                         patch_src):
        fn_start = m.start(1)
        line = patch_src[:fn_start].count("\n") + 1
        test_fns.append({"name": m.group(1), "line": line})
    patch_codes = sorted(set(re.findall(r"VIZ-PATCH-\d{4}", patch_src)))
    write("patch.json", {
        "protocol_version": "0.1",
        "diff_scene_line": 64,
        "apply_scene_patch_line": 113,
        "equivalence_test_line": next(
            t["line"] for t in test_fns
            if t["name"] == "diff_and_apply_match_full_scene_semantics"),
        "tests": test_fns,
        "test_count": len(test_fns),
        "diagnostic_codes": patch_codes,
        "scene_patch_schema_sha256": None,  # filled after schema step
    })

    # ---- schemas + anti-drift ---------------------------------------------
    schemas = {}
    checked_in_ok = {}
    for ir, fname in [("mir", "viz-mir.schema.json"),
                      ("scene-patch", "scene-patch.schema.json"),
                      ("capability", "capability.schema.json")]:
        emitted = SANDBOX / f"{ir}.schema.json"
        run([str(VIZIR), "schema", ir, "-o", str(emitted)])
        checked = ENGINE / "schemas" / fname
        schemas[ir] = {
            "emitted_sha256": sha256(emitted),
            "checked_in_path": f"schemas/{fname}",
            "matches_checked_in": emitted.read_bytes() == checked.read_bytes(),
        }
        checked_in_ok[ir] = schemas[ir]["matches_checked_in"]
    write("schemas.json", {
        "schemas": schemas,
        "anti_drift_test": {
            "file": "crates/vizir-cli/tests/cli.rs",
            "line": 312,
            "name": "schema_subcommand_emits_the_checked_in_canonical_schemas",
        },
    })
    patch_data = json.loads((DATA / "patch.json").read_text())
    patch_data["scene_patch_schema_sha256"] = schemas["scene-patch"][
        "emitted_sha256"]
    write("patch.json", patch_data)

    # ---- examples inventory -------------------------------------------------
    examples = sorted(
        str(p.relative_to(ENGINE / "examples"))
        for p in (ENGINE / "examples").rglob("*.viz.yaml")
    )
    fam = Counter(p.split("/")[0] for p in examples)
    write("examples.json", {
        "files": examples,
        "count": len(examples),
        "by_family": dict(sorted(fam.items())),
    })

    # ---- live test count (writes only to engine target/) -------------------
    # "Running <crate>" lines go to stderr, "test result" to stdout; merge to
    # keep their interleaved order.
    proc = subprocess.run(["cargo", "test", "--workspace"], cwd=str(ENGINE),
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                          text=True)
    per_crate = {}
    current = None
    for line in proc.stdout.splitlines():
        m = CRATE_TEST_RE.search(line)
        if m:
            current = m.group(1)
            continue
        m = RESULT_RE.search(line)
        if m and current:
            per_crate[current] = per_crate.get(current, 0) + int(m.group(1))
            current = None
    write("tests.json", {
        "per_crate": per_crate,
        "total_passed": sum(per_crate.values()),
        "exit": proc.returncode,
    })

    # ---- provenance --------------------------------------------------------
    write("provenance.json", {
        "engine_commit": commit,
        "engine_repo": str(ENGINE),
        "vizir_binary": str(VIZIR),
        "sandbox": str(SANDBOX),
        "primary_example": str(EXAMPLE.relative_to(ENGINE)),
        "primary_example_sha256": sha256(EXAMPLE),
        "source_refs": {
            "origin_struct": "crates/vizir-core/src/scene.rs:132-142",
            "scene_capability_requirements": "crates/vizir-core/src/capability.rs:134",
            "negotiate_scene": "crates/vizir-core/src/capability.rs:152",
            "cap0002_diagnostic": "crates/vizir-core/src/capability.rs:127-131",
            "diff_scene": "crates/vizir-core/src/patch.rs:64",
            "apply_scene_patch": "crates/vizir-core/src/patch.rs:113",
            "patch_equivalence_test": "crates/vizir-core/src/patch.rs:457",
            "cli_commands_enum": "crates/vizir-cli/src/main.rs:22-23",
            "cap0001_diagnostic": "crates/vizir-cli/src/main.rs:334",
            "nice_domain": "crates/vizir-compiler/src/lower.rs:165-166",
            "chart_provenance_strings": "crates/vizir-compiler/src/lower.rs:217-222",
            "schema_anti_drift_test": "crates/vizir-cli/tests/cli.rs:312",
        },
        "frozen_at": "2026-09-02",
        "method": "prep_data.py 一次性冻结；运行期噪声（manifest output 路径）以固定输出路径消除",
    })
    print("freeze complete")


if __name__ == "__main__":
    main()
