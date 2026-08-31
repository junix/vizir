use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use vizir_core::{
    BackendCapabilities, FontWeight, PathCommand, ResolvedStyle, Scene2D, SceneNode, TextAnchor,
    Transform2D, UnsupportedPolicy, VizResult, negotiate_scene,
};

pub fn capabilities() -> BackendCapabilities {
    BackendCapabilities {
        backend: "svg".to_owned(),
        version: "1".to_owned(),
        accepted_ir: "scene2d".to_owned(),
        supports: BTreeSet::from([
            "paint.alpha".to_owned(),
            "paint.marker-end".to_owned(),
            "scene.2d".to_owned(),
            "scene.2d.circle".to_owned(),
            "scene.2d.group".to_owned(),
            "scene.2d.line".to_owned(),
            "scene.2d.path".to_owned(),
            "scene.2d.rect".to_owned(),
            "scene.2d.text".to_owned(),
            "scene.2d.transform".to_owned(),
        ]),
        unsupported: BTreeSet::from([
            "animation.timeline".to_owned(),
            "interaction.pointer".to_owned(),
            "scene.3d.mesh".to_owned(),
        ]),
        limits: BTreeMap::from([("max-clip-depth".to_owned(), 32)]),
        lowering: BTreeMap::new(),
        unsupported_policy: UnsupportedPolicy::Error,
    }
}

pub fn render(scene: &Scene2D) -> VizResult<String> {
    negotiate_scene(scene, &capabilities())?.require_accepted()?;
    let mut output = String::new();
    writeln!(
        output,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}" role="img" aria-labelledby="vizir-title">"#,
        format_number(scene.width),
        format_number(scene.height),
        format_number(scene.width),
        format_number(scene.height)
    )
    .expect("writing to String cannot fail");
    writeln!(
        output,
        "  <title id=\"vizir-title\">{}</title>",
        escape_text(&scene.document_id)
    )
    .expect("writing to String cannot fail");
    output.push_str(
        "  <defs>\n    <marker id=\"vizir-arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\">\n      <path d=\"M 0 0 L 10 5 L 0 10 z\" fill=\"#8793A5\"/>\n    </marker>\n  </defs>\n",
    );
    if scene.background.0 != "transparent" {
        writeln!(
            output,
            "  <rect width=\"100%\" height=\"100%\" fill=\"{}\"/>",
            escape_attr(&scene.background.0)
        )
        .expect("writing to String cannot fail");
    }
    for node in &scene.nodes {
        render_node(&mut output, node, 1);
    }
    output.push_str("</svg>\n");
    Ok(output)
}

fn render_node(output: &mut String, node: &SceneNode, depth: usize) {
    let indent = "  ".repeat(depth);
    match node {
        SceneNode::Group {
            id,
            origin,
            transform,
            opacity,
            children,
            ..
        } => {
            writeln!(
                output,
                "{indent}<g id=\"{}\"{} opacity=\"{}\"{}>",
                escape_attr(id),
                origin_attrs(origin),
                format_number(*opacity),
                transform_attr(transform)
            )
            .expect("writing to String cannot fail");
            for child in children {
                render_node(output, child, depth + 1);
            }
            writeln!(output, "{indent}</g>").expect("writing to String cannot fail");
        }
        SceneNode::Rect {
            id,
            bounds,
            origin,
            radius,
            style,
        } => {
            writeln!(
                output,
                "{indent}<rect id=\"{}\"{} x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\"{}/>",
                escape_attr(id),
                origin_attrs(origin),
                format_number(bounds.x),
                format_number(bounds.y),
                format_number(bounds.width),
                format_number(bounds.height),
                format_number(*radius),
                style_attrs(style)
            )
            .expect("writing to String cannot fail");
        }
        SceneNode::Circle {
            id,
            origin,
            center,
            radius,
            style,
            ..
        } => {
            writeln!(
                output,
                "{indent}<circle id=\"{}\"{} cx=\"{}\" cy=\"{}\" r=\"{}\"{}/>",
                escape_attr(id),
                origin_attrs(origin),
                format_number(center.x),
                format_number(center.y),
                format_number(*radius),
                style_attrs(style)
            )
            .expect("writing to String cannot fail");
        }
        SceneNode::Line {
            id,
            origin,
            from,
            to,
            style,
            marker_end,
            ..
        } => {
            writeln!(
                output,
                "{indent}<line id=\"{}\"{} x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{}{} />",
                escape_attr(id),
                origin_attrs(origin),
                format_number(from.x),
                format_number(from.y),
                format_number(to.x),
                format_number(to.y),
                style_attrs(style),
                marker_attr(*marker_end)
            )
            .expect("writing to String cannot fail");
        }
        SceneNode::Path {
            id,
            origin,
            commands,
            style,
            marker_end,
            ..
        } => {
            writeln!(
                output,
                "{indent}<path id=\"{}\"{} d=\"{}\"{}{} />",
                escape_attr(id),
                origin_attrs(origin),
                path_data(commands),
                style_attrs(style),
                marker_attr(*marker_end)
            )
            .expect("writing to String cannot fail");
        }
        SceneNode::Text {
            id,
            origin,
            position,
            text,
            font_size,
            anchor,
            color,
            weight,
            ..
        } => {
            writeln!(
                output,
                "{indent}<text id=\"{}\"{} x=\"{}\" y=\"{}\" font-family=\"Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif\" font-size=\"{}\" font-weight=\"{}\" text-anchor=\"{}\" fill=\"{}\">{}</text>",
                escape_attr(id),
                origin_attrs(origin),
                format_number(position.x),
                format_number(position.y),
                format_number(*font_size),
                font_weight(*weight),
                text_anchor(*anchor),
                escape_attr(&color.0),
                escape_text(text)
            )
            .expect("writing to String cannot fail");
        }
    }
}

