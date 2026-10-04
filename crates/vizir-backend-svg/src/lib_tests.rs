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
