//! Bounded heatmap resolution shared by HIR lowering and MIR replay.
//! No data access or backend-specific marks live here.
use std::collections::BTreeSet;

use vizir_core::{
    Color, FontWeight, Frame, MirScale, NumberFormat, Point, Rect, TextAnchor, VizError, VizResult,
};

use crate::chart_layout::header_text_width;
use crate::text::TextSession;
use crate::tick_format::format_number;
use crate::{ResolvedThemeDefaults, ThemeContext};

pub(crate) fn error(detail: impl std::fmt::Display) -> VizError {
    VizError::Diagnostic(format!("VIZ-HEATMAP-0001: {detail}"))
}

/// Match the SVG backend's existing four-decimal wire projection exactly.
pub(crate) fn serialized(value: f64) -> f64 {
    let value = if value.abs() < 0.000_000_1 {
        0.0
    } else {
        value
    };
    format!("{value:.4}").parse().expect("finite coordinate")
}

/// Project each boundary once. Rect origins and extents are differences of the
/// same integer four-decimal grid, so independent width rounding cannot add gaps.
pub(crate) fn band_boundaries(range: [f64; 2], count: usize) -> VizResult<Vec<f64>> {
    if count == 0
        || count > vizir_core::MAX_HEATMAP_CATEGORIES
        || !range.iter().all(|v| v.is_finite() && v.abs() <= 1_000_000.)
        || range[0] >= range[1]
        || !(range[1] - range[0]).is_finite()
    {
        return Err(error(
            "band range must be finite, ascending, within +/-1000000, with 1..256 categories",
        ));
    }
    let span = range[1] - range[0];
    let mut result = Vec::with_capacity(count + 1);
    for i in 0..=count {
        let value = if i == count {
            range[1]
        } else {
            let fraction = i as f64 / count as f64;
            let offset = span * fraction;
            range[0] + offset
        };
        let value = serialized(value);
        if result.last().is_some_and(|previous| *previous >= value) {
            return Err(error(
                "band boundaries collapse at SVG four-decimal precision; enlarge the frame",
            ));
        }
        result.push(value);
    }
    Ok(result)
}

pub(crate) fn cell_extent(start: f64, end: f64) -> f64 {
    // Both arguments were serialized once. Integer subtraction preserves the
    // common grid even when binary floating point cannot represent a decimal.
    let units = (end * 10_000.).round() as i64 - (start * 10_000.).round() as i64;
    units as f64 / 10_000.
}

pub(crate) fn resolve_domain<'a>(
    explicit: Option<&[String]>,
    observed: impl Iterator<Item = &'a str>,
) -> VizResult<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut inferred = Vec::new();
    for value in observed {
        vizir_core::validate_heatmap_category(value)?;
        if seen.insert(value) {
            if seen.len() > vizir_core::MAX_HEATMAP_CATEGORIES {
                return Err(error("category domain exceeds 256 entries"));
            }
            inferred.push(value.to_owned());
        }
    }
    if let Some(domain) = explicit {
        if domain.is_empty() || domain.len() > vizir_core::MAX_HEATMAP_CATEGORIES {
            return Err(error("explicit category domain requires 1..256 entries"));
        }
        let mut declared = BTreeSet::new();
        for value in domain {
            vizir_core::validate_heatmap_category(value)?;
            if !declared.insert(value.as_str()) {
                return Err(error("duplicate explicit category domain entry"));
            }
        }
        if seen.iter().any(|value| !declared.contains(value)) {
            return Err(error(
                "explicit category domain does not cover every observed category",
            ));
        }
        Ok(domain.to_vec())
    } else {
        Ok(inferred)
    }
}

