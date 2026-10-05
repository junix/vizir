//! Heatmap-only validation keeps older mark behavior unchanged.
use std::collections::BTreeSet;

use crate::{
    CategoryEncoding, ChartMark, CoordinateSpaceKind, Dataset, Diagnostic, Document, GuideKind,
    GuideOrient, HeatmapChart, MAX_HEATMAP_CATEGORIES, MAX_HEATMAP_CELLS,
    MAX_HEATMAP_CELLS_PER_CALL, MAX_HEATMAP_DOMAIN_BYTES, MAX_HEATMAP_GRID_CELLS,
    MAX_HEATMAP_LABEL_BYTES, MirChart, MirDataOperator, MirScale, MirView, QuantizeColorEncoding,
    SpatialUnit, Transform2D, ValueType, VizMir, canonical_quantize_thresholds,
    quantize_color_index, validate_heatmap_category,
};

fn error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("VIZ-HEATMAP-0001", message)
}

fn label(value: &str) -> Result<(), Diagnostic> {
    if value.len() > MAX_HEATMAP_LABEL_BYTES {
        Err(error("heatmap labels cannot exceed 16384 UTF-8 bytes"))
    } else {
        Ok(())
    }
}

fn category(value: &str) -> Result<(), Diagnostic> {
    validate_heatmap_category(value).map_err(|e| error(e.to_string()))
}

fn domain<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<BTreeSet<&'a str>, Diagnostic> {
    let mut unique = BTreeSet::new();
    let mut bytes = 0usize;
    for value in values {
        category(value)?;
        if !unique.insert(value) {
            return Err(error(
                "explicit heatmap domains must not contain duplicate categories",
            ));
        }
        bytes = bytes
            .checked_add(value.len())
            .ok_or_else(|| error("heatmap domain byte count overflow"))?;
        if unique.len() > MAX_HEATMAP_CATEGORIES || bytes > MAX_HEATMAP_DOMAIN_BYTES {
            return Err(error(
                "heatmap domain exceeds category or label byte limits",
            ));
        }
    }
    if unique.is_empty() {
        return Err(error("heatmap category domains must be nonempty"));
    }
    Ok(unique)
}

fn domain_pair(x: &BTreeSet<&str>, y: &BTreeSet<&str>) -> Result<(), Diagnostic> {
    if x.len()
        .checked_mul(y.len())
        .is_none_or(|count| count > MAX_HEATMAP_GRID_CELLS)
    {
        return Err(error("heatmap domain product exceeds 65536"));
    }
    // Each axis has an independent ordered domain, so a shared spelling counts
    // once on each axis, exactly as its two domain entries are stored.
    let bytes = x
        .iter()
        .chain(y)
        .try_fold(0usize, |sum, value| sum.checked_add(value.len()));
    if bytes.is_none_or(|bytes| bytes > MAX_HEATMAP_DOMAIN_BYTES) {
        return Err(error(
            "combined heatmap x/y domain labels exceed 1048576 UTF-8 bytes",
        ));
    }
    Ok(())
}

fn reserve(count: usize, total: &mut usize) -> Result<(), Diagnostic> {
    if count > MAX_HEATMAP_CELLS {
        return Err(error(
            "heatmap rows or present cells exceed 16384 per chart",
        ));
    }
    *total = total
        .checked_add(count)
        .ok_or_else(|| error("heatmap cell count overflow"))?;
    if *total > MAX_HEATMAP_CELLS_PER_CALL {
        return Err(error("heatmap cells exceed 65536 per call"));
    }
    Ok(())
}

