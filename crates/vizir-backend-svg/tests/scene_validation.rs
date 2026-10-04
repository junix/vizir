use vizir_backend_svg::render;
use vizir_core::{
    Color, FontWeight, Origin, PathCommand, Point, Rect, ResolvedStyle, Scene2D, SceneNode,
    TextAnchor, Transform2D,
};

fn origin() -> Origin {
    Origin {
        hir_node: "source".to_owned(),
        mir_node: "resolved".to_owned(),
        data_key: None,
        data_lineage: vec![],
        generated_by: "test".to_owned(),
        explanation: "validation boundary".to_owned(),
    }
}

fn style() -> ResolvedStyle {
    ResolvedStyle {
        fill: Color::hex("#aAbBcC80"),
        stroke: Color::transparent(),
        stroke_width: 0.0,
        opacity: 1.0,
    }
}

fn nodes() -> Vec<SceneNode> {
    vec![
        SceneNode::Group {
            id: "group".to_owned(),
            bounds: Rect::default(),
            origin: origin(),
            transform: Transform2D::default(),
            opacity: 1.0,
            children: vec![],
        },
        SceneNode::Rect {
            id: "rect".to_owned(),
            bounds: Rect::default(),
            origin: origin(),
            radius: 0.0,
            style: style(),
        },
        SceneNode::Circle {
            id: "circle".to_owned(),
            bounds: Rect::default(),
            origin: origin(),
            center: Point::default(),
            radius: 0.0,
            style: style(),
        },
        SceneNode::Line {
            id: "line".to_owned(),
            bounds: Rect::default(),
            origin: origin(),
            from: Point::default(),
            to: Point::default(),
            style: style(),
            marker_end: false,
        },
        SceneNode::Path {
            id: "path".to_owned(),
            bounds: Rect::default(),
            origin: origin(),
            commands: vec![],
            style: style(),
            marker_end: false,
        },
        SceneNode::Text {
            id: "text".to_owned(),
            bounds: Rect::default(),
            origin: origin(),
            position: Point::default(),
            text: String::new(),
            font_size: 12.0,
            anchor: TextAnchor::Start,
            color: Color::hex("#123456"),
            weight: FontWeight::Regular,
        },
    ]
}

fn scene(nodes: Vec<SceneNode>) -> Scene2D {
    Scene2D {
        document_id: "document".to_owned(),
        width: 100.0,
        height: 80.0,
        background: Color::transparent(),
        nodes,
        losses: vec![],
    }
}

fn bounds_mut(node: &mut SceneNode) -> &mut Rect {
    match node {
        SceneNode::Group { bounds, .. }
        | SceneNode::Rect { bounds, .. }
        | SceneNode::Circle { bounds, .. }
        | SceneNode::Line { bounds, .. }
        | SceneNode::Path { bounds, .. }
        | SceneNode::Text { bounds, .. } => bounds,
    }
}

fn style_mut(node: &mut SceneNode) -> &mut ResolvedStyle {
    match node {
        SceneNode::Rect { style, .. }
        | SceneNode::Circle { style, .. }
        | SceneNode::Line { style, .. }
        | SceneNode::Path { style, .. } => style,
        _ => panic!("expected styled geometry"),
    }
}

fn reject_node(node: SceneNode, field: &str) {
    assert!(
        render(&scene(vec![node])).is_err(),
        "SVG accepted invalid {field}"
    );
}

#[test]
fn render_rejects_nonfinite_and_nonpositive_viewport_dimensions() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
        for component in 0..2 {
            let mut invalid = scene(vec![]);
            if component == 0 {
                invalid.width = value;
            } else {
                invalid.height = value;
            }
            assert!(
                render(&invalid).is_err(),
                "viewport component {component}: {value}"
            );
        }
    }
}

#[test]
fn render_rejects_nonfinite_bounds_on_every_node_kind() {
    for template in nodes() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for component in 0..4 {
                let mut invalid = template.clone();
                let bounds = bounds_mut(&mut invalid);
                match component {
                    0 => bounds.x = value,
                    1 => bounds.y = value,
                    2 => bounds.width = value,
                    _ => bounds.height = value,
                }
                reject_node(
                    invalid,
                    &format!("{} bounds component {component}", template.id()),
                );
            }
        }
    }
}

