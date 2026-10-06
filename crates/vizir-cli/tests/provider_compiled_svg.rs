//! Illustrative, locally generated bundles; these are not user data.
#[path = "support/provider_receipt.rs"]
mod receipt_verifier;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use vizir_compiler::{
    CompilationContext, FontResources, ThemeContext, compile_with_context, parse_text_context_json,
};
use vizir_core::Document;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn success(output: Output) -> Vec<u8> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn provider() -> Command {
    Command::new(env!("CARGO_BIN_EXE_plot-provider-vizir"))
}
struct Bundle {
    dir: tempfile::TempDir,
    files: BTreeMap<String, PathBuf>,
    version: &'static str,
}
impl Bundle {
    fn new(version: &'static str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut files = BTreeMap::new();
        let profile = root().join("examples/text/measured-font-profile.json");
        fs::copy(&profile, dir.path().join("profile.json")).unwrap();
        files.insert("text_profile".into(), dir.path().join("profile.json"));
        let mut fonts = FontResources::new();
        let fixture = root().join("crates/vizir-compiler/tests/fixtures/fonts");
        for (index, name) in ["Regular", "Medium", "Bold"].into_iter().enumerate() {
            let p = dir.path().join(format!("font-{}.otf", index + 1));
            fs::copy(fixture.join(format!("VizIRFixtureSC-{name}.otf")), &p).unwrap();
            let bytes = fs::read(&p).unwrap();
            fonts.insert(&hash(&bytes), bytes).unwrap();
            files.insert(format!("font_{}", index + 1), p);
        }
        let document: Document = serde_json::from_value(json!({
            "version":version,"id":"illustrative/provider","width":900,"height":400,
            "datasets":{"data":{"key":"id","rows":[{"id":"a","x":1.0,"y":2.0},{"id":"b","x":2.0,"y":4.0}]}},
            "views":[
                {"kind":"chart.line","id":"chart","title":"Illustrative context","dataset":"data",
                "frame":{"x":0,"y":0,"width":600,"height":400},"x":{"field":"x"},"y":{"field":"y"}},
                {"kind":"geometry.scene","id":"notes/labels","frame":{"x":600,"y":0,"width":300,"height":400},
                "children":[{"type":"text","id":"hello","x":20,"y":60,"font_size":18,"anchor":"start","text":"中文测试 Hello  "}]}
            ]
        })).unwrap();
        let context = CompilationContext::new()
            .with_theme(ThemeContext::resolve("azure").unwrap())
            .with_text(parse_text_context_json(&fs::read(profile).unwrap()).unwrap());
        let compiled = compile_with_context(&document, &context, &fonts).unwrap();
        let input = dir.path().join("compiled.json");
        fs::write(&input, serde_json::to_vec_pretty(&compiled.mir).unwrap()).unwrap();
        files.insert("input".into(), input);
        Self {
            dir,
            files,
            version,
        }
    }
    fn pins(&self) -> Value {
        self.files
            .iter()
            .map(|(role, path)| (role.clone(), Value::String(hash(&fs::read(path).unwrap()))))
            .collect()
    }
    fn command(&self) -> Command {
        self.command_with(&self.files, &self.pins(), &self.output(), &self.receipt())
    }
    fn command_with(
        &self,
        files: &BTreeMap<String, PathBuf>,
        pins: &Value,
        output: &Path,
        receipt: &Path,
    ) -> Command {
        let mut c = provider();
        c.arg(match self.version {
            "0.4" => "render-compiled-svg",
            "0.5" => "render-compiled-svg-v2",
            "0.7" => "render-compiled-svg-v3",
            "0.9" => "render-compiled-svg-v4",
            _ => panic!("unsupported test profile"),
        })
        .arg(&files["input"]);
        for (role, path) in files {
            if role != "input" {
                c.arg(format!("--{}", role.replace('_', "-"))).arg(path);
            }
        }
        c.arg("--resource-pins")
            .arg(pins.to_string())
            .arg("--output")
            .arg(output)
            .arg("--receipt")
            .arg(receipt);
        c
    }
    fn output(&self) -> PathBuf {
        self.dir.path().join("figure.svg")
    }
    fn receipt(&self) -> PathBuf {
        self.dir.path().join("figure.receipt.json")
    }
    fn old_outputs(&self) {
        fs::write(self.output(), b"old SVG").unwrap();
        fs::write(self.receipt(), b"old receipt").unwrap();
    }
    fn reject(&self, mut c: Command) {
        let output = c.output().unwrap();
        assert!(
            !output.status.success(),
            "unexpected successful provider invocation"
        );
        assert!(output.stdout.is_empty());
        assert_eq!(fs::read(self.output()).unwrap(), b"old SVG");
        assert_eq!(fs::read(self.receipt()).unwrap(), b"old receipt");
        assert!(fs::read_dir(self.dir.path()).unwrap().all(|p| {
            !p.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".vizir-")
        }));
    }
}