pub(crate) fn check_domains(x: &[String], y: &[String]) -> VizResult<()> {
    if x.is_empty()
        || y.is_empty()
        || x.len() > vizir_core::MAX_HEATMAP_CATEGORIES
        || y.len() > vizir_core::MAX_HEATMAP_CATEGORIES
        || x.len().checked_mul(y.len()).is_none_or(|n| n > 65_536)
    {
        return Err(error(
            "heatmap domains exceed the 256-per-axis or 65536-pair limit",
        ));
    }
    let mut bytes = 0usize;
    for domain in [x, y] {
        let mut unique = BTreeSet::new();
        for value in domain {
            vizir_core::validate_heatmap_category(value)?;
            if !unique.insert(value) {
                return Err(error("duplicate category domain entry"));
            }
            bytes = bytes
                .checked_add(value.len())
                .ok_or_else(|| error("domain byte count overflow"))?;
        }
    }
    if bytes > vizir_core::MAX_HEATMAP_DOMAIN_BYTES {
        return Err(error("category domain labels exceed 1 MiB"));
    }
    Ok(())
}

/// VizIR quantize-ramp/1. This is a pinned derivation, not a theme-registry token.
pub(crate) fn palette(defaults: Option<&ResolvedThemeDefaults>) -> VizResult<Vec<Color>> {
    let fallback;
    let defaults = if let Some(defaults) = defaults {
        defaults
    } else {
        fallback = ThemeContext::resolve("azure")?;
        &fallback.defaults
    };
    let rgb = |color: &Color| -> VizResult<[u8; 3]> {
        if color.0.len() != 7 || !color.0.starts_with('#') {
            return Err(error("canonical ramp endpoints require opaque RGB"));
        }
        let mut values = [0; 3];
        for (i, value) in values.iter_mut().enumerate() {
            *value = u8::from_str_radix(&color.0[1 + i * 2..3 + i * 2], 16)
                .map_err(|_| error("invalid ramp endpoint"))?;
        }
        Ok(values)
    };
    let low = rgb(&defaults.group_fills[0])?;
    let high = rgb(&defaults.mark)?;
    Ok((0..5)
        .map(|index| {
            let fraction = index as f64 / 4.0;
            let mut channels = [0u8; 3];
            for i in 0..3 {
                let delta = f64::from(high[i]) - f64::from(low[i]);
                let offset = delta * fraction;
                let sample = f64::from(low[i]) + offset;
                channels[i] = (sample + 0.5).floor() as u8;
            }
            Color::hex(&format!(
                "#{:02x}{:02x}{:02x}",
                channels[0], channels[1], channels[2]
            ))
        })
        .collect())
}

pub(crate) struct IntervalLegend {
    pub labels: Vec<String>,
    pub colors: Vec<Color>,
}

impl IntervalLegend {
    pub fn new(scale: &MirScale, format: Option<&NumberFormat>) -> VizResult<Self> {
        let MirScale::QuantizeColor {
            domain,
            thresholds,
            range,
            ..
        } = scale
        else {
            return Err(error("missing quantitative color scale"));
        };
        let expected = vizir_core::canonical_quantize_thresholds(*domain, range.len())?;
        if thresholds.len() != expected.len()
            || !thresholds
                .iter()
                .zip(&expected)
                .all(|(a, b)| a.to_bits() == b.to_bits())
        {
            return Err(error(
                "stored quantitative thresholds differ from canonical intervals",
            ));
        }
        let endpoint = |v: f64| {
            let v = if v == 0. { 0. } else { v };
            match format {
                Some(format) => format_number(v, Some(format)),
                None if v == 0. || (0.0001..1_000_000.).contains(&v.abs()) => v.to_string(),
                None => format!("{v:e}"),
            }
        };
        if domain[0] == domain[1] {
            return Ok(Self {
                labels: vec![format!("= {}", endpoint(domain[0]))],
                colors: vec![range[(range.len() - 1) / 2].clone()],
            });
        }
        let boundaries = std::iter::once(domain[0])
            .chain(thresholds.iter().copied())
            .chain(std::iter::once(domain[1]))
            .map(endpoint)
            .collect::<Vec<_>>();
        if boundaries.windows(2).any(|p| p[0] == p[1]) {
            return Err(error(
                "number_format produces indistinguishable adjacent interval boundaries; choose another precision or notation",
            ));
        }
        let labels = boundaries
            .windows(2)
            .enumerate()
            .map(|(i, p)| {
                format!(
                    "[{}, {}{}",
                    p[0],
                    p[1],
                    if i + 1 == range.len() { "]" } else { ")" }
                )
            })
            .collect();
        Ok(Self {
            labels,
            colors: range.clone(),
        })
    }
}

