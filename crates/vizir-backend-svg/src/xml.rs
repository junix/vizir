//! XML 1.0 target representability, separate from Scene2D's opaque identities.
use vizir_core::{Scene2D, SceneNode, VizError, VizResult};

// XML 1.0 Fifth Edition, Char production [2]. Rust strings already exclude
// surrogate code points and values above U+10FFFF.
// https://www.w3.org/TR/REC-xml/#charsets
fn validate_string(value: &str, source: &str) -> VizResult<()> {
    for (offset, ch) in value.char_indices() {
        if !matches!(ch, '\u{9}' | '\u{a}' | '\u{d}' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
        {
            return Err(VizError::Diagnostic(format!(
                "VIZ-SVG-0001: XML 1.0 cannot represent U+{:04X} at {source} (UTF-8 byte offset {offset})",
                u32::from(ch)
            )));
        }
    }
    Ok(())
}

/// Check every source string that SVG emits, before building any output. Do not
/// validate metadata that this target does not emit (explanations and losses).
/// Node positions are zero-based preorder ordinals, so diagnostics stay bounded
/// even for deeply nested scenes or very long, private node IDs.
pub(super) fn validate_strings(scene: &Scene2D) -> VizResult<()> {
    validate_string(&scene.document_id, "document_id")?;
    validate_string(&scene.background.0, "background")?;
    let mut stack = vec![scene.nodes.iter()];
    let mut index = 0usize;
    while let Some(nodes) = stack.last_mut() {
        let Some(node) = nodes.next() else {
            stack.pop();
            continue;
        };
        let source = format!("nodes(preorder)[{index}]");
        index += 1;
        validate_string(node.id(), &format!("{source}.id"))?;
        let origin = node.origin();
        for (field, value) in [
            ("hir_node", &origin.hir_node),
            ("mir_node", &origin.mir_node),
            ("generated_by", &origin.generated_by),
        ] {
            validate_string(value, &format!("{source}.origin.{field}"))?;
        }
        if let Some(key) = &origin.data_key {
            validate_string(key, &format!("{source}.origin.data_key"))?;
        }
        for (i, value) in origin.data_lineage.iter().enumerate() {
            validate_string(value, &format!("{source}.origin.data_lineage[{i}]"))?;
        }
        match node {
            SceneNode::Group { children, .. } => stack.push(children.iter()),
            SceneNode::Rect { style, .. }
            | SceneNode::Circle { style, .. }
            | SceneNode::Line { style, .. }
            | SceneNode::Path { style, .. } => {
                validate_string(&style.fill.0, &format!("{source}.style.fill"))?;
                validate_string(&style.stroke.0, &format!("{source}.style.stroke"))?;
            }
            SceneNode::Text { text, color, .. } => {
                validate_string(text, &format!("{source}.text"))?;
                validate_string(&color.0, &format!("{source}.color"))?;
            }
        }
    }
    Ok(())
}
