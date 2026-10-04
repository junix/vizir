//! A bounded, opt-in explorer over an immutable Scene2D snapshot.
//! This module does not evaluate data, change layout, or accept arbitrary HTML.
use std::fmt::{self, Write as _};
use std::io;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use vizir_backend_svg::SvgRenderContext;
use vizir_core::{Origin, PathCommand, Scene2D, SceneNode, VizError, VizResult};

pub const FORMAT: &str = "vizir-interaction/1";
pub const PROFILE: &str = "explorer-v1";
pub const MAX_NODES: usize = 8_192;
pub const MAX_DEPTH: usize = 64;
pub const MAX_PATH_COMMANDS: usize = 262_144;
pub const MAX_STRING_BYTES: usize = 4_096;
pub const MAX_SOURCE_STRING_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_METADATA_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_SCENE_JSON_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_HTML_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_LINEAGE_ENTRIES: usize = 32_768;
pub const RUNTIME: &str = include_str!("../runtime/runtime.mjs");
pub const STYLESHEET: &str = include_str!("../runtime/explorer.css");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerOptions {
    pub instance_key: String,
}
impl Default for ExplorerOptions {
    fn default() -> Self {
        Self {
            instance_key: "main".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CameraPolicy {
    pub min_zoom: f64,
    pub max_zoom: f64,
    pub zoom_factor: f64,
    pub pan_fraction: f64,
    pub drag_threshold_css_px: f64,
}
impl Default for CameraPolicy {
    fn default() -> Self {
        Self {
            min_zoom: 1.0,
            max_zoom: 16.0,
            zoom_factor: 1.25,
            pan_fraction: 0.1,
            drag_threshold_css_px: 4.0,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NodeIdentity {
    pub scene_node_id: String,
    #[serde(deserialize_with = "required_nullable_parent")]
    pub parent_scene_node_id: Option<String>,
    pub dom_id: String,
    pub origin: Origin,
}
fn required_nullable_parent<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(decoder)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InteractionContext {
    pub format: String,
    pub profile: String,
    pub document_id: String,
    pub scene_sha256: String,
    pub runtime_payload_sha256: String,
    pub instance_key: String,
    pub home: Viewport,
    pub camera: CameraPolicy,
    pub nodes: Vec<NodeIdentity>,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ExportManifest {
    pub format: String,
    pub profile: String,
    pub instance_key: String,
    pub scene_sha256: String,
    pub runtime_sha256: String,
    pub runtime_payload_sha256: String,
    pub stylesheet_sha256: String,
    pub node_count: usize,
}
#[derive(Debug)]
pub struct HtmlExport {
    pub html: String,
    pub manifest: ExportManifest,
}

pub fn capabilities() -> vizir_core::BackendCapabilities {
    let mut result = vizir_backend_svg::capabilities();
    result.backend = "html".into();
    result.unsupported.remove("interaction.pointer");
    result.supports.extend(
        [
            "interaction.pointer",
            "interaction.keyboard",
            "interaction.camera.document",
            "interaction.inspect.origin",
            "interaction.select.scene-node",
        ]
        .map(str::to_owned),
    );
    result.unsupported.extend(
        [
            "interaction.select.linked",
            "interaction.chart-domain",
            "runtime.data-update",
        ]
        .map(str::to_owned),
    );
    result.limits.insert("max-nodes".into(), MAX_NODES as u64);
    result
}

pub fn interaction_schema() -> serde_json::Value {
    let mut schema =
        serde_json::to_value(schemars::schema_for!(InteractionContext)).expect("schema serializes");
    schema["properties"]["format"]["const"] = FORMAT.into();
    schema["properties"]["profile"]["const"] = PROFILE.into();
    schema["properties"]["instance_key"]["pattern"] = r"^[a-z][a-z0-9-]{0,31}$(?![\s\S])".into();
    schema["properties"]["scene_sha256"]["pattern"] = r"^[0-9a-f]{64}$(?![\s\S])".into();
    schema["properties"]["runtime_payload_sha256"]["pattern"] = r"^[0-9a-f]{64}$(?![\s\S])".into();
    schema["properties"]["nodes"]["maxItems"] = MAX_NODES.into();
    if let Some(required) = schema["$defs"]["NodeIdentity"]["required"].as_array_mut()
        && !required.iter().any(|field| field == "parent_scene_node_id")
    {
        required.push("parent_scene_node_id".into());
    }
    for name in ["x", "y"] {
        schema["$defs"]["Viewport"]["properties"][name]["const"] = 0.into();
    }
    for name in ["width", "height"] {
        schema["$defs"]["Viewport"]["properties"][name]["minimum"] = 1.into();
        schema["$defs"]["Viewport"]["properties"][name]["maximum"] = 1_000_000.into();
    }
    let camera = serde_json::to_value(CameraPolicy::default()).expect("camera serializes");
    for (key, value) in camera.as_object().expect("camera object") {
        schema["$defs"]["CameraPolicy"]["properties"][key]["const"] = value.clone();
    }
    schema
}

/// Standalone HTML. For multiple instances, use `render_fragment` for each
/// unique key and include `runtime_script()` and `STYLESHEET` once in the host document.
/// Hosts must adopt this package's CSP hashes or stricter equivalent policy.
pub fn render_html(scene: &Scene2D, options: &ExplorerOptions) -> VizResult<HtmlExport> {
    render(scene, options, true)
}
pub fn render_fragment(scene: &Scene2D, options: &ExplorerOptions) -> VizResult<HtmlExport> {
    render(scene, options, false)
}

/// Exact module bytes for a host embedding fragments. The identity covers the
/// unmodified checked-in payload, excluding this generated configuration prefix.
pub fn runtime_script() -> String {
    format!(
        "const VIZIR_RUNTIME_PAYLOAD_SHA256 = \"{}\";\n{}",
        hex(&Sha256::digest(RUNTIME.as_bytes())),
        RUNTIME
    )
}

pub fn content_security_policy() -> String {
    format!(
        "default-src 'none'; script-src 'sha256-{}'; style-src 'sha256-{}'; connect-src 'none'; img-src 'none'; font-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'",
        base64(&Sha256::digest(runtime_script().as_bytes())),
        base64(&Sha256::digest(STYLESHEET.as_bytes()))
    )
}

fn render(scene: &Scene2D, options: &ExplorerOptions, standalone: bool) -> VizResult<HtmlExport> {
    let svg_context = SvgRenderContext::new(&options.instance_key)?;
    // Must precede every recursive validator, serializer and SVG call.
    let nodes = preflight(scene)?;
    vizir_core::validate_scene(scene).map_err(|d| VizError::validation(&d))?;
    let scene_sha256 = scene_hash(scene)?;
    let context = InteractionContext {
        format: FORMAT.into(),
        profile: PROFILE.into(),
        document_id: scene.document_id.clone(),
        scene_sha256: scene_sha256.clone(),
        runtime_payload_sha256: hex(&Sha256::digest(RUNTIME.as_bytes())),
        instance_key: options.instance_key.clone(),
        home: Viewport {
            x: 0.0,
            y: 0.0,
            width: scene.width,
            height: scene.height,
        },
        camera: CameraPolicy::default(),
        nodes: nodes
            .iter()
            .map(|(node, parent)| NodeIdentity {
                scene_node_id: node.id().into(),
                parent_scene_node_id: parent.map(str::to_owned),
                dom_id: svg_context.node_id(node.id()),
                origin: node.origin().clone(),
            })
            .collect(),
    };
    let mut metadata = BoundedBytes::new(MAX_METADATA_BYTES);
    serde_json::to_writer(&mut metadata, &context).map_err(|_| {
        error(
            "0002",
            "metadata JSON exceeds its bounded writer or cannot serialize",
        )
    })?;
    let metadata = std::str::from_utf8(&metadata.bytes).expect("serde_json emits UTF-8");
    let escaped_metadata_bytes = 2 + metadata
        .chars()
        .map(|ch| match ch {
            '<' | '&' | '\u{2028}' | '\u{2029}' => 6,
            _ => ch.len_utf8(),
        })
        .sum::<usize>();
    if escaped_metadata_bytes > MAX_METADATA_BYTES {
        return Err(error("0002", "HTML-safe metadata script exceeds 4 MiB"));
    }
    let mut output = BoundedText::new(MAX_HTML_BYTES);
    if standalone {
        write!(output, "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta http-equiv=\"Content-Security-Policy\" content=\"{}\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>VizIR explorer</title>\n<style>{}</style>\n</head>\n<body>\n", content_security_policy(), STYLESHEET).map_err(output_error)?;
    }
    write_fragment(&mut output, scene, &svg_context, metadata)?;
    if standalone {
        write!(
            output,
            "<script type=\"module\">{}</script>\n</body>\n</html>\n",
            runtime_script()
        )
        .map_err(output_error)?;
    }
    let manifest = ExportManifest {
        format: "html".into(),
        profile: PROFILE.into(),
        instance_key: options.instance_key.clone(),
        scene_sha256,
        runtime_sha256: hex(&Sha256::digest(runtime_script().as_bytes())),
        runtime_payload_sha256: hex(&Sha256::digest(RUNTIME.as_bytes())),
        stylesheet_sha256: hex(&Sha256::digest(STYLESHEET.as_bytes())),
        node_count: nodes.len(),
    };
    Ok(HtmlExport {
        html: output.text,
        manifest,
    })
}

fn write_fragment(
    output: &mut BoundedText,
    scene: &Scene2D,
    namespace: &SvgRenderContext,
    metadata: &str,
) -> VizResult<()> {
    let key = namespace.instance_key();
    writeln!(
        output,
        "<section class=\"vizir-explorer\" data-vizir-explorer=\"1\" data-vizir-instance=\"{key}\">"
    )
    .map_err(output_error)?;
    writeln!(
        output,
        "<div class=\"vizir-toolbar\" role=\"group\" aria-label=\"Visualization controls\">"
    )
    .map_err(output_error)?;
    for (action, label) in [
        ("zoom-in", "Zoom in"),
        ("zoom-out", "Zoom out"),
        ("reset", "Reset view"),
        ("pan", "Pan"),
    ] {
        let pressed = if action == "pan" {
            " aria-pressed=\"false\""
        } else {
            ""
        };
        writeln!(output, "<button type=\"button\" id=\"vzi-{key}-control-{action}\" data-vizir-action=\"{action}\"{pressed}>{label}</button>").map_err(output_error)?;
    }
    writeln!(output, "<label for=\"vzi-{key}-control-picker\">Source element</label><select id=\"vzi-{key}-control-picker\" data-vizir-picker=\"\"><option value=\"\">No selection</option></select>\n<span data-vizir-status=\"\" role=\"status\" aria-live=\"polite\">Static figure. Explorer initializes locally.</span>\n</div>\n<p class=\"vizir-help\">Select a source element to inspect its recorded identity. Pan enables dragging. Focus the viewport for arrow keys, +/− zoom, Home reset, and Escape clear.</p>\n<div class=\"vizir-viewport\" data-vizir-viewport=\"\" tabindex=\"0\" role=\"group\" aria-label=\"Visualization viewport\">").map_err(output_error)?;
    vizir_backend_svg::render_to_with_context(scene, namespace, output)?;
    writeln!(output, "</div>\n<pre data-vizir-inspector=\"\" aria-label=\"Source identity\">No source element selected.</pre>\n<script type=\"application/json\" data-vizir-metadata=\"\">").map_err(output_error)?;
    for ch in metadata.chars() {
        match ch {
            '<' => output.write_str("\\u003c"),
            '&' => output.write_str("\\u0026"),
            '\u{2028}' => output.write_str("\\u2028"),
            '\u{2029}' => output.write_str("\\u2029"),
            _ => output.write_char(ch),
        }
        .map_err(output_error)?;
    }
    writeln!(output, "\n</script>\n</section>").map_err(output_error)?;
    Ok(())
}

fn error(code: &str, message: &str) -> VizError {
    VizError::Diagnostic(format!("VIZ-WEB-{code}: {message}"))
}
fn output_error(_: fmt::Error) -> VizError {
    error("0002", "HTML output exceeds the 32 MiB writer limit")
}

struct BoundedText {
    text: String,
    limit: usize,
}
impl BoundedText {
    fn new(limit: usize) -> Self {
        Self {
            text: String::new(),
            limit,
        }
    }
}
impl fmt::Write for BoundedText {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if s.len() > self.limit.saturating_sub(self.text.len()) {
            return Err(fmt::Error);
        }
        self.text.push_str(s);
        Ok(())
    }
}
struct BoundedBytes {
    bytes: Vec<u8>,
    limit: usize,
}
impl BoundedBytes {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
}
impl io::Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("byte budget"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct HashWriter {
    hash: Sha256,
    count: usize,
}
impl io::Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_SCENE_JSON_BYTES.saturating_sub(self.count) {
            return Err(io::Error::other("scene JSON budget"));
        }
        self.hash.update(bytes);
        self.count += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn scene_hash(scene: &Scene2D) -> VizResult<String> {
    let mut writer = HashWriter {
        hash: Sha256::new(),
        count: 0,
    };
    serde_json::to_writer(&mut writer, scene).map_err(|_| {
        error(
            "0002",
            "canonical scene JSON exceeds 16 MiB or cannot serialize",
        )
    })?;
    Ok(hex(&writer.hash.finalize()))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        result.push(ALPHABET[((value >> 18) & 63) as usize] as char);
        result.push(ALPHABET[((value >> 12) & 63) as usize] as char);
        result.push(if chunk.len() > 1 {
            ALPHABET[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            ALPHABET[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}

/// Conservatively reserve escaped emission bytes, including every duplicate
/// identity/parent occurrence, before cloning metadata or recursive work.
fn preflight(scene: &Scene2D) -> VizResult<Vec<(&SceneNode, Option<&str>)>> {
    if !scene.width.is_finite()
        || !scene.height.is_finite()
        || !(1.0..=1_000_000.0).contains(&scene.width)
        || !(1.0..=1_000_000.0).contains(&scene.height)
    {
        return Err(error(
            "0001",
            "HTML scene dimensions must be in [1, 1000000]",
        ));
    }
    let mut budget = Budget::default();
    budget.string(&scene.document_id)?;
    budget.string(&scene.background.0)?;
    if scene.nodes.len() > MAX_NODES || scene.losses.len() > MAX_NODES {
        return Err(error("0002", "node or loss-record count exceeds 8192"));
    }
    for loss in &scene.losses {
        for value in [&loss.source, &loss.target, &loss.reason] {
            budget.string(value)?;
        }
    }
    let mut nodes = Vec::new();
    let mut stack: Vec<_> = scene
        .nodes
        .iter()
        .rev()
        .map(|n| (n, None, 1usize))
        .collect();
    let mut lineage_entries = 0usize;
    let mut commands = 0usize;
    let mut metadata_reserve = 2048 + scene.document_id.len() * 6;
    let mut svg_reserve = 4096 + scene.document_id.len() * 6;
    while let Some((node, parent, depth)) = stack.pop() {
        if depth > MAX_DEPTH || nodes.len() + stack.len() + node.children().len() + 1 > MAX_NODES {
            return Err(error("0002", "node count or depth exceeds explorer bounds"));
        }
        budget.string(node.id())?;
        let origin = node.origin();
        let mut origin_bytes = 0;
        for value in [
            &origin.hir_node,
            &origin.mir_node,
            &origin.generated_by,
            &origin.explanation,
        ] {
            budget.string(value)?;
            origin_bytes += value.len();
        }
        if let Some(value) = &origin.data_key {
            budget.string(value)?;
            origin_bytes += value.len();
        }
        lineage_entries = lineage_entries
            .checked_add(origin.data_lineage.len())
            .ok_or_else(|| error("0002", "lineage entry budget overflow"))?;
        if lineage_entries > MAX_LINEAGE_ENTRIES {
            return Err(error("0002", "aggregate lineage entries exceed 32768"));
        }
        for value in &origin.data_lineage {
            budget.string(value)?;
            origin_bytes += value.len();
        }
        metadata_reserve += 512
            + node.id().len() * 8
            + parent.map_or(0, str::len) * 6
            + origin_bytes * 6
            + origin.data_lineage.len() * 3;
        svg_reserve +=
            1024 + depth * 4 + node.id().len() * 8 + origin_bytes * 6 + origin.data_lineage.len();
        let style = match node {
            SceneNode::Rect { style, .. }
            | SceneNode::Circle { style, .. }
            | SceneNode::Line { style, .. }
            | SceneNode::Path { style, .. } => Some(style),
            _ => None,
        };
        if let Some(style) = style {
            budget.string(&style.fill.0)?;
            budget.string(&style.stroke.0)?;
            svg_reserve += (style.fill.0.len() + style.stroke.0.len()) * 6;
        }
        if let SceneNode::Text { text, color, .. } = node {
            budget.string(text)?;
            budget.string(&color.0)?;
            svg_reserve += (text.len() + color.0.len()) * 6;
        }
        // Finite f64 decimal formatting is at most 316 bytes per scalar.
        // Reserve regular node numeric fields independently of path commands.
        svg_reserve += 16 * 316;
        if let SceneNode::Path { commands: path, .. } = node {
            commands = commands
                .checked_add(path.len())
                .ok_or_else(|| error("0002", "path command budget overflow"))?;
            if commands > MAX_PATH_COMMANDS {
                return Err(error("0002", "path commands exceed 262144"));
            }
            for command in path {
                svg_reserve += match command {
                    PathCommand::Move { to } | PathCommand::Line { to } => {
                        4 + number_reserve(to.x) + number_reserve(to.y)
                    }
                    PathCommand::Cubic {
                        control1,
                        control2,
                        to,
                    } => {
                        8 + [control1.x, control1.y, control2.x, control2.y, to.x, to.y]
                            .into_iter()
                            .map(number_reserve)
                            .sum::<usize>()
                    }
                    PathCommand::Close => 2,
                };
            }
        }
        if metadata_reserve > MAX_METADATA_BYTES
            || svg_reserve + metadata_reserve * 6 + RUNTIME.len() + STYLESHEET.len() + 16_384
                > MAX_HTML_BYTES
        {
            return Err(error(
                "0002",
                "conservative escaped metadata/output reserve exceeds explorer budget",
            ));
        }
        nodes.push((node, parent));
        stack.extend(
            node.children()
                .iter()
                .rev()
                .map(|child| (child, Some(node.id()), depth + 1)),
        );
    }
    Ok(nodes)
}
fn number_reserve(value: f64) -> usize {
    format!("{value:.4}").len()
}
#[derive(Default)]
struct Budget {
    strings: usize,
}
impl Budget {
    fn string(&mut self, value: &str) -> VizResult<()> {
        if value.len() > MAX_STRING_BYTES {
            return Err(error("0002", "a source string exceeds 4096 UTF-8 bytes"));
        }
        self.strings = self
            .strings
            .checked_add(value.len())
            .ok_or_else(|| error("0002", "source string budget overflow"))?;
        if self.strings > MAX_SOURCE_STRING_BYTES {
            return Err(error("0002", "aggregate source strings exceed 4 MiB"));
        }
        Ok(())
    }
}

impl InteractionContext {
    /// Bounded typed decoding, including duplicate/unknown field rejection.
    /// This checks the export contract, not source authenticity or DOM state.
    pub fn parse(bytes: &[u8]) -> VizResult<Self> {
        if bytes.len() > MAX_METADATA_BYTES {
            return Err(error("0002", "metadata input exceeds 4 MiB"));
        }
        let value: Self = serde_json::from_slice(bytes)?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> VizResult<()> {
        use std::collections::BTreeSet;
        if self.format != FORMAT
            || self.profile != PROFILE
            || self.camera != CameraPolicy::default()
            || self.home.x != 0.0
            || self.home.y != 0.0
            || !self.home.width.is_finite()
            || !self.home.height.is_finite()
            || !(1.0..=1_000_000.0).contains(&self.home.width)
            || !(1.0..=1_000_000.0).contains(&self.home.height)
            || self.runtime_payload_sha256 != hex(&Sha256::digest(RUNTIME.as_bytes()))
            || self.scene_sha256.len() != 64
            || !self
                .scene_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(error(
                "0001",
                "unsupported or invalid interaction profile identity/camera",
            ));
        }
        let namespace = SvgRenderContext::new(&self.instance_key)?;
        let mut budget = Budget::default();
        budget.string(&self.document_id)?;
        if self.document_id.trim().is_empty() || self.nodes.len() > MAX_NODES {
            return Err(error("0001", "invalid document identity or node count"));
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut ancestry: Vec<&str> = Vec::new();
        let mut lineage_count = 0usize;
        for node in &self.nodes {
            budget.string(&node.scene_node_id)?;
            if node.scene_node_id.trim().is_empty()
                || node.dom_id != namespace.node_id(&node.scene_node_id)
                || seen.contains(node.scene_node_id.as_str())
            {
                return Err(error("0001", "invalid or duplicate node identity"));
            }
            match node.parent_scene_node_id.as_deref() {
                None => ancestry.clear(),
                Some(parent) => {
                    budget.string(parent)?;
                    let index = ancestry
                        .iter()
                        .position(|id| *id == parent)
                        .ok_or_else(|| {
                            error("0001", "parent must be active in preorder ancestry")
                        })?;
                    ancestry.truncate(index + 1);
                }
            }
            ancestry.push(&node.scene_node_id);
            if ancestry.len() > MAX_DEPTH {
                return Err(error("0002", "metadata parent depth exceeds 64"));
            }
            seen.insert(&node.scene_node_id);
            let origin = &node.origin;
            for s in [&origin.hir_node, &origin.mir_node, &origin.generated_by] {
                budget.string(s)?;
                if s.trim().is_empty() {
                    return Err(error("0001", "blank required provenance identity"));
                }
            }
            budget.string(&origin.explanation)?;
            if let Some(key) = &origin.data_key {
                budget.string(key)?;
            }
            lineage_count = lineage_count
                .checked_add(origin.data_lineage.len())
                .ok_or_else(|| error("0002", "lineage count overflow"))?;
            if lineage_count > MAX_LINEAGE_ENTRIES {
                return Err(error("0002", "lineage count exceeds 32768"));
            }
            for source in &origin.data_lineage {
                budget.string(source)?;
            }
        }
        let mut writer = BoundedBytes::new(MAX_METADATA_BYTES);
        serde_json::to_writer(&mut writer, self)
            .map_err(|_| error("0002", "metadata exceeds 4 MiB"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    #[test]
    fn sinks_reject_before_growing_past_their_byte_limits() {
        let mut text = BoundedText::new(4);
        text.write_str("é").unwrap();
        assert!(text.write_str("漢").is_err());
        assert_eq!(text.text, "é");
        let mut bytes = BoundedBytes::new(3);
        bytes.write_all(b"ab").unwrap();
        assert!(bytes.write_all(b"cd").is_err());
        assert_eq!(bytes.bytes, b"ab");
        let mut hash = HashWriter {
            hash: Sha256::new(),
            count: MAX_SCENE_JSON_BYTES - 1,
        };
        assert!(hash.write_all(b"ab").is_err());
        assert_eq!(hash.count, MAX_SCENE_JSON_BYTES - 1);
    }
    #[test]
    fn csp_base64_and_payload_identity_have_exact_byte_definitions() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
        let payload_hash = hex(&Sha256::digest(RUNTIME.as_bytes()));
        assert_eq!(
            runtime_script(),
            format!("const VIZIR_RUNTIME_PAYLOAD_SHA256 = \"{payload_hash}\";\n{RUNTIME}")
        );
        assert!(
            content_security_policy()
                .contains(&base64(&Sha256::digest(runtime_script().as_bytes())))
        );
        assert!(!content_security_policy().contains("unsafe-inline"));
        assert!(!RUNTIME.to_ascii_lowercase().contains("</script"));
        assert!(!STYLESHEET.to_ascii_lowercase().contains("</style"));
    }
}
