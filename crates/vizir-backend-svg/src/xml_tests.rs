use super::*;
use vizir_core::{Color, Origin, Point, Rect};

fn scene() -> Scene2D {
    Scene2D {
        document_id: "document".into(),
        width: 100.0,
        height: 80.0,
        background: Color::transparent(),
        nodes: vec![SceneNode::Text {
            id: "label".into(),
            bounds: Rect::default(),
            origin: Origin {
                hir_node: "hir".into(),
                mir_node: "mir".into(),
                data_key: Some("key".into()),
                data_lineage: vec!["row".into()],
                generated_by: "test".into(),
                explanation: String::new(),
            },
            position: Point::default(),
            text: "text".into(),
            font_size: 12.0,
            anchor: TextAnchor::Start,
            color: Color::hex("#000000"),
            weight: FontWeight::Regular,
        }],
        losses: Vec::new(),
    }
}

#[test]
fn xml_parser_preserves_all_emitted_identity_strings_and_text() {
    // Distinct whitespace must remain distinct after parsing, including CRLF.
    let value = "汉字😺é e\u{301} /:~.#?%[] &<>\"' \t\n\r\r\n";
    let mut scene = scene();
    scene.document_id = value.into();
    let SceneNode::Text {
        id, origin, text, ..
    } = &mut scene.nodes[0]
    else {
        unreachable!()
    };
    *id = value.into();
    *text = value.into();
    origin.hir_node = value.into();
    origin.mir_node = value.into();
    origin.generated_by = value.into();
    origin.data_key = Some(value.into());
    origin.data_lineage = vec![value.into(), "comma,inside".into(), value.into()];
    let expected_lineage = origin.data_lineage.join(",");
    let svg = render(&scene).unwrap();
    let xml = roxmltree::Document::parse(&svg).unwrap();
    assert_eq!(
        xml.descendants()
            .find(|n| n.has_tag_name("title"))
            .unwrap()
            .text(),
        Some(value)
    );
    let text = xml.descendants().find(|n| n.has_tag_name("text")).unwrap();
    for attr in [
        "id",
        "data-hir-node",
        "data-mir-node",
        "data-generated-by",
        "data-key",
    ] {
        assert_eq!(text.attribute(attr), Some(value), "{attr}");
    }
    assert_eq!(
        text.attribute("data-lineage"),
        Some(expected_lineage.as_str())
    );
    assert_eq!(text.text(), Some(value));
    assert_eq!(svg, render(&scene).unwrap());
}

#[test]
fn xml_10_legal_boundaries_round_trip_and_illegal_characters_fail_closed() {
    let mut scene = scene();
    let legal =
        "x\u{9}\u{a}\u{d}\u{20}\u{7f}\u{85}\u{d7ff}\u{e000}\u{fffd}\u{10000}\u{1fffe}\u{10ffff}";
    scene.document_id = legal.into();
    let svg = render(&scene).unwrap();
    let xml = roxmltree::Document::parse(&svg).unwrap();
    assert_eq!(
        xml.descendants()
            .find(|n| n.has_tag_name("title"))
            .unwrap()
            .text(),
        Some(legal)
    );
    for cp in (0..=0x1f)
        .chain([0xfffe, 0xffff])
        .filter(|c| ![9, 10, 13].contains(c))
    {
        let value = format!("private{}tail", char::from_u32(cp).unwrap());
        scene.document_id = value;
        // XML representability is not a restriction on generic core identifiers.
        validate_scene(&scene).unwrap();
        let error = render(&scene).unwrap_err().to_string();
        assert!(error.contains("VIZ-SVG-0001"));
        assert!(error.contains(&format!("U+{cp:04X}")));
        assert!(error.contains("document_id"));
        assert!(error.contains("byte offset 7"));
        assert!(!error.contains("private"));
        assert!(error.len() < 180);
    }
}

#[test]
fn preflight_covers_every_emitted_string_field_before_generic_validation() {
    for field in [
        "document_id",
        "background",
        "id",
        "origin.hir_node",
        "origin.mir_node",
        "origin.generated_by",
        "origin.data_key",
        "origin.data_lineage[1]",
        "text",
        "color",
    ] {
        let mut scene = scene();
        let bad = format!("{}\u{1}hidden", "private".repeat(10_000));
        let SceneNode::Text {
            id,
            origin,
            text,
            color,
            ..
        } = &mut scene.nodes[0]
        else {
            unreachable!()
        };
        match field {
            "document_id" => scene.document_id = bad,
            "background" => scene.background = Color(bad),
            "id" => *id = bad,
            "origin.hir_node" => origin.hir_node = bad,
            "origin.mir_node" => origin.mir_node = bad,
            "origin.generated_by" => origin.generated_by = bad,
            "origin.data_key" => origin.data_key = Some(bad),
            "origin.data_lineage[1]" => origin.data_lineage.push(bad),
            "text" => *text = bad,
            "color" => *color = Color(bad),
            _ => unreachable!(),
        }
        let error = render(&scene).unwrap_err().to_string();
        assert!(error.contains("VIZ-SVG-0001"), "{error}");
        assert!(error.contains(field), "{field}: {error}");
        assert!(error.contains("U+0001"));
        assert!(error.len() < 180, "{error}");
        assert!(!error.contains("private"));
    }
}

#[test]
fn preflight_checks_nested_groups_and_every_styled_primitive() {
    let scene = scene();
    let origin = scene.nodes[0].origin().clone();
    let bounds = Rect::default();
    let style = ResolvedStyle {
        fill: Color::hex("#112233"),
        stroke: Color::hex("#445566"),
        stroke_width: 1.0,
        opacity: 1.0,
    };
    let nodes = vec![
        SceneNode::Rect {
            id: "rect".into(),
            bounds,
            origin: origin.clone(),
            radius: 0.0,
            style: style.clone(),
        },
        SceneNode::Circle {
            id: "circle".into(),
            bounds,
            origin: origin.clone(),
            center: Point::default(),
            radius: 1.0,
            style: style.clone(),
        },
        SceneNode::Line {
            id: "line".into(),
            bounds,
            origin: origin.clone(),
            from: Point::default(),
            to: Point::default(),
            marker_end: false,
            style: style.clone(),
        },
        SceneNode::Path {
            id: "path".into(),
            bounds,
            origin: origin.clone(),
            commands: vec![PathCommand::Move {
                to: Point::default(),
            }],
            marker_end: false,
            style,
        },
    ];
    for node in nodes {
        for field in ["fill", "stroke"] {
            let mut scene = scene.clone();
            let mut node = node.clone();
            let (SceneNode::Rect { style, .. }
            | SceneNode::Circle { style, .. }
            | SceneNode::Line { style, .. }
            | SceneNode::Path { style, .. }) = &mut node
            else {
                unreachable!()
            };
            if field == "fill" {
                style.fill.0.push('\0');
            } else {
                style.stroke.0.push('\0');
            }
            scene.nodes = vec![SceneNode::Group {
                id: "group".into(),
                bounds,
                origin: origin.clone(),
                transform: Transform2D::default(),
                opacity: 1.0,
                children: vec![node],
            }];
            let error = render(&scene).unwrap_err().to_string();
            assert!(
                error.contains(&format!("nodes(preorder)[1].style.{field}")),
                "{error}"
            );
            assert!(error.contains("U+0000"));
        }
    }
    // Non-emitted metadata is not constrained by SVG's XML target.
    let mut scene = scene;
    let SceneNode::Text { origin, .. } = &mut scene.nodes[0] else {
        unreachable!()
    };
    origin.explanation = "unemitted\0explanation".into();
    render(&scene).unwrap();
}
