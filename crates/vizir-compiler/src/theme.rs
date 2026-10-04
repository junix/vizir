//! Opt-in durable compilation context. Existing HIR, MIR and APIs stay unchanged.
use diagram_theme::{Theme, Token};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vizir_core::{
    Color, Document, GeometryNode, MirView, Scene2D, View, VizError, VizMir, VizResult,
};

use crate::materialize::MaterializationLimits;

pub const THEMED_MIR_FORMAT: &str = "vizir-themed-mir/1";
pub const THEME_REGISTRY_REVISION: &str = "1cc4e6667aa86444a7e9055549aa85cac293dc07";
pub const THEME_NAMES: [&str; 14] = Theme::NAMES;

/// Resolved compiler defaults, not a canvas paint instruction or authored styles.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResolvedThemeDefaults {
    pub ink: Color,
    pub muted: Color,
    pub grid: Color,
    pub mark: Color,
    pub series: [Color; 8],
    pub point_interior: Color,
    pub node_fill: Color,
    pub node_stroke: Color,
    pub group_fills: [Color; 8],
    pub edge: Color,
}

/// Identity and exact resolved defaults are checked together before execution.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ThemeContext {
    pub name: String,
    pub registry_spec: String,
    pub registry_version: String,
    pub registry_revision: String,
    pub defaults: ResolvedThemeDefaults,
}

impl ThemeContext {
    pub fn resolve(name: &str) -> VizResult<Self> {
        if name.len() > 64 {
            return Err(theme_error("0001", "theme name exceeds 64 bytes"));
        }
        let theme = Theme::by_name(name).ok_or_else(|| {
            theme_error(
                "0001",
                format!("unknown theme {name:?}; use {}", THEME_NAMES.join(", ")),
            )
        })?;
        let t = theme.tokens();
        let color = |token: Token| Color::hex(token.hex());
        Ok(Self {
            name: name.to_owned(),
            registry_spec: diagram_theme::SPEC.to_owned(),
            registry_version: diagram_theme::REGISTRY_VERSION.to_owned(),
            registry_revision: THEME_REGISTRY_REVISION.to_owned(),
            defaults: ResolvedThemeDefaults {
                ink: color(t.ink),
                muted: color(t.muted),
                grid: color(t.grid),
                mark: color(t.s1),
                series: t.series().map(color),
                point_interior: color(t.paper),
                node_fill: color(t.panel),
                node_stroke: color(t.line),
                group_fills: t
                    .series()
                    .map(|s| Color::hex(&diagram_theme::derive::soften(s, t.panel).to_hex())),
                edge: color(t.edge),
            },
        })
    }

    pub fn validate(&self) -> VizResult<()> {
        if self != &Self::resolve(&self.name)? {
            return Err(theme_error(
                "0002",
                "theme identity, registry pin or resolved defaults differ from the canonical context; normalize again from HIR with --theme",
            ));
        }
        Ok(())
    }
}

/// Versioned context around the existing executable MIR, not a second renderer.
/// Keep this envelope when persisting or refreshing; extracting `mir` alone opts
/// back into the legacy API and loses theme-dependent guide defaults.
#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ThemedMir {
    pub format: String,
    pub theme: ThemeContext,
    pub mir: VizMir,
}

impl ThemedMir {
    pub fn validate_context(&self) -> VizResult<()> {
        if self.format != THEMED_MIR_FORMAT {
            return Err(theme_error(
                "0003",
                format!("unsupported themed MIR format {:?}", self.format),
            ));
        }
        if !matches!(self.mir.version.as_str(), "0.1" | "0.2" | "0.3")
            || self.mir.source_hir_version != self.mir.version
        {
            return Err(theme_error(
                "0003",
                "themed context requires matching supported inner MIR and source HIR versions",
            ));
        }
        vizir_core::validate_mir_capabilities(&self.mir)
            .map_err(|diagnostics| VizError::validation(&diagnostics))?;
        self.theme.validate()
    }
}

#[derive(Debug, Clone)]
pub struct ThemedCompilation {
    pub mir: ThemedMir,
    pub scene: Scene2D,
}

pub fn lower_to_themed_mir(document: &Document, name: &str) -> VizResult<ThemedMir> {
    let theme = ThemeContext::resolve(name)?;
    let limits = check_document_work(document, MaterializationLimits::default())?;
    let mir = crate::lower::lower_to_mir_with_defaults(document, Some(&theme.defaults), limits)?;
    Ok(ThemedMir {
        format: THEMED_MIR_FORMAT.to_owned(),
        theme,
        mir,
    })
}

pub fn compile_with_theme(document: &Document, name: &str) -> VizResult<ThemedCompilation> {
    let mir = lower_to_themed_mir(document, name)?;
    let scene = build_themed_scene(&mir)?;
    Ok(ThemedCompilation { mir, scene })
}

pub fn build_themed_scene(mir: &ThemedMir) -> VizResult<Scene2D> {
    build_themed_scene_with_limits(mir, MaterializationLimits::default())
}

pub fn build_themed_scene_with_limits(
    mir: &ThemedMir,
    limits: MaterializationLimits,
) -> VizResult<Scene2D> {
    mir.validate_context()?;
    let limits = reserve_arrow_work(&mir.mir, limits)?;
    crate::scene_builder::build_scene_with_defaults(&mir.mir, limits, Some(&mir.theme.defaults))
}

pub fn rematerialize_themed_mir(mir: &ThemedMir) -> VizResult<ThemedMir> {
    rematerialize_themed_mir_with_limits(mir, MaterializationLimits::default())
}

