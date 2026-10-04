//! CLI routing for source HIR and its opt-in durable theme compilation context.
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use vizir_compiler::{
    MAX_THEMED_MIR_JSON_BYTES, ThemeContext, ThemedMir, build_themed_scene, compile,
    compile_with_theme, parse_themed_mir_json, rematerialize_themed_mir,
};
use vizir_core::{
    Document, Scene2D, VizError, VizMir, VizResult, parse_document, validate_document,
};

// Bound the JSON discriminator read before parsing any nested envelope payload.
// serde_json retains its built-in recursion bound; the compiler adds its normal
// typed expression/schema/value/geometry bounds before execution and cloning.
const MAX_JSON_INPUT_BYTES: u64 = MAX_THEMED_MIR_JSON_BYTES as u64;

pub(crate) enum Input {
    Hir(Document, Option<String>),
    Themed(Box<ThemedMir>),
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum Normalized {
    Legacy(Box<VizMir>),
    Themed(Box<ThemedMir>),
}

pub(crate) struct Compiled {
    pub mir: Normalized,
    pub scene: Scene2D,
}

impl Normalized {
    pub fn inner(&self) -> &VizMir {
        match self {
            Self::Legacy(mir) => mir,
            Self::Themed(mir) => &mir.mir,
        }
    }
    pub fn theme(&self) -> Option<&ThemeContext> {
        match self {
            Self::Legacy(_) => None,
            Self::Themed(mir) => Some(&mir.theme),
        }
    }
}

pub(crate) fn read(path: &Path, theme: Option<String>) -> VizResult<Input> {
    if let Some(name) = &theme {
        ThemeContext::resolve(name)?;
    }
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("json"))
    {
        let file = open_regular_json(path)?;
        let mut bytes = Vec::new();
        file.take(MAX_JSON_INPUT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| VizError::Read {
                path: path.display().to_string(),
                source,
            })?;
        if bytes.len() as u64 > MAX_JSON_INPUT_BYTES {
            return Err(VizError::Diagnostic(
                "VIZ-THEME-0004: JSON input exceeds the 32 MiB parsing limit".to_owned(),
            ));
        }
        #[derive(Deserialize)]
        struct Header {
            #[serde(default, deserialize_with = "present")]
            format: Option<serde_json::Value>,
        }
        fn present<'de, D: serde::Deserializer<'de>>(
            d: D,
        ) -> Result<Option<serde_json::Value>, D::Error> {
            serde_json::Value::deserialize(d).map(Some)
        }
        let header: Header = serde_json::from_slice(&bytes)?;
        if header.format.is_some() {
            let mir = parse_themed_mir_json(&bytes)?;
            mir.validate_context()?;
            if let Some(name) = theme
                && name != mir.theme.name
            {
                return Err(VizError::Diagnostic("VIZ-THEME-0006: --theme conflicts with persisted context; re-theme from HIR instead".to_owned()));
            }
            return Ok(Input::Themed(Box::new(mir)));
        }
        return Ok(Input::Hir(serde_json::from_slice(&bytes)?, theme));
    }
    Ok(Input::Hir(parse_document(path)?, theme))
}

impl Input {
    pub fn validate(&self) -> VizResult<String> {
        match self {
            Self::Hir(document, None) => {
                validate_document(document).map_err(|d| VizError::validation(&d))?;
                Ok(format!(
                    "valid: {} (VizHIR {}, {} views)",
                    document.id,
                    document.version,
                    document.views.len()
                ))
            }
            _ => {
                let compiled = self.compile(false)?;
                let mir = compiled.mir.inner();
                let theme = compiled.mir.theme().expect("themed input");
                Ok(format!(
                    "valid: {} (VizHIR {}, {} views, theme {})",
                    mir.document_id,
                    mir.source_hir_version,
                    mir.views.len(),
                    theme.name
                ))
            }
        }
    }

    pub fn compile(&self, refresh: bool) -> VizResult<Compiled> {
        match self {
            Self::Hir(document, None) => {
                let compiled = compile(document)?;
                Ok(Compiled {
                    mir: Normalized::Legacy(Box::new(compiled.mir)),
                    scene: compiled.scene,
                })
            }
            Self::Hir(document, Some(name)) => {
                let compiled = compile_with_theme(document, name)?;
                Ok(Compiled {
                    mir: Normalized::Themed(Box::new(compiled.mir)),
                    scene: compiled.scene,
                })
            }
            Self::Themed(mir) => {
                // Preflight in the compiler happens before any large clone.
                let mir = if refresh {
                    rematerialize_themed_mir(mir)?
                } else {
                    let scene = build_themed_scene(mir)?;
                    return Ok(Compiled {
                        mir: Normalized::Themed(mir.clone()),
                        scene,
                    });
                };
                let scene = build_themed_scene(&mir)?;
                Ok(Compiled {
                    mir: Normalized::Themed(Box::new(mir)),
                    scene,
                })
            }
        }
    }
}

fn open_regular_json(path: &Path) -> VizResult<File> {
    let read_error = |source| VizError::Read {
        path: path.display().to_string(),
        source,
    };
    let nonregular =
        || VizError::Diagnostic("VIZ-THEME-0008: JSON input must be a regular file".to_owned());
    if !std::fs::metadata(path).map_err(read_error)?.is_file() {
        return Err(nonregular());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A FIFO swapped in after metadata must not block this open.
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    }
    let file = options.open(path).map_err(read_error)?;
    if !file.metadata().map_err(read_error)?.is_file() {
        return Err(nonregular());
    }
    Ok(file)
}
