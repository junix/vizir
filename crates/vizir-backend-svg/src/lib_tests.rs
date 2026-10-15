use super::*;
use vizir_core::{Color, Origin, Point, Rect, SceneNode};

#[test]
fn svg_escapes_text_and_keeps_transparent_root() {
    let scene = Scene2D {
        document_id: "a & b".to_owned(),
        width: 100.0,
        height: 80.0,
        background: Color::transparent(),
        nodes: vec![SceneNode::Text {
            id: "label".to_owned(),
            bounds: Rect::default(),
            origin: Origin {
                hir_node: "label".to_owned(),
                mir_node: "label".to_owned(),
                data_key: None,
                data_lineage: Vec::new(),
                generated_by: "test".to_owned(),
                explanation: "test".to_owned(),
            },
            position: Point { x: 10.0, y: 20.0 },
            text: "x < y".to_owned(),
            font_size: 12.0,
            anchor: TextAnchor::Start,
            color: Color::hex("#000000"),
            weight: FontWeight::Regular,
        }],
        losses: Vec::new(),
    };
    let svg = render(&scene).unwrap();
    assert!(svg.contains("a &amp; b"));
    assert!(svg.contains("x &lt; y"));
    assert!(!svg.contains("width=\"100%\""));
}

#[test]
fn svg_escapes_gt_in_text_and_quotes_in_attributes() {
    let mut scene = Scene2D {
        document_id: "a & b < c > d".to_owned(),
        width: 100.0,
        height: 80.0,
        background: Color::hex("#112233"),
        nodes: vec![SceneNode::Text {
            id: "label\" & '< >".to_owned(),
            bounds: Rect::default(),
            origin: Origin {
                hir_node: "label".to_owned(),
                mir_node: "label".to_owned(),
                data_key: None,
                data_lineage: Vec::new(),
                generated_by: "test".to_owned(),
                explanation: "test".to_owned(),
            },
            position: Point { x: 10.0, y: 20.0 },
            text: "x < y > z & w".to_owned(),
            font_size: 12.0,
            anchor: TextAnchor::Start,
            color: Color::hex("#000000"),
            weight: FontWeight::Regular,
        }],
        losses: Vec::new(),
    };
    let svg = render(&scene).unwrap();
    assert!(svg.contains("a &amp; b &lt; c &gt; d"));
    assert!(svg.contains("x &lt; y &gt; z &amp; w"));
    assert!(svg.contains("id=\"label&quot; &amp; &apos;&lt; &gt;\""));
    scene.background = Color("#11\"22'33".to_owned());
    let error = render(&scene).unwrap_err().to_string();
    assert!(error.contains("VIZ-TYPE-0004"));
    assert!(error.contains("background"));
}

fn arrow_scene(document_id: &str) -> Scene2D {
    let origin = Origin {
        hir_node: "n".to_owned(),
        mir_node: "n".to_owned(),
        data_key: None,
        data_lineage: Vec::new(),
        generated_by: "test".to_owned(),
        explanation: "test".to_owned(),
    };
    Scene2D {
        document_id: document_id.to_owned(),
        width: 100.0,
        height: 80.0,
        background: Color::transparent(),
        nodes: vec![SceneNode::Line {
            id: "edge".to_owned(),
            bounds: Rect::default(),
            origin,
            from: Point { x: 0.0, y: 0.0 },
            to: Point { x: 50.0, y: 50.0 },
            marker_end: true,
            style: ResolvedStyle {
                fill: Color::transparent(),
                stroke: Color::hex("#445566"),
                stroke_width: 1.0,
                opacity: 1.0,
            },
        }],
        losses: Vec::new(),
    }
}

/// Every `id="..."` value in an SVG string.
fn ids(svg: &str) -> Vec<String> {
    svg.split(" id=\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().to_owned())
        .collect()
}

fn render_namespaced(scene: &Scene2D, key: &str) -> String {
    let context = SvgRenderContext::new(key).unwrap();
    let mut out = String::new();
    render_to_with_context(scene, &context, &mut out).unwrap();
    out
}

#[test]
fn namespaced_embeds_have_disjoint_ids_and_resolve_their_own_references() {
    let first = render_namespaced(&arrow_scene("first"), "one");
    let second = render_namespaced(&arrow_scene("second"), "two");
    let (first_ids, second_ids) = (ids(&first), ids(&second));
    assert!(first_ids.len() >= 3, "{first_ids:?}"); // title, marker, node
    assert!(
        first_ids.iter().all(|id| !second_ids.contains(id)),
        "{first_ids:?} vs {second_ids:?}"
    );
    for (svg, key) in [(&first, "one"), (&second, "two")] {
        let context = SvgRenderContext::new(key).unwrap();
        assert!(svg.contains(&format!("aria-labelledby=\"{}\"", context.title_id())));
        assert!(svg.contains(&format!("<title id=\"{}\">", context.title_id())));
        assert!(svg.contains(&format!("<marker id=\"{}\"", context.marker_id())));
        assert!(svg.contains(&format!("marker-end=\"url(#{})\"", context.marker_id())));
        assert!(svg.contains(&format!("id=\"{}\"", context.node_id("edge"))));
        // No fixed global identifier leaks into a namespaced embed.
        assert!(!svg.contains("vizir-title") && !svg.contains("vizir-arrow"));
    }
}

#[test]
fn same_instance_key_is_deterministic_and_embedding_twice_requires_distinct_keys() {
    let scene = arrow_scene("doc");
    assert_eq!(
        render_namespaced(&scene, "main"),
        render_namespaced(&scene, "main")
    );
    // Same document under the same key collides by design: callers must pick distinct keys.
    assert_eq!(
        ids(&render_namespaced(&scene, "main")),
        ids(&render_namespaced(&scene, "main"))
    );
    assert_ne!(
        ids(&render_namespaced(&scene, "main")),
        ids(&render_namespaced(&scene, "copy"))
    );
    assert!(SvgRenderContext::new("Bad Key").is_err());
    assert!(SvgRenderContext::new("").is_err());
}

#[test]
fn legacy_render_keeps_fixed_identifiers() {
    let svg = render(&arrow_scene("doc")).unwrap();
    assert!(svg.contains("aria-labelledby=\"vizir-title\""));
    assert!(svg.contains("<marker id=\"vizir-arrow\""));
    assert!(svg.contains("id=\"edge\""));
    assert_eq!(svg, render(&arrow_scene("doc")).unwrap());
}
