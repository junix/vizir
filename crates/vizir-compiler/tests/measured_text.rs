#[path = "support/text_fixtures.rs"]
mod fixtures;
use serde_json::{Value, json};
use vizir_compiler::{
    FontResources, MaterializationLimits, TextLimits, build_compiled_scene, compile,
    compile_with_context, compile_with_context_with_limits, lower_to_compiled_mir,
    parse_compiled_mir_json, rematerialize_compiled_mir,
};
use vizir_core::{
    Document, PathCommand, Revision, SceneNode, apply_scene_patch, diff_scene, find_scene_node,
};
fn geometry(text: &str) -> Document {
    serde_json::from_value(json!({"version":"0.2","id":"measured","width":600,"height":260,"views":[{"kind":"geometry.scene","id":"g","frame":{"x":10,"y":10,"width":580,"height":240},"children":[{"type":"text","id":"label","x":20,"y":55,"text":text,"font_size":24,"anchor":"start","weight":"regular"}]}]})).unwrap()
}
fn value(d: &Document) -> Value {
    serde_json::to_value(d).unwrap()
}
fn parsed(v: Value) -> Document {
    serde_json::from_value(v).unwrap()
}
fn chart() -> Document {
    serde_json::from_value(json!({"version":"0.2","id":"chart-text","width":640,"height":420,"datasets":{"data":{"key":"id","rows":[{"id":"b","city":"北京","n":3},{"id":"a","city":"上海","n":4}]}},"views":[{"kind":"chart.bar","id":"chart","title":"中文服务 AV","dataset":"data","frame":{"x":0,"y":0,"width":640,"height":420},"category":{"field":"city","label":"城市"},"value":{"field":"n","label":"均值"}}]})).unwrap()
}
fn paths(nodes: &[SceneNode]) -> usize {
    nodes
        .iter()
        .map(|n| match n {
            SceneNode::Text { .. } => panic!("unoutlined text"),
            SceneNode::Path { .. } => 1,
            SceneNode::Group { children, .. } => paths(children),
            _ => 0,
        })
        .sum()
}
#[test]
fn latin_cjk_combining_and_all_weights_are_paths_with_preserved_origins() {
    let (ctx, res) = fixtures::profile();
    for text in [
        "AV office ffi",
        "Cafe\u{301} A\u{30a} x\u{301}",
        "gypq Café Å A\u{30a}\u{301}",
        "中文字体测量",
        "温度 Temperature 23.5",
    ] {
        for weight in ["regular", "medium", "bold"] {
            let mut d = value(&geometry(text));
            d["views"][0]["children"][0]["weight"] = weight.into();
            let c = compile_with_context(&parsed(d), &ctx, &res).unwrap();
            assert_eq!(paths(&c.scene.nodes), 1);
            let SceneNode::Path {
                id,
                origin,
                bounds,
                commands,
                ..
            } = find_scene_node(&c.scene.nodes, "g/label").unwrap()
            else {
                panic!()
            };
            assert_eq!(id, "g/label");
            assert_eq!(origin.hir_node, "label");
            assert!(origin.explanation.contains("original text"));
            assert!(origin.explanation.contains(text));
            assert!(bounds.width > 0. && bounds.height > 0.);
            assert!(!commands.is_empty());
            assert!(
                c.scene
                    .losses
                    .iter()
                    .any(|l| l.reason.contains("selection"))
            );
        }
    }
}
#[test]
fn chart_ranges_use_measured_text_and_preserve_data_order() {
    let (ctx, res) = fixtures::profile();
    let source = chart();
    let old = compile(&source).unwrap();
    let c = compile_with_context(&source, &ctx, &res).unwrap();
    assert_eq!(c.mir.mir.data, old.mir.data);
    let node = find_scene_node(&c.scene.nodes, "chart/axis/y/title").unwrap();
    assert!(matches!(node, SceneNode::Path { .. }));
    assert!(paths(&c.scene.nodes) > 10);
    let a = serde_json::to_value(&old.mir).unwrap();
    let b = serde_json::to_value(&c.mir.mir).unwrap();
    assert_eq!(a["views"][0]["mark"], b["views"][0]["mark"]);
    assert_eq!(
        a["views"][0]["scales"][1]["domain"],
        b["views"][0]["scales"][1]["domain"]
    );
}
#[test]
fn normalized_replay_and_patches_are_deterministic() {
    let (ctx, res) = fixtures::profile();
    let a = compile_with_context(&geometry("中文"), &ctx, &res).unwrap();
    let bytes = serde_json::to_vec(&a.mir).unwrap();
    let decoded = parse_compiled_mir_json(&bytes).unwrap();
    assert_eq!(a.scene, build_compiled_scene(&decoded, &res).unwrap());
    assert_eq!(decoded, rematerialize_compiled_mir(&decoded, &res).unwrap());
    let b = compile_with_context(&geometry("测试"), &ctx, &res).unwrap();
    let patch = diff_scene(&a.scene, &b.scene, Revision(1), Revision(2), "text").unwrap();
    assert_eq!(
        apply_scene_patch(&a.scene, Revision(1), &patch).unwrap().0,
        b.scene
    );
    assert_eq!(
        a.scene,
        compile_with_context(&geometry("中文"), &ctx, &res)
            .unwrap()
            .scene
    );
}
#[test]
fn strict_failure_cases_cover_every_text_source_and_no_silent_fallback() {
    let (ctx, res) = fixtures::profile();
    for text in ["two\nlines", "tab\tlabel", "bad\u{2028}line", "missing 😀"] {
        let d = geometry(text);
        assert!(compile_with_context(&d, &ctx, &res).is_err());
        assert!(lower_to_compiled_mir(&d, &ctx, &res).is_err());
    }
    assert!(
        compile_with_context(&geometry("A"), &ctx, &FontResources::new())
            .unwrap_err()
            .to_string()
            .contains("missing font")
    );
    let mut bad = ctx.clone();
    bad.text.as_mut().unwrap().faces.medium.sha256 =
        bad.text.as_ref().unwrap().faces.regular.sha256.clone();
    assert!(
        compile_with_context(&geometry("A"), &bad, &res)
            .unwrap_err()
            .to_string()
            .contains("weight")
    );
}
#[test]
fn narrow_panels_and_long_categories_fail_without_truncation() {
    let (ctx, res) = fixtures::profile();
    let mut d = value(&chart());
    d["datasets"]["data"]["rows"][0]["city"] =
        "中文中文中文中文中文中文中文中文中文中文中文中文中文中文中文中文中文中文中文中文".into();
    assert!(
        compile_with_context(&parsed(d), &ctx, &res)
            .unwrap_err()
            .to_string()
            .contains("category")
    );
    let mut d = value(&chart());
    d["views"][0]["frame"]["width"] = 180.into();
    d["views"][0]["title"] = "Long long long long long long title".into();
    assert!(compile_with_context(&parsed(d), &ctx, &res).is_err());
}
#[test]
fn slash_ids_do_not_collide_with_outlining() {
    let (ctx, res) = fixtures::profile();
    let mut d = value(&geometry("A"));
    d["views"][0]["children"][0]["id"] = "a".into();
    d["views"][0]["children"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"rect","id":"a/outline","x":200,"y":100,"width":10,"height":10}));
    let c = compile_with_context(&parsed(d), &ctx, &res).unwrap();
    assert!(matches!(
        find_scene_node(&c.scene.nodes, "g/a"),
        Some(SceneNode::Path { .. })
    ));
    assert!(find_scene_node(&c.scene.nodes, "g/a/outline").is_some());
}
#[test]
fn declared_locale_is_provenance_and_active_language_is_rejected() {
    let (mut ctx, res) = fixtures::profile();
    let a = compile_with_context(&geometry("中文"), &ctx, &res).unwrap();
    ctx.text.as_mut().unwrap().declared_locale = "ja-JP".into();
    assert_eq!(
        a.scene,
        compile_with_context(&geometry("中文"), &ctx, &res)
            .unwrap()
            .scene
    );
    ctx.text.as_mut().unwrap().shaping_language = "ja".into();
    assert!(ctx.validate().is_err());
    for tag in ["---", "en--US", "", "x", "en_Us"] {
        let (mut ctx, _) = fixtures::profile();
        ctx.text.as_mut().unwrap().declared_locale = tag.into();
        assert!(ctx.validate().is_err());
    }
}
#[test]
fn all_whole_call_text_budgets_fail_closed() {
    let (ctx, res) = fixtures::profile();
    for field in 0..7 {
        let mut limits = TextLimits::default();
        match field {
            0 => limits.max_label_bytes = 0,
            1 => limits.max_text_bytes = 0,
            2 => limits.max_labels = 0,
            3 => limits.max_glyphs = 0,
            4 => limits.max_outline_commands = 0,
            5 => limits.max_cache_bytes = 0,
            _ => limits.max_output_bytes = 1,
        };
        let e = compile_with_context_with_limits(
            &geometry("AV"),
            &ctx,
            &res,
            MaterializationLimits::default(),
            limits,
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("VIZ-TEXT-0003"), "{field}: {e}");
    }
}
#[test]
fn finite_size_position_and_transform_boundaries_are_explicit() {
    let (ctx, res) = fixtures::profile();
    for size in [0.1, 4096.1] {
        let mut d = value(&geometry("A"));
        d["views"][0]["children"][0]["font_size"] = size.into();
        assert!(
            compile_with_context(&parsed(d), &ctx, &res)
                .unwrap_err()
                .to_string()
                .contains("font size")
        );
    }
    let mut d = value(&geometry("A"));
    d["views"][0]["children"][0]["x"] = 1_000_001.into();
    assert!(compile_with_context(&parsed(d), &ctx, &res).is_err());
    for (scale, accept) in [(1.25, true), (0.00001, false), (0.0, false)] {
        let mut d = value(&geometry("AV"));
        let node = d["views"][0]["children"][0].clone();
        d["views"][0]["children"] = json!([{"type":"group","id":"group","transform":{"scale":{"x":scale,"y":scale}},"children":[node]}]);
        let result = compile_with_context(&parsed(d), &ctx, &res);
        assert_eq!(result.is_ok(), accept, "scale {scale}: {result:?}");
    }
}
#[test]
fn emitted_paths_are_on_svg_precision_grid() {
    let (ctx, res) = fixtures::profile();
    let mut d = value(&geometry("Cafe\u{301} 中文"));
    d["views"][0]["children"][0]["x"] = 20.1234567.into();
    let c = compile_with_context(&parsed(d), &ctx, &res).unwrap();
    let SceneNode::Path { commands, .. } = find_scene_node(&c.scene.nodes, "g/label").unwrap()
    else {
        panic!()
    };
    for command in commands {
        let points = match command {
            PathCommand::Move { to } | PathCommand::Line { to } => vec![to],
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => vec![control1, control2, to],
            PathCommand::Close => vec![],
        };
        for point in points {
            for n in [point.x, point.y] {
                assert_eq!(n, format!("{n:.4}").parse::<f64>().unwrap());
            }
        }
    }
}

