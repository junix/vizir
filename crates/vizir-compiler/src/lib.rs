mod chart_layout;
mod context;
mod context_wire;
mod heatmap;
mod layout;
mod lower;
mod materialize;
mod plot_alignment;
mod scene_builder;
mod shared_legend;
mod text;
mod text_layout;
mod theme;
mod theme_wire;
mod tick_format;

use vizir_core::{Document, Scene2D, VizMir, VizResult};

pub use layout::{LayeredLayoutProvider, LayoutProvider, LayoutResult};
pub use lower::lower_to_mir;
pub use materialize::{MaterializationLimits, rematerialize_mir, rematerialize_mir_with_limits};
pub use scene_builder::{build_scene, build_scene_with_limits};
pub use theme::{
    ResolvedThemeDefaults, THEME_NAMES, THEME_REGISTRY_REVISION, THEMED_MIR_FORMAT, ThemeContext,
    ThemedCompilation, ThemedMir, build_themed_scene, build_themed_scene_with_limits,
    compile_with_theme, lower_to_themed_mir, rematerialize_themed_mir,
    rematerialize_themed_mir_with_limits, themed_mir_schema,
};

#[derive(Debug, Clone)]
pub struct Compilation {
    pub mir: VizMir,
    pub scene: Scene2D,
}

pub fn compile(document: &Document) -> VizResult<Compilation> {
    let mir = lower_to_mir(document)?;
    let scene = build_scene(&mir)?;
    Ok(Compilation { mir, scene })
}

pub use theme_wire::{MAX_THEMED_MIR_JSON_BYTES, parse_themed_mir_json};

pub use context::{
    COMPILED_MIR_FORMAT, CompilationContext, CompiledMir, ContextCompilation, build_compiled_scene,
    build_compiled_scene_with_limits, compile_compiled_mir, compile_compiled_mir_with_limits,
    compile_with_context, compile_with_context_with_limits, compiled_mir_schema,
    lower_to_compiled_mir, lower_to_compiled_mir_with_limits, rematerialize_compiled_mir,
    rematerialize_compiled_mir_with_limits,
};
pub use context_wire::{
    MAX_COMPILED_MIR_JSON_BYTES, parse_compiled_mir_json, parse_text_context_json,
};
pub use text::{
    FontFace, FontResources, TEXT_ENGINE, TEXT_MAX_FONT_BYTES, TEXT_MAX_FONT_TOTAL_BYTES,
    TEXT_MAX_OUTPUT_BYTES, TEXT_PROFILE, TextContext, TextFaces, TextLimits,
};

pub use context_wire::parse_text_layout_context_json;
pub use text_layout::{
    TEXT_LAYOUT_ENGINE, TEXT_LAYOUT_PROFILE, TextLayoutContext, TextLayoutTarget,
};

pub use text_layout::{SemanticTextLayoutTarget, TEXT_LAYOUT_SEMANTIC_PROFILE, TextLayoutRole};

pub use text_layout::{TEXT_LAYOUT_CATEGORY_PROFILE, TEXT_LAYOUT_HEATMAP_PROFILE};

pub use text_layout::{DiagramTextLayoutTarget, TEXT_LAYOUT_DIAGRAM_PROFILE};
