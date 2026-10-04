//! Shared, bounded heatmap arithmetic. No Cartesian cell allocation occurs here.
use crate::{VizError, VizResult};

pub const DEFAULT_HEATMAP_BINS: usize = 5;

pub const MAX_HEATMAP_CATEGORIES: usize = 256;
pub const MAX_HEATMAP_CELLS: usize = 16_384;
pub const MAX_HEATMAP_CELLS_PER_CALL: usize = 65_536;
pub const MAX_HEATMAP_LABEL_BYTES: usize = 16_384;
pub const MAX_HEATMAP_DOMAIN_BYTES: usize = 1_048_576;
pub const MAX_HEATMAP_GRID_CELLS: usize = 65_536;

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
