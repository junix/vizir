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
    let scene = Scene2D {
        document_id: "a & b < c > d".to_owned(),
        width: 100.0,
        height: 80.0,
        // The renderer escapes whatever it is handed, so raw quotes in the
        // background must never break out of the fill attribute.
        background: Color("#11\"22'33".to_owned()),
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
    assert!(svg.contains("fill=\"#11&quot;22&apos;33\""));
    assert!(!svg.contains("#11\"22'33"));
}