#[test]
fn direct_svg_bytes_and_native_receipt_values_are_equal() {
    for version in ["0.4", "0.5", "0.7", "0.9"] {
        let b = Bundle::new(version);
        let verify_receipt = match version {
            "0.4" => receipt_verifier::verify,
            "0.5" => receipt_verifier::verify_v2,
            "0.7" => receipt_verifier::verify_v3,
            "0.9" => receipt_verifier::verify_v4,
            _ => unreachable!(),
        };
        assert!(success(b.command().output().unwrap()).is_empty());
        let direct_svg = b.dir.path().join("direct.svg");
        let direct_manifest = b.dir.path().join("direct.json");
        let mut c = Command::new(env!("CARGO_BIN_EXE_vizir"));
        c.arg("render")
            .arg(&b.files["input"])
            .args(["--format", "svg"])
            .arg("--output")
            .arg(&direct_svg)
            .arg("--manifest")
            .arg(&direct_manifest);
        for (role, p) in &b.files {
            if role.starts_with("font_") {
                c.arg("--font")
                    .arg(format!("{}={}", hash(&fs::read(p).unwrap()), p.display()));
            }
        }
        success(c.output().unwrap());
        let svg = fs::read(b.output()).unwrap();
        assert_eq!(svg, fs::read(direct_svg).unwrap());
        let receipt = read(&b.receipt());
        let manifest = read(&direct_manifest);
        let actual_resources: BTreeMap<String, Vec<u8>> = b
            .files
            .iter()
            .map(|(role, path)| (role.clone(), fs::read(path).unwrap()))
            .collect();
        let version_description: Value = serde_json::from_slice(&success(
            provider().args(["describe", "--json"]).output().unwrap(),
        ))
        .unwrap();
        let provider_version = version_description["provider"]["version"].as_str().unwrap();
        let receipt_bytes = fs::read(b.receipt()).unwrap();
        verify_receipt(
            &receipt_bytes,
            &actual_resources,
            &svg,
            &manifest,
            provider_version,
        )
        .unwrap();
        for change in [
            "unknown",
            "schema_version",
            "profile",
            "source_ir_version",
            "provider",
            "primary_hash",
            "primary_size",
            "argument",
            "role",
            "input_hash",
            "input_size",
            "input_role",
            "duplicate_role",
            "null_layout",
            "context",
            "losses",
            "capability",
            "background",
        ] {
            let mut changed = receipt.clone();
            match change {
                "unknown" => changed["path"] = json!("/unexpected/file"),
                "schema_version" => changed["schema_version"] = json!("vizir.render-receipt/v0"),
                "profile" => changed["profile"] = json!("vizir-compiled-svg/0"),
                "source_ir_version" => changed["source_ir_version"] = json!("0.1"),
                "provider" => changed["provider"]["version"] = json!("invented-build"),
                "primary_hash" => {
                    changed["artifact_receipt"]["primary"]["sha256"] = json!("0".repeat(64))
                }
                "primary_size" => changed["artifact_receipt"]["primary"]["bytes"] = json!(0),
                "argument" => changed["artifact_receipt"]["primary"]["argument"] = json!("receipt"),
                "role" => changed["artifact_receipt"]["primary"]["role"] = json!("other"),
                "input_hash" => {
                    changed["artifact_receipt"]["inputs"][0]["sha256"] = json!("0".repeat(64))
                }
                "input_size" => changed["artifact_receipt"]["inputs"][0]["bytes"] = json!(0),
                "input_role" => changed["artifact_receipt"]["inputs"][0]["role"] = json!("hidden"),
                "duplicate_role" => {
                    changed["artifact_receipt"]["inputs"][1]["role"] =
                        changed["artifact_receipt"]["inputs"][0]["role"].clone()
                }
                "null_layout" => changed["compilation_context"]["text_layout"] = Value::Null,
                "context" => {
                    changed["compilation_context"]["text"]["declared_locale"] = json!("en-US")
                }
                "losses" => changed["losses"] = json!([]),
                "capability" => changed["capability_report"]["decisions"] = json!([]),
                "background" => changed["background"] = json!("#ffffff"),
                _ => unreachable!(),
            }
            assert!(
                verify_receipt(
                    &serde_json::to_vec(&changed).unwrap(),
                    &actual_resources,
                    &svg,
                    &manifest,
                    provider_version
                )
                .is_err(),
                "{change}"
            );
        }
        let duplicate = String::from_utf8(receipt_bytes.clone()).unwrap().replacen(
            "{",
            "{\"schema_version\":\"vizir.render-receipt/v1\",",
            1,
        );
        assert!(
            verify_receipt(
                duplicate.as_bytes(),
                &actual_resources,
                &svg,
                &manifest,
                provider_version
            )
            .is_err()
        );
        let mut changed_svg = svg.clone();
        changed_svg.push(b' ');
        assert!(
            verify_receipt(
                &receipt_bytes,
                &actual_resources,
                &changed_svg,
                &manifest,
                provider_version
            )
            .is_err()
        );
        let mut changed_inputs = actual_resources.clone();
        changed_inputs.get_mut("text_profile").unwrap().push(b' ');
        assert!(
            verify_receipt(
                &receipt_bytes,
                &changed_inputs,
                &svg,
                &manifest,
                provider_version
            )
            .is_err()
        );

        for field in [
            "document_id",
            "source_ir_version",
            "source_context_format",
            "compilation_context",
            "background",
            "capability_report",
            "losses",
        ] {
            assert_eq!(receipt[field], manifest[field], "{field}");
        }
        assert_eq!(receipt["artifact_receipt"]["primary"]["sha256"], hash(&svg));
        assert_eq!(receipt["artifact_receipt"]["primary"]["bytes"], svg.len());
        assert_eq!(receipt["artifact_receipt"]["primary"]["argument"], "output");
        let inputs = receipt["artifact_receipt"]["inputs"].as_array().unwrap();
        assert_eq!(inputs.len(), b.files.len());
        for (record, (role, path)) in inputs.iter().zip(&b.files) {
            assert_eq!(record["role"], *role);
            let bytes = fs::read(path).unwrap();
            assert_eq!(record["sha256"], hash(&bytes));
            assert_eq!(record["bytes"], bytes.len());
        }
        let bytes = fs::read(b.receipt()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(b.dir.path().to_str().unwrap()));
        assert!(!String::from_utf8_lossy(&bytes).contains(".vizir-"));
        // A copied-only bundle needs no original resource directory or current cwd.
        let copied = tempfile::tempdir().unwrap();
        let mut files = BTreeMap::new();
        for (role, path) in &b.files {
            let dest = copied.path().join(path.file_name().unwrap());
            fs::copy(path, &dest).unwrap();
            files.insert(role.clone(), dest);
        }
        let pins = b.pins();
        let expected_receipt = bytes;
        let mut c = b.command_with(
            &files,
            &pins,
            &copied.path().join("other.svg"),
            &copied.path().join("other.json"),
        );
        c.current_dir("/");
        drop(b);
        success(c.output().unwrap());
        assert_eq!(fs::read(copied.path().join("other.svg")).unwrap(), svg);
        assert_eq!(
            fs::read(copied.path().join("other.json")).unwrap(),
            expected_receipt
        );
    }
}

