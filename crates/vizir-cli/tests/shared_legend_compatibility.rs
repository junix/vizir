//! Byte fixtures captured from the published fa14dc7 executable before 0.8.
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

#[test]
fn all_published_native_versions_retain_exact_hir_mir_scene_svg_bytes() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/shared-legend-legacy-bytes.json")).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let name = case["source"].as_str().unwrap();
        let mut input = root.join(name);
        if let Some(expected) = case["hir_sha256"].as_str() {
            let bytes = run(&["compose", path(&input)]);
            assert_eq!(digest(&bytes), expected, "{name} HIR");
            input = temp.path().join("source.json");
            fs::write(&input, bytes).unwrap();
        }
        let mut options = Vec::new();
        if let Some(fixture) = case["profile"].as_str() {
            let font_dir = root
                .join("crates/vizir-compiler/tests/fixtures")
                .join(fixture);
            let manifest: Value =
                serde_json::from_slice(&fs::read(font_dir.join("manifest.json")).unwrap()).unwrap();
            let mut faces = serde_json::Map::new();
            for font in manifest["fonts"].as_array().unwrap() {
                faces.insert(font["style"].as_str().unwrap().to_ascii_lowercase(), json!({"sha256":font["sha256"],"face_index":font["face_index"],"weight":font["weight"]}));
                options.extend([
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
            options.extend(["--text-profile".to_owned(), path(&profile).to_owned()]);
        }
        if let Some(theme) = case["theme"].as_str() {
            options.extend(["--theme".to_owned(), theme.to_owned()]);
        }
        for command in ["normalize", "lower"] {
            let mut args = vec![command, path(&input)];
            args.extend(options.iter().map(String::as_str));
            let bytes = run(&args);
            assert_eq!(
                digest(&bytes),
                case[format!("{command}_sha256")].as_str().unwrap(),
                "{name} {command}"
            );
        }
        let svg = temp.path().join("output.svg");
        let mut args = vec![
            "render",
            path(&input),
            "--format",
            "svg",
            "--output",
            path(&svg),
        ];
        args.extend(options.iter().map(String::as_str));
        run(&args);
        assert_eq!(
            digest(&fs::read(svg).unwrap()),
            case["svg_sha256"].as_str().unwrap(),
            "{name} SVG"
        );
    }
}
