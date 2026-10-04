use std::collections::BTreeSet;

use crate::{Diagnostic, PathCommand, Rect, ResolvedStyle, Scene2D, SceneNode};

use super::{
    validate_color, validate_finite, validate_non_negative, validate_point, validate_positive,
    validate_unit,
};

/// Validate the resolved static scene without changing its geometry or metadata.
///
/// IDs are opaque, nonblank strings. Bounds may be approximate and degenerate;
/// this does not check containment, exact geometry bounds, or backend limits.
pub fn validate_scene(scene: &Scene2D) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    validate_nonblank(&scene.document_id, "document_id", &mut diagnostics);
    validate_positive(scene.width, "width", &mut diagnostics);
    validate_positive(scene.height, "height", &mut diagnostics);
    validate_color(&scene.background, "background", &mut diagnostics);
    let mut ids = BTreeSet::new();
    for (index, node) in scene.nodes.iter().enumerate() {
        validate_node(node, &format!("nodes[{index}]"), &mut ids, &mut diagnostics);
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn validate_node<'a>(
    node: &'a SceneNode,
    source: &str,
    ids: &mut BTreeSet<&'a str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    validate_nonblank(node.id(), &format!("{source}.id"), diagnostics);
    if !ids.insert(node.id()) {
        diagnostics.push(
            Diagnostic::new(
                "VIZ-SCENE-0102",
                format!("duplicate scene node id {:?}", node.id()),
            )
            .at(format!("{source}.id")),
        );
    }
    let origin = node.origin();
    for (field, value) in [
        ("hir_node", &origin.hir_node),
        ("mir_node", &origin.mir_node),
        ("generated_by", &origin.generated_by),
    ] {
        validate_nonblank(value, &format!("{source}.origin.{field}"), diagnostics);
    }
    // Data keys/lineage are application data, not scene identities. Explanation
    // may be empty, and provenance references need not resolve within the scene.
    let bounds = match node {
        SceneNode::Group { bounds, .. }
        | SceneNode::Rect { bounds, .. }
        | SceneNode::Circle { bounds, .. }
        | SceneNode::Line { bounds, .. }
        | SceneNode::Path { bounds, .. }
        | SceneNode::Text { bounds, .. } => bounds,
    };
    validate_bounds(bounds, &format!("{source}.bounds"), diagnostics);
    match node {
        SceneNode::Group {
            transform,
            opacity,
            children,
            ..
        } => {
            validate_point(
                transform.translate,
                &format!("{source}.transform.translate"),
                diagnostics,
            );
            validate_point(
                transform.scale,
                &format!("{source}.transform.scale"),
                diagnostics,
            );
            validate_finite(
                transform.rotate_degrees,
                &format!("{source}.transform.rotate_degrees"),
                diagnostics,
            );
            validate_unit(*opacity, &format!("{source}.opacity"), diagnostics);
            for (index, child) in children.iter().enumerate() {
                validate_node(
                    child,
                    &format!("{source}.children[{index}]"),
                    ids,
                    diagnostics,
                );
            }
        }
        SceneNode::Rect { radius, style, .. } => {
            validate_non_negative(*radius, &format!("{source}.radius"), diagnostics);
            validate_style(style, &format!("{source}.style"), diagnostics);
        }
        SceneNode::Circle {
            center,
            radius,
            style,
            ..
        } => {
            validate_point(*center, &format!("{source}.center"), diagnostics);
            validate_non_negative(*radius, &format!("{source}.radius"), diagnostics);
            validate_style(style, &format!("{source}.style"), diagnostics);
        }
        SceneNode::Line {
            from, to, style, ..
        } => {
            validate_point(*from, &format!("{source}.from"), diagnostics);
            validate_point(*to, &format!("{source}.to"), diagnostics);
            validate_style(style, &format!("{source}.style"), diagnostics);
        }
        SceneNode::Path {
            commands, style, ..
        } => {
            for (index, command) in commands.iter().enumerate() {
                let source = format!("{source}.commands[{index}]");
                match command {
                    PathCommand::Move { to } | PathCommand::Line { to } => {
                        validate_point(*to, &format!("{source}.to"), diagnostics);
                    }
                    PathCommand::Cubic {
                        control1,
                        control2,
                        to,
                    } => {
                        validate_point(*control1, &format!("{source}.control1"), diagnostics);
                        validate_point(*control2, &format!("{source}.control2"), diagnostics);
                        validate_point(*to, &format!("{source}.to"), diagnostics);
                    }
                    PathCommand::Close => {}
                }
            }
            validate_style(style, &format!("{source}.style"), diagnostics);
        }
        SceneNode::Text {
            position,
            font_size,
            color,
            ..
        } => {
            validate_point(*position, &format!("{source}.position"), diagnostics);
            validate_positive(*font_size, &format!("{source}.font_size"), diagnostics);
            validate_color(color, &format!("{source}.color"), diagnostics);
        }
    }
}

fn validate_nonblank(value: &str, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    if value.trim().is_empty() {
        diagnostics.push(
            Diagnostic::new(
                "VIZ-SCENE-0101",
                "identity or origin value must not be blank",
            )
            .at(source),
        );
    }
}

fn validate_bounds(bounds: &Rect, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    validate_finite(bounds.x, &format!("{source}.x"), diagnostics);
    validate_finite(bounds.y, &format!("{source}.y"), diagnostics);
    validate_non_negative(bounds.width, &format!("{source}.width"), diagnostics);
    validate_non_negative(bounds.height, &format!("{source}.height"), diagnostics);
}

fn validate_style(style: &ResolvedStyle, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    validate_color(&style.fill, &format!("{source}.fill"), diagnostics);
    validate_color(&style.stroke, &format!("{source}.stroke"), diagnostics);
    validate_non_negative(
        style.stroke_width,
        &format!("{source}.stroke_width"),
        diagnostics,
    );
    validate_unit(style.opacity, &format!("{source}.opacity"), diagnostics);
}