fn style_attrs(style: &ResolvedStyle) -> String {
    format!(
        " fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" opacity=\"{}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"",
        escape_attr(&style.fill.0),
        escape_attr(&style.stroke.0),
        format_number(style.stroke_width),
        format_number(style.opacity)
    )
}

fn transform_attr(transform: &Transform2D) -> String {
    if transform == &Transform2D::default() {
        return String::new();
    }
    format!(
        " transform=\"translate({} {}) rotate({}) scale({} {})\"",
        format_number(transform.translate.x),
        format_number(transform.translate.y),
        format_number(transform.rotate_degrees),
        format_number(transform.scale.x),
        format_number(transform.scale.y)
    )
}

fn origin_attrs(origin: &vizir_core::Origin) -> String {
    let data_key = origin
        .data_key
        .as_ref()
        .map(|value| format!(" data-key=\"{}\"", escape_attr(value)))
        .unwrap_or_default();
    let data_lineage = if origin.data_lineage.is_empty() {
        String::new()
    } else {
        format!(
            " data-lineage=\"{}\"",
            escape_attr(&origin.data_lineage.join(","))
        )
    };
    format!(
        " data-hir-node=\"{}\" data-mir-node=\"{}\" data-generated-by=\"{}\"{}{}",
        escape_attr(&origin.hir_node),
        escape_attr(&origin.mir_node),
        escape_attr(&origin.generated_by),
        data_key,
        data_lineage
    )
}

fn marker_attr(enabled: bool) -> &'static str {
    if enabled {
        " marker-end=\"url(#vizir-arrow)\""
    } else {
        ""
    }
}

fn path_data(commands: &[PathCommand]) -> String {
    let mut output = String::new();
    for command in commands {
        if !output.is_empty() {
            output.push(' ');
        }
        match command {
            PathCommand::Move { to } => {
                write!(output, "M {} {}", format_number(to.x), format_number(to.y))
            }
            PathCommand::Line { to } => {
                write!(output, "L {} {}", format_number(to.x), format_number(to.y))
            }
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => write!(
                output,
                "C {} {} {} {} {} {}",
                format_number(control1.x),
                format_number(control1.y),
                format_number(control2.x),
                format_number(control2.y),
                format_number(to.x),
                format_number(to.y)
            ),
            PathCommand::Close => {
                output.push('Z');
                continue;
            }
        }
        .expect("writing to String cannot fail");
    }
    output
}

fn text_anchor(anchor: TextAnchor) -> &'static str {
    match anchor {
        TextAnchor::Start => "start",
        TextAnchor::Middle => "middle",
        TextAnchor::End => "end",
    }
}

fn font_weight(weight: FontWeight) -> u16 {
    match weight {
        FontWeight::Regular => 400,
        FontWeight::Medium => 500,
        FontWeight::Bold => 700,
    }
}

fn format_number(value: f64) -> String {
    let value = if value.abs() < 0.000_000_1 {
        0.0
    } else {
        value
    };
    let mut output = format!("{value:.4}");
    while output.contains('.') && output.ends_with('0') {
        output.pop();
    }
    if output.ends_with('.') {
        output.pop();
    }
    output
}

fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(value: &str) -> String {
    escape_text(value)
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
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
}
