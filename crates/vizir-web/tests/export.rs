use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use vizir_core::{
    Color, FontWeight, Origin, PathCommand, Point, Rect, Scene2D, SceneNode, TextAnchor,
    Transform2D,
};
use vizir_web::{ExplorerOptions, InteractionContext, render_fragment, render_html};

fn scene() -> Scene2D {
    Scene2D {
        document_id: "mixed".into(),
        width: 100.123456,
        height: 80.7654321,
        background: Color::hex("#FFFFFF"),
        losses: vec![],
        nodes: vec![SceneNode::Text {
            id: "opaque/: 汉字 & <\"\t\n\r".into(),
            bounds: Rect::default(),
            origin: Origin {
                hir_node: "\u{feff}".into(),
                mir_node: "m".into(),
                data_key: Some("".into()),
                data_lineage: vec!["".into(), "a,b".into(), " c ".into()],
                generated_by: "test".into(),
                explanation: "</script><img src=x onerror=alert(1)> & \u{2028}\u{2029}".into(),
            },
            position: Point { x: 10.0, y: 20.0 },
            text: "</script><svg onload=alert(1)> & literal".into(),
            font_size: 12.0,
            anchor: TextAnchor::Start,
            color: Color::hex("#000000"),
            weight: FontWeight::Regular,
        }],
    }
}
fn metadata(html: &str) -> String {
    html.split("data-vizir-metadata=\"\">\n")
        .nth(1)
        .unwrap()
        .split("\n</script>")
        .next()
        .unwrap()
        .into()
}
fn svg(html: &str) -> &str {
    let start = html.find("<svg ").unwrap();
    let end = start + html[start..].find("</svg>").unwrap() + 6;
    &html[start..end]
}

