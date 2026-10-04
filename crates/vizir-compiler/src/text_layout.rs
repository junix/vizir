//! Explicit source-targeted wrapping policy. Source ranges are byte preserving.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, ops::Range};
use unicode_segmentation::UnicodeSegmentation;
use vizir_core::{VizError, VizResult};

pub const TEXT_LAYOUT_PROFILE: &str = "vizir-text-wrap/1";
pub const TEXT_LAYOUT_SEMANTIC_PROFILE: &str = "vizir-text-wrap/2";
pub const TEXT_LAYOUT_ENGINE: &str =
    "unicode-linebreak/0.1.5(unicode15.0.0);unicode-segmentation/1.13.3(unicode17.0.0)";
pub(crate) const MAX_LINES: usize = 256;
pub(crate) const MAX_TARGETS: usize = 256;

pub(crate) fn error(detail: impl std::fmt::Display) -> VizError {
    VizError::Diagnostic(format!("VIZ-TEXT-0007: {detail}"))
}

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextLayoutTarget {
    pub view_id: String,
    pub node_id: String,
    pub max_width: f64,
    #[serde(deserialize_with = "crate::text::deserialize_u32_number")]
    pub max_lines: u32,
    pub line_height: f64,
}
impl TextLayoutTarget {
    pub fn new(
        view_id: impl Into<String>,
        node_id: impl Into<String>,
        max_width: f64,
        max_lines: u32,
        line_height: f64,
    ) -> Self {
        Self {
            view_id: view_id.into(),
            node_id: node_id.into(),
            max_width,
            max_lines,
            line_height,
        }
    }
}

/// A supported semantic text slot, resolved against original source fields.
#[non_exhaustive]
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub enum TextLayoutRole {
    #[serde(rename = "chart.title")]
    ChartTitle,
}

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SemanticTextLayoutTarget {
    pub view_id: String,
    pub role: TextLayoutRole,
    pub max_width: f64,
    #[serde(deserialize_with = "crate::text::deserialize_u32_number")]
    pub max_lines: u32,
    pub line_height: f64,
}
impl SemanticTextLayoutTarget {
    pub fn chart_title(
        view_id: impl Into<String>,
        max_width: f64,
        max_lines: u32,
        line_height: f64,
    ) -> Self {
        Self {
            view_id: view_id.into(),
            role: TextLayoutRole::ChartTitle,
            max_width,
            max_lines,
            line_height,
        }
    }
}

// None means the field was absent. Explicit null must not erase the distinction
// needed to keep all semantic-target spellings outside the frozen v1 contract.
fn deserialize_present_semantic_targets<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<SemanticTextLayoutTarget>>, D::Error> {
    Vec::<SemanticTextLayoutTarget>::deserialize(deserializer).map(Some)
}

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextLayoutContext {
    pub profile: String,
    pub engine: String,
    pub targets: Vec<TextLayoutTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_targets: Option<Vec<SemanticTextLayoutTarget>>,
}
impl<'de> Deserialize<'de> for TextLayoutContext {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            profile: String,
            engine: String,
            targets: Vec<TextLayoutTarget>,
            #[serde(default, deserialize_with = "deserialize_present_semantic_targets")]
            semantic_targets: Option<Vec<SemanticTextLayoutTarget>>,
        }
        let fields = Fields::deserialize(deserializer)?;
        let result = Self {
            profile: fields.profile,
            engine: fields.engine,
            targets: fields.targets,
            semantic_targets: fields.semantic_targets,
        };
        result
            .validate_profile_shape()
            .map_err(serde::de::Error::custom)?;
        Ok(result)
    }
}

impl TextLayoutContext {
    /// Construct the original geometry-only v1 policy, without new wire fields.
    pub fn new(targets: Vec<TextLayoutTarget>) -> Self {
        Self {
            profile: TEXT_LAYOUT_PROFILE.into(),
            engine: TEXT_LAYOUT_ENGINE.into(),
            targets,
            semantic_targets: None,
        }
    }

    /// Opt into v2 explicitly, retaining any geometry targets already supplied.
    pub fn with_semantic_targets(mut self, targets: Vec<SemanticTextLayoutTarget>) -> Self {
        self.profile = TEXT_LAYOUT_SEMANTIC_PROFILE.into();
        self.semantic_targets = Some(targets);
        self
    }

    pub fn validate(&self) -> VizResult<()> {
        if !matches!(
            self.profile.as_str(),
            TEXT_LAYOUT_PROFILE | TEXT_LAYOUT_SEMANTIC_PROFILE
        ) || self.engine != TEXT_LAYOUT_ENGINE
        {
            return Err(error(
                "unsupported wrapping profile or Unicode engine identity",
            ));
        }
        self.validate_profile_shape()?;
        let semantic_targets = self.semantic_targets.as_deref().unwrap_or_default();
        if self.targets.len() + semantic_targets.len() == 0
            || self.targets.len() + semantic_targets.len() > MAX_TARGETS
        {
            return Err(error(
                "wrapping requires between 1 and 256 explicit source targets in total",
            ));
        }
        let mut seen = BTreeSet::new();
        for t in &self.targets {
            validate_id(&t.view_id)?;
            validate_id(&t.node_id)?;
            if !seen.insert((&t.view_id, &t.node_id)) {
                return Err(error("duplicate wrapping source (view_id, node_id) target"));
            }
            validate_dimensions(t.max_width, t.max_lines, t.line_height)?;
        }
        let mut semantic_seen = BTreeSet::new();
        for t in semantic_targets {
            validate_id(&t.view_id)?;
            if !semantic_seen.insert((&t.view_id, t.role)) {
                return Err(error("duplicate wrapping source (view_id, role) target"));
            }
            validate_dimensions(t.max_width, t.max_lines, t.line_height)?;
        }
        Ok(())
    }

