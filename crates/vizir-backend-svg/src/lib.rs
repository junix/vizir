use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};

use vizir_core::{
    BackendCapabilities, FontWeight, PathCommand, ResolvedStyle, Scene2D, SceneNode, TextAnchor,
    Transform2D, UnsupportedPolicy, VizError, VizResult, negotiate_scene, validate_scene,
};

mod xml;

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

/// Opt-in identity domain for embedding the same scene more than once.
/// The legacy `render` entry point deliberately keeps its exact existing bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvgRenderContext {
    instance_key: String,
}

impl SvgRenderContext {
    pub fn new(instance_key: &str) -> VizResult<Self> {
        let valid = !instance_key.is_empty()
            && instance_key.len() <= 32
            && instance_key.as_bytes()[0].is_ascii_lowercase()
            && instance_key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !valid {
            return Err(VizError::Diagnostic(
                "VIZ-SVG-0003: instance key must match [a-z][a-z0-9-]{0,31}".into(),
            ));
        }
        Ok(Self {
            instance_key: instance_key.into(),
        })
    }

    pub fn instance_key(&self) -> &str {
        &self.instance_key
    }

    pub fn node_id(&self, id: &str) -> String {
        let mut result = format!("vzi-{}-node-", self.instance_key);
        for byte in id.as_bytes() {
            write!(result, "{byte:02x}").expect("writing to String cannot fail");
        }
        result
    }

    pub fn title_id(&self) -> String {
        format!("vzi-{}-title", self.instance_key)
    }
    pub fn marker_id(&self) -> String {
        format!("vzi-{}-marker", self.instance_key)
    }
}

pub fn render(scene: &Scene2D) -> VizResult<String> {
    let mut output = String::new();
    render_into(scene, None, &mut output)?;
    Ok(output)
}

/// Stream ordinary SVG through the same renderer, allowing callers to bound output.
/// This preserves the exact legacy `render` byte format.
pub fn render_to(scene: &Scene2D, output: &mut impl Write) -> VizResult<()> {
    render_into(scene, None, output)
}

/// Stream namespaced SVG to a caller-owned (optionally bounded) sink. This is
/// the same primitive renderer as `render`, not a second rendering backend.
/// A bounded runtime must preflight traversal depth before entering this API.
pub fn render_to_with_context(
    scene: &Scene2D,
    context: &SvgRenderContext,
    output: &mut impl Write,
) -> VizResult<()> {
    render_into(scene, Some(context), output)
}

fn writer_error(_: fmt::Error) -> VizError {
    VizError::Diagnostic("VIZ-SVG-0002: SVG output writer refused more bytes".into())
}

fn render_into(
    scene: &Scene2D,
    context: Option<&SvgRenderContext>,
    output: &mut impl Write,
) -> VizResult<()> {
    xml::validate_strings(scene)?;
    validate_scene(scene).map_err(|diagnostics| VizError::validation(&diagnostics))?;
    negotiate_scene(scene, &capabilities())?.require_accepted()?;
    let title_id = context.map_or_else(|| "vizir-title".into(), SvgRenderContext::title_id);
    let marker_id = context.map_or_else(|| "vizir-arrow".into(), SvgRenderContext::marker_id);
    let canvas = if context.is_some() {
        " data-vizir-canvas=\"1\""
    } else {
        ""
    };
    writeln!(
        output,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}" role="img" aria-labelledby="{}"{}>"#,
        canvas_number(scene.width, context), canvas_number(scene.height, context),
        canvas_number(scene.width, context), canvas_number(scene.height, context), title_id, canvas
    ).map_err(writer_error)?;
    writeln!(
        output,
        "  <title id=\"{}\">{}</title>",
        title_id,
        escape_text(&scene.document_id)
    )
    .map_err(writer_error)?;
    writeln!(output, "  <defs>\n    <marker id=\"{}\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\">\n      <path d=\"M 0 0 L 10 5 L 0 10 z\" fill=\"#8793A5\"/>\n    </marker>\n  </defs>", marker_id).map_err(writer_error)?;
    if scene.background.0 != "transparent" {
        let (width, height) = if context.is_some() {
            (scene.width.to_string(), scene.height.to_string())
        } else {
            ("100%".into(), "100%".into())
        };
        writeln!(
            output,
            "  <rect width=\"{}\" height=\"{}\" fill=\"{}\"/>",
            width,
            height,
            escape_attr(&scene.background.0)
        )
        .map_err(writer_error)?;
    }
    for node in &scene.nodes {
        render_node(output, node, 1, context)?;
    }
    output.write_str("</svg>\n").map_err(writer_error)?;
    Ok(())
}