pub(crate) fn label_bounds(
    source: &str,
    size: f64,
    weight: FontWeight,
    position: Point,
    anchor: TextAnchor,
    text: Option<&TextSession>,
) -> VizResult<Rect> {
    if source
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err(error(
            "heatmap axes and legend require single-line labels without control characters",
        ));
    }
    if let Some(text) = text {
        return text.single_line_box_anchored(source, size, weight, position, anchor);
    }
    let width = header_text_width(source, size);
    let x = position.x
        - match anchor {
            TextAnchor::Start => 0.,
            TextAnchor::Middle => width / 2.,
            TextAnchor::End => width,
        };
    Ok(Rect {
        x,
        y: position.y - size,
        width,
        height: size * 1.25,
    })
}

pub(crate) struct HeatmapLayout {
    pub plot: [f64; 4],
    pub x_labels: Vec<Point>,
    pub y_labels: Vec<Point>,
    pub legend: Vec<Point>, // upper-left swatch corners
    pub legend_labels: Vec<Point>,
    pub x_title: Point,
    pub y_title: Point,
    pub legend_title: Point,
}

impl HeatmapLayout {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        frame: Frame,
        title: Option<&str>,
        x: &[String],
        y: &[String],
        x_title: &str,
        y_title: &str,
        legend_title: &str,
        legend: &IntervalLegend,
        text: Option<&TextSession>,
    ) -> VizResult<Self> {
        check_domains(x, y)?;
        if let Some(text) = text {
            let labels = x.len() + y.len() + legend.labels.len() * 2 + 4;
            text.reserve_heatmap_collisions(labels)?;
        }
        if ![
            frame.x,
            frame.y,
            frame.width,
            frame.height,
            frame.x + frame.width,
            frame.y + frame.height,
        ]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
        {
            return Err(error(
                "heatmap frame coordinates must be finite and within +/-1000000",
            ));
        }
        let metric = |label: &str, size, weight, anchor| {
            label_bounds(label, size, weight, Point { x: 0., y: 0. }, anchor, text)
        };
        let xs = x
            .iter()
            .map(|s| metric(s, 10., FontWeight::Regular, TextAnchor::Middle))
            .collect::<VizResult<Vec<_>>>()?;
        let ys = y
            .iter()
            .map(|s| metric(s, 10., FontWeight::Regular, TextAnchor::End))
            .collect::<VizResult<Vec<_>>>()?;
        let ls = legend
            .labels
            .iter()
            .map(|s| metric(s, 10.5, FontWeight::Regular, TextAnchor::Start))
            .collect::<VizResult<Vec<_>>>()?;
        let xt = metric(x_title, 12.5, FontWeight::Medium, TextAnchor::Middle)?;
        let yt = metric(y_title, 12.5, FontWeight::Medium, TextAnchor::Start)?;
        let lt = metric(legend_title, 12.5, FontWeight::Medium, TextAnchor::Start)?;
        let mut checks = Vec::new();
        let mut header_bottom = frame.y + 12.;
        if let Some(title) = title {
            let bounds = if let Some(bounds) = text
                .map(|t| t.chart_title_bounds(id, Some(title), frame))
                .transpose()?
                .flatten()
            {
                bounds
            } else {
                label_bounds(
                    title,
                    18.,
                    FontWeight::Bold,
                    Point {
                        x: frame.x + 18.,
                        y: frame.y + 28.,
                    },
                    TextAnchor::Start,
                    text,
                )?
            };
            header_bottom = header_bottom.max(bounds.y + bounds.height + 10.);
            checks.push(bounds);
        }
        let y_width = ys.iter().map(|b| b.width).fold(0., f64::max);
        let legend_width = ls.iter().map(|b| b.width + 20.001).fold(lt.width, f64::max);
        let x_height = xs.iter().map(|b| b.height).fold(0., f64::max);
        let row_height = ls.iter().map(|b| b.height).fold(12., f64::max);
        let left = frame.x + 64_f64.max(y_width + 26.);
        let right = frame.x + frame.width - 16. - legend_width - 20.;
        let top = (header_bottom + yt.height.max(lt.height) + 12.).max(frame.y + 50.);
        let bottom = frame.y + frame.height - 12. - xt.height - 10. - x_height - 8.;
        let plot = [left, top, right, bottom];
        if right - left < 64.
            || bottom - top < 64.
            || bottom - top < (row_height + 8.) * ls.len() as f64 - 8.
        {
            return Err(error(format!(
                "chart {id:?} cannot fit its axes, quantitative legend and 64px by 64px plot; enlarge its frame"
            )));
        }
        let xb = band_boundaries([left, right], x.len())?;
        let yb = band_boundaries([top, bottom], y.len())?;
        let mut x_labels = Vec::new();
        let mut y_labels = Vec::new();
        for (i, b) in xs.iter().enumerate() {
            let position = Point {
                x: (xb[i] + xb[i + 1]) / 2.,
                y: bottom + 8. - b.y,
            };
            let actual = label_bounds(
                &x[i],
                10.,
                FontWeight::Regular,
                position,
                TextAnchor::Middle,
                text,
            )?;
            if actual.x < xb[i] + 4. || actual.x + actual.width > xb[i + 1] - 4. {
                return Err(error(format!(
                    "chart {id:?} x category {i} does not fit its band at fixed 10px; enlarge the frame or shorten the label"
                )));
            }
            checks.push(actual);
            x_labels.push(position);
        }
        for (i, b) in ys.iter().enumerate() {
            let position = Point {
                x: left - 10.,
                y: (yb[i] + yb[i + 1]) / 2. - b.y - b.height / 2.,
            };
            let actual = label_bounds(
                &y[i],
                10.,
                FontWeight::Regular,
                position,
                TextAnchor::End,
                text,
            )?;
            if actual.y < yb[i] + 2. || actual.y + actual.height > yb[i + 1] - 2. {
                return Err(error(format!(
                    "chart {id:?} y category {i} does not fit its band at fixed 10px; enlarge the frame"
                )));
            }
            checks.push(actual);
            y_labels.push(position);
        }
        let x_title_point = Point {
            x: (left + right) / 2.,
            y: bottom + 8. + x_height + 10. - xt.y,
        };
        let y_title_point = Point {
            x: frame.x + 16.,
            y: top - 12. - yt.y - yt.height,
        };
        let legend_title_point = Point {
            x: right + 20.,
            y: top - 12. - lt.y - lt.height,
        };
        checks.push(label_bounds(
            x_title,
            12.5,
            FontWeight::Medium,
            x_title_point,
            TextAnchor::Middle,
            text,
        )?);
        checks.push(label_bounds(
            y_title,
            12.5,
            FontWeight::Medium,
            y_title_point,
            TextAnchor::Start,
            text,
        )?);
        checks.push(label_bounds(
            legend_title,
            12.5,
            FontWeight::Medium,
            legend_title_point,
            TextAnchor::Start,
            text,
        )?);
        let mut swatches = Vec::new();
        let mut legend_labels = Vec::new();
        for (i, b) in ls.iter().enumerate() {
            let row_top = top + i as f64 * (row_height + 8.);
            let point = Point {
                x: right + 40.001 - b.x,
                y: row_top + (row_height - b.height) / 2. - b.y,
            };
            checks.push(label_bounds(
                &legend.labels[i],
                10.5,
                FontWeight::Regular,
                point,
                TextAnchor::Start,
                text,
            )?);
            let swatch = Point {
                x: serialized(right + 20.),
                y: serialized(row_top + (row_height - 12.) / 2.),
            };
            let swatch_bounds = Rect {
                x: swatch.x,
                y: swatch.y,
                width: 12.,
                height: 12.,
            };
            let label_left = checks.last().expect("legend label checked").x;
            if label_left < swatch_bounds.x + swatch_bounds.width + 8. {
                return Err(error(
                    "quantitative legend text cannot fit with an 8px swatch gap",
                ));
            }
            checks.push(swatch_bounds);
            swatches.push(swatch);
            legend_labels.push(point);
        }
        let frame_end = [
            serialized(frame.x) + serialized(frame.width),
            serialized(frame.y) + serialized(frame.height),
        ];
        for (i, b) in checks.iter().enumerate() {
            if b.x < serialized(frame.x)
                || b.y < serialized(frame.y)
                || b.x + b.width > frame_end[0]
                || b.y + b.height > frame_end[1]
            {
                return Err(error(format!(
                    "chart {id:?} label {i} does not fit its serialized frame"
                )));
            }
            if b.x < serialized(right)
                && b.x + b.width > serialized(left)
                && b.y < serialized(bottom)
                && b.y + b.height > serialized(top)
            {
                return Err(error(format!("chart {id:?} label {i} overlaps the plot")));
            }
        }
        for (i, a) in checks.iter().enumerate() {
            for b in &checks[i + 1..] {
                if a.width > 0.
                    && b.width > 0.
                    && a.x < b.x + b.width
                    && b.x < a.x + a.width
                    && a.y < b.y + b.height
                    && b.y < a.y + a.height
                {
                    return Err(error(format!(
                        "chart {id:?} has overlapping labels; enlarge the frame or shorten labels"
                    )));
                }
            }
        }
        Ok(Self {
            plot,
            x_labels,
            y_labels,
            legend: swatches,
            legend_labels,
            x_title: x_title_point,
            y_title: y_title_point,
            legend_title: legend_title_point,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ramp_uses_unfused_encoded_rgb_half_up_in_both_directions() {
        let mut defaults = ThemeContext::resolve("azure").unwrap().defaults;
        defaults.group_fills[0] = Color::hex("#0002fe");
        defaults.mark = Color::hex("#0200ff");
        let colors = palette(Some(&defaults)).unwrap();
        assert_eq!(
            colors.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(),
            vec!["#0002fe", "#0102fe", "#0101ff", "#0201ff", "#0200ff"]
        );
    }
    #[test]
    fn serialized_boundaries_share_exact_integer_units_and_reject_collapse() {
        let edges = band_boundaries([0.123456, 100.987654], 7).unwrap();
        for i in 0..7 {
            let units = |v: f64| (v * 10000.).round() as i64;
            assert_eq!(
                units(edges[i]) + units(cell_extent(edges[i], edges[i + 1])),
                units(edges[i + 1])
            );
        }
        for range in [[0., 0.0001], [1., 1.], [-1e6, 1e6 + 1.], [f64::NAN, 1.]] {
            assert!(band_boundaries(range, 2).is_err());
        }
    }
    #[test]
    fn legend_rejects_noncanonical_negative_zero_threshold() {
        let scale = MirScale::QuantizeColor {
            id: "q".into(),
            domain: [-1., 1.],
            thresholds: vec![-0.],
            range: vec![Color::hex("#111111"), Color::hex("#222222")],
        };
        assert!(IntervalLegend::new(&scale, None).is_err());
    }
    #[test]
    fn automatic_endpoint_format_is_roundtrip_distinct_and_bounded() {
        let scale = MirScale::QuantizeColor {
            id: "q".into(),
            domain: [1e-300, 3e-300],
            thresholds: vizir_core::canonical_quantize_thresholds([1e-300, 3e-300], 2).unwrap(),
            range: vec![Color::hex("#111111"), Color::hex("#222222")],
        };
        let labels = IntervalLegend::new(&scale, None).unwrap().labels;
        assert!(labels.iter().all(|s| s.len() < 64 && s.contains('e')));
        let format = NumberFormat {
            notation: vizir_core::NumberNotation::Fixed,
            precision: 12,
        };
        assert!(IntervalLegend::new(&scale, Some(&format)).is_err());
    }
}