pub fn rematerialize_themed_mir_with_limits(
    mir: &ThemedMir,
    limits: MaterializationLimits,
) -> VizResult<ThemedMir> {
    mir.validate_context()?;
    let limits = reserve_arrow_work(&mir.mir, limits)?;
    // The existing executor preflights before cloning, and retains all styles/scales.
    let inner = crate::rematerialize_mir_with_limits(&mir.mir, limits)?;
    Ok(ThemedMir {
        format: mir.format.clone(),
        theme: mir.theme.clone(),
        mir: inner,
    })
}

pub fn themed_mir_schema() -> serde_json::Value {
    let mut schema =
        serde_json::to_value(schemars::schema_for!(ThemedMir)).expect("schema serializes");
    schema["properties"]["format"]["const"] = THEMED_MIR_FORMAT.into();
    schema["$defs"]["ThemeContext"]["properties"]["name"]["enum"] = serde_json::json!(THEME_NAMES);
    schema["$defs"]["ThemeContext"]["properties"]["registry_spec"]["const"] =
        diagram_theme::SPEC.into();
    schema["$defs"]["ThemeContext"]["properties"]["registry_version"]["const"] =
        diagram_theme::REGISTRY_VERSION.into();
    schema["$defs"]["ThemeContext"]["properties"]["registry_revision"]["const"] =
        THEME_REGISTRY_REVISION.into();
    schema["$defs"]["ThemeContext"]["oneOf"] = serde_json::json!(THEME_NAMES.map(
        |name| serde_json::json!({"const": ThemeContext::resolve(name).expect("canonical name")})
    ));
    // Keep the complete legacy wrapper branch, including its two matching
    // version pairs, exact. The isolated VizMirV03 branch pins its own pair.
    schema["$defs"]["VizMir"]["oneOf"][0]["allOf"]
        .as_array_mut()
        .expect("legacy MIR constraints")
        .push(serde_json::json!({"oneOf": [
            {"properties": {"version": {"const": "0.1"}, "source_hir_version": {"const": "0.1"}}},
            {"properties": {"version": {"const": "0.2"}, "source_hir_version": {"const": "0.2"}}}
        ]}));
    close_declared_objects(&mut schema);
    schema
}

pub(crate) fn reserve_arrow_work(
    mir: &VizMir,
    limits: MaterializationLimits,
) -> VizResult<MaterializationLimits> {
    let edges = mir.views.iter().filter_map(|view| match view {
        MirView::Diagram(diagram) => Some(diagram.edges.len()),
        _ => None,
    });
    reserve_edges(edges, limits)
}

fn reserve_edges(
    mut edges: impl Iterator<Item = usize>,
    limits: MaterializationLimits,
) -> VizResult<MaterializationLimits> {
    let commands = edges.try_fold(0usize, |total, count| {
        total.checked_add(count.checked_mul(4)?)
    });
    if commands.is_none_or(|count| {
        count > limits.max_expression_nodes || count as u64 > limits.max_evaluation_steps
    }) {
        return Err(theme_error(
            "0004",
            "generated arrow command work exceeds materialization limits",
        ));
    }
    let commands = commands.expect("checked command count");
    Ok(MaterializationLimits {
        max_expression_nodes: limits.max_expression_nodes - commands,
        max_evaluation_steps: limits.max_evaluation_steps - commands as u64,
        ..limits
    })
}

pub(crate) fn check_document_work(
    document: &Document,
    limits: MaterializationLimits,
) -> VizResult<MaterializationLimits> {
    let limits = reserve_edges(
        document.views.iter().filter_map(|view| match view {
            View::Diagram(diagram) => Some(diagram.edges.len()),
            _ => None,
        }),
        limits,
    )?;
    // Bound recursive geometry before themed lowering, which expands defaults.
    let mut pending = Vec::new();
    for view in &document.views {
        if let View::Geometry(geometry) = view {
            if geometry.children.len() > limits.max_expression_nodes.saturating_sub(pending.len()) {
                return Err(theme_error(
                    "0004",
                    "themed geometry exceeds materialization limits",
                ));
            }
            pending.extend(geometry.children.iter().map(|node| (node, 1usize)));
        }
    }
    let mut count = 0usize;
    while let Some((node, depth)) = pending.pop() {
        count += 1;
        if depth > limits.max_expression_depth.min(64) || count > limits.max_expression_nodes {
            return Err(theme_error(
                "0004",
                "themed geometry exceeds materialization limits",
            ));
        }
        if let GeometryNode::Group { children, .. } = node {
            if children.len()
                > limits
                    .max_expression_nodes
                    .saturating_sub(count + pending.len())
            {
                return Err(theme_error(
                    "0004",
                    "themed geometry exceeds materialization limits",
                ));
            }
            pending.extend(children.iter().map(|child| (child, depth + 1)));
        }
    }
    Ok(limits)
}

pub(crate) fn theme_error(code: &str, message: impl std::fmt::Display) -> VizError {
    VizError::Diagnostic(format!("VIZ-THEME-{code}: {message}"))
}

// New context rejects fields that legacy tagged enums would discard. Tighten
// only copies in this schema; arbitrary inline metadata maps stay open.
fn close_declared_objects(schema: &mut serde_json::Value) {
    match schema {
        serde_json::Value::Object(object) => {
            if object.get("type").is_some_and(|v| v == "object")
                && object.contains_key("properties")
            {
                object
                    .entry("additionalProperties")
                    .or_insert(serde_json::Value::Bool(false));
            }
            for child in object.values_mut() {
                close_declared_objects(child);
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                close_declared_objects(child);
            }
        }
        _ => {}
    }
}
