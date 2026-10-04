use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use vizir_core::{Revision, Scene2D, apply_scene_patch, diff_scene};

const OLD_OUTPUT: &[u8] = b"existing output must survive\n";
const OLD_PROVENANCE: &[u8] = b"existing provenance must survive\n";
const CSV: &[u8] = b"id,x,n,active,note\na,1.5,-2,true,hello\nb,-0.0,42,false,\n";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn vizir() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vizir"))
}
fn accepted(output: Output) -> Vec<u8> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn rejected(output: Output) -> String {
    assert!(!output.status.success(), "invalid import was accepted");
    assert!(output.stdout.is_empty(), "{:?}", output.stdout);
    let error = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!error.is_empty(), "failure needs a diagnostic");
    error
}
fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn assert_clean(directory: &Path) {
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        assert!(
            !entry.file_name().to_string_lossy().starts_with(".vizir-"),
            "publication left staging debris"
        );
        if entry.file_type().unwrap().is_dir() {
            assert_clean(&entry.path());
        }
    }
}
fn basic_spec() -> Value {
    json!({"format":"vizir-csv-import/1", "key":"id", "columns":[
        {"name":"id","type":"string"}, {"name":"x","type":"float64"},
        {"name":"n","type":"int64"}, {"name":"active","type":"bool"},
        {"name":"note","type":"string"}
    ]})
}
fn basic_hir(version: &str) -> Value {
    json!({"version":version,"id":"csv-cli","width":640,"height":440,
        "datasets":{}, "views":[{"kind":"geometry.scene","id":"notes",
            "frame":{"x":0,"y":0,"width":640,"height":440},"children":[]}]})
}
fn basic_composition(version: &str) -> Value {
    json!({"schema":version,"id":"csv-cli","width":640,"height":440,
        "layout":{"kind":"grid","columns":1},"datasets":{},
        "panels":[{"kind":"geometry.scene","id":"notes","children":[]}]})
}

struct Fixture {
    dir: tempfile::TempDir,
    csv: PathBuf,
    spec: PathBuf,
    template: PathBuf,
    output: PathBuf,
    provenance: PathBuf,
    kind: &'static str,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("input.csv");
        let spec = dir.path().join("spec.json");
        let template = dir.path().join("template.json");
        let output = dir.path().join("imported.json");
        let provenance = dir.path().join("provenance.json");
        fs::write(&csv, CSV).unwrap();
        write_json(&spec, &basic_spec());
        write_json(&template, &basic_hir("0.4"));
        Self {
            dir,
            csv,
            spec,
            template,
            output,
            provenance,
            kind: "hir",
        }
    }
    fn command(&self, replace: bool) -> Command {
        let mut command = vizir();
        command
            .arg("import-csv")
            .arg(&self.csv)
            .arg("--template")
            .arg(&self.template)
            .args(["--template-kind", self.kind, "--dataset", "samples"])
            .arg("--spec")
            .arg(&self.spec)
            .arg("--output")
            .arg(&self.output)
            .arg("--provenance")
            .arg(&self.provenance);
        if replace {
            command.arg("--replace-dataset");
        }
        command
    }
    fn run(&self, replace: bool) -> Output {
        self.command(replace).output().unwrap()
    }
    fn failed_preserving_pair(&self, replace: bool) -> String {
        fs::write(&self.output, OLD_OUTPUT).unwrap();
        fs::write(&self.provenance, OLD_PROVENANCE).unwrap();
        let error = rejected(self.run(replace));
        assert_eq!(fs::read(&self.output).unwrap(), OLD_OUTPUT);
        assert_eq!(fs::read(&self.provenance).unwrap(), OLD_PROVENANCE);
        assert_clean(self.dir.path());
        error
    }
}

