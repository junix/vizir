use std::path::PathBuf;

use vizir_compiler::{build_scene, compile};
use vizir_core::{
    Revision, Scene2D, VizMir, apply_scene_patch, diff_scene, parse_document, validate_scene,
};

#[test]
fn all_examples_validate_after_scene_and_mir_json_round_trips_and_patch_application() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut paths = Vec::new();
    for directory in std::fs::read_dir(root).unwrap() {
        for file in std::fs::read_dir(directory.unwrap().path()).unwrap() {
            let path = file.unwrap().path();
            if path.to_string_lossy().ends_with(".viz.yaml") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    assert_eq!(paths.len(), 15);
    for path in paths {
        let compilation = compile(&parse_document(&path).unwrap()).unwrap();
        validate_scene(&compilation.scene).unwrap();
        let decoded_scene: Scene2D =
            serde_json::from_slice(&serde_json::to_vec(&compilation.scene).unwrap()).unwrap();
        validate_scene(&decoded_scene).unwrap();
        let decoded_mir: VizMir =
            serde_json::from_slice(&serde_json::to_vec(&compilation.mir).unwrap()).unwrap();
        let rebuilt = build_scene(&decoded_mir).unwrap();
        validate_scene(&rebuilt).unwrap();
        let mut changed = rebuilt.clone();
        changed.width += 1.0;
        let patch = diff_scene(&rebuilt, &changed, Revision(1), Revision(2), "roundtrip").unwrap();
        let (patched, revision) = apply_scene_patch(&rebuilt, Revision(1), &patch).unwrap();
        assert_eq!(revision, Revision(2));
        assert_eq!(patched, changed, "{}", path.display());
        validate_scene(&patched).unwrap();
    }
}