#[test]
fn describe_is_stable_closed_and_all_parameters_have_one_cli_mapping() {
    let first = success(provider().args(["describe", "--json"]).output().unwrap());
    let other = tempfile::tempdir().unwrap();
    let second = success(
        provider()
            .current_dir(other.path())
            .args(["describe", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(first, second);
    let describe: Value = serde_json::from_slice(&first).unwrap();
    let fixture: Value =
        serde_json::from_str(include_str!("../assets/compiled-svg-command-v1.json")).unwrap();
    assert_eq!(describe["commands"][0], fixture);
    assert_eq!(describe["provider"]["id"], "plot-provider-vizir");
    assert_eq!(
        describe["source"]["local_code_path"],
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(
        describe["schema_version"],
        "plot-provider-vizir.describe/v1"
    );
    assert_eq!(
        describe["operations"],
        json!([
            "render-compiled-svg",
            "render-compiled-svg-v2",
            "render-compiled-svg-v3",
            "render-compiled-svg-v4"
        ])
    );
    let fixture_v2: Value =
        serde_json::from_str(include_str!("../assets/compiled-svg-command-v2.json")).unwrap();
    assert_eq!(describe["commands"][1], fixture_v2);
    assert_eq!(
        fixture_v2["capability_id"],
        "visualization.vizir.render-compiled-svg-v2"
    );
    let fixture_v3: Value =
        serde_json::from_str(include_str!("../assets/compiled-svg-command-v3.json")).unwrap();
    assert_eq!(describe["commands"][4], fixture_v3);
    assert_eq!(
        fixture_v3["capability_id"],
        "visualization.vizir.render-compiled-svg-v3"
    );
    let fixture_v4: Value =
        serde_json::from_str(include_str!("../assets/compiled-svg-command-v4.json")).unwrap();
    assert_eq!(describe["commands"][5], fixture_v4);
    assert_eq!(
        fixture_v4["capability_id"],
        "visualization.vizir.render-compiled-svg-v4"
    );
    assert_eq!(describe["commands"].as_array().unwrap().len(), 6);
    assert_eq!(describe["provider"]["protocol_versions"], json!([1]));
    for fixture in [&fixture, &fixture_v2, &fixture_v3, &fixture_v4] {
        let mut mapped = vec!["input".to_string()];
        for f in fixture["cli_spec"]["flags"].as_array().unwrap() {
            mapped.push(f["name"].as_str().unwrap().into());
            assert_eq!(
                f["flag"],
                format!("--{}", f["name"].as_str().unwrap().replace('_', "-"))
            );
        }
        mapped.sort();
        assert_eq!(
            mapped,
            fixture["input_schema"]["properties"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        );
        assert_eq!(fixture["cli_spec"]["flags"][5]["kind"], "json");
    }
    let doctor: Value = serde_json::from_slice(&success(
        provider().args(["doctor", "--json"]).output().unwrap(),
    ))
    .unwrap();
    assert_eq!(doctor["available"], true);
    assert_eq!(doctor["ok"], true);
    assert_eq!(fs::read_dir(other.path()).unwrap().count(), 0);
}

#[test]
fn json_and_context_failures_preserve_both_outputs() {
    for version in ["0.4", "0.5", "0.7", "0.9"] {
        let b = Bundle::new(version);
        b.old_outputs();
        let original = read(&b.files["input"]);
        for change in [
            "format",
            "mir_version",
            "hir_version",
            "missing_theme",
            "null_theme",
            "null_text",
            "null_layout",
            "unknown",
            "bare",
            "engine",
            "face",
            "stale",
            "missing_glyph",
        ] {
            let mut value = original.clone();
            match change {
                "format" => value["format"] = json!("vizir-compiled-mir/2"),
                "mir_version" => value["mir"]["version"] = json!("0.3"),
                "hir_version" => value["mir"]["source_hir_version"] = json!("0.3"),
                "missing_theme" => {
                    value["context"].as_object_mut().unwrap().remove("theme");
                }
                "null_theme" => value["context"]["theme"] = Value::Null,
                "null_text" => value["context"]["text"] = Value::Null,
                "null_layout" => value["context"]["text_layout"] = Value::Null,
                "unknown" => value["context"]["path"] = json!("/hidden/resource"),
                "bare" => value = value["mir"].take(),
                "engine" => value["context"]["text"]["engine"] = json!("other"),
                "face" => value["context"]["text"]["faces"]["regular"]["face_index"] = json!(31),
                "stale" => {
                    value["mir"]["data"]["data/data"]["operator"]["rows"][0]["y"] = json!(3.0)
                }
                "missing_glyph" => value["mir"]["views"][1]["children"][0]["text"] = json!("🦄"),
                _ => unreachable!(),
            }
            write(&b.files["input"], &value);
            b.reject(b.command());
        }
        write(&b.files["input"], &original);
        let bytes = fs::read_to_string(&b.files["input"]).unwrap();
        fs::write(
            &b.files["input"],
            bytes.replacen("{", "{\"format\":\"vizir-compiled-mir/1\",", 1),
        )
        .unwrap();
        b.reject(b.command());
        write(&b.files["input"], &original);
        let profile = read(&b.files["text_profile"]);
        for change in ["unknown", "null", "mismatch", "weight"] {
            let mut p = profile.clone();
            match change {
                "unknown" => p["path"] = json!("/hidden.ttf"),
                "null" => p["faces"] = Value::Null,
                "mismatch" => p["declared_locale"] = json!("en-US"),
                "weight" => p["faces"]["bold"]["weight"] = json!(400),
                _ => unreachable!(),
            }
            write(&b.files["text_profile"], &p);
            b.reject(b.command());
        }
    }
}

#[test]
fn exact_raw_pins_missing_extra_duplicate_and_changed_font_fail_closed() {
    for version in ["0.4", "0.5", "0.7", "0.9"] {
        let b = Bundle::new(version);
        b.old_outputs();
        let pins = b.pins();
        for role in b.files.keys() {
            let mut bad = pins.clone();
            bad.as_object_mut().unwrap().remove(role);
            b.reject(b.command_with(&b.files, &bad, &b.output(), &b.receipt()));
        }
        let mut bad = pins.clone();
        bad["text_layout"] = json!("a".repeat(64));
        b.reject(b.command_with(&b.files, &bad, &b.output(), &b.receipt()));
        let profile = &b.files["text_profile"];
        let mut bytes = fs::read(profile).unwrap();
        bytes.push(b' ');
        fs::write(profile, &bytes).unwrap();
        b.reject(b.command_with(&b.files, &pins, &b.output(), &b.receipt()));
        // Same typed profile with updated raw pin succeeds.
        success(b.command().output().unwrap());
        b.old_outputs();
        let font = &b.files["font_1"];
        let old = fs::read(font).unwrap();
        fs::write(font, b"changed font").unwrap();
        b.reject(b.command_with(&b.files, &pins, &b.output(), &b.receipt()));
        b.reject(b.command());
        fs::write(font, old).unwrap();
        // Same bytes at distinct paths are still duplicate font slots.
        fs::copy(&b.files["font_1"], &b.files["font_2"]).unwrap();
        b.reject(b.command());
    }
}

#[test]
fn all_inputs_and_both_outputs_reject_same_path_hardlink_and_symlink_aliases() {
    for version in ["0.4", "0.5", "0.7", "0.9"] {
        let b = Bundle::new(version);
        b.old_outputs();
        for (role, path) in &b.files {
            let original = fs::read(path).unwrap();
            for mode in ["same", "hard", "sym"] {
                let alias = b.dir.path().join(format!("alias-{role}-{mode}"));
                let destination = match mode {
                    "same" => path.clone(),
                    "hard" => {
                        fs::hard_link(path, &alias).unwrap();
                        alias
                    }
                    "sym" => {
                        #[cfg(unix)]
                        {
                            std::os::unix::fs::symlink(path, &alias).unwrap();
                            alias
                        }
                        #[cfg(not(unix))]
                        {
                            continue;
                        }
                    }
                    _ => unreachable!(),
                };
                for receipt_alias in [false, true] {
                    let out = if receipt_alias {
                        b.output()
                    } else {
                        destination.clone()
                    };
                    let receipt = if receipt_alias {
                        destination.clone()
                    } else {
                        b.receipt()
                    };
                    let mut c = b.command_with(&b.files, &b.pins(), &out, &receipt);
                    assert!(!c.output().unwrap().status.success());
                    assert_eq!(fs::read(path).unwrap(), original);
                    assert_eq!(fs::read(b.output()).unwrap(), b"old SVG");
                    assert_eq!(fs::read(b.receipt()).unwrap(), b"old receipt");
                }
            }
        }
        b.reject(b.command_with(&b.files, &b.pins(), &b.output(), &b.output()));
        let mut files = b.files.clone();
        files.insert("font_2".into(), files["font_1"].clone());
        b.reject(b.command_with(&files, &b.pins(), &b.output(), &b.receipt()));
    }
}

#[test]
fn explicit_relative_paths_work_and_resource_escape_is_rejected() {
    for version in ["0.4", "0.5", "0.7", "0.9"] {
        let b = Bundle::new(version);
        let files = b
            .files
            .iter()
            .map(|(k, v)| (k.clone(), PathBuf::from(v.file_name().unwrap())))
            .collect();
        let mut c = b.command_with(&files, &b.pins(), &b.output(), &b.receipt());
        c.current_dir(b.dir.path());
        success(c.output().unwrap());
        b.old_outputs();
        let outside = tempfile::tempdir().unwrap();
        let external = outside.path().join("font.otf");
        fs::copy(&b.files["font_1"], &external).unwrap();
        let mut files = b.files.clone();
        files.insert("font_1".into(), external.clone());
        b.reject(b.command_with(&files, &b.pins(), &b.output(), &b.receipt()));
        #[cfg(unix)]
        {
            let link = b.dir.path().join("escape");
            std::os::unix::fs::symlink(&external, &link).unwrap();
            files.insert("font_1".into(), link);
            b.reject(b.command_with(&files, &b.pins(), &b.output(), &b.receipt()));
            let external_input = outside.path().join("compiled.json");
            fs::copy(&b.files["input"], &external_input).unwrap();
            let input_link = b.dir.path().join("input-escape");
            std::os::unix::fs::symlink(&external_input, &input_link).unwrap();
            files = b.files.clone();
            files.insert("input".into(), input_link);
            b.reject(b.command_with(&files, &b.pins(), &b.output(), &b.receipt()));
            let internal = b.dir.path().join("internal-font");
            std::os::unix::fs::symlink(&b.files["font_1"], &internal).unwrap();
            files = b.files.clone();
            files.insert("font_1".into(), internal);
            success(
                b.command_with(&files, &b.pins(), &b.output(), &b.receipt())
                    .output()
                    .unwrap(),
            );
        }
    }
}

#[test]
fn budgets_nonregular_and_publication_failure_preserve_outputs() {
    for version in ["0.4", "0.5", "0.7", "0.9"] {
        let b = Bundle::new(version);
        b.old_outputs();
        for (role, limit) in [
            ("input", 32 * 1024 * 1024),
            ("text_profile", 256 * 1024),
            ("font_1", 32 * 1024 * 1024),
        ] {
            let file = &b.files[role];
            let original = fs::read(file).unwrap();
            let pins = b.pins();
            fs::File::create(file).unwrap().set_len(limit + 1).unwrap();
            // Pin deliberately need not hash an over-budget file to reject it.
            b.reject(b.command_with(&b.files, &pins, &b.output(), &b.receipt()));
            fs::write(file, original).unwrap();
        }
        let mut files = b.files.clone();
        files.insert("font_1".into(), b.dir.path().to_path_buf());
        b.reject(b.command_with(&files, &b.pins(), &b.output(), &b.receipt()));
        let blocked = b.dir.path().join("blocked");
        fs::create_dir(&blocked).unwrap();
        let mut c = b.command_with(&b.files, &b.pins(), &b.output(), &blocked);
        assert!(!c.output().unwrap().status.success());
        assert_eq!(fs::read(b.output()).unwrap(), b"old SVG");
        assert_eq!(fs::read(b.receipt()).unwrap(), b"old receipt");
    }
}

#[test]
fn all_native_wrapping_profiles_replay_and_require_exact_layout_files() {
    for version in ["0.4", "0.5", "0.7", "0.9"] {
        for (example, layout) in [
            ("wrapped-text", "wrapped-text"),
            ("wrapped-chart-titles", "chart-title"),
            ("wrapped-bar-categories", "bar-category"),
            ("wrapped-diagram-nodes", "diagram-node"),
        ] {
            let mut b = Bundle::new(version);
            fs::copy(
                root().join("examples/text/wrapping-font-profile.json"),
                &b.files["text_profile"],
            )
            .unwrap();
            for (index, name) in ["Regular", "Medium", "Bold"].into_iter().enumerate() {
                fs::copy(root().join(format!("crates/vizir-compiler/tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-{name}.otf")),
                &b.files[&format!("font_{}", index + 1)]).unwrap();
            }
            let layout_path = b.dir.path().join("layout.json");
            fs::copy(
                root().join(format!("examples/text/{layout}-layout.json")),
                &layout_path,
            )
            .unwrap();
            b.files.insert("text_layout".into(), layout_path.clone());
            let source = b.dir.path().join("source.json");
            success(
                Command::new(env!("CARGO_BIN_EXE_vizir"))
                    .arg("compose")
                    .arg(root().join(format!("examples/composition/{example}.compose.yaml")))
                    .arg("--output")
                    .arg(&source)
                    .output()
                    .unwrap(),
            );
            let mut hir = read(&source);
            hir["version"] = json!(version);
            if version != "0.4" {
                let heatmap = labeled_hir();
                hir["datasets"]["values"] = heatmap["datasets"]["values"].clone();
                let mut view = heatmap["views"][0].clone();
                let width = hir["width"].as_f64().unwrap();
                view["frame"]["x"] = json!(width);
                hir["width"] = json!(width + 1600.0);
                hir["height"] = json!(hir["height"].as_f64().unwrap().max(800.0));
                hir["views"].as_array_mut().unwrap().push(view);
            }
            write(&source, &hir);
            let mut normalize = Command::new(env!("CARGO_BIN_EXE_vizir"));
            normalize
                .arg("normalize")
                .arg(&source)
                .args(["--theme", "azure"])
                .arg("--text-profile")
                .arg(&b.files["text_profile"])
                .arg("--text-layout")
                .arg(&layout_path)
                .arg("--output")
                .arg(&b.files["input"]);
            let mut direct = Command::new(env!("CARGO_BIN_EXE_vizir"));
            direct
                .arg("render")
                .arg(&b.files["input"])
                .args(["--format", "svg"])
                .arg("--output")
                .arg(b.dir.path().join("direct.svg"))
                .arg("--manifest")
                .arg(b.dir.path().join("direct.json"));
            for (role, path) in &b.files {
                if role.starts_with("font_") {
                    let mapping = format!("{}={}", hash(&fs::read(path).unwrap()), path.display());
                    normalize.arg("--font").arg(&mapping);
                    direct.arg("--font").arg(mapping);
                }
            }
            success(normalize.output().unwrap());
            success(direct.output().unwrap());
            success(b.command().output().unwrap());
            assert_eq!(
                fs::read(b.output()).unwrap(),
                fs::read(b.dir.path().join("direct.svg")).unwrap(),
                "{example}"
            );
            let receipt = read(&b.receipt());
            let manifest = read(&b.dir.path().join("direct.json"));
            for field in ["compilation_context", "capability_report", "losses"] {
                assert_eq!(receipt[field], manifest[field], "{example}: {field}");
            }
            let original_layout = fs::read(&layout_path).unwrap();
            let original_compiled = read(&b.files["input"]);
            b.old_outputs();
            let mut omitted = b.files.clone();
            omitted.remove("text_layout");
            let mut pins = b.pins();
            pins.as_object_mut().unwrap().remove("text_layout");
            b.reject(b.command_with(&omitted, &pins, &b.output(), &b.receipt()));
            let mut unknown = read(&layout_path);
            unknown["profile"] = json!("vizir-text-wrap/6");
            write(&layout_path, &unknown);
            b.reject(b.command());
            fs::write(&layout_path, &original_layout).unwrap();
            let mut mismatched = read(&layout_path);
            mismatched["engine"] = json!("different");
            write(&layout_path, &mismatched);
            b.reject(b.command());
            fs::write(&layout_path, &original_layout).unwrap();
            let mut missing = original_compiled.clone();
            missing["context"]
                .as_object_mut()
                .unwrap()
                .remove("text_layout");
            write(&b.files["input"], &missing);
            b.reject(b.command());
            write(&b.files["input"], &original_compiled);
            fs::File::create(&layout_path)
                .unwrap()
                .set_len(256 * 1024 + 1)
                .unwrap();
            b.reject(b.command());
        }
    }
}

#[test]
fn standalone_arguments_reject_noncontiguous_fonts_duplicate_flags_and_nonobject_pins() {
    for version in ["0.4", "0.5", "0.7", "0.9"] {
        let b = Bundle::new(version);
        b.old_outputs();
        let mut files = b.files.clone();
        files.remove("font_2");
        let mut pins = b.pins();
        pins.as_object_mut().unwrap().remove("font_2");
        b.reject(b.command_with(&files, &pins, &b.output(), &b.receipt()));
        for pins in [
            Value::Null,
            json!([]),
            json!("not an object"),
            json!({"input":null}),
        ] {
            b.reject(b.command_with(&b.files, &pins, &b.output(), &b.receipt()));
        }
        let mut duplicate = b.command();
        duplicate.arg("--font-1").arg(&b.files["font_1"]);
        b.reject(duplicate);
    }
}

#[test]
fn oversized_receipt_and_native_text_fit_fail_without_truncation_or_publication() {
    for version in ["0.4", "0.5", "0.7", "0.9"] {
        let b = Bundle::new(version);
        b.old_outputs();
        let original = read(&b.files["input"]);
        let mut oversized = original.clone();
        oversized["mir"]["losses"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "source":"illustrative-budget-test","target":"scene2d","fidelity":"lossless",
                "reason":"x".repeat(8 * 1024 * 1024)
            }));
        write(&b.files["input"], &oversized);
        let output = b.command().output().unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("receipt byte budget"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read(b.output()).unwrap(), b"old SVG");
        assert_eq!(fs::read(b.receipt()).unwrap(), b"old receipt");
        let mut bad_fit = original;
        bad_fit["mir"]["views"][1]["children"][0]["font_size"] = json!(4096);
        write(&b.files["input"], &bad_fit);
        let output = b.command().output().unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("VIZ-TEXT-0006"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read(b.output()).unwrap(), b"old SVG");
        assert_eq!(fs::read(b.receipt()).unwrap(), b"old receipt");
    }
}

fn labeled_hir() -> Value {
    json!({"version":"0.5","id":"illustrative/labeled-provider","width":1600,"height":800,
        "background":"transparent",
        "datasets":{"values":{"key":"id","rows":[
            {"id":"zero","x":"A","y":"R","n":0},
            {"id":"min","x":"B","y":"R","n":i64::MIN},
            {"id":"max","x":"A","y":"S","n":i64::MAX}
        ]}},
        "views":[{"kind":"chart.heatmap","id":"heatmap","dataset":"values",
            "frame":{"x":0,"y":0,"width":1600,"height":800},
            "x":{"field":"x","domain":["A","B","C"]},"y":{"field":"y","domain":["R","S"]},
            "color":{"field":"n","palette":["#000000","#FFFFFF"]},"value_labels":{}}]})
}
fn add_fonts(command: &mut Command, b: &Bundle) {
    for (role, path) in &b.files {
        if role.starts_with("font_") {
            command.arg("--font").arg(format!(
                "{}={}",
                hash(&fs::read(path).unwrap()),
                path.display()
            ));
        }
    }
}
fn normalize_labeled(b: &Bundle, source: &Value) {
    let path = b.dir.path().join("source.json");
    write(&path, source);
    let mut c = Command::new(env!("CARGO_BIN_EXE_vizir"));
    c.arg("normalize")
        .arg(&path)
        .args(["--theme", "azure"])
        .arg("--text-profile")
        .arg(&b.files["text_profile"])
        .arg("--output")
        .arg(&b.files["input"]);
    if let Some(layout) = b.files.get("text_layout") {
        c.arg("--text-layout").arg(layout);
    }
    add_fonts(&mut c, b);
    success(c.output().unwrap());
}
fn verify_direct(b: &Bundle) -> String {
    success(b.command().output().unwrap());
    let svg_path = b.dir.path().join("direct.svg");
    let native_path = b.dir.path().join("direct.json");
    let mut c = Command::new(env!("CARGO_BIN_EXE_vizir"));
    c.arg("render")
        .arg(&b.files["input"])
        .args(["--format", "svg"])
        .arg("--output")
        .arg(&svg_path)
        .arg("--manifest")
        .arg(&native_path);
    add_fonts(&mut c, b);
    success(c.output().unwrap());
    let svg = fs::read(b.output()).unwrap();
    assert_eq!(svg, fs::read(svg_path).unwrap());
    let resources = b
        .files
        .iter()
        .map(|(role, path)| (role.clone(), fs::read(path).unwrap()))
        .collect();
    let description: Value = serde_json::from_slice(&success(
        provider().args(["describe", "--json"]).output().unwrap(),
    ))
    .unwrap();
    let verify = match b.version {
        "0.4" => receipt_verifier::verify,
        "0.5" => receipt_verifier::verify_v2,
        "0.7" => receipt_verifier::verify_v3,
        "0.9" => receipt_verifier::verify_v4,
        _ => unreachable!(),
    };
    verify(
        &fs::read(b.receipt()).unwrap(),
        &resources,
        &svg,
        &read(&native_path),
        description["provider"]["version"].as_str().unwrap(),
    )
    .unwrap();
    String::from_utf8(svg).unwrap()
}

#[test]
fn labeled_inline_and_typed_csv_preserve_sparse_zero_int64_contrast_and_full_receipt() {
    for version in ["0.5", "0.7", "0.9"] {
        let mut expected: Option<(Vec<u8>, Vec<u8>)> = None;
        for csv in [false, true] {
            let b = Bundle::new(version);
            let mut hir = labeled_hir();
            hir["version"] = json!(version);
            if csv {
                let csv_path = b.dir.path().join("values.csv");
                fs::write(&csv_path, b"id,x,y,n\nzero,A,R,0\nmin,B,R,-9223372036854775808\nmax,A,S,9223372036854775807\n").unwrap();
                let template = b.dir.path().join("template.json");
                hir["datasets"] = json!({});
                write(&template, &hir);
                let spec = b.dir.path().join("spec.json");
                write(
                    &spec,
                    &json!({"format":"vizir-csv-import/1","key":"id","columns":[
                {"name":"id","type":"string"},{"name":"x","type":"string"},
                {"name":"y","type":"string"},{"name":"n","type":"int64"}]}),
                );
                let imported = b.dir.path().join("imported.json");
                success(
                    Command::new(env!("CARGO_BIN_EXE_vizir"))
                        .arg("import-csv")
                        .arg(csv_path)
                        .arg("--template")
                        .arg(template)
                        .args(["--template-kind", "hir", "--dataset", "values"])
                        .arg("--spec")
                        .arg(spec)
                        .arg("--output")
                        .arg(&imported)
                        .arg("--provenance")
                        .arg(b.dir.path().join("csv-provenance.json"))
                        .output()
                        .unwrap(),
                );
                hir = read(&imported);
            }
            normalize_labeled(&b, &hir);
            let input = read(&b.files["input"]);
            assert_eq!(
                input["mir"]["views"][0]["mark"]["value_labels"]["instances"],
                json!([
            {"key":"zero","text":"0"}, {"key":"min","text":"-9223372036854775808"},
            {"key":"max","text":"9223372036854775807"}])
            );
            let svg = verify_direct(&b);
            let parsed = roxmltree::Document::parse(&svg).unwrap();
            let labels: Vec<_> = parsed
                .descendants()
                .filter(|n| {
                    n.attribute("id").is_some_and(|id| {
                        id.strip_prefix("heatmap/cell-label/")
                            .is_some_and(|key| !key.contains('/'))
                    })
                })
                .collect();
            // Label groups are keyed; unused category pairs have no synthetic group.
            for key in ["zero", "min", "max"] {
                let id = format!("heatmap/cell-label/{key}");
                let node = parsed
                    .descendants()
                    .find(|n| n.attribute("id") == Some(id.as_str()))
                    .unwrap();
                assert_eq!(node.attribute("data-key"), Some(key));
                assert!(node.descendants().any(|n| n.attribute("fill")
                    == Some(if key == "min" { "#FFFFFF" } else { "#000000" })));
            }
            assert_eq!(labels.len(), 3);
            assert_eq!(
                parsed
                    .descendants()
                    .filter(|n| n
                        .attribute("id")
                        .is_some_and(|id| id.starts_with("heatmap/cell/")))
                    .count(),
                3
            );
            let pair = (
                fs::read(b.output()).unwrap(),
                fs::read(b.receipt()).unwrap(),
            );
            if let Some(expected) = &expected {
                assert_eq!(&pair, expected);
            } else {
                expected = Some(pair);
            }
        }
    }
}

#[test]
fn labeled_cache_mutations_fail_without_refresh_or_publication() {
    for version in ["0.5", "0.7", "0.9"] {
        let b = Bundle::new(version);
        let mut hir = labeled_hir();
        hir["version"] = json!(version);
        normalize_labeled(&b, &hir);
        let original = read(&b.files["input"]);
        b.old_outputs();
        for mutation in [
            "text",
            "key",
            "order",
            "count",
            "row",
            "unknown",
            "null",
            "duplicate",
        ] {
            let mut v = original.clone();
            let labels = &mut v["mir"]["views"][0]["mark"]["value_labels"];
            match mutation {
                "text" => labels["instances"][0]["text"] = json!("fabricated"),
                "key" => labels["instances"][0]["key"] = json!("missing"),
                "order" => labels["instances"].as_array_mut().unwrap().swap(0, 1),
                "count" => {
                    labels["instances"].as_array_mut().unwrap().pop();
                }
                "row" => v["mir"]["data"]["data/values"]["operator"]["rows"][0]["n"] = json!(1),
                "unknown" => labels["unknown"] = json!(true),
                "null" => *labels = Value::Null,
                "duplicate" => {}
                _ => unreachable!(),
            }
            write(&b.files["input"], &v);
            if mutation == "duplicate" {
                let text = fs::read_to_string(&b.files["input"]).unwrap();
                fs::write(
                    &b.files["input"],
                    text.replacen(
                        "\"value_labels\": {",
                        "\"value_labels\": {}, \"value_labels\": {",
                        1,
                    ),
                )
                .unwrap();
            }
            b.reject(b.command());
        }
    }
}

#[test]
fn profiles_are_disjoint_and_old_descriptor_and_receipt_schema_are_frozen() {
    assert_eq!(
        hash(include_bytes!("../assets/compiled-svg-command-v2.json")),
        "96a462e9bf1b53940c7c21f60561f8e1aca7928dc9cf7b7a47c97484fe568acf"
    );
    assert_eq!(
        hash(include_bytes!(
            "../../../schemas/render-receipt-v2.schema.json"
        )),
        "6ada9f459c745d3aeb4087407a6140ea19f158f329c72a5d41b1374b30a6015d"
    );
    let mut v3 = Bundle::new("0.7");
    v3.old_outputs();
    let original = read(&v3.files["input"]);
    for rejected in ["0.4", "0.5", "0.6", "0.8", "0.9", "1.0"] {
        let mut input = original.clone();
        input["mir"]["version"] = json!(rejected);
        input["mir"]["source_hir_version"] = json!(rejected);
        write(&v3.files["input"], &input);
        v3.reject(v3.command());
    }
    write(&v3.files["input"], &original);
    for old in ["0.4", "0.5"] {
        v3.version = old;
        v3.reject(v3.command());
    }

    assert_eq!(
        hash(include_bytes!("../assets/compiled-svg-command-v1.json")),
        "6ff9df004ae7f27c147fe00e2f2431db3c3189aba959da8de166f089d9f4e638"
    );
    assert_eq!(
        hash(include_bytes!(
            "../../../schemas/render-receipt.schema.json"
        )),
        "ce8afc4a2211fc7d9b3cb84a6353b911a741af6115b00b24ec15ad04575d0300"
    );
    for version in ["0.4", "0.5"] {
        let mut b = Bundle::new(version);
        b.old_outputs();
        b.version = if version == "0.4" { "0.5" } else { "0.4" };
        b.reject(b.command());
        b.version = version;
        let original = read(&b.files["input"]);
        for future in ["0.6", "0.7", "0.8", "0.9", "1.0"] {
            let mut input = original.clone();
            input["mir"]["version"] = json!(future);
            input["mir"]["source_hir_version"] = json!(future);
            write(&b.files["input"], &input);
            b.reject(b.command());
        }
    }
}

#[test]
fn native_heatmap_wrap_five_stays_outside_legacy_provider_profiles() {
    use vizir_compiler::{SemanticTextLayoutTarget, TextLayoutContext};
    for version in ["0.4", "0.5"] {
        let mut b = Bundle::new(version);
        let mut fonts = FontResources::new();
        for role in ["font_1", "font_2", "font_3"] {
            let bytes = fs::read(&b.files[role]).unwrap();
            fonts.insert(&hash(&bytes), bytes).unwrap();
        }
        let layout = TextLayoutContext::new(vec![]).with_heatmap_x_labels(vec![
            SemanticTextLayoutTarget::heatmap_x_category_labels("h", 40., 4, 16.),
        ]);
        let document:Document=serde_json::from_value(json!({"version":version,"id":"h-wrap","width":720,"height":400,"datasets":{"d":{"key":"id","rows":[{"id":"a","x":"中文测试中文测试","y":"测试","v":1}]}},"views":[{"kind":"chart.heatmap","id":"h","frame":{"x":0,"y":0,"width":720,"height":400},"dataset":"d","x":{"field":"x"},"y":{"field":"y"},"color":{"field":"v"}}]})).unwrap();
        let ctx = CompilationContext::new()
            .with_theme(ThemeContext::resolve("azure").unwrap())
            .with_text(
                parse_text_context_json(&fs::read(&b.files["text_profile"]).unwrap()).unwrap(),
            )
            .with_text_layout(layout.clone());
        let compiled = compile_with_context(&document, &ctx, &fonts).unwrap();
        write(
            &b.files["input"],
            &serde_json::to_value(compiled.mir).unwrap(),
        );
        let p = b.dir.path().join("layout.json");
        write(&p, &serde_json::to_value(layout).unwrap());
        b.files.insert("text_layout".into(), p);
        b.old_outputs();
        let result = b.command().output().unwrap();
        assert!(
            String::from_utf8_lossy(&result.stderr)
                .contains("only text-wrap/1 through text-wrap/4")
        );
        b.reject(b.command());
    }
}

#[test]
fn v3_replays_authored_domains_stable_subset_colors_and_receipt() {
    let b = Bundle::new("0.7");
    let source = success(
        Command::new(env!("CARGO_BIN_EXE_vizir"))
            .arg("compose")
            .arg(root().join("examples/composition/shared-numeric-domains.compose.yaml"))
            .output()
            .unwrap(),
    );
    let hir: Value = serde_json::from_slice(&source).unwrap();
    assert_eq!(hir["version"], "0.7");
    normalize_labeled(&b, &hir);
    let original = read(&b.files["input"]);
    for panel in original["mir"]["views"].as_array().unwrap() {
        for (i, domain) in [json!([0., 1.]), json!([0., 10.])].into_iter().enumerate() {
            assert_eq!(panel["scales"][i]["domain"], domain);
            assert_eq!(panel["scales"][i]["out_of_domain"], "reject");
        }
        let colors = panel["scales"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["type"] == "ordinal-color")
            .unwrap();
        assert_eq!(colors["domain"], json!(["Alpha", "Beta"]));
        assert_eq!(colors["range"], json!(["#2563eb", "#138b90"]));
    }
    let svg = verify_direct(&b);
    let document = roxmltree::Document::parse(&svg).unwrap();
    for id in ["all-series/series/Beta", "beta-only/series/Beta"] {
        let n = document
            .descendants()
            .find(|n| n.attribute("id") == Some(id))
            .unwrap();
        assert!(
            n.descendants()
                .any(|n| n.attribute("stroke") == Some("#138b90"))
        );
    }
    assert!(
        !document
            .descendants()
            .any(|n| n.attribute("id") == Some("beta-only/series/Alpha"))
    );
    let receipt = read(&b.receipt());
    assert_eq!(receipt["schema_version"], "vizir.render-receipt/v3");
    assert_eq!(receipt["profile"], "vizir-compiled-svg/3");
    assert_eq!(receipt["source_ir_version"], "0.7");
    b.old_outputs();
    for mutation in ["out_of_domain", "category", "stale", "null", "unknown"] {
        let mut changed = original.clone();
        match mutation {
            "out_of_domain" => {
                changed["mir"]["data"]["data/both"]["operator"]["rows"][0]["y"] = json!(11)
            }
            "category" => {
                changed["mir"]["data"]["data/both"]["operator"]["rows"][0]["series"] =
                    json!("Gamma")
            }
            "stale" => changed["mir"]["data"]["data/both"]["operator"]["rows"][0]["y"] = json!(4),
            "null" => changed["mir"]["views"][0]["scales"][0]["domain"] = Value::Null,
            "unknown" => changed["mir"]["views"][0]["scales"][0]["hidden"] = json!(true),
            _ => unreachable!(),
        }
        write(&b.files["input"], &changed);
        b.reject(b.command());
    }
}

#[test]
fn v3_and_v4_heatmap_wrap_five_is_exact_persisted_and_closed() {
    use vizir_compiler::{SemanticTextLayoutTarget, TextLayoutContext};
    for version in ["0.7", "0.9"] {
        let mut b = Bundle::new(version);
        let mut fonts = FontResources::new();
        for role in ["font_1", "font_2", "font_3"] {
            let bytes = fs::read(&b.files[role]).unwrap();
            fonts.insert(&hash(&bytes), bytes).unwrap();
        }
        let layout = TextLayoutContext::new(vec![]).with_heatmap_x_labels(vec![
            SemanticTextLayoutTarget::heatmap_x_category_labels("h", 40., 4, 16.),
        ]);
        let document: Document = serde_json::from_value(
            json!({"version":version,"id":"h-wrap-provider","width":720,"height":400,
        "datasets":{"d":{"key":"id","rows":[{"id":"a","x":"中文测试中文测试","y":"测试","v":1}]}},
        "views":[{"kind":"chart.heatmap","id":"h","frame":{"x":0,"y":0,"width":720,"height":400},
        "dataset":"d","x":{"field":"x"},"y":{"field":"y"},"color":{"field":"v","domain":[0,10]}}]}),
        )
        .unwrap();
        let ctx = CompilationContext::new()
            .with_theme(ThemeContext::resolve("azure").unwrap())
            .with_text(
                parse_text_context_json(&fs::read(&b.files["text_profile"]).unwrap()).unwrap(),
            )
            .with_text_layout(layout.clone());
        let compiled = compile_with_context(&document, &ctx, &fonts).unwrap();
        let original = serde_json::to_value(compiled.mir).unwrap();
        write(&b.files["input"], &original);
        let policy_path = b.dir.path().join("layout.json");
        let policy = serde_json::to_value(layout).unwrap();
        write(&policy_path, &policy);
        b.files.insert("text_layout".into(), policy_path.clone());
        let svg = verify_direct(&b);
        assert!(svg.contains("<path"));
        assert_eq!(
            read(&b.receipt())["compilation_context"]["text_layout"],
            policy
        );
        let copy = tempfile::tempdir().unwrap();
        let mut copied = BTreeMap::new();
        for (role, path) in &b.files {
            let to = copy.path().join(path.file_name().unwrap());
            fs::copy(path, &to).unwrap();
            copied.insert(role.clone(), to);
        }
        let mut replay = b.command_with(
            &copied,
            &b.pins(),
            &copy.path().join("figure.svg"),
            &copy.path().join("receipt.json"),
        );
        success(replay.current_dir("/").output().unwrap());
        assert_eq!(
            fs::read(copy.path().join("figure.svg")).unwrap(),
            svg.as_bytes()
        );
        assert_eq!(
            fs::read(copy.path().join("receipt.json")).unwrap(),
            fs::read(b.receipt()).unwrap()
        );
        b.old_outputs();
        for mutation in [
            "missing",
            "unknown",
            "null",
            "mismatch",
            "stale_range",
            "stale_text",
        ] {
            let mut changed = original.clone();
            let mut policy_changed = policy.clone();
            match mutation {
                "missing" => {
                    changed["context"]
                        .as_object_mut()
                        .unwrap()
                        .remove("text_layout");
                }
                "unknown" => policy_changed["profile"] = json!("vizir-text-wrap/6"),
                "null" => changed["context"]["text_layout"] = Value::Null,
                "mismatch" => policy_changed["semantic_targets"][0]["max_width"] = json!(39),
                "stale_range" => {
                    changed["context"]["text_layout"]["semantic_targets"][0]["line_height"] =
                        json!(40);
                    policy_changed = changed["context"]["text_layout"].clone();
                }
                "stale_text" => {
                    changed["mir"]["data"]["data/d"]["operator"]["rows"][0]["x"] = json!("中文")
                }
                _ => unreachable!(),
            }
            write(&b.files["input"], &changed);
            write(&policy_path, &policy_changed);
            b.reject(b.command());
        }
        write(&b.files["input"], &original);
        write(&policy_path, &policy);
        let mut missing = b.files.clone();
        missing.remove("text_layout");
        let mut pins = b.pins();
        pins.as_object_mut().unwrap().remove("text_layout");
        b.reject(b.command_with(&missing, &pins, &b.output(), &b.receipt()));
    }
}

#[test]
fn v3_receipt_schema_has_exact_native_wrap_five_branch_without_widening_legacy() {
    let v3: Value = serde_json::from_str(include_str!(
        "../../../schemas/render-receipt-v3.schema.json"
    ))
    .unwrap();
    let native: Value =
        serde_json::from_str(include_str!("../../../schemas/compiled-mir.schema.json")).unwrap();
    for name in [
        "TextLayoutContext",
        "HeatmapTextLayoutContext",
        "HeatmapTextLayoutRole",
        "HeatmapSemanticTextLayoutTarget",
    ] {
        assert_eq!(v3["$defs"][name], native["$defs"][name]);
    }
    assert_eq!(v3["properties"]["source_ir_version"]["const"], "0.7");
    assert_eq!(v3["additionalProperties"], false);
    for old in [
        include_str!("../../../schemas/render-receipt.schema.json"),
        include_str!("../../../schemas/render-receipt-v2.schema.json"),
    ] {
        let legacy: Value = serde_json::from_str(old).unwrap();
        assert_eq!(
            legacy["$defs"]["TextLayoutContext"]["oneOf"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert!(legacy["$defs"].get("HeatmapTextLayoutContext").is_none());
    }
}

fn aligned_v4_bundle() -> Bundle {
    let mut b = Bundle::new("0.9");
    fs::copy(
        root().join("examples/text/wrapping-font-profile.json"),
        &b.files["text_profile"],
    )
    .unwrap();
    for (index, name) in ["Regular", "Medium", "Bold"].into_iter().enumerate() {
        fs::copy(
            root().join(format!(
                "crates/vizir-compiler/tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-{name}.otf"
            )),
            &b.files[&format!("font_{}", index + 1)],
        )
        .unwrap();
    }
    let source = success(
        Command::new(env!("CARGO_BIN_EXE_vizir"))
            .arg("compose")
            .arg(root().join("examples/composition/aligned-numeric-grid.compose.yaml"))
            .output()
            .unwrap(),
    );
    let hir: Value = serde_json::from_slice(&source).unwrap();
    assert_eq!(hir["version"], "0.9");
    assert_eq!(
        hir["plot_alignment"]["members"].as_array().unwrap().len(),
        4
    );
    assert_eq!(hir["shared_legend"]["members"].as_array().unwrap().len(), 3);
    let layout = vizir_compiler::TextLayoutContext::new(vec![]).with_semantic_targets(vec![
        vizir_compiler::SemanticTextLayoutTarget::chart_title("trend", 210., 8, 28.),
    ]);
    let layout_path = b.dir.path().join("layout.json");
    write(&layout_path, &serde_json::to_value(layout).unwrap());
    b.files.insert("text_layout".into(), layout_path);
    normalize_labeled(&b, &hir);
    b
}

fn chart_scale<'a>(chart: &'a Value, axis: &str) -> &'a Value {
    let scale_id = chart["mark"][axis]["scale"].as_str().unwrap();
    chart["scales"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == scale_id)
        .unwrap()
}

#[test]
fn v4_composed_alignment_shared_legend_and_wrapped_fonts_match_native_and_copied_replay() {
    let b = aligned_v4_bundle();
    let input = read(&b.files["input"]);
    let mir = &input["mir"];
    assert_eq!(mir["version"], "0.9");
    assert_eq!(mir["source_hir_version"], "0.9");
    assert_eq!(mir["plot_alignment"]["id"], "comparison-plots");
    assert_eq!(mir["plot_alignment"]["mode"], "uniform");
    let views = mir["views"].as_array().unwrap();
    assert_eq!(views.len(), 4);
    let mut expected_insets: Option<[f64; 4]> = None;
    for (index, id) in ["trend", "measurements", "area", "comparison"]
        .into_iter()
        .enumerate()
    {
        let view = &views[index];
        assert_eq!(view["id"], id);
        assert_eq!(
            mir["plot_alignment"]["members"][index],
            json!({"view":id,"x_scale":chart_scale(view,"x")["id"],"y_scale":chart_scale(view,"y")["id"]})
        );
        let frame = &view["frame"];
        let x = &chart_scale(view, "x")["range"];
        let y = &chart_scale(view, "y")["range"];
        let number = |v: &Value| v.as_f64().unwrap();
        let insets = [
            number(&x[0]) - number(&frame["x"]),
            number(&frame["x"]) + number(&frame["width"]) - number(&x[1]),
            number(&y[1]) - number(&frame["y"]),
            number(&frame["y"]) + number(&frame["height"]) - number(&y[0]),
        ];
        if let Some(expected) = expected_insets {
            for (actual, expected) in insets.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 1e-8);
            }
        } else {
            expected_insets = Some(insets);
        }
        if index < 3 {
            let color = view["scales"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["type"] == "ordinal-color")
                .unwrap();
            assert_eq!(color["domain"], json!(["Alpha", "Beta", "Reserved"]));
            assert_eq!(
                mir["shared_legend"]["members"][index],
                json!({"view":id,"scale":color["id"]})
            );
            assert!(
                !view["guides"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|g| g["kind"] == "legend")
            );
        }
    }
    assert_eq!(chart_scale(&views[0], "x")["domain"], json!([0., 2.]));
    assert_eq!(chart_scale(&views[3], "x")["domain"], json!([0., 4.]));
    assert_eq!(
        chart_scale(&views[0], "y")["domain"],
        json!([0., 10_000_000.])
    );
    assert_eq!(
        chart_scale(&views[3], "y")["domain"],
        json!([0., 20_000_000.])
    );
    let svg = verify_direct(&b);
    let xml = roxmltree::Document::parse(&svg).unwrap();
    assert!(!xml.descendants().any(|n| n.has_tag_name("text")));
    assert!(xml.descendants().any(|n| n.has_tag_name("path")));
    assert!(
        xml.descendants()
            .any(|n| n.attribute("id") == Some("comparison/legend/0/swatch"))
    );
    assert_eq!(
        xml.descendants()
            .filter(|n| n.attribute("id") == Some("shared-legend:10:series-key"))
            .count(),
        1
    );
    for id in ["trend", "measurements", "area"] {
        assert!(!xml.descendants().any(|n| {
            n.attribute("id")
                .is_some_and(|node_id| node_id.starts_with(&format!("{id}/legend/")))
        }));
    }
    let receipt = read(&b.receipt());
    assert_eq!(receipt["schema_version"], "vizir.render-receipt/v4");
    assert_eq!(receipt["profile"], "vizir-compiled-svg/4");
    assert_eq!(receipt["source_ir_version"], "0.9");
    assert_eq!(receipt["compilation_context"], input["context"]);
    assert_eq!(
        receipt["compilation_context"]["text_layout"],
        read(&b.files["text_layout"])
    );

    // Replay only copied declared resources, after the original bundle is gone.
    let copied = tempfile::tempdir().unwrap();
    let mut files = BTreeMap::new();
    for (role, path) in &b.files {
        let destination = copied.path().join(path.file_name().unwrap());
        fs::copy(path, &destination).unwrap();
        files.insert(role.clone(), destination);
    }
    let receipt_bytes = fs::read(b.receipt()).unwrap();
    let mut command = b.command_with(
        &files,
        &b.pins(),
        &copied.path().join("copied.svg"),
        &copied.path().join("copied.receipt.json"),
    );
    command.current_dir("/");
    drop(b);
    assert!(success(command.output().unwrap()).is_empty());
    assert_eq!(
        fs::read(copied.path().join("copied.svg")).unwrap(),
        svg.as_bytes()
    );
    assert_eq!(
        fs::read(copied.path().join("copied.receipt.json")).unwrap(),
        receipt_bytes
    );
}

#[test]
fn v4_stale_alignment_shared_legend_and_native_fields_fail_atomically() {
    let b = aligned_v4_bundle();
    // Establish a successful baseline so a missing resource cannot mask a mutation.
    verify_direct(&b);
    let original = read(&b.files["input"]);
    b.old_outputs();
    for mutation in [
        "one_aligned_range",
        "all_aligned_ranges",
        "y_range",
        "alignment_ref",
        "alignment_null",
        "alignment_unknown",
        "alignment_member_unknown",
        "legend_ref",
        "legend_color",
        "legend_domain",
        "legend_local_conflict",
        "legend_null",
        "legend_unknown",
        "legend_member_unknown",
        "scatter_cache",
        "native_unknown",
        "native_null",
        "duplicate_alignment",
        "duplicate_legend",
    ] {
        let mut changed = original.clone();
        let mir = &mut changed["mir"];
        match mutation {
            "one_aligned_range" | "all_aligned_ranges" => {
                for index in 0..if mutation == "all_aligned_ranges" { 4 } else { 1 } {
                    let range = &mut mir["views"][index]["scales"][0]["range"];
                    range[0] = json!(range[0].as_f64().unwrap() + 1.0);
                }
            }
            "y_range" => {
                let range = &mut mir["views"][0]["scales"][1]["range"];
                range[1] = json!(range[1].as_f64().unwrap() + 1.0);
            }
            "alignment_ref" => mir["plot_alignment"]["members"][0]["x_scale"] = json!("measurements/x"),
            "alignment_null" => mir["plot_alignment"] = Value::Null,
            "alignment_unknown" => mir["plot_alignment"]["hidden"] = json!(true),
            "alignment_member_unknown" => mir["plot_alignment"]["members"][0]["hidden"] = json!(true),
            "legend_ref" => mir["shared_legend"]["members"][0]["scale"] = json!("measurements/color"),
            "legend_color" | "legend_domain" => {
                let color = mir["views"][1]["scales"].as_array_mut().unwrap().iter_mut()
                    .find(|s| s["type"] == "ordinal-color").unwrap();
                if mutation == "legend_color" {
                    color["range"][2] = json!("#123456");
                } else {
                    color["domain"][2] = json!("Forged");
                }
            }
            "legend_local_conflict" => mir["views"][0]["guides"].as_array_mut().unwrap().push(
                json!({"id":"trend/guides/forged-legend","kind":"legend","scale":"trend/color","label":"series","orient":"right"})),
            "legend_null" => mir["shared_legend"] = Value::Null,
            "legend_unknown" => mir["shared_legend"]["hidden"] = json!(true),
            "legend_member_unknown" => mir["shared_legend"]["members"][0]["hidden"] = json!(true),
            "scatter_cache" => mir["views"][1]["mark"]["instances"][0]["y"] = json!(999),
            "native_unknown" => mir["views"][0]["hidden"] = json!(true),
            "native_null" => mir["views"][1]["mark"]["instances"] = Value::Null,
            "duplicate_alignment" | "duplicate_legend" => {},
            _ => unreachable!(),
        }
        write(&b.files["input"], &changed);
        if mutation.starts_with("duplicate_") {
            let field = if mutation == "duplicate_alignment" {
                "plot_alignment"
            } else {
                "shared_legend"
            };
            let bytes = fs::read_to_string(&b.files["input"]).unwrap();
            let needle = format!("\"{field}\": {{");
            assert!(bytes.contains(&needle));
            fs::write(
                &b.files["input"],
                bytes.replacen(&needle, &format!("\"{field}\": {{}}, {needle}"), 1),
            )
            .unwrap();
        }
        let result = b.command().output().unwrap();
        assert!(!result.status.success(), "accepted {mutation}");
        assert!(!result.stderr.is_empty(), "no diagnostic for {mutation}");
        b.reject(b.command());
    }
}

#[test]
fn v4_requires_matching_hir_mir_nine_and_legacy_profiles_reject_real_nine_bundles() {
    let mut b = Bundle::new("0.9");
    b.old_outputs();
    let original = read(&b.files["input"]);
    for version in ["0.4", "0.5", "0.6", "0.7", "0.8", "1.0"] {
        for (mir_version, hir_version) in [(version, version), (version, "0.9"), ("0.9", version)] {
            let mut changed = original.clone();
            changed["mir"]["version"] = json!(mir_version);
            changed["mir"]["source_hir_version"] = json!(hir_version);
            write(&b.files["input"], &changed);
            b.reject(b.command());
        }
    }
    write(&b.files["input"], &original);
    for version in ["0.4", "0.5", "0.7"] {
        b.version = version;
        b.reject(b.command());
    }
    let mut composed = aligned_v4_bundle();
    composed.old_outputs();
    for version in ["0.4", "0.5", "0.7"] {
        composed.version = version;
        composed.reject(composed.command());
    }
    // Genuine older envelopes are rejected too, not merely relabeled 0.9 data.
    for version in ["0.4", "0.5", "0.7", "0.8"] {
        let mut old = Bundle::new(version);
        old.old_outputs();
        old.version = "0.9";
        old.reject(old.command());
    }
}

#[test]
fn v4_receipt_schema_changes_only_profile_constants_and_freezes_v3() {
    assert_eq!(
        hash(include_bytes!("../assets/compiled-svg-command-v3.json")),
        "50a61afe4c24069e21cbf2809ccb71388f01d0051d50fef30af4b2e4c8bb15d2"
    );
    assert_eq!(
        hash(include_bytes!(
            "../../../schemas/render-receipt-v3.schema.json"
        )),
        "cd7eee67aa242053ce3f574edd141db2ef000dbb496de785d589bd56c7807392"
    );
    let mut expected: Value = serde_json::from_str(include_str!(
        "../../../schemas/render-receipt-v3.schema.json"
    ))
    .unwrap();
    expected["properties"]["schema_version"]["const"] = json!("vizir.render-receipt/v4");
    expected["properties"]["profile"]["const"] = json!("vizir-compiled-svg/4");
    expected["properties"]["source_ir_version"]["const"] = json!("0.9");
    let actual: Value = serde_json::from_str(include_str!(
        "../../../schemas/render-receipt-v4.schema.json"
    ))
    .unwrap();
    assert_eq!(actual, expected);
}
