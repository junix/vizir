//! Explicit source-targeted wrapping policy. Source ranges are byte preserving.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, ops::Range};
use unicode_segmentation::UnicodeSegmentation;
use vizir_core::{VizError, VizResult};

pub const TEXT_LAYOUT_PROFILE: &str = "vizir-text-wrap/1";
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

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextLayoutContext {
    pub profile: String,
    pub engine: String,
    pub targets: Vec<TextLayoutTarget>,
}
impl TextLayoutContext {
    pub fn new(targets: Vec<TextLayoutTarget>) -> Self {
        Self {
            profile: TEXT_LAYOUT_PROFILE.into(),
            engine: TEXT_LAYOUT_ENGINE.into(),
            targets,
        }
    }
    pub fn validate(&self) -> VizResult<()> {
        if self.profile != TEXT_LAYOUT_PROFILE || self.engine != TEXT_LAYOUT_ENGINE {
            return Err(error(
                "unsupported wrapping profile or Unicode engine identity",
            ));
        }
        if self.targets.is_empty() || self.targets.len() > MAX_TARGETS {
            return Err(error(
                "wrapping requires between 1 and 256 explicit source targets",
            ));
        }
        let mut seen = BTreeSet::new();
        for t in &self.targets {
            if [&t.view_id, &t.node_id]
                .into_iter()
                .any(|s| s.is_empty() || s.len() > 256)
            {
                return Err(error("wrapping source IDs must contain 1..256 UTF-8 bytes"));
            }
            if !seen.insert((&t.view_id, &t.node_id)) {
                return Err(error("duplicate wrapping source (view_id, node_id) target"));
            }
            if !t.max_width.is_finite()
                || !(0.25..=1_000_000.).contains(&t.max_width)
                || !t.line_height.is_finite()
                || !(0.25..=1_000_000.).contains(&t.line_height)
                || t.max_lines == 0
                || t.max_lines as usize > MAX_LINES
            {
                return Err(error(
                    "wrapping width/line_height require finite 0.25..1000000; max_lines requires integer 1..256",
                ));
            }
        }
        Ok(())
    }
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
