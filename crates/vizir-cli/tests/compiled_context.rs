use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_vizir"))
        .args(args)
        .output()
        .unwrap()
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn rejected(output: Output, message: &str) {
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
}
fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}
fn write(p: &Path, value: &Value) {
    fs::write(p, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn source(dir: &Path) -> PathBuf {
    let p = dir.join("source.json");
    write(
        &p,
        &json!({"version":"0.1","id":"context-cli","width":640,"height":400,"views":[
        {"kind":"geometry.scene","id":"labels","frame":{"x":0,"y":0,"width":640,"height":400},
         "children":[{"type":"text","id":"hello","x":20,"y":50,"text":"Hello"}]}]}),
    );
    p
}
fn profile(dir: &Path) -> PathBuf {
    let p = dir.join("profile.json");
    write(
        &p,
        &json!({"profile":vizir_compiler::TEXT_PROFILE,"engine":vizir_compiler::TEXT_ENGINE,"declared_locale":"en-US","shaping_language":"default",
        "faces":{"regular":{"sha256":EMPTY_SHA,"face_index":0,"weight":400},
                 "medium":{"sha256":EMPTY_SHA,"face_index":0,"weight":500},
                 "bold":{"sha256":EMPTY_SHA,"face_index":0,"weight":700}}}),
    );
    p
}

#[test]
fn every_compiler_command_accepts_text_flags_and_diagnoses_missing_resources() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path());
    let profile = profile(t.path());
    for command in ["validate", "normalize", "lower"] {
        rejected(
            run(&[command, path(&input), "--text-profile", path(&profile)]),
            "missing font resource",
        );
        rejected(
            run(&[
                command,
                path(&input),
                "--font",
                &format!("{EMPTY_SHA}=unused.ttf"),
            ]),
            "--font requires",
        );
    }
    let out = t.path().join("out.svg");
    rejected(
        run(&[
            "render",
            path(&input),
            "--text-profile",
            path(&profile),
            "--format",
            "svg",
            "-o",
            path(&out),
        ]),
        "missing font resource",
    );
    assert!(!out.exists());
    rejected(
        run(&[
            "explain",
            path(&input),
            "--text-profile",
            path(&profile),
            "--node",
            "labels/hello",
        ]),
        "missing font resource",
    );
}

#[test]
fn persisted_context_rejects_adding_text_or_changing_theme_before_resource_reads() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path());
    let profile = profile(t.path());
    let themed: Value = serde_json::from_str(&success(run(&[
        "normalize",
        path(&input),
        "--theme",
        "azure",
    ])))
    .unwrap();
    let saved = t.path().join("saved.json");
    write(&saved, &themed);
    rejected(
        run(&["lower", path(&saved), "--text-profile", path(&profile)]),
        "original HIR",
    );
    let compiled = json!({"format":vizir_compiler::COMPILED_MIR_FORMAT,"context":{"theme":themed["theme"]},"mir":themed["mir"]});
    write(&saved, &compiled);
    success(run(&["validate", path(&saved), "--theme", "azure"]));
    rejected(
        run(&["lower", path(&saved), "--theme", "sage"]),
        "original HIR",
    );
    rejected(
        run(&["normalize", path(&saved), "--text-profile", path(&profile)]),
        "original HIR",
    );
    let mut empty = compiled;
    empty["context"] = json!({});
    write(&saved, &empty);
    rejected(
        run(&["validate", path(&saved), "--theme", "azure"]),
        "original HIR",
    );
}

