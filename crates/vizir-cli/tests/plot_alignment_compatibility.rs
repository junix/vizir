//! Exact byte and additive schema snapshots captured from published 965ebd3.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn run(args: &[&str]) -> Vec<u8> {
    let output = Command::new(env!("CARGO_BIN_EXE_vizir"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/plot-alignment-legacy-bytes.json")).unwrap()
}
fn value_digest(value: &Value) -> String {
    digest(&serde_json::to_vec(value).unwrap())
}

#[test]
fn published_versions_and_replays_retain_exact_output_bytes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().unwrap();
    for case in fixture()["cases"].as_array().unwrap() {
        let name = case["source"].as_str().unwrap();
        let mut input = root.join(name);
        if let Some(expected) = case["hir_sha256"].as_str() {
            let bytes = run(&["compose", path(&input)]);
            assert_eq!(digest(&bytes), expected, "{name} HIR");
            input = temp.path().join("source.json");
            fs::write(&input, bytes).unwrap();
        }
        let mut options = Vec::new();
        let mut font_options = Vec::new();
        if let Some(profile) = case["profile"].as_str() {
            let font_dir = root
                .join("crates/vizir-compiler/tests/fixtures")
                .join(profile);
            let manifest: Value =
                serde_json::from_slice(&fs::read(font_dir.join("manifest.json")).unwrap()).unwrap();
            let mut faces = serde_json::Map::new();
            for font in manifest["fonts"].as_array().unwrap() {
                faces.insert(font["style"].as_str().unwrap().to_ascii_lowercase(), json!({"sha256":font["sha256"],"face_index":font["face_index"],"weight":font["weight"]}));
                font_options.extend([
                    "--font".to_owned(),
                    format!(
                        "{}={}",
                        font["sha256"].as_str().unwrap(),
                        font_dir.join(font["file"].as_str().unwrap()).display()
                    ),
                ]);
            }
            let profile = temp.path().join("profile.json");
            fs::write(&profile, serde_json::to_vec(&json!({"profile":vizir_compiler::TEXT_PROFILE,"engine":vizir_compiler::TEXT_ENGINE,"declared_locale":"zh-CN","shaping_language":"default","faces":faces})).unwrap()).unwrap();
            options.extend(font_options.clone());
            options.extend(["--text-profile".to_owned(), path(&profile).to_owned()]);
        }
        if let Some(theme) = case["theme"].as_str() {
            options.extend(["--theme".to_owned(), theme.to_owned()]);
        }
        let mut args = vec!["normalize", path(&input)];
        args.extend(options.iter().map(String::as_str));
        let normalized = run(&args);
        assert_eq!(
            digest(&normalized),
            case["normalize_sha256"].as_str().unwrap(),
            "{name} normalize"
        );
        let mut persisted: Value = serde_json::from_slice(&normalized).unwrap();
        if persisted.get("format").is_none() {
            persisted = json!({"format":"vizir-compiled-mir/1","context":{},"mir":persisted});
        }
        let replay = temp.path().join("replay.json");
        fs::write(&replay, serde_json::to_vec(&persisted).unwrap()).unwrap();
        for (source, options, prefix) in
            [(&input, &options, ""), (&replay, &font_options, "replay_")]
        {
            for command in ["normalize", "lower"] {
                let mut args = vec![command, path(source)];
                args.extend(options.iter().map(String::as_str));
                let bytes = run(&args);
                assert_eq!(
                    digest(&bytes),
                    case[format!("{prefix}{command}_sha256")].as_str().unwrap(),
                    "{name} {prefix}{command}"
                );
            }
            let svg = temp.path().join("output.svg");
            let mut args = vec!["render", path(source), "--format", "svg", "-o", path(&svg)];
            args.extend(options.iter().map(String::as_str));
            run(&args);
            assert_eq!(
                digest(&fs::read(svg).unwrap()),
                case[format!("{prefix}svg_sha256")].as_str().unwrap(),
                "{name} {prefix}SVG"
            );
        }
    }
}

#[test]
fn existing_schema_definitions_and_version_variants_remain_byte_identical() {
    for baseline in fixture()["schemas"].as_array().unwrap() {
        let kind = baseline["kind"].as_str().unwrap();
        let mut current: Value = serde_json::from_slice(&run(&["schema", kind])).unwrap();
        let mut definitions = current.as_object_mut().unwrap().remove("$defs").unwrap();
        for (location, hashes) in baseline["unions"].as_object().unwrap() {
            let container = if location == "root" {
                &mut current
            } else {
                &mut definitions[location]
            };
            let variants = container.as_object_mut().unwrap().remove("oneOf").unwrap();
            let variants = variants.as_array().unwrap();
            assert!(
                variants.len() > hashes.as_array().unwrap().len(),
                "{kind} must add a version variant"
            );
            for (index, expected) in hashes.as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    value_digest(&variants[index]),
                    expected.as_str().unwrap(),
                    "{kind} {location} old variant {index}"
                );
            }
        }
        assert_eq!(
            value_digest(&current),
            baseline["root_sha256"].as_str().unwrap(),
            "{kind} root contract"
        );
        for (name, expected) in baseline["definitions"].as_object().unwrap() {
            assert!(definitions.get(name).is_some(), "{kind} removed {name}");
            assert_eq!(
                value_digest(&definitions[name]),
                expected.as_str().unwrap(),
                "{kind} old definition {name}"
            );
        }
    }
}