#[test]
fn render_rejects_negative_bounds_extents_on_every_node_kind() {
    for template in nodes() {
        for component in 0..2 {
            let mut invalid = template.clone();
            let bounds = bounds_mut(&mut invalid);
            if component == 0 {
                bounds.width = -1.0;
            } else {
                bounds.height = -1.0;
            }
            reject_node(
                invalid,
                &format!("{} negative bounds extent {component}", template.id()),
            );
        }
    }
}

#[test]
fn render_rejects_every_nonfinite_transform_component() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for component in 0..5 {
            let mut invalid = nodes().remove(0);
            if let SceneNode::Group { transform, .. } = &mut invalid {
                match component {
                    0 => transform.translate.x = value,
                    1 => transform.translate.y = value,
                    2 => transform.rotate_degrees = value,
                    3 => transform.scale.x = value,
                    _ => transform.scale.y = value,
                }
            }
            reject_node(invalid, &format!("transform component {component}"));
        }
    }
}

#[test]
fn render_rejects_invalid_group_and_shape_opacity() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01, 1.01] {
        for mut invalid in nodes().into_iter().take(5) {
            if let SceneNode::Group { opacity, .. } = &mut invalid {
                *opacity = value;
            } else {
                style_mut(&mut invalid).opacity = value;
            }
            reject_node(invalid, "opacity");
        }
    }
}

#[test]
fn render_rejects_invalid_stroke_width_for_every_styled_shape() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        for mut invalid in nodes().into_iter().skip(1).take(4) {
            style_mut(&mut invalid).stroke_width = value;
            reject_node(invalid, "stroke width");
        }
    }
}

#[test]
fn render_rejects_invalid_rect_and_circle_radii() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        for mut invalid in nodes().into_iter().skip(1).take(2) {
            match &mut invalid {
                SceneNode::Rect { radius, .. } | SceneNode::Circle { radius, .. } => {
                    *radius = value
                }
                _ => unreachable!(),
            }
            reject_node(invalid, "radius");
        }
    }
}

#[test]
fn render_rejects_nonfinite_circle_line_and_text_points() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for component in 0..8 {
            let mut invalid = nodes().remove(match component {
                0..=1 => 2,
                2..=5 => 3,
                _ => 5,
            });
            match &mut invalid {
                SceneNode::Circle { center, .. } => {
                    if component == 0 {
                        center.x = value;
                    } else {
                        center.y = value;
                    }
                }
                SceneNode::Line { from, to, .. } => match component {
                    2 => from.x = value,
                    3 => from.y = value,
                    4 => to.x = value,
                    _ => to.y = value,
                },
                SceneNode::Text { position, .. } => {
                    if component == 6 {
                        position.x = value;
                    } else {
                        position.y = value;
                    }
                }
                _ => unreachable!(),
            }
            reject_node(invalid, &format!("point component {component}"));
        }
    }
}

#[test]
fn render_rejects_every_nonfinite_path_command_coordinate() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for component in 0..10 {
            let mut commands = vec![
                PathCommand::Move {
                    to: Point::default(),
                },
                PathCommand::Line {
                    to: Point::default(),
                },
                PathCommand::Cubic {
                    control1: Point::default(),
                    control2: Point::default(),
                    to: Point::default(),
                },
                PathCommand::Close,
            ];
            let command = &mut commands[match component {
                0..=1 => 0,
                2..=3 => 1,
                _ => 2,
            }];
            match command {
                PathCommand::Move { to } | PathCommand::Line { to } => {
                    if component % 2 == 0 {
                        to.x = value;
                    } else {
                        to.y = value;
                    }
                }
                PathCommand::Cubic {
                    control1,
                    control2,
                    to,
                } => match component {
                    4 => control1.x = value,
                    5 => control1.y = value,
                    6 => control2.x = value,
                    7 => control2.y = value,
                    8 => to.x = value,
                    _ => to.y = value,
                },
                PathCommand::Close => unreachable!(),
            }
            let mut invalid = nodes().remove(4);
            if let SceneNode::Path {
                commands: target, ..
            } = &mut invalid
            {
                *target = commands;
            }
            reject_node(invalid, &format!("path coordinate {component}"));
        }
    }
}

#[test]
fn render_rejects_nonfinite_and_nonpositive_font_size() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0] {
        let mut invalid = nodes().remove(5);
        if let SceneNode::Text { font_size, .. } = &mut invalid {
            *font_size = value;
        }
        reject_node(invalid, "font size");
    }
}