/// Run before cloning datasets/panels or inferring data types.
pub(crate) fn preflight_source(
    dataset: Option<&Dataset>,
    x: &CategoryEncoding,
    y: &CategoryEncoding,
    color: &QuantizeColorEncoding,
    total: &mut usize,
    value_labels: Option<&crate::HeatmapValueLabels>,
    label_total: &mut usize,
) -> Result<(), Diagnostic> {
    for encoding in [x, y] {
        label(encoding.label.as_deref().unwrap_or(&encoding.field))?;
        if encoding
            .domain
            .as_ref()
            .is_some_and(|domain| domain.len() > MAX_HEATMAP_CATEGORIES)
        {
            return Err(error("heatmap axes support at most 256 categories"));
        }
    }
    label(color.label.as_deref().unwrap_or(&color.field))?;
    if color
        .palette
        .as_ref()
        .is_some_and(|palette| !(2..=9).contains(&palette.len()))
    {
        return Err(error("heatmap palettes require 2..=9 colors when present"));
    }
    let Some(dataset) = dataset else {
        return Ok(());
    };
    reserve(dataset.rows.len(), total)?;
    if value_labels.is_some() {
        crate::reserve_heatmap_value_labels(label_total, dataset.rows.len())
            .map_err(|e| error(e.to_string()))?;
    }
    if dataset.rows.is_empty() {
        return Err(error("heatmaps require nonempty data"));
    }
    let mut sets = Vec::with_capacity(2);
    for encoding in [x, y] {
        let explicit = encoding
            .domain
            .as_ref()
            .map(|values| domain(values.iter().map(String::as_str)))
            .transpose()?;
        let mut inferred = BTreeSet::new();
        for row in &dataset.rows {
            let value = row
                .get(&encoding.field)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| error("heatmap x/y fields must contain non-null strings"))?;
            category(value)?;
            if let Some(explicit) = &explicit {
                if !explicit.contains(value) {
                    return Err(error(
                        "explicit heatmap domains must cover every data category",
                    ));
                }
            } else {
                inferred.insert(value);
                if inferred.len() > MAX_HEATMAP_CATEGORIES {
                    return Err(error("heatmap axes support at most 256 categories"));
                }
            }
        }
        sets.push(explicit.unwrap_or(inferred));
    }
    domain_pair(&sets[0], &sets[1])
}

fn validate_value_label_options(
    format: Option<&crate::NumberFormat>,
    color: Option<&crate::Color>,
    palette: Option<&[crate::Color]>,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(format) = format {
        super::validate_number_format(format, &format!("{source}.number_format"), diagnostics);
    }
    if let Some(color) = color {
        if crate::heatmap_opaque_rgb(color).is_none() {
            diagnostics.push(
                error("value_labels.color requires opaque #RRGGBB or #RRGGBBFF")
                    .at(format!("{source}.color")),
            );
        }
    } else if palette.is_some_and(|colors| {
        colors
            .iter()
            .any(|color| crate::heatmap_opaque_rgb(color).is_none())
    }) {
        diagnostics.push(error("automatic value label contrast requires opaque cell fills; set value_labels.color to an opaque RGB color for transparent or translucent fills").at(source));
    }
}

pub(crate) fn validate_source(
    document: &Document,
    chart: &HeatmapChart,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(dataset) = document.datasets.get(&chart.dataset) else {
        diagnostics
            .push(error("heatmap references an unknown dataset").at(format!("{source}.dataset")));
        return;
    };
    if let Some(labels) = &chart.value_labels {
        validate_value_label_options(
            labels.number_format.as_ref(),
            labels.color.as_ref(),
            chart.color.palette.as_deref(),
            &format!("{source}.value_labels"),
            diagnostics,
        );
    }
    let mut pairs = BTreeSet::new();
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    for (index, row) in dataset.rows.iter().enumerate() {
        if let (Some(x), Some(y)) = (
            row.get(&chart.x.field).and_then(serde_json::Value::as_str),
            row.get(&chart.y.field).and_then(serde_json::Value::as_str),
        ) && !pairs.insert((x, y))
        {
            diagnostics.push(
                error("duplicate heatmap (x, y) category pair")
                    .at(format!("datasets.{}.rows[{index}]", chart.dataset)),
            );
        }
        match row
            .get(&chart.color.field)
            .and_then(serde_json::Value::as_f64)
        {
            Some(value) if value.is_finite() => {
                minimum = minimum.min(value);
                maximum = maximum.max(value);
                if let Some([lo, hi]) = chart.color.domain
                    && (value < lo || value > hi)
                {
                    diagnostics.push(
                        error("heatmap value lies outside the explicit color domain")
                            .at(format!("datasets.{}.rows[{index}]", chart.dataset)),
                    );
                }
            }
            _ => diagnostics.push(
                error("heatmap color fields must contain finite numbers")
                    .at(format!("datasets.{}.rows[{index}]", chart.dataset)),
            ),
        }
    }
    if let Some(format) = &chart.color.number_format {
        super::validate_number_format(
            format,
            &format!("{source}.color.number_format"),
            diagnostics,
        );
    }
    if let Some(palette) = &chart.color.palette {
        for (index, color) in palette.iter().enumerate() {
            super::validate_color(
                color,
                &format!("{source}.color.palette[{index}]"),
                diagnostics,
            );
        }
    }
    let domain = chart.color.domain.unwrap_or([minimum, maximum]);
    // Omitted palettes use the canonical shared default bin count.
    let bins = chart
        .color
        .palette
        .as_ref()
        .map_or(crate::DEFAULT_HEATMAP_BINS, Vec::len);
    if let Err(e) = canonical_quantize_thresholds(domain, bins) {
        diagnostics.push(error(e.to_string()).at(format!("{source}.color.domain")));
    }
}