#[test]
fn mandatory_paths_and_template_kind_are_explicit_cli_arguments() {
    let f = Fixture::new();
    for missing in [
        "--output",
        "--provenance",
        "--template-kind",
        "--dataset",
        "--spec",
    ] {
        let args = [
            ("--template", f.template.to_str().unwrap()),
            ("--template-kind", "hir"),
            ("--dataset", "samples"),
            ("--spec", f.spec.to_str().unwrap()),
            ("--output", f.output.to_str().unwrap()),
            ("--provenance", f.provenance.to_str().unwrap()),
        ];
        let mut command = vizir();
        command.arg("import-csv").arg(&f.csv);
        for (flag, value) in args {
            if flag != missing {
                command.args([flag, value]);
            }
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(rejected(output).contains(missing));
        assert!(!f.output.exists());
        assert!(!f.provenance.exists());
    }
    let mut f = f;
    f.kind = "mir";
    assert_eq!(f.run(false).status.code(), Some(2));
    assert_clean(f.dir.path());
}

#[test]
fn import_preserves_types_rows_and_unused_fields_with_exact_reproducible_provenance() {
    let f = Fixture::new();
    let sources = [&f.csv, &f.spec, &f.template].map(|p| fs::read(p).unwrap());
    accepted(f.run(false));
    let output = fs::read(&f.output).unwrap();
    let manifest = fs::read(&f.provenance).unwrap();
    assert_ne!(output.last(), Some(&b'\n'));
    assert_ne!(manifest.last(), Some(&b'\n'));
    let document = read_json(&f.output);
    assert_eq!(document["version"], "0.4");
    assert_eq!(document["datasets"]["samples"]["key"], "id");
    let rows = &document["datasets"]["samples"]["rows"];
    assert_eq!(
        rows[0],
        json!({"id":"a","x":1.5,"n":-2,"active":true,"note":"hello"})
    );
    assert_eq!(
        rows[1],
        json!({"id":"b","x":-0.0,"n":42,"active":false,"note":""})
    );
    assert!(rows[1]["x"].as_f64().unwrap().is_sign_negative());
    assert_eq!(rows[0]["n"].as_i64(), Some(-2));
    let provenance = read_json(&f.provenance);
    assert_eq!(provenance["format"], "vizir-csv-provenance/1");
    assert_eq!(provenance["importer_profile"], "vizir-csv-import/1");
    assert_eq!(provenance["dataset"], "samples");
    assert_eq!(provenance["key"], "id");
    assert_eq!(provenance["row_count"], 2);
    assert_eq!(
        provenance["header"],
        json!(["id", "x", "n", "active", "note"])
    );
    assert_eq!(provenance["columns"], basic_spec()["columns"]);
    assert_eq!(provenance["had_utf8_bom"], false);
    assert_eq!(
        provenance["replacement"],
        json!({"policy":"require_absent","replaced_existing":false})
    );
    for (name, bytes) in [
        ("csv", &sources[0]),
        ("spec", &sources[1]),
        ("template", &sources[2]),
        ("output", &output),
    ] {
        assert_eq!(provenance[name]["sha256"], sha256(bytes));
        assert_eq!(provenance[name]["bytes"], bytes.len());
    }
    for name in ["template", "output"] {
        assert_eq!(provenance[name]["kind"], "hir");
        assert_eq!(provenance[name]["source_version"], "0.4");
    }
    let text = String::from_utf8(manifest.clone()).unwrap();
    assert!(!text.contains(f.dir.path().to_str().unwrap()));
    for field in ["path", "timestamp", "created_at", "uri"] {
        assert!(!text.contains(&format!("\"{field}\"")));
    }
    accepted(f.run(false));
    assert_eq!(fs::read(&f.output).unwrap(), output);
    assert_eq!(fs::read(&f.provenance).unwrap(), manifest);
    for (path, original) in [&f.csv, &f.spec, &f.template].into_iter().zip(sources) {
        assert_eq!(fs::read(path).unwrap(), original);
    }
    accepted(vizir().arg("validate").arg(&f.output).output().unwrap());
    assert_clean(f.dir.path());
}

#[test]
fn hir_and_composition_imports_keep_all_supported_source_versions() {
    for version in ["0.1", "0.2", "0.3", "0.4"] {
        let f = Fixture::new();
        write_json(&f.template, &basic_hir(version));
        accepted(f.run(false));
        assert_eq!(read_json(&f.output)["version"], version);
        accepted(vizir().arg("validate").arg(&f.output).output().unwrap());
    }
    for (version, hir_version) in [
        ("vizir-composition/0.1", "0.2"),
        ("vizir-composition/0.2", "0.3"),
        ("vizir-composition/0.3", "0.4"),
    ] {
        let mut f = Fixture::new();
        f.kind = "composition";
        write_json(&f.template, &basic_composition(version));
        accepted(f.run(false));
        let output = read_json(&f.output);
        assert_eq!(output["schema"], version);
        assert!(output.get("version").is_none());
        assert!(output.get("views").is_none());
        assert_eq!(output["panels"][0]["kind"], "geometry.scene");
        let provenance = read_json(&f.provenance);
        for name in ["template", "output"] {
            assert_eq!(provenance[name]["kind"], "composition");
            assert_eq!(provenance[name]["source_version"], version);
        }
        let composed = accepted(vizir().arg("compose").arg(&f.output).output().unwrap());
        assert_eq!(
            serde_json::from_slice::<Value>(&composed).unwrap()["version"],
            hir_version
        );
    }
}

#[test]
fn replacement_requires_an_existing_target_and_preserves_unrelated_data() {
    for kind in ["hir", "composition"] {
        let mut f = Fixture::new();
        f.kind = kind;
        let mut template = if kind == "hir" {
            basic_hir("0.4")
        } else {
            basic_composition("vizir-composition/0.3")
        };
        write_json(&f.template, &template);
        f.failed_preserving_pair(true);
        template["datasets"]["samples"] = json!({"key":"obsolete","rows":[]});
        template["datasets"]["keep"] = json!({"key":"id","rows":[{"id":"untouched","value":7}]});
        write_json(&f.template, &template);
        f.failed_preserving_pair(false);
        accepted(f.run(true));
        let output = read_json(&f.output);
        assert_eq!(output["datasets"]["keep"], template["datasets"]["keep"]);
        assert_eq!(output["datasets"]["samples"]["key"], "id");
        assert_eq!(
            output["datasets"]["samples"]["rows"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            read_json(&f.provenance)["replacement"],
            json!({"policy":"require_present","replaced_existing":true})
        );
        assert_clean(f.dir.path());
    }
}

#[test]
fn insertion_precedes_validation_but_other_invalid_references_still_fail() {
    for kind in ["hir", "composition"] {
        let mut f = Fixture::new();
        f.kind = kind;
        let mut template = if kind == "hir" {
            basic_hir("0.4")
        } else {
            basic_composition("vizir-composition/0.3")
        };
        let mut chart = json!({"kind":"chart.scatter","id":"points","dataset":"samples",
            "x":{"field":"x"},"y":{"field":"n"}});
        let views = if kind == "hir" {
            chart["frame"] = json!({"x":0,"y":0,"width":640,"height":440});
            "views"
        } else {
            "panels"
        };
        template[views] = json!([chart]);
        write_json(&f.template, &template);
        accepted(f.run(false));
        template[views][0]["dataset"] = "still-missing".into();
        write_json(&f.template, &template);
        f.failed_preserving_pair(false);
        template[views][0]["dataset"] = "samples".into();
        template["datasets"]["invalid-other"] = json!({"key":"id","rows":[]});
        write_json(&f.template, &template);
        f.failed_preserving_pair(false);
    }
}

#[test]
fn accepted_csv_preserves_bom_literal_quoting_whitespace_and_embedded_line_endings() {
    let f = Fixture::new();
    fs::write(&f.csv, b"\xef\xbb\xbfid,x,n,active,note\r\na,1e1,-2,true,\"first, \"\"quoted\"\"\r\nsecond\nthird\"\nb,-0,42,false,  spaces stay  ").unwrap();
    accepted(f.run(false));
    let document = read_json(&f.output);
    let rows = &document["datasets"]["samples"]["rows"];
    assert_eq!(rows[0]["note"], "first, \"quoted\"\r\nsecond\nthird");
    assert_eq!(rows[1]["note"], "  spaces stay  ");
    assert_eq!(rows[0]["x"], 10.0);
    assert!(rows[1]["x"].as_f64().unwrap().is_sign_negative());
    let provenance = read_json(&f.provenance);
    assert_eq!(provenance["had_utf8_bom"], true);
    assert_eq!(
        provenance["csv"]["sha256"],
        sha256(&fs::read(&f.csv).unwrap())
    );
}

#[test]
fn malformed_csv_never_creates_or_replaces_either_output() {
    let cases: &[&[u8]] = &[
        b"",
        b"id,x,n,active,note\n",
        b"id,x,n,active,note\n\n",
        b"id,x,n,active,note\na,1,2,true,ok\n\n",
        b"x,id,n,active,note\n1,a,2,true,ok\n",
        b"id,x,n,active,note,extra\na,1,2,true,ok,no\n",
        b"id,x,n,active,note\na,1,2,true\n",
        b"id,x,n,active,note\na,1,2,true,ok,extra\n",
        b"id,x,n,active,note\na,1,2,true,bare\rreturn\n",
        b"id,x,n,active,note\ra,1,2,true,ok\r",
        b"id,x,n,active,note\na,1,2,true,\"unclosed",
        b"id,x,n,active,note\na,1,2,true,un\"quoted\n",
        b"id,x,n,active,note\na,1,2,true,\"closed\"tail\n",
        b"id,x,n,active,note\na,1,2,true,\"bare\rreturn\"\n",
        b"id,x,n,active,note\na,1,2,true,nul\0byte\n",
        b"id,x,n,active,note\na,1,2,true,\xff\n",
        b"\xef\xbb\xbf\xef\xbb\xbfid,x,n,active,note\na,1,2,true,ok\n",
        b"id,x,n,active,note\n,1,2,true,ok\n",
        b"id,x,n,active,note\na,1,2,true,ok\na,2,3,false,duplicate\n",
        b"# comment\nid,x,n,active,note\na,1,2,true,ok\n",
        b"id,x,n,active,note\na,,2,true,ok\n",
        b"id,x,n,active,note\na,1,,true,ok\n",
        b"id,x,n,active,note\na,1,2,,ok\n",
        b"id,x,n,active,note\na,1,2,True,ok\n",
        b"id,x,n,active,note\na,1,2,1,ok\n",
    ];
    for (index, bytes) in cases.iter().enumerate() {
        let f = Fixture::new();
        fs::write(&f.csv, bytes).unwrap();
        rejected(f.run(false));
        assert!(!f.output.exists(), "case {index}");
        assert!(!f.provenance.exists(), "case {index}");
        let error = f.failed_preserving_pair(false);
        assert!(error.contains("VIZ-CSV"), "case {index}: {error}");
    }
}

#[test]
fn explicit_numeric_grammar_range_and_canonical_key_rules_are_enforced() {
    for (x, n) in [
        ("+1", "2"),
        ("01", "2"),
        (".1", "2"),
        ("1.", "2"),
        ("1e", "2"),
        ("NaN", "2"),
        ("inf", "2"),
        ("1e309", "2"),
        ("1e-999", "2"),
        (" 1", "2"),
        ("1 ", "2"),
        ("null", "2"),
        ("1", "+2"),
        ("1", "02"),
        ("1", "2.0"),
        ("1", "2e0"),
        ("1", "9223372036854775808"),
        ("1", "-9223372036854775809"),
    ] {
        let f = Fixture::new();
        fs::write(&f.csv, format!("id,x,n,active,note\na,{x},{n},true,ok\n")).unwrap();
        f.failed_preserving_pair(false);
    }
    let f = Fixture::new();
    fs::write(&f.csv, b"id,x,n,active,note\na,5e-324,-9223372036854775808,true,\nb,-0e999,9223372036854775807,false,\n").unwrap();
    accepted(f.run(false));
    let rows = &read_json(&f.output)["datasets"]["samples"]["rows"];
    assert!(rows[0]["x"].as_f64().unwrap().is_subnormal());
    assert_eq!(rows[0]["n"].as_i64(), Some(i64::MIN));
    assert_eq!(rows[1]["n"].as_i64(), Some(i64::MAX));
    assert!(rows[1]["x"].as_f64().unwrap().is_sign_negative());
    let mut spec = basic_spec();
    spec["columns"][0]["type"] = "int64".into();
    write_json(&f.spec, &spec);
    fs::write(&f.csv, b"id,x,n,active,note\n-0,1,2,true,\n0,2,3,false,\n").unwrap();
    f.failed_preserving_pair(false);
    fs::write(&f.csv, b"id,x,n,active,note\n-1,1,2,true,\n2,2,3,false,\n").unwrap();
    accepted(f.run(false));
    assert_eq!(
        read_json(&f.output)["datasets"]["samples"]["rows"][0]["id"].as_i64(),
        Some(-1)
    );
}

#[test]
fn specification_is_strict_json_with_ordered_unique_nonnullable_columns() {
    let base = basic_spec();
    let mut cases = vec![];
    for (pointer, value) in [
        ("/format", json!("vizir-csv-import/2")),
        ("/format", Value::Null),
        ("/key", Value::Null),
        ("/key", json!("missing")),
        ("/columns", Value::Null),
        ("/columns", json!([])),
        ("/columns/0/name", json!("")),
        ("/columns/0/name", json!("a\nb")),
        ("/columns/0/name", json!("x")),
        ("/columns/0/name", json!("a".repeat(257))),
        ("/columns/0/type", Value::Null),
        ("/columns/0/type", json!("number")),
        ("/columns/0/type", json!("float64")),
        ("/columns/0/type", json!("bool")),
    ] {
        let mut spec = base.clone();
        *spec.pointer_mut(pointer).unwrap() = value;
        cases.push(serde_json::to_vec(&spec).unwrap());
    }
    for (pointer, field, value) in [
        ("", "path", json!("data.csv")),
        ("", "nullable", json!(true)),
        ("/columns/0", "nullable", json!(false)),
        ("/columns/0", "format", json!("string")),
    ] {
        let mut spec = base.clone();
        spec.pointer_mut(pointer).unwrap()[field] = value;
        cases.push(serde_json::to_vec(&spec).unwrap());
    }
    let canonical = serde_json::to_string(&base).unwrap();
    cases.push(
        canonical
            .replace("\"key\":\"id\"", "\"key\":\"id\",\"key\":\"id\"")
            .into_bytes(),
    );
    cases.push(
        canonical
            .replacen("\"name\":\"id\"", "\"name\":\"id\",\"name\":\"id\"", 1)
            .into_bytes(),
    );
    cases.push(b"format: vizir-csv-import/1\nkey: id\ncolumns: []\n".to_vec());
    cases.push([b"\xef\xbb\xbf".as_slice(), canonical.as_bytes()].concat());
    for bytes in cases {
        let f = Fixture::new();
        fs::write(&f.spec, bytes).unwrap();
        f.failed_preserving_pair(false);
    }
}

#[test]
fn template_decoder_rejects_duplicate_keys_tags_multiple_documents_and_wrong_kind() {
    let f = Fixture::new();
    let source = serde_json::to_string(&basic_hir("0.4")).unwrap();
    let duplicate = source.replace(
        "\"id\":\"csv-cli\"",
        "\"id\":\"csv-cli\",\"id\":\"csv-cli\"",
    );
    fs::write(&f.template, duplicate).unwrap();
    f.failed_preserving_pair(false);
    let mut f = f;
    f.template = f.dir.path().join("template.yaml");
    let yaml = serde_yaml::to_string(&basic_hir("0.4")).unwrap();
    for source in [
        format!("{yaml}\nid: csv-cli\n"),
        format!("{yaml}\n---\n{yaml}"),
        yaml.replace("id: csv-cli", "id: !custom csv-cli"),
        yaml.replace("datasets: {}", "datasets: {duplicate: {}, duplicate: {}}"),
    ] {
        fs::write(&f.template, source).unwrap();
        f.failed_preserving_pair(false);
    }
    fs::write(&f.template, &yaml).unwrap();
    accepted(f.run(false));
    f.template = f.dir.path().join("template.authoring");
    fs::write(&f.template, &yaml).unwrap();
    accepted(f.run(false));
    f.kind = "composition";
    f.failed_preserving_pair(false);
}

#[test]
fn bounded_inputs_reject_oversized_files_and_non_regular_files_without_publication() {
    for (which, limit) in [(0, 8 * 1024 * 1024), (1, 256 * 1024), (2, 8 * 1024 * 1024)] {
        let f = Fixture::new();
        let path = [&f.csv, &f.spec, &f.template][which];
        fs::write(path, vec![b' '; limit + 1]).unwrap();
        f.failed_preserving_pair(false);
    }
    for which in 0..3 {
        let f = Fixture::new();
        let path = [&f.csv, &f.spec, &f.template][which];
        fs::remove_file(path).unwrap();
        fs::create_dir(path).unwrap();
        f.failed_preserving_pair(false);
    }
    let f = Fixture::new();
    fs::write(
        &f.csv,
        format!(
            "id,x,n,active,note\na,1,2,true,{}\n",
            "x".repeat(64 * 1024 + 1)
        ),
    )
    .unwrap();
    f.failed_preserving_pair(false);
}

#[test]
fn each_destination_rejects_aliases_of_every_source_or_of_the_other_destination() {
    for source_index in 0..3 {
        for destination_index in 0..2 {
            let mut f = Fixture::new();
            let source = [&f.csv, &f.spec, &f.template][source_index].clone();
            let original = fs::read(&source).unwrap();
            if destination_index == 0 {
                f.output = source.clone();
            } else {
                f.provenance = source.clone();
            }
            let error = rejected(f.run(false));
            assert!(error.contains("VIZ-PATH"), "{error}");
            assert_eq!(fs::read(&source).unwrap(), original);
            assert_clean(f.dir.path());
        }
    }
    for existing in [false, true] {
        let mut f = Fixture::new();
        f.provenance = f.output.clone();
        if existing {
            fs::write(&f.output, OLD_OUTPUT).unwrap();
        }
        let error = rejected(f.run(false));
        assert!(error.contains("VIZ-PATH"), "{error}");
        if existing {
            assert_eq!(fs::read(&f.output).unwrap(), OLD_OUTPUT);
        } else {
            assert!(!f.output.exists());
        }
    }
}

#[test]
fn hardlinked_sources_and_paired_destinations_are_protected() {
    for source_index in 0..3 {
        for destination_index in 0..2 {
            let mut f = Fixture::new();
            let source = [&f.csv, &f.spec, &f.template][source_index].clone();
            let original = fs::read(&source).unwrap();
            let alias = f.dir.path().join("alias");
            fs::hard_link(&source, &alias).unwrap();
            if destination_index == 0 {
                f.output = alias.clone();
            } else {
                f.provenance = alias.clone();
            }
            rejected(f.run(false));
            assert_eq!(fs::read(&source).unwrap(), original);
            assert_eq!(fs::read(&alias).unwrap(), original);
            assert_clean(f.dir.path());
        }
    }
    let f = Fixture::new();
    fs::write(&f.output, OLD_OUTPUT).unwrap();
    fs::hard_link(&f.output, &f.provenance).unwrap();
    rejected(f.run(false));
    assert_eq!(fs::read(&f.output).unwrap(), OLD_OUTPUT);
    assert_eq!(fs::read(&f.provenance).unwrap(), OLD_OUTPUT);
}

#[cfg(unix)]
#[test]
fn explicit_regular_symlink_inputs_work_but_symlink_alias_destinations_fail() {
    use std::os::unix::fs::symlink;
    for source_index in 0..3 {
        for destination_index in 0..2 {
            let mut f = Fixture::new();
            let source = [&f.csv, &f.spec, &f.template][source_index].clone();
            let original = fs::read(&source).unwrap();
            let alias = f.dir.path().join("alias");
            symlink(&source, &alias).unwrap();
            if destination_index == 0 {
                f.output = alias.clone();
            } else {
                f.provenance = alias.clone();
            }
            rejected(f.run(false));
            assert_eq!(fs::read(&source).unwrap(), original);
            assert_eq!(fs::read_link(&alias).unwrap(), source);
            assert_clean(f.dir.path());
        }
    }
    let mut f = Fixture::new();
    let linked = f.dir.path().join("linked.csv");
    symlink(&f.csv, &linked).unwrap();
    f.csv = linked;
    let linked = f.dir.path().join("linked-spec.json");
    symlink(&f.spec, &linked).unwrap();
    f.spec = linked;
    let linked = f.dir.path().join("linked-template.json");
    symlink(&f.template, &linked).unwrap();
    f.template = linked;
    accepted(f.run(false));
    assert_clean(f.dir.path());
}

#[test]
fn paired_publication_prepares_both_destinations_before_replacing_either() {
    for bad_destination in 0..2 {
        for existing in [false, true] {
            let f = Fixture::new();
            let paths = [&f.output, &f.provenance];
            let bad = paths[bad_destination];
            let good = paths[1 - bad_destination];
            fs::create_dir(bad).unwrap();
            fs::write(bad.join("keep"), OLD_OUTPUT).unwrap();
            if existing {
                fs::write(good, OLD_PROVENANCE).unwrap();
            }
            rejected(f.run(false));
            assert_eq!(fs::read(bad.join("keep")).unwrap(), OLD_OUTPUT);
            if existing {
                assert_eq!(fs::read(good).unwrap(), OLD_PROVENANCE);
            } else {
                assert!(!good.exists());
            }
            assert_clean(f.dir.path());
        }
    }
    let mut f = Fixture::new();
    f.output = f.dir.path().join("nested/imported.json");
    f.provenance = f.dir.path().join("other/provenance.json");
    accepted(f.run(false));
    assert!(f.output.is_file());
    assert!(f.provenance.is_file());
    assert_clean(f.dir.path());
}

fn imported_example(name: &str) -> Fixture {
    let mut f = Fixture::new();
    let example = root().join("examples/import-csv");
    if name == "area" {
        f.csv = example.join("area.csv");
        f.spec = example.join("area.spec.json");
        f.template = example.join("area.template.yaml");
    } else {
        f.csv = example.join("checks.csv");
        f.spec = example.join("checks.spec.json");
        f.template = example.join("dashboard.template.yaml");
        f.kind = "composition";
    }
    let dataset = if name == "area" { "signals" } else { "checks" };
    let mut command = vizir();
    command
        .arg("import-csv")
        .arg(&f.csv)
        .arg("--template")
        .arg(&f.template)
        .args(["--template-kind", f.kind, "--dataset", dataset])
        .arg("--spec")
        .arg(&f.spec)
        .arg("--output")
        .arg(&f.output)
        .arg("--provenance")
        .arg(&f.provenance);
    accepted(command.output().unwrap());
    if name != "area" {
        let hir = f.dir.path().join("composed.json");
        accepted(
            vizir()
                .arg("compose")
                .arg(&f.output)
                .arg("--output")
                .arg(&hir)
                .output()
                .unwrap(),
        );
        f.output = hir;
    }
    f
}
fn flatten<'a>(nodes: &'a [Value], output: &mut Vec<&'a Value>) {
    for node in nodes {
        output.push(node);
        if let Some(children) = node["children"].as_array() {
            flatten(children, output);
        }
    }
}
fn scene_nodes(scene: &Value) -> Vec<&Value> {
    let mut nodes = vec![];
    flatten(scene["nodes"].as_array().unwrap(), &mut nodes);
    nodes
}

#[test]
fn executable_csv_examples_use_the_ordinary_normalize_replay_scene_explain_and_svg_pipeline() {
    for (name, dataset, version, row_count, node_id) in [
        ("area", "signals", "0.3", 10, "signals/area/Alpha"),
        ("dashboard", "checks", "0.4", 7, "coverage/cell/jobs-mon"),
    ] {
        let f = imported_example(name);
        let hir = read_json(&f.output);
        assert_eq!(hir["version"], version);
        let rows = &hir["datasets"][dataset]["rows"];
        assert_eq!(rows.as_array().unwrap().len(), row_count);
        assert_eq!(rows[0]["batch"].as_i64(), Some(42));
        assert!(rows[0]["note"].is_string());
        assert!(
            rows[0][if name == "area" {
                "checked"
            } else {
                "verified"
            }]
            .is_boolean()
        );
        accepted(vizir().arg("validate").arg(&f.output).output().unwrap());
        let mir_file = f.dir.path().join("mir.json");
        accepted(
            vizir()
                .arg("normalize")
                .arg(&f.output)
                .args(["--theme", "azure"])
                .arg("--output")
                .arg(&mir_file)
                .output()
                .unwrap(),
        );
        let envelope = read_json(&mir_file);
        let mir = &envelope["mir"];
        assert_eq!(mir["version"], version);
        assert_eq!(mir["source_hir_version"], version);
        assert_eq!(
            mir["data"][format!("data/{dataset}")]["operator"]["rows"],
            *rows
        );
        let direct = accepted(
            vizir()
                .arg("lower")
                .arg(&f.output)
                .args(["--theme", "azure"])
                .output()
                .unwrap(),
        );
        let replay = accepted(vizir().arg("lower").arg(&mir_file).output().unwrap());
        assert_eq!(direct, replay);
        let scene: Value = serde_json::from_slice(&replay).unwrap();
        let nodes = scene_nodes(&scene);
        assert!(nodes.iter().any(|n| n["id"] == node_id));
        if name == "dashboard" {
            let cells: Vec<_> = nodes
                .iter()
                .filter(|n| {
                    n["id"]
                        .as_str()
                        .is_some_and(|s| s.starts_with("coverage/cell/"))
                })
                .collect();
            assert_eq!(cells.len(), 7, "unobserved pairs must stay empty");
            assert_eq!(mir["views"][0]["mark"]["instances"][1]["value"], 0.0);
        } else {
            assert_eq!(
                mir["views"][0]["mark"]["series"][0]["points"][0]["key"],
                "alpha-0"
            );
            assert_eq!(
                rows[0]["sample_id"], "beta-3",
                "import never sorts source rows"
            );
        }
        let explanation = String::from_utf8(accepted(
            vizir()
                .arg("explain")
                .arg(&mir_file)
                .args(["--node", node_id])
                .output()
                .unwrap(),
        ))
        .unwrap();
        assert!(explanation.contains(node_id));
        let svg = f.dir.path().join("example.svg");
        accepted(
            vizir()
                .arg("render")
                .arg(&mir_file)
                .args(["--format", "svg", "--output"])
                .arg(&svg)
                .output()
                .unwrap(),
        );
        let svg = fs::read_to_string(svg).unwrap();
        let xml = roxmltree::Document::parse(&svg).unwrap();
        assert!(
            xml.descendants()
                .any(|n| n.attribute("id") == Some(node_id))
        );
        assert_clean(f.dir.path());
    }
}

#[test]
fn reimported_data_uses_existing_stable_scene_patch_identity() {
    let f = imported_example("dashboard");
    let before: Scene2D = serde_json::from_slice(&accepted(
        vizir().arg("lower").arg(&f.output).output().unwrap(),
    ))
    .unwrap();
    let csv = f.dir.path().join("updated.csv");
    let source = fs::read_to_string(&f.csv).unwrap();
    fs::write(
        &csv,
        source.replace("api-tue,Tue,API,1,10,true", "api-tue,Tue,API,1,16,true"),
    )
    .unwrap();
    let output = f.dir.path().join("updated.json");
    let provenance = f.dir.path().join("updated.provenance.json");
    accepted(
        vizir()
            .arg("import-csv")
            .arg(csv)
            .arg("--template")
            .arg(&f.output)
            .args([
                "--template-kind",
                "hir",
                "--dataset",
                "checks",
                "--replace-dataset",
            ])
            .arg("--spec")
            .arg(&f.spec)
            .arg("--output")
            .arg(&output)
            .arg("--provenance")
            .arg(provenance)
            .output()
            .unwrap(),
    );
    let after: Scene2D = serde_json::from_slice(&accepted(
        vizir().arg("lower").arg(&output).output().unwrap(),
    ))
    .unwrap();
    let patch = diff_scene(&before, &after, Revision(1), Revision(2), "csv-update").unwrap();
    assert!(!patch.operations.is_empty());
    let patch_json = serde_json::to_value(&patch).unwrap();
    assert!(
        patch_json["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| op["op"] == "replace-node" && op["id"] == "coverage/cell/api-tue")
    );
    let (applied, revision) = apply_scene_patch(&before, Revision(1), &patch).unwrap();
    assert_eq!(revision, Revision(2));
    assert_eq!(applied, after);
    assert_clean(f.dir.path());
}

fn font_arguments() -> Vec<String> {
    let fixtures = root().join("crates/vizir-compiler/tests/fixtures/wrapping-fonts");
    read_json(&fixtures.join("manifest.json"))["fonts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|font| {
            format!(
                "{}={}",
                font["sha256"].as_str().unwrap(),
                fixtures.join(font["file"].as_str().unwrap()).display()
            )
        })
        .collect()
}
fn add_fonts(command: &mut Command, fonts: &[String]) {
    for font in fonts {
        command.args(["--font", font]);
    }
}

#[test]
fn imported_examples_keep_measured_title_and_light_dark_context_replay() {
    let fonts = font_arguments();
    for name in ["area", "dashboard"] {
        let f = imported_example(name);
        let title_id = if name == "area" {
            "signals/title"
        } else {
            "coverage/title"
        };
        for theme in ["azure", "azure-dark"] {
            let compiled = f.dir.path().join(format!("{theme}.compiled.json"));
            let profile = root().join("examples/text/wrapping-font-profile.json");
            let layout = root().join(format!("examples/import-csv/{name}-title-layout.json"));
            let mut normalize = vizir();
            normalize
                .arg("normalize")
                .arg(&f.output)
                .args(["--theme", theme])
                .arg("--text-profile")
                .arg(&profile)
                .arg("--text-layout")
                .arg(&layout)
                .arg("--output")
                .arg(&compiled);
            add_fonts(&mut normalize, &fonts);
            accepted(normalize.output().unwrap());
            let envelope = read_json(&compiled);
            assert_eq!(envelope["format"], "vizir-compiled-mir/1");
            assert_eq!(envelope["context"]["text_layout"], read_json(&layout));
            let mut direct = vizir();
            direct
                .arg("lower")
                .arg(&f.output)
                .args(["--theme", theme])
                .arg("--text-profile")
                .arg(profile)
                .arg("--text-layout")
                .arg(layout);
            add_fonts(&mut direct, &fonts);
            let direct = accepted(direct.output().unwrap());
            let mut replay = vizir();
            replay.arg("lower").arg(&compiled);
            add_fonts(&mut replay, &fonts);
            let replay = accepted(replay.output().unwrap());
            assert_eq!(direct, replay);
            let scene: Value = serde_json::from_slice(&replay).unwrap();
            let nodes = scene_nodes(&scene);
            let title = nodes.iter().find(|node| node["id"] == title_id).unwrap();
            assert_eq!(title["type"], "path");
            let svg = f.dir.path().join(format!("{theme}.svg"));
            let mut render = vizir();
            render
                .arg("render")
                .arg(&compiled)
                .args(["--format", "svg", "--output"])
                .arg(&svg);
            add_fonts(&mut render, &fonts);
            accepted(render.output().unwrap());
            roxmltree::Document::parse(&fs::read_to_string(svg).unwrap()).unwrap();
        }
        assert_clean(f.dir.path());
    }
}

#[test]
fn native_csv_example_pngs_have_transparent_and_visible_pixels_when_renderer_is_available() {
    if !Command::new("rsvg-convert")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        eprintln!("rsvg-convert unavailable; native CSV PNG smoke is skipped");
        return;
    }
    for (name, size) in [("area", (1040, 600)), ("dashboard", (1440, 1100))] {
        let f = imported_example(name);
        let output = f.dir.path().join("example.png");
        accepted(
            vizir()
                .arg("render")
                .arg(&f.output)
                .args(["--format", "png", "--background", "transparent", "--output"])
                .arg(&output)
                .output()
                .unwrap(),
        );
        let file = std::io::BufReader::new(fs::File::open(output).unwrap());
        let mut decoder = png::Decoder::new(file);
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().unwrap();
        let mut bytes = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut bytes).unwrap();
        assert_eq!((info.width, info.height), size);
        assert_eq!(info.color_type, png::ColorType::Rgba);
        let pixels = &bytes[..info.buffer_size()];
        assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 0));
        assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
        assert_clean(f.dir.path());
    }
}

