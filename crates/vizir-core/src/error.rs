use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            source: None,
            help: None,
        }
    }

    pub fn at(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

#[derive(Debug, Error)]
pub enum VizError {
    #[error("{0}")]
    Diagnostic(String),

    #[error("failed to read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

impl VizError {
    pub fn validation(diagnostics: &[Diagnostic]) -> Self {
        let rendered = diagnostics
            .iter()
            .map(|diagnostic| {
                let source = diagnostic
                    .source
                    .as_ref()
                    .map(|value| format!(" at {value}"))
                    .unwrap_or_default();
                let help = diagnostic
                    .help
                    .as_ref()
                    .map(|value| format!("\n  help: {value}"))
                    .unwrap_or_default();
                format!(
                    "{}: {}{}{}",
                    diagnostic.code, diagnostic.message, source, help
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        Self::Diagnostic(rendered)
    }
}

pub type VizResult<T> = Result<T, VizError>;
