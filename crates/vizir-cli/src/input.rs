//! CLI routing for HIR and durable, identity-only compilation context.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vizir_compiler::{
    COMPILED_MIR_FORMAT, CompilationContext, CompiledMir, FontResources,
    MAX_COMPILED_MIR_JSON_BYTES, TEXT_MAX_FONT_BYTES, TextContext, TextLayoutContext, ThemeContext,
    ThemedMir, build_themed_scene, compile, compile_compiled_mir, compile_with_context,
    compile_with_theme, parse_compiled_mir_json, parse_text_context_json,
    parse_text_layout_context_json, parse_themed_mir_json, rematerialize_themed_mir,
};
use vizir_core::{
    Document, Scene2D, VizError, VizMir, VizResult, parse_document, validate_document,
};

const MAX_JSON_INPUT_BYTES: usize = MAX_COMPILED_MIR_JSON_BYTES;

#[derive(Debug, Default, clap::Args)]
pub(crate) struct TextOptions {
    /// Opt in from HIR using a strict, identity-only measured-text JSON profile.
    #[arg(long, value_name = "PATH")]
    pub text_profile: Option<PathBuf>,
    /// Opt in from HIR using a strict geometry-only text-wrapping JSON policy.
    #[arg(long, value_name = "PATH")]
    pub text_layout: Option<PathBuf>,
    /// Exact font bytes for a profile face; repeat for each distinct SHA-256.
    #[arg(long = "font", value_name = "SHA256=PATH")]
    pub fonts: Vec<String>,
}

pub(crate) struct Input {
    source: Source,
    resources: FontResources,
    resource_paths: Vec<PathBuf>,
}

enum Source {
    Hir(
        Box<Document>,
        Option<String>,
        Option<Box<TextContext>>,
        Option<Box<TextLayoutContext>>,
    ),
    Themed(Box<ThemedMir>),
    Compiled(Box<CompiledMir>),
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum Normalized {
    Legacy(Box<VizMir>),
    Themed(Box<ThemedMir>),
    Compiled(Box<CompiledMir>),
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
            Self::Compiled(mir) => &mir.mir,
        }
    }
    pub fn theme(&self) -> Option<&ThemeContext> {
        match self {
            Self::Legacy(_) => None,
            Self::Themed(mir) => Some(&mir.theme),
            Self::Compiled(mir) => mir.context.theme.as_ref(),
        }
    }
    pub fn context(&self) -> Option<&CompilationContext> {
        match self {
            Self::Compiled(mir) => Some(&mir.context),
            _ => None,
        }
    }
    pub fn context_format(&self) -> Option<&str> {
        match self {
            Self::Legacy(_) => None,
            Self::Themed(mir) => Some(&mir.format),
            Self::Compiled(mir) => Some(&mir.format),
        }
    }
}

pub(crate) fn read(path: &Path, theme: Option<String>, options: TextOptions) -> VizResult<Input> {
    if let Some(name) = &theme {
        ThemeContext::resolve(name)?;
    }
    let mut resource_paths = Vec::new();
    let profile = if let Some(path) = &options.text_profile {
        let bytes = read_bounded_regular(path, MAX_JSON_INPUT_BYTES, "text profile")?;
        resource_paths.push(path.clone());
        Some(parse_text_context_json(&bytes)?)
    } else {
        None
    };
    let layout = if let Some(path) = &options.text_layout {
        let bytes = read_bounded_regular(path, MAX_JSON_INPUT_BYTES, "text layout")?;
        resource_paths.push(path.clone());
        Some(parse_text_layout_context_json(&bytes)?)
    } else {
        None
    };
    let source = if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("json"))
    {
        let bytes = read_bounded_regular(path, MAX_JSON_INPUT_BYTES, "JSON input")?;
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
        match header.format {
            Some(format)
                if format.as_str().is_some_and(|s| {
                    s == COMPILED_MIR_FORMAT || s.starts_with("vizir-compiled-mir/")
                }) =>
            {
                let mir = parse_compiled_mir_json(&bytes)?;
                if let Some(name) = &theme
                    && mir.context.theme.as_ref().map(|theme| &theme.name) != Some(name)
                {
                    return Err(context_conflict("--theme"));
                }
                if let Some(profile) = &profile
                    && mir.context.text.as_ref() != Some(profile)
                {
                    return Err(context_conflict("--text-profile"));
                }
                if let Some(layout) = &layout
                    && mir.context.text_layout.as_ref() != Some(layout)
                {
                    return Err(context_conflict("--text-layout"));
                }
                Source::Compiled(Box::new(mir))
            }
            Some(_) => {
                let mir = parse_themed_mir_json(&bytes)?;
                if let Some(name) = &theme
                    && name != &mir.theme.name
                {
                    return Err(VizError::Diagnostic("VIZ-THEME-0006: --theme conflicts with persisted context; re-theme from HIR instead".to_owned()));
                }
                if profile.is_some() {
                    return Err(context_conflict("--text-profile"));
                }
                if layout.is_some() {
                    return Err(context_conflict("--text-layout"));
                }
                Source::Themed(Box::new(mir))
            }
            None => Source::Hir(
                Box::new(serde_json::from_slice(&bytes)?),
                theme,
                profile.map(Box::new),
                layout.map(Box::new),
            ),
        }
    } else {
        Source::Hir(
            Box::new(parse_document(path)?),
            theme,
            profile.map(Box::new),
            layout.map(Box::new),
        )
    };
    let has_text = match &source {
        Source::Hir(_, _, profile, _) => profile.is_some(),
        Source::Themed(_) => false,
        Source::Compiled(mir) => mir.context.text.is_some(),
    };
    if !has_text && options.text_layout.is_some() {
        return Err(VizError::Diagnostic(
            "VIZ-CONTEXT-0006: --text-layout requires a measured --text-profile or persisted measured context".to_owned(),
        ));
    }
    if !has_text && !options.fonts.is_empty() {
        return Err(VizError::Diagnostic("VIZ-CONTEXT-0006: --font requires a measured --text-profile or persisted measured context".to_owned()));
    }
    let mut resources = FontResources::new();
    let mut hashes = std::collections::BTreeSet::new();
    for mapping in &options.fonts {
        let (hash, file) = mapping.split_once('=').ok_or_else(|| {
            VizError::Diagnostic("VIZ-CONTEXT-0008: --font must be SHA256=PATH".to_owned())
        })?;
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || file.is_empty()
        {
            return Err(VizError::Diagnostic("VIZ-CONTEXT-0008: --font requires a lowercase 64-digit SHA-256 and a nonempty path".to_owned()));
        }
        if !hashes.insert(hash) {
            return Err(VizError::Diagnostic(format!(
                "VIZ-CONTEXT-0008: duplicate --font mapping for {hash}"
            )));
        }
        let file = PathBuf::from(file);
        let bytes = read_bounded_regular(&file, TEXT_MAX_FONT_BYTES, "font resource")?;
        resources.insert(hash, bytes)?;
        resource_paths.push(file);
    }
    Ok(Input {
        source,
        resources,
        resource_paths,
    })
}