#[cfg(unix)]
#[test]
fn oversized_hardlinked_destinations_reject_before_backup_or_publication() {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, symlink};
    for (destination_index, limit) in [(0, 64 * 1024 * 1024), (1, 1024 * 1024)] {
        for through_symlink in [false, true] {
            let mut f = Fixture::new();
            let target = f.dir.path().join("oversized");
            fs::write(&target, OLD_OUTPUT).unwrap();
            fs::OpenOptions::new()
                .write(true)
                .open(&target)
                .unwrap()
                .set_len(limit + 1)
                .unwrap();
            let alias = f.dir.path().join("hardlink");
            fs::hard_link(&target, &alias).unwrap();
            let destination = if through_symlink {
                let link = f.dir.path().join("symlink");
                symlink(&alias, &link).unwrap();
                link
            } else {
                alias.clone()
            };
            let other = if destination_index == 0 {
                f.output = destination;
                f.provenance.clone()
            } else {
                f.provenance = destination;
                f.output.clone()
            };
            fs::write(&other, OLD_PROVENANCE).unwrap();
            let before = fs::metadata(&target).unwrap();
            rejected(f.run(false));
            let after = fs::metadata(&target).unwrap();
            assert_eq!(after.len(), limit + 1);
            assert_eq!(after.ino(), before.ino());
            assert_eq!(after.dev(), before.dev());
            assert_eq!(after.nlink(), 2);
            assert_eq!(fs::metadata(&alias).unwrap().ino(), after.ino());
            let mut prefix = vec![0; OLD_OUTPUT.len()];
            fs::File::open(&target)
                .unwrap()
                .read_exact(&mut prefix)
                .unwrap();
            assert_eq!(prefix, OLD_OUTPUT);
            assert_eq!(fs::read(&other).unwrap(), OLD_PROVENANCE);
            assert_clean(f.dir.path());
        }
    }
}

