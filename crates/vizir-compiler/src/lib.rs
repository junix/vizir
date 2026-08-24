mod layout;
mod lower;
mod scene_builder;

use vizir_core::{Document, Scene2D, VizError, VizMir, VizResult, validate_document, validate_mir};

pub use layout::{LayeredLayoutProvider, LayoutProvider, LayoutResult};
pub use lower::lower_to_mir;
pub use scene_builder::build_scene;

#[derive(Debug, Clone)]
pub struct Compilation {
    pub mir: VizMir,
    pub scene: Scene2D,
}

pub fn compile(document: &Document) -> VizResult<Compilation> {
    validate_document(document).map_err(|diagnostics| VizError::validation(&diagnostics))?;
    let mir = lower_to_mir(document)?;
    validate_mir(&mir).map_err(|diagnostics| VizError::validation(&diagnostics))?;
    let scene = build_scene(&mir)?;
    Ok(Compilation { mir, scene })
}
