mod chart_layout;
mod layout;
mod lower;
mod materialize;
mod scene_builder;
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
