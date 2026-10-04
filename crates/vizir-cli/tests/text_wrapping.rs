use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CONTENT: &str = "Measured geometry wraps across several lines.\n\n中文测试";

fn run(args: &[&str], fonts: &[String]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vizir"));
    command.args(args);
    for font in fonts {
        command.args(["--font", font]);
    }
    command.output().unwrap()
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
fn read(p: &Path) -> Value {
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}
fn source(dir: &Path, content: &str) -> PathBuf {
    let p = dir.join("source.json");
    write(
        &p,
        &json!({"version":"0.2","id":"wrapping-cli","width":640,"height":400,"views":[
        {"kind":"geometry.scene","id":"labels","frame":{"x":0,"y":0,"width":640,"height":400},
        "children":[{"type":"text","id":"hello","x":20,"y":50,"font_size":16,"anchor":"start","text":content}]}]}),
    );
    p
}
fn policy() -> Value {
    json!({"profile":vizir_compiler::TEXT_LAYOUT_PROFILE,"engine":vizir_compiler::TEXT_LAYOUT_ENGINE,"targets":[
        {"view_id":"labels","node_id":"hello","max_width":140.0,"max_lines":12,"line_height":32.0}]})
}
fn layout(dir: &Path) -> PathBuf {
    let p = dir.join("layout.json");
    write(&p, &policy());
    p
}
fn measured(dir: &Path) -> (PathBuf, Vec<String>) {
    let fixture_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../vizir-compiler/tests/fixtures/fonts");
    let manifest = read(&fixture_dir.join("manifest.json"));
    let mut faces = serde_json::Map::new();
    let mut mappings = Vec::new();
    for font in manifest["fonts"].as_array().unwrap() {
        faces.insert(font["style"].as_str().unwrap().to_ascii_lowercase(), json!({"sha256":font["sha256"],"face_index":font["face_index"],"weight":font["weight"]}));
        mappings.push(format!(
            "{}={}",
            font["sha256"].as_str().unwrap(),
            fixture_dir.join(font["file"].as_str().unwrap()).display()
        ));
    }
    let p = dir.join("profile.json");
    write(
        &p,
        &json!({"profile":vizir_compiler::TEXT_PROFILE,"engine":vizir_compiler::TEXT_ENGINE,
        "declared_locale":"en-US","shaping_language":"default","faces":faces}),
    );
    (p, mappings)
}

#[test]
fn wrapped_context_roundtrips_all_commands_without_persisting_paths() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), CONTENT);
    let layout = layout(t.path());
    let (profile, fonts) = measured(t.path());
    let saved = t.path().join("compiled.json");
    let refreshed = t.path().join("refreshed.json");
    let svg = t.path().join("wrapped.svg");
    let manifest = t.path().join("wrapped.render.json");
    success(run(
        &[
            "validate",
            path(&input),
            "--theme",
            "azure",
            "--text-profile",
            path(&profile),
            "--text-layout",
            path(&layout),
        ],
        &fonts,
    ));
    success(run(
        &[
            "normalize",
            path(&input),
            "--theme",
            "azure",
            "--text-profile",
            path(&profile),
            "--text-layout",
            path(&layout),
            "-o",
            path(&saved),
        ],
        &fonts,
    ));
    let compiled = read(&saved);
    assert_eq!(compiled["format"], vizir_compiler::COMPILED_MIR_FORMAT);
    assert_eq!(compiled["context"]["text_layout"], read(&layout));
    assert_eq!(compiled["context"]["text"], read(&profile));
    assert_eq!(compiled["mir"]["views"][0]["children"][0]["text"], CONTENT);
    let serialized = fs::read_to_string(&saved).unwrap();
    assert!(!serialized.contains(path(t.path())));
    assert!(!serialized.contains(".otf"));
    let direct = success(run(
        &[
            "lower",
            path(&input),
            "--theme",
            "azure",
            "--text-profile",
            path(&profile),
            "--text-layout",
            path(&layout),
        ],
        &fonts,
    ));
    let replay = success(run(&["lower", path(&saved)], &fonts));
    assert_eq!(direct, replay);
    success(run(
        &["validate", path(&saved), "--text-layout", path(&layout)],
        &fonts,
    ));
    success(run(
        &[
            "normalize",
            path(&saved),
            "--text-layout",
            path(&layout),
            "-o",
            path(&refreshed),
        ],
        &fonts,
    ));
    assert_eq!(read(&refreshed), compiled);
    success(run(
        &[
            "render",
            path(&saved),
            "--text-layout",
            path(&layout),
            "--format",
            "svg",
            "-o",
            path(&svg),
            "--manifest",
            path(&manifest),
        ],
        &fonts,
    ));
    let rendered = fs::read_to_string(&svg).unwrap();
    assert!(rendered.contains("<path"));
    assert!(!rendered.contains("<text"));
    assert_eq!(read(&manifest)["compilation_context"], compiled["context"]);
    let explanation = success(run(
        &[
            "explain",
            path(&saved),
            "--text-layout",
            path(&layout),
            "--node",
            "labels/hello",
        ],
        &fonts,
    ));
    assert!(explanation.contains("hir-node: hello"));
    assert!(explanation.contains(CONTENT));
}