#[test]
fn render_rejects_nonportable_colors_in_all_color_positions() {
    for value in [
        "",
        "red",
        "#fff",
        "#12345g",
        "#1234567",
        "#123456789",
        " transparent ",
        "url(#paint)",
        "#11\"22'33",
    ] {
        let mut invalid = scene(vec![]);
        invalid.background = Color(value.to_owned());
        assert!(render(&invalid).is_err(), "background {value:?}");
        for template in nodes().into_iter().skip(1).take(4) {
            for is_fill in [true, false] {
                let mut invalid = template.clone();
                let style = style_mut(&mut invalid);
                if is_fill {
                    style.fill = Color(value.to_owned());
                } else {
                    style.stroke = Color(value.to_owned());
                }
                reject_node(invalid, "shape color");
            }
        }
        let mut invalid = nodes().remove(5);
        if let SceneNode::Text { color, .. } = &mut invalid {
            *color = Color(value.to_owned());
        }
        reject_node(invalid, "text color");
    }
}

#[test]
fn render_rejects_blank_and_duplicate_identities_recursively() {
    let mut invalid = scene(vec![]);
    invalid.document_id = "\t \n".to_owned();
    assert!(render(&invalid).is_err());
    let mut group = nodes().remove(0);
    let mut blank = nodes().remove(1);
    if let SceneNode::Rect { id, .. } = &mut blank {
        *id = " \t".to_owned();
    }
    if let SceneNode::Group { children, .. } = &mut group {
        children.push(blank);
    }
    reject_node(group, "blank nested id");
    let mut group = nodes().remove(0);
    let rect = nodes().remove(1);
    if let SceneNode::Group { children, .. } = &mut group {
        children.push(rect.clone());
    }
    assert!(
        render(&scene(vec![rect, group])).is_err(),
        "cross-level duplicate id"
    );
}

#[test]
fn render_accepts_portable_colors_empty_content_and_degenerate_geometry() {
    for value in [
        "transparent",
        "#000000",
        "#aAbBcC",
        "#aAbBcC00",
        "#FFFFFFFF",
    ] {
        let mut valid = scene(nodes());
        valid.background = Color(value.to_owned());
        for node in &mut valid.nodes {
            let bounds = bounds_mut(node);
            bounds.x = -10.0;
            bounds.y = -20.0;
            match node {
                SceneNode::Group {
                    transform, opacity, ..
                } => {
                    transform.scale = Point { x: -2.0, y: 0.0 };
                    transform.translate = Point { x: -1.0, y: -2.0 };
                    *opacity = 0.0;
                }
                SceneNode::Text { color, .. } => *color = Color(value.to_owned()),
                _ => {
                    let style = style_mut(node);
                    style.fill = Color(value.to_owned());
                    style.stroke = Color(value.to_owned());
                    style.opacity = 0.0;
                }
            }
        }
        assert!(render(&valid).is_ok(), "portable color {value:?}");
    }
    assert!(render(&scene(vec![])).is_ok(), "empty scene content");
}

#[test]
fn render_accepts_opaque_nonblank_ids_and_escapes_their_attributes() {
    let mut valid = scene(vec![nodes().remove(1)]);
    valid.document_id = "document & 雪 <quoted>".to_owned();
    if let SceneNode::Rect { id, .. } = &mut valid.nodes[0] {
        *id = " id with spaces 雪 \"double\" 'single' & <> ".to_owned();
    }
    let svg = render(&valid).unwrap();
    assert!(svg.contains("document &amp; 雪 &lt;quoted&gt;"));
    assert!(svg.contains("id with spaces 雪 &quot;double&quot; &apos;single&apos; &amp; &lt;&gt;"));
}

#[test]
fn render_does_not_impose_a_move_first_path_requirement() {
    for command in [
        PathCommand::Line {
            to: Point::default(),
        },
        PathCommand::Cubic {
            control1: Point::default(),
            control2: Point::default(),
            to: Point::default(),
        },
        PathCommand::Close,
    ] {
        let mut valid = nodes().remove(4);
        if let SceneNode::Path { commands, .. } = &mut valid {
            commands.push(command);
        }
        assert!(render(&scene(vec![valid])).is_ok());
    }
}