#[test]
fn profiles_and_font_reads_are_strict_bounded_and_regular() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path());
    let profile = profile(t.path());
    let bad = t.path().join("bad.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&profile).unwrap()).unwrap();
    value["path"] = json!("/etc/passwd");
    write(&bad, &value);
    rejected(
        run(&["validate", path(&input), "--text-profile", path(&bad)]),
        "unknown field",
    );
    fs::write(&bad, "{\"profile\":\"a\",\"profile\":\"b\"}").unwrap();
    rejected(
        run(&["validate", path(&input), "--text-profile", path(&bad)]),
        "duplicate",
    );
    fs::File::create(&bad)
        .unwrap()
        .set_len(32 * 1024 * 1024 + 1)
        .unwrap();
    rejected(
        run(&["validate", path(&input), "--text-profile", path(&bad)]),
        "32 MiB",
    );
    rejected(
        run(&["validate", path(&input), "--text-profile", path(t.path())]),
        "regular file",
    );
    rejected(
        run(&[
            "validate",
            path(&input),
            "--text-profile",
            path(&profile),
            "--font",
            "bad",
        ]),
        "SHA256=PATH",
    );
    rejected(
        run(&[
            "validate",
            path(&input),
            "--text-profile",
            path(&profile),
            "--font",
            &format!("{EMPTY_SHA}={}", bad.display()),
        ]),
        "32 MiB",
    );
    rejected(
        run(&[
            "validate",
            path(&input),
            "--text-profile",
            path(&profile),
            "--font",
            &format!("{EMPTY_SHA}={}", t.path().display()),
        ]),
        "regular file",
    );
    let missing = t.path().join("missing.ttf");
    rejected(
        run(&[
            "validate",
            path(&input),
            "--text-profile",
            path(&profile),
            "--font",
            &format!("{EMPTY_SHA}={}", missing.display()),
        ]),
        "missing.ttf",
    );
    let empty = t.path().join("empty.ttf");
    fs::write(&empty, b"").unwrap();
    let mapping = format!("{EMPTY_SHA}={}", empty.display());
    rejected(
        run(&[
            "validate",
            path(&input),
            "--text-profile",
            path(&profile),
            "--font",
            &mapping,
            "--font",
            &mapping,
        ]),
        "duplicate --font mapping",
    );
    let uppercase = format!("{}={}", EMPTY_SHA.to_ascii_uppercase(), empty.display());
    rejected(
        run(&[
            "validate",
            path(&input),
            "--text-profile",
            path(&profile),
            "--font",
            &uppercase,
        ]),
        "lowercase",
    );
    let wrong = t.path().join("wrong.ttf");
    fs::write(&wrong, b"not the pinned font").unwrap();
    rejected(
        run(&[
            "validate",
            path(&input),
            "--text-profile",
            path(&profile),
            "--font",
            &format!("{EMPTY_SHA}={}", wrong.display()),
        ]),
        "do not match SHA256",
    );
}

#[test]
fn output_and_manifest_cannot_overwrite_profiles_or_font_sources() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path());
    let profile = profile(t.path());
    let original = fs::read(&profile).unwrap();
    for command in ["normalize", "lower"] {
        rejected(
            run(&[
                command,
                path(&input),
                "--text-profile",
                path(&profile),
                "-o",
                path(&profile),
            ]),
            "VIZ-PATH-0001",
        );
        assert_eq!(fs::read(&profile).unwrap(), original);
    }
    let font = t.path().join("font.ttf");
    fs::write(&font, b"").unwrap();
    let mapping = format!("{EMPTY_SHA}={}", font.display());
    let out = t.path().join("out.svg");
    rejected(
        run(&[
            "render",
            path(&input),
            "--text-profile",
            path(&profile),
            "--font",
            &mapping,
            "--format",
            "svg",
            "-o",
            path(&out),
            "--manifest",
            path(&font),
        ]),
        "VIZ-PATH-0001",
    );
    assert!(fs::read(&font).unwrap().is_empty());
    assert!(!out.exists());
    #[cfg(unix)]
    {
        let linked = t.path().join("font-link.ttf");
        std::os::unix::fs::symlink(&font, &linked).unwrap();
        rejected(
            run(&[
                "lower",
                path(&input),
                "--text-profile",
                path(&profile),
                "--font",
                &mapping,
                "-o",
                path(&linked),
            ]),
            "VIZ-PATH-0001",
        );
        let hard = t.path().join("profile-link.json");
        fs::hard_link(&profile, &hard).unwrap();
        rejected(
            run(&[
                "normalize",
                path(&input),
                "--text-profile",
                path(&profile),
                "-o",
                path(&hard),
            ]),
            "VIZ-PATH-0001",
        );
    }
}

#[cfg(unix)]
#[test]
fn profile_and_font_fifos_never_wait_for_a_writer() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path());
    let profile = profile(t.path());
    let fifo = t.path().join("resource");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    for font in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_vizir"));
        command.args([
            "validate",
            path(&input),
            "--text-profile",
            if font { path(&profile) } else { path(&fifo) },
        ]);
        if font {
            command.args(["--font", &format!("{EMPTY_SHA}={}", fifo.display())]);
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("resource FIFO read blocked");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        rejected(child.wait_with_output().unwrap(), "regular file");
    }
}

#[test]
fn compiled_schema_matches_checked_in_contract_and_legacy_flags_stay_unwrapped() {
    let emitted: Value = serde_json::from_str(&success(run(&["schema", "compiled-mir"]))).unwrap();
    let saved: Value =
        serde_json::from_str(include_str!("../../../schemas/compiled-mir.schema.json")).unwrap();
    assert_eq!(emitted, saved);
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path());
    let legacy: Value = serde_json::from_str(&success(run(&["normalize", path(&input)]))).unwrap();
    assert!(legacy.get("format").is_none());
    let themed: Value = serde_json::from_str(&success(run(&[
        "normalize",
        path(&input),
        "--theme",
        "azure",
    ])))
    .unwrap();
    assert_eq!(themed["format"], "vizir-themed-mir/1");
    assert!(themed.get("context").is_none());
}