#[test]
fn every_command_requires_measured_context_for_layout() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), "Hello");
    let layout = layout(t.path());
    for command in ["validate", "normalize", "lower"] {
        rejected(
            run(
                &[command, path(&input), "--text-layout", path(&layout)],
                &[],
            ),
            "--text-layout requires",
        );
    }
    let output = t.path().join("out.svg");
    rejected(
        run(
            &[
                "render",
                path(&input),
                "--text-layout",
                path(&layout),
                "--format",
                "svg",
                "-o",
                path(&output),
            ],
            &[],
        ),
        "--text-layout requires",
    );
    assert!(!output.exists());
    rejected(
        run(
            &[
                "explain",
                path(&input),
                "--text-layout",
                path(&layout),
                "--node",
                "labels/hello",
            ],
            &[],
        ),
        "--text-layout requires",
    );
}

#[test]
fn persisted_layout_cannot_be_added_or_changed_before_font_reads() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), "Hello");
    let layout = layout(t.path());
    let (profile, fonts) = measured(t.path());
    let saved = t.path().join("compiled.json");
    success(run(
        &[
            "normalize",
            path(&input),
            "--text-profile",
            path(&profile),
            "-o",
            path(&saved),
        ],
        &fonts,
    ));
    let invalid_font = vec![format!(
        "{}={}",
        "0".repeat(64),
        t.path().join("never-open.ttf").display()
    )];
    rejected(
        run(
            &["validate", path(&saved), "--text-layout", path(&layout)],
            &invalid_font,
        ),
        "original HIR",
    );
    success(run(
        &[
            "normalize",
            path(&input),
            "--theme",
            "azure",
            "-o",
            path(&saved),
        ],
        &[],
    ));
    rejected(
        run(
            &["lower", path(&saved), "--text-layout", path(&layout)],
            &invalid_font,
        ),
        "original HIR",
    );
    success(run(
        &[
            "normalize",
            path(&input),
            "--text-profile",
            path(&profile),
            "--text-layout",
            path(&layout),
            "-o",
            path(&saved),
        ],
        &fonts,
    ));
    let mut changed = policy();
    changed["targets"][0]["max_width"] = 180.0.into();
    write(&layout, &changed);
    rejected(
        run(
            &["normalize", path(&saved), "--text-layout", path(&layout)],
            &invalid_font,
        ),
        "original HIR",
    );
}

