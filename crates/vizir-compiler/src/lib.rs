mod chart_layout;
mod layout;
mod lower;
mod materialize;
mod scene_builder;
mod tick_format;

use vizir_core::{Document, Scene2D, VizMir, VizResult};

pub use layout::{LayeredLayoutProvider, LayoutProvider, LayoutResult};
pub use lower::lower_to_mir;
pub use materialize::{MaterializationLimits, rematerialize_mir, rematerialize_mir_with_limits};
pub use scene_builder::{build_scene, build_scene_with_limits};

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