#[test]
fn fractional_translation_and_rotation_resolve_at_actual_svg_precision() {
    let (ctx, res) = fixtures::profile();
    let mut d = value(&geometry("AV"));
    let mut node = d["views"][0]["children"][0].clone();
    node["x"] = 100.into();
    node["y"] = 100.into();
    d["views"][0]["children"] = json!([{"type":"group","id":"group","transform":{"translate":{"x":20.123456,"y":20.123456},"rotate_degrees":13.123456,"scale":{"x":1.234567,"y":1.234567}},"children":[node]}]);
    let c = compile_with_context(&parsed(d), &ctx, &res).unwrap();
    let SceneNode::Group { transform, .. } = find_scene_node(&c.scene.nodes, "g/group").unwrap()
    else {
        panic!()
    };
    assert_eq!(transform.translate.x, 20.1235);
    assert_eq!(transform.rotate_degrees, 13.1235);
    assert_eq!(transform.scale.x, 1.2346);
}

#[test]
fn malformed_required_font_tables_and_collections_fail_before_shaping() {
    use sha2::{Digest, Sha256};
    let original = include_bytes!("fixtures/fonts/VizIRFixtureSC-Regular.otf");
    for tag in [b"hhea", b"maxp", b"head", b"hmtx"] {
        let (mut ctx, mut resources) = fixtures::profile();
        let mut bytes = original.to_vec();
        let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
        let record = (0..count)
            .map(|i| 12 + i * 16)
            .find(|i| &bytes[*i..*i + 4] == tag)
            .unwrap();
        let offset =
            u32::from_be_bytes(bytes[record + 8..record + 12].try_into().unwrap()) as usize;
        let length =
            u32::from_be_bytes(bytes[record + 12..record + 16].try_into().unwrap()) as usize;
        if tag == b"hmtx" {
            bytes[record + 12..record + 16].copy_from_slice(&0u32.to_be_bytes());
        } else {
            bytes[offset..offset + length].fill(0);
        }
        let hash = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        ctx.text.as_mut().unwrap().faces.regular.sha256 = hash.clone();
        resources.insert(&hash, bytes).unwrap();
        assert!(
            compile_with_context(&geometry("AV"), &ctx, &resources).is_err(),
            "{tag:?}"
        );
    }
    for count in [0u32, 33, u32::MAX] {
        let (mut ctx, mut resources) = fixtures::profile();
        let mut bytes = b"ttcf\0\x01\0\0".to_vec();
        bytes.extend_from_slice(&count.to_be_bytes());
        let hash = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        ctx.text.as_mut().unwrap().faces.regular.sha256 = hash.clone();
        resources.insert(&hash, bytes).unwrap();
        assert!(compile_with_context(&geometry("A"), &ctx, &resources).is_err());
    }
}

