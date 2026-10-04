use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use vizir_core::Scene2D;
use vizir_web::{ExplorerOptions, SelectionLinks};
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vizir"))
}
fn source() -> PathBuf {
    root().join("examples/interaction/linked-services.viz.yaml")
}
fn successful(mut command: Command) -> Vec<u8> {
    let out = command.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}
fn lowered() -> Scene2D {
    let mut c = cli();
    c.arg("lower").arg(source());
    serde_json::from_slice(&successful(c)).unwrap()
}
fn authoring_spec(instance: &str) -> SelectionLinks {
    let scene = lowered();
    let v1 = vizir_web::render_html(
        &scene,
        &ExplorerOptions {
            instance_key: instance.into(),
        },
    )
    .unwrap();
    SelectionLinks {
        format: vizir_web::LINKS_FORMAT.into(),
        document_id: scene.document_id,
        scene_sha256: v1.manifest.scene_sha256,
        instance_key: instance.into(),
        groups: vec![
            vizir_web::SelectionGroup {
                id: "api".into(),
                members: vec![
                    "status/api-card".into(),
                    "topology/node/api/shape".into(),
                    "topology/node/api/label".into(),
                    "latency/point/api".into(),
                ],
            },
            vizir_web::SelectionGroup {
                id: "database".into(),
                members: vec![
                    "status/db-card".into(),
                    "topology/node/db/shape".into(),
                    "latency/point/db".into(),
                ],
            },
        ],
    }
}
fn render(input: &Path, spec: &Path, output: &Path, manifest: &Path) -> Command {
    let mut c = cli();
    c.arg("render")
        .arg(input)
        .args([
            "--format",
            "html",
            "--interaction-profile",
            "explorer-linked-v1",
            "--selection-links",
        ])
        .arg(spec)
        .arg("-o")
        .arg(output)
        .arg("--manifest")
        .arg(manifest);
    c
}
fn structural(path: &Path) {
    let out = Command::new("python3")
        .arg(root().join("tools/check_web_package.py"))
        .arg(path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
fn metadata(html: &str) -> Value {
    serde_json::from_str(
        html.split("data-vizir-metadata=\"\">\n")
            .nth(1)
            .unwrap()
            .split("\n</script>")
            .next()
            .unwrap(),
    )
    .unwrap()
}
#[test]
fn exact_mixed_chart_graph_geometry_spec_exports_and_runs_native_reducer() {
    let t = tempfile::tempdir().unwrap();
    let spec = t.path().join("links.json");
    let output = t.path().join("linked.html");
    let manifest = t.path().join("linked.json");
    fs::write(&spec, serde_json::to_vec(&authoring_spec("main")).unwrap()).unwrap();
    successful(render(&source(), &spec, &output, &manifest));
    structural(&output);
    let html = fs::read_to_string(&output).unwrap();
    let context = metadata(&html);
    assert_eq!(context["format"], "vizir-interaction/2");
    assert_eq!(context["link_groups"][0]["members"][0], "latency/point/api");
    let report: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(report["interaction"]["profile"], "explorer-linked-v1");
    assert_eq!(report["capability_report"]["backend"], "html-linked");
    let context_path = t.path().join("context.json");
    fs::write(&context_path, serde_json::to_vec(&context).unwrap()).unwrap();
    let out=Command::new("node").args(["--input-type=module","-e",r#"import fs from 'node:fs';import{pathToFileURL}from'node:url';const r=await import(pathToFileURL(process.argv[1]));const c=r.validateContext(JSON.parse(fs.readFileSync(process.argv[2],'utf8')));let s=r.createState(c);s=r.reduce(s,{type:'Select',reference:r.nodeReference(c,'latency/point/api')},c);if(s.linked.length!==3||s.selected_group_id!=='api')throw Error('links');s=r.reduce(s,{type:'Select',reference:r.nodeReference(c,'status/api-card')},c);if(s.linked.length!==3||!s.linked.some(x=>x.scene_node_id==='latency/point/api'))throw Error('primary rotation');s=r.reduce(s,{type:'Escape'},c);if(s.selected||s.linked.length)throw Error('clear');"#]).arg(root().join("crates/vizir-web/runtime/runtime.mjs")).arg(context_path).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
#[test]
fn linked_flags_are_explicit_and_invalid_specs_preserve_both_destinations() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("source.yaml");
    fs::copy(source(), &input).unwrap();
    let spec = t.path().join("links.json");
    let output = t.path().join("out.html");
    let manifest = t.path().join("out.json");
    for bad in [
        json!({}),
        json!({"format":"vizir-selection-links/2"}),
        serde_json::to_value({
            let mut s = authoring_spec("main");
            s.scene_sha256 = "0".repeat(64);
            s
        })
        .unwrap(),
    ] {
        fs::write(&spec, serde_json::to_vec(&bad).unwrap()).unwrap();
        fs::write(&output, "old output").unwrap();
        fs::write(&manifest, "old manifest").unwrap();
        let out = render(&input, &spec, &output, &manifest).output().unwrap();
        assert!(!out.status.success());
        assert_eq!(fs::read_to_string(&output).unwrap(), "old output");
        assert_eq!(fs::read_to_string(&manifest).unwrap(), "old manifest");
    }
    fs::write(&spec, serde_json::to_vec(&authoring_spec("main")).unwrap()).unwrap();
    for args in [
        vec![
            "--format",
            "html",
            "--interaction-profile",
            "explorer-linked-v1",
        ],
        vec![
            "--format",
            "html",
            "--interaction-profile",
            "explorer-v1",
            "--selection-links",
        ],
        vec!["--format", "svg", "--selection-links"],
    ] {
        let needs = args.last() == Some(&"--selection-links");
        let mut c = cli();
        c.arg("render").arg(&input).args(args);
        if needs {
            c.arg(&spec);
        }
        let out = c.arg("-o").arg(&output).output().unwrap();
        assert!(!out.status.success());
        assert_eq!(fs::read_to_string(&output).unwrap(), "old output");
    }
    assert!(fs::read_dir(t.path()).unwrap().all(|p| {
        !p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".vizir-")
    }));
}
#[test]
fn spec_source_aliases_and_budget_fail_before_publication() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("source.yaml");
    fs::copy(source(), &input).unwrap();
    let before = fs::read(&input).unwrap();
    let spec = t.path().join("links.json");
    fs::write(&spec, serde_json::to_vec(&authoring_spec("main")).unwrap()).unwrap();
    let oldspec = fs::read(&spec).unwrap();
    let output = t.path().join("out.html");
    let manifest = t.path().join("out.json");
    for (sourcefile, linkfile, outfile, report) in [
        (&input, &spec, &spec, &manifest),
        (&input, &spec, &output, &spec),
        (&input, &input, &output, &manifest),
        (&input, &spec, &input, &manifest),
        (&input, &spec, &output, &input),
    ] {
        assert!(
            !render(sourcefile, linkfile, outfile, report)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    assert_eq!(fs::read(&input).unwrap(), before);
    assert_eq!(fs::read(&spec).unwrap(), oldspec);
    let alias = t.path().join("link-alias");
    fs::hard_link(&spec, &alias).unwrap();
    assert!(
        !render(&input, &spec, &alias, &manifest)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        !render(&input, &spec, &output, &alias)
            .output()
            .unwrap()
            .status
            .success()
    );
    #[cfg(unix)]
    {
        let sym = t.path().join("sym");
        std::os::unix::fs::symlink(&spec, &sym).unwrap();
        assert!(
            !render(&input, &spec, &sym, &manifest)
                .output()
                .unwrap()
                .status
                .success()
        );
        assert!(
            !render(&input, &spec, &output, &sym)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    fs::write(&spec, vec![b' '; vizir_web::MAX_LINK_SPEC_BYTES + 1]).unwrap();
    assert!(
        !render(&input, &spec, &output, &manifest)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!output.exists());
    assert!(!manifest.exists());
}
#[test]
fn additive_link_schemas_match_and_v1_stays_unchanged() {
    for name in ["selection-links", "interaction-linked"] {
        let mut c = cli();
        c.args(["schema", name]);
        let actual: Value = serde_json::from_slice(&successful(c)).unwrap();
        let expected: Value = serde_json::from_slice(
            &fs::read(root().join(format!("schemas/{name}.schema.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(actual, expected);
    }
    let old: Value =
        serde_json::from_slice(&fs::read(root().join("schemas/interaction.schema.json")).unwrap())
            .unwrap();
    assert_eq!(old, vizir_web::interaction_schema());
}
#[test]
fn current_payload_supports_structurally_isolated_v1_and_v2_fragments() {
    let t = tempfile::tempdir().unwrap();
    let scene = lowered();
    let v1 = vizir_web::render_fragment(
        &scene,
        &ExplorerOptions {
            instance_key: "single".into(),
        },
    )
    .unwrap();
    let links = authoring_spec("linked");
    let v2 = vizir_web::render_linked_fragment(
        &scene,
        &ExplorerOptions {
            instance_key: "linked".into(),
        },
        &links,
    )
    .unwrap();
    assert_eq!(
        v1.manifest.runtime_payload_sha256,
        v2.manifest.runtime_payload_sha256
    );
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"{}\"><style>{}</style></head><body>{}{}<script type=\"module\">{}</script></body></html>",
        vizir_web::content_security_policy(),
        vizir_web::STYLESHEET,
        v1.html,
        v2.html,
        vizir_web::runtime_script()
    );
    let path = t.path().join("mixed.html");
    fs::write(&path, html).unwrap();
    structural(&path);
    assert_eq!(metadata(&v1.html)["format"], "vizir-interaction/1");
    assert!(metadata(&v1.html).get("link_groups").is_none());
}

#[test]
fn checked_in_link_example_is_bound_to_its_exact_compiled_snapshot() {
    let checked: SelectionLinks = serde_json::from_slice(
        &fs::read(root().join("examples/interaction/linked-services.links.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(checked, authoring_spec("main"));
}