#[test]
fn layout_json_is_strict_bounded_and_explicit() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), "Hello");
    let layout = layout(t.path());
    for bad in [
        json!({"profile":vizir_compiler::TEXT_LAYOUT_PROFILE,"engine":vizir_compiler::TEXT_LAYOUT_ENGINE,"targets":[],"path":"/etc/passwd"}),
        {
            let mut p = policy();
            p["targets"][0]["font_path"] = "never-open.ttf".into();
            p
        },
    ] {
        write(&layout, &bad);
        rejected(
            run(
                &["validate", path(&input), "--text-layout", path(&layout)],
                &[],
            ),
            "unknown field",
        );
    }
    for raw in [
        format!(
            "{{\"profile\":\"{}\",\"profile\":\"{}\",\"targets\":[]}}",
            vizir_compiler::TEXT_LAYOUT_PROFILE,
            vizir_compiler::TEXT_LAYOUT_PROFILE
        ),
        serde_json::to_string(&policy()).unwrap().replace(
            "\"node_id\":\"hello\"",
            "\"node_id\":\"hello\",\"node_id\":\"hello\"",
        ),
    ] {
        fs::write(&layout, raw).unwrap();
        rejected(
            run(
                &["validate", path(&input), "--text-layout", path(&layout)],
                &[],
            ),
            "duplicate",
        );
    }
    fs::File::create(&layout)
        .unwrap()
        .set_len(32 * 1024 * 1024 + 1)
        .unwrap();
    rejected(
        run(
            &["validate", path(&input), "--text-layout", path(&layout)],
            &[],
        ),
        "32 MiB",
    );
    rejected(
        run(
            &["validate", path(&input), "--text-layout", path(t.path())],
            &[],
        ),
        "regular file",
    );
    #[cfg(unix)]
    {
        let fifo = t.path().join("layout-fifo");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        use std::process::Stdio;
        use std::time::{Duration, Instant};
        let mut child = Command::new(env!("CARGO_BIN_EXE_vizir"))
            .args(["validate", path(&input), "--text-layout", path(&fifo)])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("layout FIFO read blocked");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        rejected(child.wait_with_output().unwrap(), "regular file");
    }
}

#[test]
fn failures_preserve_outputs_and_layout_sources_including_aliases() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), CONTENT);
    let layout = layout(t.path());
    let original = fs::read(&layout).unwrap();
    let (profile, fonts) = measured(t.path());
    for command in ["normalize", "lower"] {
        rejected(
            run(
                &[
                    command,
                    path(&input),
                    "--text-profile",
                    path(&profile),
                    "--text-layout",
                    path(&layout),
                    "-o",
                    path(&layout),
                ],
                &fonts,
            ),
            "VIZ-PATH-0001",
        );
        assert_eq!(fs::read(&layout).unwrap(), original);
    }
    let svg = t.path().join("out.svg");
    let manifest = t.path().join("out.render.json");
    fs::write(&svg, b"prior SVG").unwrap();
    fs::write(&manifest, b"prior manifest").unwrap();
    rejected(
        run(
            &[
                "render",
                path(&input),
                "--text-profile",
                path(&profile),
                "--text-layout",
                path(&layout),
                "--format",
                "svg",
                "-o",
                path(&svg),
                "--manifest",
                path(&layout),
            ],
            &fonts,
        ),
        "VIZ-PATH-0001",
    );
    assert_eq!(fs::read(&layout).unwrap(), original);
    #[cfg(unix)]
    {
        let symlink = t.path().join("layout-symlink.json");
        std::os::unix::fs::symlink(&layout, &symlink).unwrap();
        let hardlink = t.path().join("layout-hardlink.json");
        fs::hard_link(&layout, &hardlink).unwrap();
        for alias in [&symlink, &hardlink] {
            rejected(
                run(
                    &[
                        "lower",
                        path(&input),
                        "--text-profile",
                        path(&profile),
                        "--text-layout",
                        path(&layout),
                        "-o",
                        path(alias),
                    ],
                    &fonts,
                ),
                "VIZ-PATH-0001",
            );
        }
    }
    let mut too_short = policy();
    too_short["targets"][0]["max_lines"] = 1.into();
    write(&layout, &too_short);
    rejected(
        run(
            &[
                "render",
                path(&input),
                "--text-profile",
                path(&profile),
                "--text-layout",
                path(&layout),
                "--format",
                "svg",
                "-o",
                path(&svg),
                "--manifest",
                path(&manifest),
            ],
            &fonts,
        ),
        "VIZ-",
    );
    assert_eq!(fs::read(&svg).unwrap(), b"prior SVG");
    assert_eq!(fs::read(&manifest).unwrap(), b"prior manifest");
}

