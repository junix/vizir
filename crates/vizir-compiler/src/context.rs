//! Durable, extensible compiler context around the unchanged executable MIR.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vizir_core::{Document, Scene2D, VizError, VizMir, VizResult};

use crate::text::{FontResources, TextContext, TextLimits, TextSession};
use crate::text_layout::TextLayoutContext;
use crate::{MaterializationLimits, ThemeContext};

pub const COMPILED_MIR_FORMAT: &str = "vizir-compiled-mir/1";

/// Exact identities needed to reproduce compiler decisions. Resource paths and
/// font bytes are deliberately separate and must be supplied again on replay.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompilationContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemeContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_layout: Option<TextLayoutContext>,
}

impl CompilationContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_theme(mut self, theme: ThemeContext) -> Self {
        self.theme = Some(theme);
        self
    }

    pub fn with_text(mut self, text: TextContext) -> Self {
        self.text = Some(text);
        self
    }

    pub fn with_text_layout(mut self, text_layout: TextLayoutContext) -> Self {
        self.text_layout = Some(text_layout);
        self
    }

    pub fn validate(&self) -> VizResult<()> {
        if let Some(theme) = &self.theme {
            theme.validate()?;
        }
        if let Some(text) = &self.text {
            text.validate()?;
        }
        if let Some(layout) = &self.text_layout {
            if self.text.is_none() {
                return Err(context_error(
                    "0006",
                    "text_layout requires measured text context",
                ));
            }
            layout.validate()?;
        }
        Ok(())
    }
}

/// Keep this envelope to retain the context used during HIR layout. Adding or
/// changing layout context requires compiling the original HIR again.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompiledMir {
    pub format: String,
    pub context: CompilationContext,
    pub mir: VizMir,
}

impl CompiledMir {
    pub fn new(context: CompilationContext, mir: VizMir) -> VizResult<Self> {
        let result = Self {
            format: COMPILED_MIR_FORMAT.to_owned(),
            context,
            mir,
        };
        result.validate_context()?;
        Ok(result)
    }

    /// Adapt the legacy theme-only envelope without inventing text context or
    /// changing its existing APIs, schema, materialization or layout decisions.
    pub fn from_themed(mir: crate::ThemedMir) -> VizResult<Self> {
        mir.validate_context()?;
        Self::new(CompilationContext::new().with_theme(mir.theme), mir.mir)
    }