    fn validate_profile_shape(&self) -> VizResult<()> {
        match self.profile.as_str() {
            TEXT_LAYOUT_PROFILE if self.semantic_targets.is_some() => {
                return Err(error("vizir-text-wrap/1 does not permit semantic_targets"));
            }
            TEXT_LAYOUT_SEMANTIC_PROFILE
                if self.semantic_targets.as_ref().is_none_or(Vec::is_empty) =>
            {
                return Err(error(
                    "vizir-text-wrap/2 requires nonempty semantic_targets",
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

fn validate_id(id: &str) -> VizResult<()> {
    if id.is_empty() || id.len() > 256 {
        return Err(error("wrapping source IDs must contain 1..256 UTF-8 bytes"));
    }
    Ok(())
}

fn validate_dimensions(max_width: f64, max_lines: u32, line_height: f64) -> VizResult<()> {
    if !max_width.is_finite()
        || !(0.25..=1_000_000.).contains(&max_width)
        || !line_height.is_finite()
        || !(0.25..=1_000_000.).contains(&line_height)
        || max_lines == 0
        || max_lines as usize > MAX_LINES
    {
        return Err(error(
            "wrapping width/line_height require finite 0.25..1000000; max_lines requires integer 1..256",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Paragraph {
    pub text: Range<usize>,
    pub separator: Range<usize>,
}

/// The permitted separators are LF, CRLF, LS and PS. A terminal separator
/// creates an empty final paragraph. Nothing trims, normalizes or drops bytes.
pub(crate) fn paragraphs(source: &str) -> VizResult<Vec<Paragraph>> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut chars = source.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '\u{00ad}' {
            return Err(error(format!(
                "soft hyphen U+00AD at UTF-8 byte {i} requires unsupported discretionary-hyphen emission; edit the original source"
            )));
        }
        let end = match c {
            '\r' => {
                if chars.next_if(|(_, c)| *c == '\n').is_none() {
                    return Err(error("bare CR is unsupported; use LF, CRLF, LS or PS"));
                }
                Some(i + 2)
            }
            '\n' | '\u{2028}' | '\u{2029}' => Some(i + c.len_utf8()),
            _ if c.is_control() => {
                return Err(error(
                    "wrapping does not support tabs or control characters",
                ));
            }
            _ => None,
        };
        use unicode_bidi::BidiClass::*;
        if matches!(
            unicode_bidi::bidi_class(c),
            R | AL | AN | RLE | RLO | LRE | LRO | PDF | RLI | LRI | FSI | PDI
        ) || matches!(c, '\u{200e}' | '\u{200f}' | '\u{061c}')
        {
            return Err(error(
                "wrapping profile supports horizontal LTR text only; RTL and directional controls are unsupported",
            ));
        }
        if let Some(end) = end {
            result.push(Paragraph {
                text: start..i,
                separator: i..end,
            });
            start = end;
            if result.len() >= MAX_LINES {
                return Err(error("hard breaks exceed 256 logical lines before shaping"));
            }
        }
    }
    result.push(Paragraph {
        text: start..source.len(),
        separator: source.len()..source.len(),
    });
    Ok(result)
}

/// UAX14 opportunities intersected with Unicode extended-grapheme boundaries.
/// Terminal source end is always a candidate; there is no emergency splitting.
pub(crate) fn opportunities(text: &str) -> Vec<usize> {
    let boundaries: BTreeSet<usize> = text
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    unicode_linebreak::linebreaks(text)
        .map(|(i, _)| i)
        .filter(|i| *i > 0 && boundaries.contains(i))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separators_cover_original_bytes_and_empty_lines() {
        for s in ["", "A\n\n", "\r\nA\r\n", " \u{2028}\u{2029}", "  A  "] {
            let p = paragraphs(s).unwrap();
            let reconstructed: String = p
                .iter()
                .map(|p| format!("{}{}", &s[p.text.clone()], &s[p.separator.clone()]))
                .collect();
            assert_eq!(reconstructed, s);
        }
        assert_eq!(paragraphs("A\n\n").unwrap().len(), 3);
        assert_eq!(paragraphs("\r\n").unwrap().len(), 2);
        for s in ["A\rB", "A\tB", "A\u{000b}B", "Aא", "\u{2066}A\u{2069}"] {
            assert!(paragraphs(s).is_err());
        }
    }
    #[test]
    fn versions_and_grapheme_intersection_are_exact() {
        assert_eq!(unicode_linebreak::UNICODE_VERSION, (15, 0, 0));
        assert_eq!(unicode_segmentation::UNICODE_VERSION, (17, 0, 0));
        assert_eq!(opportunities("e\u{301} e\u{301}"), vec![4, 7]);
        assert_eq!(
            opportunities("中文（中文）中文。"),
            vec![3, 6, 12, 18, 21, 27]
        );
    }
}
