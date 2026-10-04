use serde_json::{Value, json};
use vizir_compiler::{
    COMPILED_MIR_FORMAT, CompilationContext, CompiledMir, FontResources,
    MAX_COMPILED_MIR_JSON_BYTES, ThemeContext, build_compiled_scene, compile, compile_with_context,
    compile_with_theme, compiled_mir_schema, parse_compiled_mir_json, parse_text_context_json,
    rematerialize_compiled_mir,
};
use vizir_core::{Document, MirView};

fn source() -> Document {
    serde_json::from_value(json!({"version":"0.2","id":"context","width":640,"height":400,
        "datasets":{"data":{"key":"id","rows":[{"id":"a","x":1.0,"y":2.0},{"id":"b","x":2.0,"y":4.0}]}},
        "views":[{"kind":"chart.line","id":"chart","title":"Context","dataset":"data",
        "frame":{"x":0,"y":0,"width":640,"height":400},"x":{"field":"x"},"y":{"field":"y"}}]})).unwrap()
}

#[test]
fn empty_and_theme_only_context_use_unchanged_legacy_artifacts() {
    let document = source();
    let resources = FontResources::new();
    let old = compile(&document).unwrap();
    let new = compile_with_context(&document, &CompilationContext::new(), &resources).unwrap();
    assert_eq!(old.mir, new.mir.mir);
    assert_eq!(old.scene, new.scene);
    assert_eq!(new.mir.format, COMPILED_MIR_FORMAT);
    assert_eq!(
        serde_json::to_value(&new.mir).unwrap()["context"],
        json!({})
    );
    let old = compile_with_theme(&document, "azure").unwrap();
    let context = CompilationContext::new().with_theme(ThemeContext::resolve("azure").unwrap());
    let new = compile_with_context(&document, &context, &resources).unwrap();
    assert_eq!(old.mir.mir, new.mir.mir);
    assert_eq!(old.scene, new.scene);
    let adapted = CompiledMir::from_themed(old.mir.clone()).unwrap();
    assert_eq!(adapted, new.mir);
    assert_eq!(old.mir.format, "vizir-themed-mir/1");
    assert!(
        serde_json::to_value(old.mir)
            .unwrap()
            .get("context")
            .is_none()
    );
}

#[test]
fn compiled_roundtrip_and_refresh_retain_exact_context_and_explicit_scales() {
    let resources = FontResources::new();
    let context = CompilationContext::new().with_theme(ThemeContext::resolve("sage-dark").unwrap());
    let compilation = compile_with_context(&source(), &context, &resources).unwrap();
    let bytes = serde_json::to_vec(&compilation.mir).unwrap();
    let decoded = parse_compiled_mir_json(&bytes).unwrap();
    assert_eq!(compilation.mir, decoded);
    assert_eq!(
        compilation.scene,
        build_compiled_scene(&decoded, &resources).unwrap()
    );
    let mut edited: Value = serde_json::from_slice(&bytes).unwrap();
    edited["mir"]["data"]["data/data"]["operator"]["rows"][0]["y"] = json!(3.0);
    let edited: CompiledMir = serde_json::from_value(edited).unwrap();
    assert!(build_compiled_scene(&edited, &resources).is_err());
    let refreshed = rematerialize_compiled_mir(&edited, &resources).unwrap();
    assert_eq!(refreshed.context, context);
    let (MirView::Chart(before), MirView::Chart(after)) =
        (&decoded.mir.views[0], &refreshed.mir.views[0])
    else {
        panic!()
    };
    assert_eq!(before.scales, after.scales);
    assert_eq!(
        rematerialize_compiled_mir(&refreshed, &resources).unwrap(),
        refreshed
    );
}