#[test]
fn absent_or_null_layout_remains_omitted_and_strict_in_compiled_context() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), "Hello");
    let (profile, fonts) = measured(t.path());
    let output = success(run(
        &["normalize", path(&input), "--text-profile", path(&profile)],
        &fonts,
    ));
    let mut compiled: Value = serde_json::from_str(&output).unwrap();
    assert!(compiled["context"].get("text_layout").is_none());
    compiled["context"]["text_layout"] = Value::Null;
    let parsed =
        vizir_compiler::parse_compiled_mir_json(&serde_json::to_vec(&compiled).unwrap()).unwrap();
    assert!(
        serde_json::to_value(parsed).unwrap()["context"]
            .get("text_layout")
            .is_none()
    );
    compiled["context"]["text_layout"] = policy();
    compiled["context"]["text_layout"]["targets"][0]["discarded"] = true.into();
    assert!(
        vizir_compiler::parse_compiled_mir_json(&serde_json::to_vec(&compiled).unwrap())
            .unwrap_err()
            .to_string()
            .contains("unknown field")
    );
    compiled["context"]["text_layout"] = policy();
    compiled["context"]["text"] = Value::Null;
    assert!(
        vizir_compiler::parse_compiled_mir_json(&serde_json::to_vec(&compiled).unwrap())
            .unwrap_err()
            .to_string()
            .contains("requires measured text")
    );
}

#[test]
fn strict_layout_parser_checks_policy_identity_and_numeric_byte_budgets() {
    use vizir_compiler::parse_text_layout_context_json;
    let parsed = parse_text_layout_context_json(&serde_json::to_vec(&policy()).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), policy());
    let mut cases = Vec::new();
    for field in ["profile", "engine"] {
        let mut p = policy();
        p[field] = "unsupported".into();
        cases.push(p);
    }
    for (field, value) in [
        ("max_width", json!(0.0)),
        ("max_width", json!(1_000_001.0)),
        ("line_height", json!(0.0)),
        ("line_height", json!(1_000_001.0)),
        ("max_lines", json!(0)),
        ("max_lines", json!(257)),
        ("max_lines", json!(1.5)),
        ("view_id", json!("")),
        ("node_id", json!("a".repeat(257))),
        ("node_id", json!("中".repeat(100))),
    ] {
        let mut p = policy();
        p["targets"][0][field] = value;
        cases.push(p);
    }
    let mut empty = policy();
    empty["targets"] = json!([]);
    cases.push(empty);
    let mut duplicate = policy();
    let mut second = duplicate["targets"][0].clone();
    second["max_width"] = 160.0.into();
    duplicate["targets"].as_array_mut().unwrap().push(second);
    cases.push(duplicate);
    for invalid in cases {
        assert!(
            parse_text_layout_context_json(&serde_json::to_vec(&invalid).unwrap()).is_err(),
            "{invalid}"
        );
    }
    assert!(
        parse_text_layout_context_json(&vec![
            b' ';
            vizir_compiler::MAX_COMPILED_MIR_JSON_BYTES + 1
        ])
        .unwrap_err()
        .to_string()
        .contains("32 MiB")
    );
}

#[test]
fn soft_hyphen_failure_preserves_rendered_outputs_and_leaves_no_staging() {
    let t = tempfile::tempdir().unwrap();
    let input = source(t.path(), CONTENT);
    let layout = layout(t.path());
    let (profile, fonts) = measured(t.path());
    let svg = t.path().join("wrapped.svg");
    let manifest = t.path().join("wrapped.render.json");
    let args = [
        "render",
        path(&input),
        "--text-profile",
        path(&profile),
        "--text-layout",
        path(&layout),
        "--format",
        "svg",
        "-o",
        path(&svg),
        "--manifest",
        path(&manifest),
    ];
    success(run(&args, &fonts));
    let svg_before = fs::read(&svg).unwrap();
    let manifest_before = fs::read(&manifest).unwrap();
    assert!(String::from_utf8_lossy(&svg_before).contains("<path"));
    assert_eq!(
        read(&manifest)["compilation_context"]["text_layout"],
        policy()
    );
    let entries = || {
        fs::read_dir(t.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<std::collections::BTreeSet<_>>()
    };
    let entries_before = entries();
    assert!(
        !entries_before
            .iter()
            .any(|name| name.to_string_lossy().starts_with(".vizir-"))
    );
    // The leading CJK characters distinguish byte index 9 from character index 5.
    source(t.path(), "中文 ab\u{00ad}cd");
    let failure = run(&args, &fonts);
    let diagnostic = String::from_utf8_lossy(&failure.stderr);
    assert!(diagnostic.contains("U+00AD"), "{diagnostic}");
    assert!(diagnostic.contains("byte 9"), "{diagnostic}");
    rejected(failure, "soft hyphen");
    assert_eq!(fs::read(&svg).unwrap(), svg_before);
    assert_eq!(fs::read(&manifest).unwrap(), manifest_before);
    assert_eq!(entries(), entries_before);
}
