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
}
impl Bundle {
    fn new() -> Self {
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
            "version":"0.4","id":"illustrative/provider","width":900,"height":400,
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
        Self { dir, files }
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
        c.arg("render-compiled-svg").arg(&files["input"]);
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
    let b = Bundle::new();
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
    receipt_verifier::verify(
        &receipt_bytes,
        &actual_resources,
        &svg,
        &manifest,
        provider_version,
    )
    .unwrap();
    for change in [
        "unknown",
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
            "context" => changed["compilation_context"]["text"]["declared_locale"] = json!("en-US"),
            "losses" => changed["losses"] = json!([]),
            "capability" => changed["capability_report"]["decisions"] = json!([]),
            "background" => changed["background"] = json!("#ffffff"),
            _ => unreachable!(),
        }
        assert!(
            receipt_verifier::verify(
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
        receipt_verifier::verify(
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
        receipt_verifier::verify(
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
        receipt_verifier::verify(
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
    assert_eq!(describe["operations"], json!(["render-compiled-svg"]));
    assert_eq!(describe["provider"]["protocol_versions"], json!([1]));
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
    let b = Bundle::new();
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
            "stale" => value["mir"]["data"]["data/data"]["operator"]["rows"][0]["y"] = json!(3.0),
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

#[test]
fn exact_raw_pins_missing_extra_duplicate_and_changed_font_fail_closed() {
    let b = Bundle::new();
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

#[test]
fn all_inputs_and_both_outputs_reject_same_path_hardlink_and_symlink_aliases() {
    let b = Bundle::new();
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

#[test]
fn explicit_relative_paths_work_and_resource_escape_is_rejected() {
    let b = Bundle::new();
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

#[test]
fn budgets_nonregular_and_publication_failure_preserve_outputs() {
    let b = Bundle::new();
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

#[test]
fn all_native_wrapping_profiles_replay_and_require_exact_layout_files() {
    for (example, layout) in [
        ("wrapped-text", "wrapped-text"),
        ("wrapped-chart-titles", "chart-title"),
        ("wrapped-bar-categories", "bar-category"),
        ("wrapped-diagram-nodes", "diagram-node"),
    ] {
        let mut b = Bundle::new();
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
        hir["version"] = json!("0.4");
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
        unknown["profile"] = json!("vizir-text-wrap/5");
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

#[test]
fn standalone_arguments_reject_noncontiguous_fonts_duplicate_flags_and_nonobject_pins() {
    let b = Bundle::new();
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

#[test]
fn oversized_receipt_and_native_text_fit_fail_without_truncation_or_publication() {
    let b = Bundle::new();
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