fn measured_profile(dir: &Path) -> (PathBuf, Vec<String>) {
    let fixture_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../vizir-compiler/tests/fixtures/fonts");
    let manifest: Value =
        serde_json::from_slice(&fs::read(fixture_dir.join("manifest.json")).unwrap()).unwrap();
    let mut faces = serde_json::Map::new();
    let mut mappings = Vec::new();
    for font in manifest["fonts"].as_array().unwrap() {
        let role = font["style"].as_str().unwrap().to_ascii_lowercase();
        faces.insert(role, json!({"sha256":font["sha256"],"face_index":font["face_index"],"weight":font["weight"]}));
        mappings.push(format!(
            "{}={}",
            font["sha256"].as_str().unwrap(),
            fixture_dir.join(font["file"].as_str().unwrap()).display()
        ));
    }
    let p = dir.join("measured-profile.json");
    write(
        &p,
        &json!({"profile":vizir_compiler::TEXT_PROFILE,"engine":vizir_compiler::TEXT_ENGINE,"declared_locale":"en-US","shaping_language":"default","faces":faces}),
    );
    (p, mappings)
}
fn with_fonts(args: &[&str], fonts: &[String]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vizir"));
    command.args(args);
    for font in fonts {
        command.args(["--font", font]);
    }
    command.output().unwrap()
}

#[test]
fn measured_profile_roundtrips_all_cli_stages_and_manifest_fidelity() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path());
    let (profile, fonts) = measured_profile(t.path());
    let saved = t.path().join("compiled.json");
    let refreshed = t.path().join("refreshed.json");
    let output = t.path().join("outline.svg");
    let manifest = t.path().join("manifest.json");
    success(with_fonts(
        &[
            "validate",
            path(&input),
            "--text-profile",
            path(&profile),
            "--theme",
            "azure",
        ],
        &fonts,
    ));
    success(with_fonts(
        &[
            "normalize",
            path(&input),
            "--text-profile",
            path(&profile),
            "--theme",
            "azure",
            "-o",
            path(&saved),
        ],
        &fonts,
    ));
    let persisted: Value = serde_json::from_slice(&fs::read(&saved).unwrap()).unwrap();
    assert_eq!(persisted["format"], vizir_compiler::COMPILED_MIR_FORMAT);
    assert_eq!(
        persisted["context"]["text"],
        serde_json::from_slice::<Value>(&fs::read(&profile).unwrap()).unwrap()
    );
    assert!(!fs::read_to_string(&saved).unwrap().contains(".otf"));
    let direct = success(with_fonts(
        &[
            "lower",
            path(&input),
            "--theme",
            "azure",
            "--text-profile",
            path(&profile),
        ],
        &fonts,
    ));
    let replay = success(with_fonts(&["lower", path(&saved)], &fonts));
    assert_eq!(direct, replay);
    success(with_fonts(
        &[
            "validate",
            path(&saved),
            "--text-profile",
            path(&profile),
            "--theme",
            "azure",
        ],
        &fonts,
    ));
    success(with_fonts(
        &["normalize", path(&saved), "-o", path(&refreshed)],
        &fonts,
    ));
    assert_eq!(
        persisted,
        serde_json::from_slice::<Value>(&fs::read(&refreshed).unwrap()).unwrap()
    );
    success(with_fonts(
        &[
            "render",
            path(&saved),
            "--format",
            "svg",
            "-o",
            path(&output),
            "--manifest",
            path(&manifest),
        ],
        &fonts,
    ));
    let svg = fs::read_to_string(&output).unwrap();
    assert!(svg.contains("<path"));
    assert!(!svg.contains("<text"));
    let report: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(report["compilation_context"], persisted["context"]);
    assert_eq!(
        report["source_context_format"],
        vizir_compiler::COMPILED_MIR_FORMAT
    );
    assert!(
        report["losses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| loss["reason"].as_str().unwrap().contains("selection"))
    );
    let explanation = success(with_fonts(
        &["explain", path(&saved), "--node", "labels/hello"],
        &fonts,
    ));
    assert!(explanation.contains("hir-node:"));
    assert!(explanation.contains("shape-measured-text"));
    // All failures precede transaction staging and preserve existing artifacts.
    let svg_before = fs::read(&output).unwrap();
    let manifest_before = fs::read(&manifest).unwrap();
    rejected(
        run(&[
            "render",
            path(&saved),
            "--format",
            "svg",
            "-o",
            path(&output),
            "--manifest",
            path(&manifest),
        ]),
        "missing font resource",
    );
    assert_eq!(fs::read(&output).unwrap(), svg_before);
    assert_eq!(fs::read(&manifest).unwrap(), manifest_before);
    let mut changed: Value = serde_json::from_slice(&fs::read(&profile).unwrap()).unwrap();
    changed["declared_locale"] = json!("zh-CN");
    let other = t.path().join("other-profile.json");
    write(&other, &changed);
    rejected(
        with_fonts(
            &["normalize", path(&saved), "--text-profile", path(&other)],
            &fonts,
        ),
        "original HIR",
    );
    rejected(
        with_fonts(&["lower", path(&saved), "--theme", "sage"], &fonts),
        "original HIR",
    );
}