#[test]
fn exact_fractional_canvas_typed_identity_and_hostile_strings_survive() {
    let source = scene();
    let export = render_html(&source, &ExplorerOptions::default()).unwrap();
    let context = InteractionContext::parse(metadata(&export.html).as_bytes()).unwrap();
    assert_eq!(context.nodes[0].origin, *source.nodes[0].origin());
    assert_eq!(context.nodes[0].scene_node_id, source.nodes[0].id());
    let doc = roxmltree::Document::parse(svg(&export.html)).unwrap();
    let root = doc.root_element();
    assert_eq!(
        root.attribute("width").unwrap().parse::<f64>().unwrap(),
        source.width
    );
    assert_eq!(
        root.attribute("height").unwrap().parse::<f64>().unwrap(),
        source.height
    );
    assert_eq!(root.attribute("viewBox"), Some("0 0 100.123456 80.7654321"));
    let text = doc.descendants().find(|n| n.has_tag_name("text")).unwrap();
    assert_eq!(
        text.attribute("data-vizir-scene-id"),
        Some(source.nodes[0].id())
    );
    assert_eq!(text.attribute("id"), Some(context.nodes[0].dom_id.as_str()));
    let bg = doc.descendants().find(|n| n.has_tag_name("rect")).unwrap();
    assert_eq!(bg.attribute("width"), Some("100.123456"));
    assert!(!metadata(&export.html).contains('<'));
    assert!(!metadata(&export.html).contains('&'));
    assert!(!metadata(&export.html).contains('\u{2028}'));
    assert_eq!(source, scene(), "export must not mutate scene");
    assert_eq!(
        export.html,
        render_html(&source, &ExplorerOptions::default())
            .unwrap()
            .html
    );
    assert_eq!(
        export.manifest.scene_sha256,
        Sha256::digest(serde_json::to_vec(&source).unwrap())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
}

#[test]
fn separate_instance_fragments_have_disjoint_complete_id_sets() {
    let source = scene();
    let mut sets = Vec::new();
    for key in ["a", "b"] {
        let export = render_fragment(
            &source,
            &ExplorerOptions {
                instance_key: key.into(),
            },
        )
        .unwrap();
        assert!(!export.html.contains("<!doctype"));
        assert!(!export.html.contains("type=\"module\""));
        let ids: BTreeSet<_> = export
            .html
            .split(" id=\"")
            .skip(1)
            .map(|part| part.split('"').next().unwrap().to_owned())
            .collect();
        assert_eq!(ids.len(), 8); // scene node, title, marker, five controls
        assert!(ids.iter().all(|id| id.starts_with(&format!("vzi-{key}-"))));
        sets.push(ids);
    }
    assert!(sets[0].is_disjoint(&sets[1]));
}

#[test]
fn aliases_cannot_collide_with_reserved_or_hostile_ids() {
    let mut source = scene();
    let node = source.nodes[0].clone();
    source.nodes.clear();
    for id in [
        "vizir-title",
        "vizir-arrow",
        "vzi-main-title",
        "control-picker",
        "__proto__",
        "a/b",
        "a?b",
        "é",
        "e\u{301}",
    ] {
        let mut n = node.clone();
        if let SceneNode::Text { id: old, .. } = &mut n {
            *old = id.into();
        }
        source.nodes.push(n);
    }
    let export = render_html(&source, &ExplorerOptions::default()).unwrap();
    let xml = roxmltree::Document::parse(svg(&export.html)).unwrap();
    let ids: Vec<_> = xml
        .descendants()
        .filter_map(|n| n.attribute("id"))
        .collect();
    assert_eq!(ids.len(), ids.iter().collect::<BTreeSet<_>>().len());
}

#[test]
fn strict_context_rejects_wrong_fields_versions_references_and_duplicates() {
    let export = render_html(&scene(), &ExplorerOptions::default()).unwrap();
    let raw = metadata(&export.html);
    for field in ["format", "profile", "scene_sha256", "instance_key"] {
        let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        value[field] = "invalid".into();
        assert!(InteractionContext::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    value["nodes"][0]["parent_scene_node_id"] = "missing".into();
    assert!(InteractionContext::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    value["unknown"] = true.into();
    assert!(InteractionContext::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    let duplicate = raw.replacen(
        "\"format\":",
        "\"format\":\"vizir-interaction/1\",\"format\":",
        1,
    );
    assert!(InteractionContext::parse(duplicate.as_bytes()).is_err());
}

#[test]
fn caps_cover_all_scene_strings_not_only_identity() {
    for field in ["text", "explanation", "color", "loss", "id"] {
        let mut source = scene();
        let huge = "x".repeat(vizir_web::MAX_STRING_BYTES + 1);
        match field {
            "loss" => source.losses.push(vizir_core::LossRecord {
                source: huge,
                target: "html".into(),
                fidelity: vizir_core::LoweringFidelity::Lossless,
                reason: "test".into(),
            }),
            _ => {
                if let SceneNode::Text {
                    id,
                    text,
                    color,
                    origin,
                    ..
                } = &mut source.nodes[0]
                {
                    match field {
                        "text" => *text = huge,
                        "explanation" => origin.explanation = huge,
                        "color" => color.0 = huge,
                        "id" => *id = huge,
                        _ => unreachable!(),
                    }
                }
            }
        }
        let err = render_html(&source, &ExplorerOptions::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("VIZ-WEB-0002"), "{field}: {err}");
        assert!(!err.contains(&"x".repeat(100)));
    }
}

#[test]
fn depth_count_and_command_limits_precede_recursive_rendering() {
    let mut source = scene();
    for depth in 1..=64 {
        source.nodes = vec![SceneNode::Group {
            id: format!("g-{depth}"),
            bounds: Rect::default(),
            origin: source.nodes[0].origin().clone(),
            transform: Transform2D::default(),
            opacity: 1.0,
            children: std::mem::take(&mut source.nodes),
        }];
    }
    assert!(
        render_html(&source, &ExplorerOptions::default())
            .unwrap_err()
            .to_string()
            .contains("depth")
    );
    let mut source = scene();
    source.nodes = vec![SceneNode::Path {
        id: "path".into(),
        origin: source.nodes[0].origin().clone(),
        bounds: Rect::default(),
        commands: vec![PathCommand::Close; vizir_web::MAX_PATH_COMMANDS + 1],
        style: vizir_core::ResolvedStyle {
            fill: Color::transparent(),
            stroke: Color::hex("#000000"),
            stroke_width: 1.0,
            opacity: 1.0,
        },
        marker_end: false,
    }];
    assert!(
        render_html(&source, &ExplorerOptions::default())
            .unwrap_err()
            .to_string()
            .contains("path commands")
    );
}

#[test]
fn invalid_dimensions_instances_and_scene_semantics_fail_closed() {
    for width in [0.0, 0.5, 1_000_001.0, f64::NAN, f64::INFINITY] {
        let mut source = scene();
        source.width = width;
        assert!(render_html(&source, &ExplorerOptions::default()).is_err());
    }
    for key in ["", "A", "0a", "a_", "a\"", "a b"] {
        assert!(
            render_html(
                &scene(),
                &ExplorerOptions {
                    instance_key: key.into()
                }
            )
            .is_err()
        );
    }
    let mut source = scene();
    source.nodes.push(source.nodes[0].clone());
    assert!(render_html(&source, &ExplorerOptions::default()).is_err());
}

#[test]
fn schema_and_backend_static_contracts_stay_separate() {
    let schema = vizir_web::interaction_schema();
    assert_eq!(schema["properties"]["format"]["const"], vizir_web::FORMAT);
    assert_eq!(schema["additionalProperties"], false);
    let svg = vizir_backend_svg::capabilities();
    assert!(svg.unsupported.contains("interaction.pointer"));
    let html = vizir_web::capabilities();
    assert!(html.supports.contains("interaction.pointer"));
    assert!(html.unsupported.contains("interaction.select.linked"));
}

#[test]
fn metadata_parent_is_required_nullable_and_active_preorder() {
    let export = render_html(&scene(), &ExplorerOptions::default()).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&metadata(&export.html)).unwrap();
    value["nodes"][0]
        .as_object_mut()
        .unwrap()
        .remove("parent_scene_node_id");
    assert!(InteractionContext::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    let mut context = InteractionContext::parse(metadata(&export.html).as_bytes()).unwrap();
    let old = context.nodes[0].clone();
    context.nodes.clear();
    let ns = vizir_backend_svg::SvgRenderContext::new("main").unwrap();
    for (id, parent) in [("a", None), ("b", Some("a")), ("c", None), ("d", Some("a"))] {
        let mut node = old.clone();
        node.scene_node_id = id.into();
        node.dom_id = ns.node_id(id);
        node.parent_scene_node_id = parent.map(str::to_owned);
        context.nodes.push(node);
    }
    assert!(context.validate().is_err());
}

#[test]
fn nullable_data_key_normalizes_to_absence_without_inventing_identity() {
    let export = render_html(&scene(), &ExplorerOptions::default()).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&metadata(&export.html)).unwrap();
    value["nodes"][0]["origin"]["data_key"] = serde_json::Value::Null;
    let decoded = InteractionContext::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(decoded.nodes[0].origin.data_key, None);
    let canonical = serde_json::to_value(&decoded).unwrap();
    assert!(canonical["nodes"][0]["origin"].get("data_key").is_none());
    let mut source = scene();
    if let SceneNode::Text { origin, .. } = &mut source.nodes[0] {
        origin.data_key = None;
    }
    let no_key = render_html(&source, &ExplorerOptions::default()).unwrap();
    let xml = roxmltree::Document::parse(svg(&no_key.html)).unwrap();
    let node = xml.descendants().find(|n| n.has_tag_name("text")).unwrap();
    assert!(node.attribute("data-key").is_none());
}
