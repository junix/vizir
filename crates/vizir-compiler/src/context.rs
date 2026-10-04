//! Durable, extensible compiler context around the unchanged executable MIR.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vizir_core::{Document, Scene2D, VizError, VizMir, VizResult};

use crate::text::{FontResources, TextContext, TextLimits, TextSession};
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

    pub fn validate(&self) -> VizResult<()> {
        if let Some(theme) = &self.theme {
            theme.validate()?;
        }
        if let Some(text) = &self.text {
            text.validate()?;
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
        .map(|context| TextSession::new(context, resources, limits))
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