#[test]
fn import_validates_source_without_invoking_normalization_or_rendering() {
    let f = Fixture::new();
    let mut template = basic_hir("0.4");
    template["views"] = json!([{"kind":"chart.scatter","id":"tiny","dataset":"samples",
        "frame":{"x":0,"y":0,"width":1,"height":1},
        "x":{"field":"x"},"y":{"field":"n"}}]);
    write_json(&f.template, &template);
    accepted(f.command(false).env("PATH", "").output().unwrap());
    assert!(f.output.is_file());
    assert!(f.provenance.is_file());
    rejected(vizir().arg("normalize").arg(&f.output).output().unwrap());
    assert_clean(f.dir.path());
}

#[test]
fn csv_spec_schema_is_independent_and_canonical_on_stdout_and_file() {
    let stdout = accepted(
        vizir()
            .args(["schema", "csv-import-spec"])
            .output()
            .unwrap(),
    );
    let schema: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(schema, vizir_core::csv_import_spec_schema());
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("schema.json");
    accepted(
        vizir()
            .args(["schema", "csv-import-spec", "--output"])
            .arg(&output)
            .output()
            .unwrap(),
    );
    assert_eq!(
        [fs::read(&output).unwrap().as_slice(), b"\n"].concat(),
        stdout
    );
    assert_clean(dir.path());
}

#[cfg(unix)]
#[test]
fn fifo_inputs_and_destinations_are_rejected_without_blocking() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    for role in 0..5 {
        let f = Fixture::new();
        let path = [&f.csv, &f.spec, &f.template, &f.output, &f.provenance][role];
        if path.exists() {
            fs::remove_file(path).unwrap();
        }
        assert!(Command::new("mkfifo").arg(path).status().unwrap().success());
        let mut child = f
            .command(false)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let started = Instant::now();
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if started.elapsed() > Duration::from_secs(5) {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("import blocked on FIFO role {role}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        rejected(child.wait_with_output().unwrap());
        if role != 3 {
            assert!(!f.output.exists());
        }
        if role != 4 {
            assert!(!f.provenance.exists());
        }
        assert_clean(f.dir.path());
    }
}