#[test]
fn strict_new_boundary_rejects_duplicates_discarded_fields_and_wrong_identity() {
    let resources = FontResources::new();
    let compiled = compile_with_context(
        &source(),
        &CompilationContext::new().with_theme(ThemeContext::resolve("azure").unwrap()),
        &resources,
    )
    .unwrap();
    let original = serde_json::to_value(&compiled.mir).unwrap();
    for mutation in ["format", "theme", "pin", "unknown", "discarded", "versions"] {
        let mut value = original.clone();
        match mutation {
            "format" => value["format"] = json!("vizir-compiled-mir/99"),
            "theme" => value["context"]["theme"]["defaults"]["ink"] = json!("#000000"),
            "pin" => value["context"]["theme"]["registry_revision"] = json!("bad"),
            "unknown" => value["context"]["resource_path"] = json!("/tmp/font.ttf"),
            "discarded" => value["mir"]["views"][0]["discard_me"] = json!(true),
            "versions" => value["mir"]["source_hir_version"] = json!("0.1"),
            _ => unreachable!(),
        }
        assert!(
            parse_compiled_mir_json(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{mutation}"
        );
    }
    let duplicate = serde_json::to_string(&original).unwrap().replacen(
        '{',
        "{\"format\":\"vizir-compiled-mir/1\",",
        1,
    );
    assert!(
        parse_compiled_mir_json(duplicate.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
    let duplicate_metadata =
        serde_json::to_string(&original)
            .unwrap()
            .replacen("\"x\":1.0", "\"x\":1.0,\"x\":1.0", 1);
    assert_ne!(
        duplicate_metadata,
        serde_json::to_string(&original).unwrap()
    );
    assert!(parse_compiled_mir_json(duplicate_metadata.as_bytes()).is_err());
    assert!(
        parse_compiled_mir_json(&vec![b' '; MAX_COMPILED_MIR_JSON_BYTES + 1])
            .unwrap_err()
            .to_string()
            .contains("32 MiB")
    );
}

#[test]
fn schema_is_separate_closed_and_pins_both_contexts() {
    let schema = compiled_mir_schema();
    assert_eq!(schema["properties"]["format"]["const"], COMPILED_MIR_FORMAT);
    assert_eq!(
        schema["$defs"]["CompilationContext"]["additionalProperties"],
        false
    );
    assert_eq!(
        schema["$defs"]["ThemeContext"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        14
    );
    assert_eq!(
        schema["$defs"]["TextContext"]["properties"]["engine"]["const"],
        vizir_compiler::TEXT_ENGINE
    );
    assert_eq!(
        schema,
        serde_json::from_str::<Value>(include_str!("../../../schemas/compiled-mir.schema.json"))
            .unwrap()
    );
}

#[test]
fn profile_wire_is_strict_and_requires_identity_only() {
    let bad = br#"{"profile":"vizir-text-outlines/1","profile":"vizir-text-outlines/1"}"#;
    assert!(
        parse_text_context_json(bad)
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
    assert!(
        parse_text_context_json(&vec![b' '; MAX_COMPILED_MIR_JSON_BYTES + 1])
            .unwrap_err()
            .to_string()
            .contains("32 MiB")
    );
}

fn measured_context() -> (CompilationContext, FontResources) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fonts");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    let mut faces = serde_json::Map::new();
    let mut resources = FontResources::new();
    for font in manifest["fonts"].as_array().unwrap() {
        let hash = font["sha256"].as_str().unwrap();
        resources
            .insert(
                hash,
                std::fs::read(dir.join(font["file"].as_str().unwrap())).unwrap(),
            )
            .unwrap();
        faces.insert(
            font["style"].as_str().unwrap().to_ascii_lowercase(),
            json!({"sha256":hash,"face_index":font["face_index"],"weight":font["weight"]}),
        );
    }
    let text= parse_text_context_json(&serde_json::to_vec(&json!({"profile":vizir_compiler::TEXT_PROFILE,"engine":vizir_compiler::TEXT_ENGINE,"declared_locale":"en-US","shaping_language":"default","faces":faces})).unwrap()).unwrap();
    (CompilationContext::new().with_text(text), resources)
}

#[test]
fn measured_replay_and_refresh_require_exact_resources_and_preserve_ranges() {
    let (context, resources) = measured_context();
    let compiled = compile_with_context(&source(), &context, &resources).unwrap();
    let persisted = parse_compiled_mir_json(&serde_json::to_vec(&compiled.mir).unwrap()).unwrap();
    assert_eq!(
        compiled.scene,
        build_compiled_scene(&persisted, &resources).unwrap()
    );
    assert!(
        build_compiled_scene(&persisted, &FontResources::new())
            .unwrap_err()
            .to_string()
            .contains("missing font resource")
    );
    assert!(rematerialize_compiled_mir(&persisted, &FontResources::new()).is_err());
    let mut edited = serde_json::to_value(&persisted).unwrap();
    edited["mir"]["data"]["data/data"]["operator"]["rows"][0]["y"] = json!(3.0);
    let edited: CompiledMir = serde_json::from_value(edited).unwrap();
    let refreshed = rematerialize_compiled_mir(&edited, &resources).unwrap();
    assert_eq!(refreshed.context, context);
    let (MirView::Chart(before), MirView::Chart(after)) =
        (&persisted.mir.views[0], &refreshed.mir.views[0])
    else {
        panic!()
    };
    assert_eq!(before.scales, after.scales);
    let value = serde_json::to_value(&refreshed).unwrap();
    assert_eq!(value["context"], serde_json::to_value(&context).unwrap());
}

#[test]
fn public_normalize_checks_geometry_labels_before_claiming_measured_context() {
    let (context, resources) = measured_context();
    let document: Document = serde_json::from_value(
        json!({"version":"0.1","id":"bad-label","width":640,"height":400,"views":[
        {"kind":"geometry.scene","id":"geometry","frame":{"x":0,"y":0,"width":640,"height":400},
         "children":[{"type":"text","id":"bad","x":20,"y":50,"text":"unsupported\nnew line"}]}]}),
    )
    .unwrap();
    let error = vizir_compiler::lower_to_compiled_mir(&document, &context, &resources)
        .unwrap_err()
        .to_string();
    assert!(error.contains("single-line"), "{error}");
}