/// Bound new collections before general validation clones expression type maps.
pub(crate) fn preflight_mir(mir: &VizMir) -> Result<(), Diagnostic> {
    let mut source_total = 0;
    let mut cache_total = 0;
    let mut label_source_total = 0;
    let mut label_cache_total = 0;
    let mut label_bytes = 0;
    for (index, view) in mir.views.iter().enumerate() {
        let MirView::Chart(chart) = view else {
            continue;
        };
        for scale in &chart.scales {
            if let MirScale::QuantizeColor {
                range, thresholds, ..
            } = scale
                && (range.len() > 9 || thresholds.len() > 8)
            {
                return Err(
                    error("quantize-color scale collections exceed bounded limits")
                        .at(format!("views[{index}].scales")),
                );
            }
        }
        let ChartMark::Heatmap {
            instances,
            value_labels,
            ..
        } = &chart.mark
        else {
            continue;
        };
        let rows = mir
            .data
            .get(&chart.source)
            .map_or(0, |data| match &data.operator {
                MirDataOperator::Inline { rows } => rows.len(),
            });
        reserve(rows, &mut source_total)?;
        reserve(instances.len(), &mut cache_total)?;
        if let Some(labels) = value_labels {
            crate::reserve_heatmap_value_labels(&mut label_source_total, rows)
                .map_err(|e| error(e.to_string()))?;
            crate::reserve_heatmap_value_labels(&mut label_cache_total, labels.instances.len())
                .map_err(|e| error(e.to_string()))?;
            for label in &labels.instances {
                crate::reserve_heatmap_value_label_bytes(&mut label_bytes, &label.text)
                    .map_err(|e| error(e.to_string()))?;
            }
        }
        for guide in &chart.guides {
            label(&guide.label)?;
        }
        for scale in &chart.scales {
            if let MirScale::Band { domain: values, .. } = scale {
                if values.len() > MAX_HEATMAP_CATEGORIES {
                    return Err(error("heatmap axes support at most 256 categories"));
                }
                domain(values.iter().map(String::as_str))?;
            }
        }
        for cell in instances {
            label(&cell.x)?;
            label(&cell.y)?;
        }
    }
    Ok(())
}

pub(crate) fn validate_quantize_scale(
    scale: &MirScale,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let MirScale::QuantizeColor {
        domain,
        thresholds,
        range,
        ..
    } = scale
    else {
        return;
    };
    if let Err(e) = quantize_color_index(domain[0], *domain, thresholds, range.len()) {
        diagnostics.push(error(e.to_string()).at(source));
    }
    for (index, color) in range.iter().enumerate() {
        super::validate_color(color, &format!("{source}.range[{index}]"), diagnostics);
    }
}