fn canvas_number(value: f64, context: Option<&SvgRenderContext>) -> String {
    if context.is_some() {
        value.to_string()
    } else {
        format_number(value)
    }
}

fn emitted_identity(id: &str, context: Option<&SvgRenderContext>) -> String {
    match context {
        None => format!("id=\"{}\"", escape_attr(id)),
        Some(context) => format!(
            "id=\"{}\" data-vizir-scene-id=\"{}\"",
            context.node_id(id),
            escape_attr(id)
        ),
    }
}

fn render_node(
    output: &mut impl Write,
    node: &SceneNode,
    depth: usize,
    context: Option<&SvgRenderContext>,
) -> VizResult<()> {
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
                "{indent}<g {}{} opacity=\"{}\"{}>",
                emitted_identity(id, context),
                origin_attrs(origin),
                format_number(*opacity),
                transform_attr(transform)
            )
            .map_err(writer_error)?;
            for child in children {
                render_node(output, child, depth + 1, context)?;
            }
            writeln!(output, "{indent}</g>").map_err(writer_error)?;
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
                "{indent}<rect {}{} x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\"{}/>",
                emitted_identity(id, context),
                origin_attrs(origin),
                format_number(bounds.x),
                format_number(bounds.y),
                format_number(bounds.width),
                format_number(bounds.height),
                format_number(*radius),
                style_attrs(style)
            )
            .map_err(writer_error)?;
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
                "{indent}<circle {}{} cx=\"{}\" cy=\"{}\" r=\"{}\"{}/>",
                emitted_identity(id, context),
                origin_attrs(origin),
                format_number(center.x),
                format_number(center.y),
                format_number(*radius),
                style_attrs(style)
            )
            .map_err(writer_error)?;
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
                "{indent}<line {}{} x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{}{} />",
                emitted_identity(id, context),
                origin_attrs(origin),
                format_number(from.x),
                format_number(from.y),
                format_number(to.x),
                format_number(to.y),
                style_attrs(style),
                marker_attr(*marker_end, context)
            )
            .map_err(writer_error)?;
        }
        SceneNode::Path {
            id,
            origin,
            commands,
            style,
            marker_end,
            ..
        } => {
            write!(
                output,
                "{indent}<path {}{} d=\"",
                emitted_identity(id, context),
                origin_attrs(origin)
            )
            .map_err(writer_error)?;
            write_path_data(output, commands)?;
            writeln!(
                output,
                "\"{}{} />",
                style_attrs(style),
                marker_attr(*marker_end, context)
            )
            .map_err(writer_error)?;
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
                "{indent}<text {}{} x=\"{}\" y=\"{}\" font-family=\"Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif\" font-size=\"{}\" font-weight=\"{}\" text-anchor=\"{}\" fill=\"{}\">{}</text>",
                emitted_identity(id, context),
                origin_attrs(origin),
                format_number(position.x),
                format_number(position.y),
                format_number(*font_size),
                font_weight(*weight),
                text_anchor(*anchor),
                escape_attr(&color.0),
                escape_text(text)
            )
            .map_err(writer_error)?;
        }
    }
    Ok(())
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

fn marker_attr(enabled: bool, context: Option<&SvgRenderContext>) -> String {
    if enabled {
        let id = context.map_or_else(|| "vizir-arrow".into(), SvgRenderContext::marker_id);
        format!(" marker-end=\"url(#{id})\"")
    } else {
        String::new()
    }
}

fn write_path_data(output: &mut impl Write, commands: &[PathCommand]) -> VizResult<()> {
    for (index, command) in commands.iter().enumerate() {
        if index != 0 {
            output.write_char(' ').map_err(writer_error)?;
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
            PathCommand::Close => output.write_char('Z'),
        }
        .map_err(writer_error)?;
    }
    Ok(())
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
        // XML end-of-line normalization otherwise turns literal CR into LF.
        .replace('\r', "&#xD;")
}

fn escape_attr(value: &str) -> String {
    escape_text(value)
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        // Character references bypass XML attribute whitespace normalization.
        .replace('\t', "&#x9;")
        .replace('\n', "&#xA;")
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

#[cfg(test)]
mod xml_tests;
