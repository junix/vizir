//! Explicit composition-owned categorical legend contracts.
//!
//! The owner references member color scales; it never carries a second copy of
//! their domain or range. Placement is separate from legacy local guide orientation.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Frame;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SharedLegendPlacement {
    Bottom,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompositionSharedLegend {
    #[schemars(length(min = 1, max = 16384), regex(pattern = "^[A-Za-z0-9_/-]+$"))]
    #[serde(deserialize_with = "deserialize_id")]
    pub id: String,
    #[serde(deserialize_with = "deserialize_member_ids")]
    #[schemars(length(min = 2, max = 64), extend("uniqueItems" = true))]
    pub members: Vec<String>,
    pub placement: SharedLegendPlacement,
    #[schemars(extend("exclusiveMinimum" = 0))]
    pub height: f64,
    #[schemars(range(min = 0))]
    pub gap: f64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_title"
    )]
    #[schemars(with = "String", length(min = 1, max = 16384))]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SharedLegend {
    #[schemars(length(min = 1, max = 16384), regex(pattern = "^[A-Za-z0-9_/-]+$"))]
    #[serde(deserialize_with = "deserialize_id")]
    pub id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_title"
    )]
    #[schemars(with = "String", length(min = 1, max = 16384))]
    pub title: Option<String>,
    pub placement: SharedLegendPlacement,
    pub frame: Frame,
    #[serde(deserialize_with = "deserialize_member_ids")]
    #[schemars(length(min = 2, max = 64), extend("uniqueItems" = true))]
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MirSharedLegend {
    #[schemars(length(min = 1, max = 16384), regex(pattern = "^[A-Za-z0-9_/-]+$"))]
    #[serde(deserialize_with = "deserialize_id")]
    pub id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_title"
    )]
    #[schemars(with = "String", length(min = 1, max = 16384))]
    pub title: Option<String>,
    pub placement: SharedLegendPlacement,
    pub frame: Frame,
    #[schemars(length(min = 2, max = 64), extend("uniqueItems" = true))]
    pub members: Vec<MirSharedLegendMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MirSharedLegendMember {
    #[schemars(length(min = 1, max = 16384), regex(pattern = "^[A-Za-z0-9_/-]+$"))]
    #[serde(deserialize_with = "deserialize_id")]
    pub view: String,
    #[schemars(length(min = 1, max = 16384), regex(pattern = "^[A-Za-z0-9_/-]+$"))]
    #[serde(deserialize_with = "deserialize_id")]
    pub scale: String,
}

fn deserialize_member_ids<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // YAML must not coerce numbers or booleans into stable member identifiers.
    Vec::<serde_json::Value>::deserialize(deserializer)?
        .into_iter()
        .map(|value| match value {
            serde_json::Value::String(id) => Ok(id),
            _ => Err(serde::de::Error::custom(
                "shared legend member IDs must be strings",
            )),
        })
        .collect()
}

fn deserialize_title<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(title) => {
            validate_title(&title).map_err(serde::de::Error::custom)?;
            Ok(Some(title))
        }
        _ => Err(serde::de::Error::custom(
            "shared legend title must be a string",
        )),
    }
}

pub(crate) fn validate_title(title: &str) -> Result<(), &'static str> {
    if title.is_empty()
        || title.len() > 16_384
        || title
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}'))
    {
        Err(
            "shared legend title must contain 1..=16384 UTF-8 bytes without control characters or line separators",
        )
    } else {
        Ok(())
    }
}

fn deserialize_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(id) => Ok(id),
        _ => Err(serde::de::Error::custom(
            "shared legend IDs and references must be strings",
        )),
    }
}