impl Input {
    pub fn check_destinations(
        &self,
        input: &Path,
        output: Option<&Path>,
        manifest: Option<&Path>,
    ) -> VizResult<()> {
        crate::paths::check_destinations(input, output, manifest)?;
        // Reuse the canonical/symlink/hard-link checks for every explicit source.
        for path in &self.resource_paths {
            crate::paths::check_destinations(path, output, manifest)?;
        }
        Ok(())
    }

    pub fn validate(&self) -> VizResult<String> {
        match &self.source {
            Source::Hir(document, None, None, None) => {
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
                let mut context = String::new();
                if let Some(theme) = compiled.mir.theme() {
                    context.push_str(&format!(", theme {}", theme.name));
                }
                if let Some(text) = compiled
                    .mir
                    .context()
                    .and_then(|context| context.text.as_ref())
                {
                    context.push_str(&format!(", text {}", text.profile));
                }
                if let Some(layout) = compiled
                    .mir
                    .context()
                    .and_then(|context| context.text_layout.as_ref())
                {
                    context.push_str(&format!(", text layout {}", layout.profile));
                }
                Ok(format!(
                    "valid: {} (VizHIR {}, {} views{})",
                    mir.document_id,
                    mir.source_hir_version,
                    mir.views.len(),
                    context
                ))
            }
        }
    }

    pub fn compile(&self, refresh: bool) -> VizResult<Compiled> {
        match &self.source {
            Source::Hir(document, None, None, None) => {
                let compiled = compile(document)?;
                Ok(Compiled {
                    mir: Normalized::Legacy(Box::new(compiled.mir)),
                    scene: compiled.scene,
                })
            }
            Source::Hir(document, Some(name), None, None) => {
                let compiled = compile_with_theme(document, name)?;
                Ok(Compiled {
                    mir: Normalized::Themed(Box::new(compiled.mir)),
                    scene: compiled.scene,
                })
            }
            Source::Hir(document, theme, Some(text), layout) => {
                let mut context = CompilationContext::new().with_text(text.as_ref().clone());
                if let Some(theme) = theme {
                    context = context.with_theme(ThemeContext::resolve(theme)?);
                }
                if let Some(layout) = layout {
                    context = context.with_text_layout(layout.as_ref().clone());
                }
                let compiled = compile_with_context(document, &context, &self.resources)?;
                Ok(Compiled {
                    mir: Normalized::Compiled(Box::new(compiled.mir)),
                    scene: compiled.scene,
                })
            }
            Source::Hir(_, _, None, Some(_)) => Err(VizError::Diagnostic(
                "VIZ-CONTEXT-0006: text_layout requires measured text context".to_owned(),
            )),
            Source::Themed(mir) => {
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
            Source::Compiled(mir) => {
                let compiled = compile_compiled_mir(mir, &self.resources, refresh)?;
                Ok(Compiled {
                    mir: Normalized::Compiled(Box::new(compiled.mir)),
                    scene: compiled.scene,
                })
            }
        }
    }
}

fn context_conflict(flag: &str) -> VizError {
    VizError::Diagnostic(format!(
        "VIZ-CONTEXT-0006: {flag} cannot add or change persisted layout context; compile the original HIR with the desired profile and theme instead"
    ))
}

fn read_bounded_regular(path: &Path, limit: usize, role: &str) -> VizResult<Vec<u8>> {
    // Retain established JSON diagnostics for the legacy themed boundary.
    let prefix = if role == "JSON input" {
        "VIZ-THEME"
    } else {
        "VIZ-CONTEXT"
    };
    read_bounded_regular_with_prefix(path, limit, role, prefix)
}

pub(crate) use crate::resource_io::read_bounded_regular_with_prefix;
