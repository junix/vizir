//! Shared, bounded heatmap arithmetic. No Cartesian cell allocation occurs here.
use crate::{VizError, VizResult};

pub const DEFAULT_HEATMAP_BINS: usize = 5;

pub const MAX_HEATMAP_CATEGORIES: usize = 256;
pub const MAX_HEATMAP_CELLS: usize = 16_384;
pub const MAX_HEATMAP_CELLS_PER_CALL: usize = 65_536;
pub const MAX_HEATMAP_LABEL_BYTES: usize = 16_384;
pub const MAX_HEATMAP_DOMAIN_BYTES: usize = 1_048_576;
pub const MAX_HEATMAP_GRID_CELLS: usize = 65_536;
/// Opted-in numeric label limits, independent of older category-label limits.
pub const MAX_HEATMAP_VALUE_LABELS: usize = 4_096;
pub const MAX_HEATMAP_VALUE_LABEL_BYTES: usize = 512;
pub const MAX_HEATMAP_VALUE_LABEL_TOTAL_BYTES: usize = 1_048_576;

/// Portable opaque RGB, including the equivalent explicit FF alpha spelling.
pub fn heatmap_opaque_rgb(color: &crate::Color) -> Option<[u8; 3]> {
    let value = color.0.as_bytes();
    if !matches!(value.len(), 7 | 9)
        || value[0] != b'#'
        || !value[1..].iter().all(u8::is_ascii_hexdigit)
        || (value.len() == 9 && !value[7..].eq_ignore_ascii_case(b"ff"))
    {
        return None;
    }
    Some([
        u8::from_str_radix(&color.0[1..3], 16).ok()?,
        u8::from_str_radix(&color.0[3..5], 16).ok()?,
        u8::from_str_radix(&color.0[5..7], 16).ok()?,
    ])
}

/// Reserve a whole-call label count before allocating or cloning caches.
pub fn reserve_heatmap_value_labels(total: &mut usize, count: usize) -> VizResult<()> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| invalid("value label count overflow"))?;
    if *total > MAX_HEATMAP_VALUE_LABELS {
        return Err(invalid(
            "heatmap value_labels exceed 4096 labels per compilation",
        ));
    }
    Ok(())
}

/// Check a generated or supplied label before copying it.
pub fn reserve_heatmap_value_label_bytes(total: &mut usize, text: &str) -> VizResult<()> {
    if text.len() > MAX_HEATMAP_VALUE_LABEL_BYTES {
        return Err(invalid("heatmap value label exceeds 512 UTF-8 bytes"));
    }
    *total = total
        .checked_add(text.len())
        .ok_or_else(|| invalid("value label byte count overflow"))?;
    if *total > MAX_HEATMAP_VALUE_LABEL_TOTAL_BYTES {
        return Err(invalid(
            "heatmap value labels exceed 1048576 UTF-8 bytes per compilation",
        ));
    }
    Ok(())
}

/// Whether compilation opted into bounded present-cell value labels.
pub fn has_heatmap_value_labels(mir: &crate::VizMir) -> bool {
    mir.views.iter().any(|view| {
        matches!(view, crate::MirView::Chart(chart)
        if matches!(chart.mark, crate::ChartMark::Heatmap { value_labels: Some(_), .. }))
    })
}

fn invalid(message: &str) -> VizError {
    VizError::Diagnostic(format!("VIZ-HEATMAP-0001: {message}"))
}

/// Categories are exact Unicode strings; ordinary spaces remain significant.
pub fn validate_heatmap_category(value: &str) -> VizResult<()> {
    if value.is_empty() || value.len() > MAX_HEATMAP_LABEL_BYTES {
        return Err(invalid(
            "category labels must contain 1..=16384 UTF-8 bytes",
        ));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}'))
    {
        return Err(invalid(
            "category labels cannot contain C0/C1 controls or Unicode line separators",
        ));
    }
    Ok(())
}

fn normalized_zero(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value }
}

/// Compute canonical thresholds using the contract's unfused operation order.
pub fn canonical_quantize_thresholds(domain: [f64; 2], bins: usize) -> VizResult<Vec<f64>> {
    let [lo, hi] = domain;
    if !(2..=9).contains(&bins) {
        return Err(invalid("quantize palettes require 2..=9 colors"));
    }
    let span = hi - lo;
    if !lo.is_finite() || !hi.is_finite() || lo > hi || !span.is_finite() {
        return Err(invalid(
            "quantize domain must be finite, ascending, and have a finite span",
        ));
    }
    if lo == hi {
        return Ok(Vec::new());
    }
    let mut thresholds = Vec::with_capacity(bins - 1);
    let mut previous = lo;
    for i in 1..bins {
        let fraction = (i as f64) / (bins as f64);
        let offset = span * fraction;
        let threshold = normalized_zero(lo + offset);
        if !threshold.is_finite() || threshold <= previous || threshold >= hi {
            return Err(invalid(
                "quantize thresholds must be representable, strictly interior, and increasing",
            ));
        }
        thresholds.push(threshold);
        previous = threshold;
    }
    Ok(thresholds)
}

/// Validate stored thresholds and select a bin. Ties go up; values never clamp.
pub fn quantize_color_index(
    value: f64,
    domain: [f64; 2],
    thresholds: &[f64],
    bins: usize,
) -> VizResult<usize> {
    let canonical = canonical_quantize_thresholds(domain, bins)?;
    if thresholds.len() != canonical.len()
        || thresholds
            .iter()
            .zip(&canonical)
            .any(|(&a, &b)| a.to_bits() != b.to_bits())
    {
        return Err(invalid(
            "quantize thresholds do not match canonical domain/palette arithmetic",
        ));
    }
    if !value.is_finite() || value < domain[0] || value > domain[1] {
        return Err(invalid(
            "quantize value must be finite and inside the color domain",
        ));
    }
    if domain[0] == domain[1] {
        return Ok((bins - 1) / 2);
    }
    Ok(thresholds.partition_point(|threshold| value >= *threshold))
}