pub(crate) fn validate_chart(
    mir: &VizMir,
    chart: &MirChart,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let ChartMark::Heatmap {
        x,
        y,
        color,
        value_labels,
        ..
    } = &chart.mark
    else {
        return;
    };
    // This slice emits absolute document-space rectangles. Until general
    // coordinate-space resolution is supported, reject plans whose transforms
    // or units would otherwise be silently ignored during execution or refresh.
    let document_space = mir.spaces.get(&chart.space);
    if document_space.is_none_or(|space| {
        space.kind != CoordinateSpaceKind::Document
            || space.parent.is_some()
            || space.unit != SpatialUnit::SceneUnit
            || space.transform_to_parent != Transform2D::default()
    }) {
        diagnostics.push(error(
            "heatmaps require an existing parentless document space in scene units with an identity transform",
        ).at(format!("{source}.space")));
    }
    if chart.scales.len() != 3
        || BTreeSet::from([x.scale.as_str(), y.scale.as_str(), color.scale.as_str()]).len() != 3
    {
        diagnostics.push(
            error("heatmaps require exactly three distinct bound scales")
                .at(format!("{source}.scales")),
        );
    }
    if let Some(data) = mir.data.get(&chart.source) {
        let MirDataOperator::Inline { rows } = &data.operator;
        if rows.is_empty() {
            diagnostics.push(
                error("heatmaps require nonempty source data").at(format!("{source}.source")),
            );
        }
    }
    let mut domains = Vec::with_capacity(2);
    for (name, binding) in [("x", x), ("y", y)] {
        match chart
            .scales
            .iter()
            .find(|scale| scale.id() == binding.scale)
        {
            Some(MirScale::Band {
                domain: values,
                range,
                padding,
                range_space,
                ..
            }) => {
                if range_space != &chart.space {
                    diagnostics.push(
                        error("heatmap band ranges must reference the chart's document space")
                            .at(format!("{source}.mark.{name}.scale")),
                    );
                }
                if *padding != 0.0
                    || !range.iter().all(|value| value.is_finite())
                    || range[0] >= range[1]
                    || !(range[1] - range[0]).is_finite()
                {
                    diagnostics.push(error("heatmap axes require zero-padding band scales with finite ascending ranges").at(format!("{source}.mark.{name}")));
                }
                match domain(values.iter().map(String::as_str)) {
                    Ok(values) => domains.push(values),
                    Err(e) => diagnostics.push(e.at(format!("{source}.mark.{name}"))),
                }
            }
            _ => diagnostics.push(
                error("heatmap x/y bindings require band scales")
                    .at(format!("{source}.mark.{name}")),
            ),
        }
        if mir
            .expressions
            .get(&binding.expression)
            .is_some_and(|expression| expression.result_type != ValueType::String)
        {
            diagnostics.push(
                error("heatmap x/y expressions must have non-null string type")
                    .at(format!("{source}.mark.{name}.expression")),
            );
        }
    }
    if domains.len() == 2
        && let Err(e) = domain_pair(&domains[0], &domains[1])
    {
        diagnostics.push(e.at(format!("{source}.scales")));
    }
    let color_scale = chart.scales.iter().find(|scale| scale.id() == color.scale);
    if let Some(labels) = value_labels {
        let palette = color_scale.and_then(|scale| match scale {
            MirScale::QuantizeColor { range, .. } => Some(range.as_slice()),
            _ => None,
        });
        validate_value_label_options(
            labels.number_format.as_ref(),
            labels.color.as_ref(),
            palette,
            &format!("{source}.mark.value_labels"),
            diagnostics,
        );
    }
    if !matches!(color_scale, Some(MirScale::QuantizeColor { .. })) {
        diagnostics.push(
            error("heatmap color binding requires a quantize-color scale")
                .at(format!("{source}.mark.color")),
        );
    }
    if mir
        .expressions
        .get(&color.expression)
        .is_some_and(|expression| {
            !matches!(
                expression.result_type,
                ValueType::Int64 | ValueType::Float64
            )
        })
    {
        diagnostics.push(
            error("heatmap color expressions must have non-null numeric type")
                .at(format!("{source}.mark.color.expression")),
        );
    }
    let guides_match = [
        (x, GuideKind::Axis, GuideOrient::Bottom),
        (y, GuideKind::Axis, GuideOrient::Left),
        (color, GuideKind::Legend, GuideOrient::Right),
    ]
    .into_iter()
    .all(|(binding, kind, orient)| {
        chart
            .guides
            .iter()
            .filter(|guide| {
                guide.scale == binding.scale && guide.kind == kind && guide.orient == orient
            })
            .count()
            == 1
    });
    if chart.guides.len() != 3 || !guides_match {
        diagnostics.push(error("heatmaps require one bottom x axis, one left y axis, and one right quantitative legend").at(format!("{source}.guides")));
    }
    // Supplied instances are replay assertions, not execution inputs. Their
    // count/byte bounds are checked before work; authoritative rows are evaluated
    // and checked by materialization, which is allowed to repair stale caches.
}