#[test]
fn profile_integer_spellings_match_json_schema_and_limits() {
    let (ctx, _) = fixtures::profile();
    let text = ctx.text.unwrap();
    let mut json = serde_json::to_value(&text).unwrap();
    json["faces"]["regular"]["face_index"] = json!(0.0);
    json["faces"]["regular"]["weight"] = json!(400.0);
    assert_eq!(
        serde_json::from_value::<vizir_compiler::TextContext>(json.clone()).unwrap(),
        text
    );
    for bad in [json!(0.5), json!(-1), json!(4294967296u64)] {
        json["faces"]["regular"]["face_index"] = bad;
        assert!(serde_json::from_value::<vizir_compiler::TextContext>(json.clone()).is_err());
    }
}

#[test]
fn valid_thin_glyph_cannot_disappear_alone_or_inside_a_label() {
    let (mut ctx, mut resources) = fixtures::profile();
    let manifest: Value =
        serde_json::from_str(include_str!("fixtures/fonts/manifest.json")).unwrap();
    let hash = manifest["precision_fixture"]["sha256"].as_str().unwrap();
    resources
        .insert(
            hash,
            include_bytes!("fixtures/fonts/VizIRPrecisionFixture-Regular.otf").to_vec(),
        )
        .unwrap();
    ctx.text.as_mut().unwrap().faces.regular.sha256 = hash.into();
    for text in ["A", "AB"] {
        let mut d = value(&geometry(text));
        d["views"][0]["children"][0]["font_size"] = 0.25.into();
        let error = compile_with_context(&parsed(d), &ctx, &resources)
            .unwrap_err()
            .to_string();
        assert!(error.contains("filled two-dimensional ink"), "{error}");
    }
    assert!(compile_with_context(&geometry(" "), &ctx, &resources).is_ok());
}