    pub fn validate_context(&self) -> VizResult<()> {
        if self.format != COMPILED_MIR_FORMAT {
            return Err(context_error(
                "0001",
                format!("unsupported compiled MIR format {:?}", self.format),
            ));
        }
        if !matches!(self.mir.version.as_str(), "0.1" | "0.2")
            || self.mir.source_hir_version != self.mir.version
        {
            return Err(context_error(
                "0001",
                "compiled context requires matching supported inner MIR and source HIR versions",
            ));
        }
        self.context.validate()
    }
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ContextCompilation {
    pub mir: CompiledMir,
    pub scene: Scene2D,
}

impl ContextCompilation {
    pub fn new(mir: CompiledMir, scene: Scene2D) -> Self {
        Self { mir, scene }
    }
}

pub fn lower_to_compiled_mir(
    document: &Document,
    context: &CompilationContext,
    resources: &FontResources,
) -> VizResult<CompiledMir> {
    lower_to_compiled_mir_with_limits(
        document,
        context,
        resources,
        MaterializationLimits::default(),
        TextLimits::default(),
    )
}

pub fn lower_to_compiled_mir_with_limits(
    document: &Document,
    context: &CompilationContext,
    resources: &FontResources,
    limits: MaterializationLimits,
    text_limits: TextLimits,
) -> VizResult<CompiledMir> {
    // Normalization promises executable text context even for geometry/diagram
    // labels that were not measured during chart layout. Validate the complete
    // scene through one session before returning the persisted MIR.
    Ok(compile_with_context_with_limits(document, context, resources, limits, text_limits)?.mir)
}

pub fn compile_with_context(
    document: &Document,
    context: &CompilationContext,
    resources: &FontResources,
) -> VizResult<ContextCompilation> {
    compile_with_context_with_limits(
        document,
        context,
        resources,
        MaterializationLimits::default(),
        TextLimits::default(),
    )
}

pub fn compile_with_context_with_limits(
    document: &Document,
    context: &CompilationContext,
    resources: &FontResources,
    limits: MaterializationLimits,
    text_limits: TextLimits,
) -> VizResult<ContextCompilation> {
    context.validate()?;
    let text = session(context, resources, text_limits)?;
    // One session shares all shaping and outline budgets across both phases.
    let mir = lower(document, context, limits, text.as_ref())?;
    let scene = scene(&mir, limits, text.as_ref())?;
    Ok(ContextCompilation { mir, scene })
}

pub fn build_compiled_scene(mir: &CompiledMir, resources: &FontResources) -> VizResult<Scene2D> {
    build_compiled_scene_with_limits(
        mir,
        resources,
        MaterializationLimits::default(),
        TextLimits::default(),
    )
}

pub fn build_compiled_scene_with_limits(
    mir: &CompiledMir,
    resources: &FontResources,
    limits: MaterializationLimits,
    text_limits: TextLimits,
) -> VizResult<Scene2D> {
    mir.validate_context()?;
    let text = session(&mir.context, resources, text_limits)?;
    scene(mir, limits, text.as_ref())
}

pub fn rematerialize_compiled_mir(
    mir: &CompiledMir,
    resources: &FontResources,
) -> VizResult<CompiledMir> {
    rematerialize_compiled_mir_with_limits(
        mir,
        resources,
        MaterializationLimits::default(),
        TextLimits::default(),
    )
}

pub fn rematerialize_compiled_mir_with_limits(
    mir: &CompiledMir,
    resources: &FontResources,
    limits: MaterializationLimits,
    text_limits: TextLimits,
) -> VizResult<CompiledMir> {
    Ok(compile_compiled_mir_with_limits(mir, resources, true, limits, text_limits)?.mir)
}

/// Validate persisted materialization or refresh data without reflowing layout,
/// then build the scene with the same text session and exact persisted context.
pub fn compile_compiled_mir(
    mir: &CompiledMir,
    resources: &FontResources,
    refresh: bool,
) -> VizResult<ContextCompilation> {
    compile_compiled_mir_with_limits(
        mir,
        resources,
        refresh,
        MaterializationLimits::default(),
        TextLimits::default(),
    )
}

pub fn compile_compiled_mir_with_limits(
    mir: &CompiledMir,
    resources: &FontResources,
    refresh: bool,
    limits: MaterializationLimits,
    text_limits: TextLimits,
) -> VizResult<ContextCompilation> {
    mir.validate_context()?;
    let text = session(&mir.context, resources, text_limits)?;
    if refresh {
        // Reserve generated arrow work and preflight before any large clone.
        let inner = crate::rematerialize_mir_with_limits(
            &mir.mir,
            crate::theme::reserve_arrow_work(&mir.mir, limits)?,
        )?;
        let refreshed = CompiledMir::new(mir.context.clone(), inner)?;
        let scene = scene(&refreshed, limits, text.as_ref())?;
        Ok(ContextCompilation {
            mir: refreshed,
            scene,
        })
    } else {
        let scene = scene(mir, limits, text.as_ref())?;
        Ok(ContextCompilation {
            mir: mir.clone(),
            scene,
        })
    }
}

fn session(
    context: &CompilationContext,
    resources: &FontResources,
    limits: TextLimits,
) -> VizResult<Option<TextSession>> {
    context
        .text
        .as_ref()
        .map(|text| {
            TextSession::new_with_layout(text, resources, limits, context.text_layout.as_ref())
        })
        .transpose()
}

fn lower(
    document: &Document,
    context: &CompilationContext,
    limits: MaterializationLimits,
    text: Option<&TextSession>,
) -> VizResult<CompiledMir> {
    let limits = crate::theme::check_document_work(document, limits)?;
    let mir = crate::lower::lower_to_mir_with_context(
        document,
        context.theme.as_ref().map(|theme| &theme.defaults),
        limits,
        text,
    )?;
    CompiledMir::new(context.clone(), mir)
}

fn scene(
    mir: &CompiledMir,
    limits: MaterializationLimits,
    text: Option<&TextSession>,
) -> VizResult<Scene2D> {
    let limits = crate::theme::reserve_arrow_work(&mir.mir, limits)?;
    crate::scene_builder::build_scene_with_context(
        &mir.mir,
        limits,
        mir.context.theme.as_ref().map(|theme| &theme.defaults),
        text,
    )
}

pub fn compiled_mir_schema() -> serde_json::Value {
    let mut schema =
        serde_json::to_value(schemars::schema_for!(CompiledMir)).expect("schema serializes");
    schema["properties"]["format"]["const"] = COMPILED_MIR_FORMAT.into();
    let themed = crate::themed_mir_schema();
    for definition in ["ThemeContext", "VizMir"] {
        schema["$defs"][definition] = themed["$defs"][definition].clone();
    }
    schema["$defs"]["TextContext"]["properties"]["profile"]["const"] =
        crate::text::TEXT_PROFILE.into();
    schema["$defs"]["TextContext"]["properties"]["engine"]["const"] =
        crate::text::TEXT_ENGINE.into();
    schema["$defs"]["TextContext"]["properties"]["shaping_language"]["const"] = "default".into();
    schema["$defs"]["TextContext"]["properties"]["declared_locale"]["minLength"] = 2.into();
    schema["$defs"]["TextContext"]["properties"]["declared_locale"]["maxLength"] = 32.into();
    schema["$defs"]["TextContext"]["properties"]["declared_locale"]["pattern"] =
        "^[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*$".into();
    schema["$defs"]["FontFace"]["properties"]["sha256"]["pattern"] = "^[0-9a-f]{64}$".into();
    schema["$defs"]["FontFace"]["properties"]["face_index"]["maximum"] = 31.into();
    schema["$defs"]["FontFace"]["properties"]["weight"]["enum"] =
        serde_json::json!([400, 500, 700]);
    for (role, weight) in [("regular", 400), ("medium", 500), ("bold", 700)] {
        schema["$defs"]["TextFaces"]["properties"][role]["properties"]["weight"]["const"] =
            weight.into();
    }
    let mut geometry_layout = schema["$defs"]["TextLayoutContext"].clone();
    geometry_layout["properties"]
        .as_object_mut()
        .expect("properties object")
        .remove("semantic_targets");
    geometry_layout["properties"]["profile"]["const"] =
        crate::text_layout::TEXT_LAYOUT_PROFILE.into();
    geometry_layout["properties"]["engine"]["const"] =
        crate::text_layout::TEXT_LAYOUT_ENGINE.into();
    geometry_layout["properties"]["targets"]["minItems"] = 1.into();
    geometry_layout["properties"]["targets"]["maxItems"] = crate::text_layout::MAX_TARGETS.into();
    geometry_layout["properties"]["targets"]["uniqueItems"] = true.into();
    let mut semantic_layout = geometry_layout.clone();
    semantic_layout["properties"]["profile"]["const"] =
        crate::text_layout::TEXT_LAYOUT_SEMANTIC_PROFILE.into();
    semantic_layout["properties"]["targets"]["minItems"] = 0.into();
    semantic_layout["properties"]["targets"]["maxItems"] = crate::text_layout::MAX_TARGETS.into();
    semantic_layout["properties"]["semantic_targets"] = serde_json::json!({
        "type": "array",
        "items": {"$ref": "#/$defs/SemanticTextLayoutTarget"},
        "minItems": 1,
        "maxItems": crate::text_layout::MAX_TARGETS,
        "uniqueItems": true
    });
    semantic_layout["required"]
        .as_array_mut()
        .expect("required array")
        .push("semantic_targets".into());
    let mut category_layout = semantic_layout.clone();
    category_layout["properties"]["profile"]["const"] =
        crate::text_layout::TEXT_LAYOUT_CATEGORY_PROFILE.into();
    category_layout["properties"]["semantic_targets"]["items"]["$ref"] =
        "#/$defs/CategorySemanticTextLayoutTarget".into();
    category_layout["properties"]["semantic_targets"]["contains"] = serde_json::json!({
        "type": "object",
        "properties": {"role": {"const": "bar.category_labels"}},
        "required": ["role"],
        "additionalProperties": true
    });
    // The combined array length, source identity duplicates and UTF-8 byte ID
    // limits are executable checks; the schema bounds each array independently.
    schema["$defs"]["TextLayoutContext"] = serde_json::json!({"oneOf": [
        {"$ref": "#/$defs/GeometryTextLayoutContext"},
        {"$ref": "#/$defs/SemanticTextLayoutContext"},
        {"$ref": "#/$defs/CategoryTextLayoutContext"}
    ]});
    schema["$defs"]["GeometryTextLayoutContext"] = geometry_layout;
    schema["$defs"]["SemanticTextLayoutContext"] = semantic_layout;
    schema["$defs"]["CategoryTextLayoutContext"] = category_layout;
    for (definition, ids) in [
        ("TextLayoutTarget", &["view_id", "node_id"][..]),
        ("SemanticTextLayoutTarget", &["view_id"][..]),
    ] {
        for id in ids {
            schema["$defs"][definition]["properties"][*id]["minLength"] = 1.into();
            // JSON Schema counts Unicode characters; runtime also caps UTF-8 bytes.
            schema["$defs"][definition]["properties"][*id]["maxLength"] = 256.into();
        }
        for dimension in ["max_width", "line_height"] {
            schema["$defs"][definition]["properties"][dimension]["minimum"] = 0.25.into();
            schema["$defs"][definition]["properties"][dimension]["maximum"] = 1_000_000.into();
        }
        schema["$defs"][definition]["properties"]["max_lines"]["minimum"] = 1.into();
        schema["$defs"][definition]["properties"]["max_lines"]["maximum"] =
            crate::text_layout::MAX_LINES.into();
    }
    // Preserve the published v2 target and role definitions exactly. Widening
    // their shared role reference would silently admit v3 targets to v2.
    schema["$defs"]["CategoryTextLayoutRole"] = schema["$defs"]["TextLayoutRole"].clone();
    schema["$defs"]["TextLayoutRole"]["enum"] = serde_json::json!(["chart.title"]);
    let mut category_target = schema["$defs"]["SemanticTextLayoutTarget"].clone();
    category_target["properties"]["role"]["$ref"] = "#/$defs/CategoryTextLayoutRole".into();
    schema["$defs"]["CategorySemanticTextLayoutTarget"] = category_target;
    // Null is an accepted spelling of absence. A present non-null layout policy
    // requires the measured-text identity rather than just a nullable text key.
    schema["$defs"]["CompilationContext"]["allOf"] = serde_json::json!([{
        "if": {
            "required": ["text_layout"],
            "properties": {"text_layout": {"type": "object"}}
        },
        "then": {
            "required": ["text"],
            "properties": {"text": {"type": "object"}}
        }
    }]);
    close_declared_objects(&mut schema);
    schema
}

pub(crate) fn context_error(code: &str, message: impl std::fmt::Display) -> VizError {
    VizError::Diagnostic(format!("VIZ-CONTEXT-{code}: {message}"))
}

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
