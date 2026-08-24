use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use vizir_core::{
    BarChart, ChartMark, Color, ColorEncoding, Document, GuideKind, GuideOrient, LineChart,
    MirBarItem, MirChart, MirDiagram, MirGeometry, MirGuide, MirPointItem, MirScale, MirSeries,
    MirView, ScatterChart, View, VizError, VizMir, VizResult, value_as_key,
};

const DEFAULT_PALETTE: [&str; 8] = [
    "#3B6EF5", "#EB5E55", "#18A999", "#F2A541", "#7A5AF8", "#D94891", "#4B8B3B", "#65758B",
];

pub fn lower_to_mir(document: &Document) -> VizResult<VizMir> {
    let mut views = Vec::with_capacity(document.views.len());
    for view in &document.views {
        views.push(match view {
            View::Scatter(chart) => {
                MirView::Chart(lower_scatter(document, chart).map_err(lowering_error)?)
            }
            View::Line(chart) => {
                MirView::Chart(lower_line(document, chart).map_err(lowering_error)?)
            }
            View::Bar(chart) => MirView::Chart(lower_bar(document, chart).map_err(lowering_error)?),
            View::Diagram(diagram) => MirView::Diagram(MirDiagram {
                id: diagram.id.clone(),
                title: diagram.title.clone(),
                frame: diagram.frame,
                nodes: diagram.nodes.clone(),
                edges: diagram.edges.clone(),
                layout_request: vizir_core::LayoutRequest {
                    id: format!("{}/layout", diagram.id),
                    algorithm: diagram.layout.clone(),
                    node_ids: diagram.nodes.iter().map(|node| node.id.clone()).collect(),
                    seed: 42,
                },
                provenance: vec![
                    "diagram.graph lowered to topology plus an explicit layout request".to_owned(),
                    format!("layout algorithm resolved to {:?}", diagram.layout),
                ],
            }),
            View::Geometry(geometry) => MirView::Geometry(MirGeometry {
                id: geometry.id.clone(),
                title: geometry.title.clone(),
                frame: geometry.frame,
                children: geometry.children.clone(),
                provenance: vec![
                    "geometry.scene is already expressed as normalized portable primitives"
                        .to_owned(),
                ],
            }),
        });
    }

    Ok(VizMir {
        version: document.version.clone(),
        document_id: document.id.clone(),
        width: document.width,
        height: document.height,
        background: document.background.clone(),
        views,
        losses: Vec::new(),
    })
}

fn lower_scatter(document: &Document, chart: &ScatterChart) -> Result<MirChart, String> {
    let dataset = dataset(document, &chart.dataset)?;
    let plot = chart_plot_bounds(chart.frame);
    let x_values = numeric_values(&dataset.rows, &chart.x.field)?;
    let y_values = numeric_values(&dataset.rows, &chart.y.field)?;
    let x_domain = nice_domain(extent(&x_values), false);
    let y_domain = nice_domain(extent(&y_values), false);
    let color_scale = color_scale(&chart.id, chart.color.as_ref(), &dataset.rows)?;
    let mut items = Vec::with_capacity(dataset.rows.len());
    for row in &dataset.rows {
        items.push(MirPointItem {
            key: key(row, &dataset.key)?,
            x: number(row, &chart.x.field)?,
            y: number(row, &chart.y.field)?,
            color_category: optional_category(row, chart.color.as_ref())?,
        });
    }

    let mut scales = vec![
        MirScale::Linear {
            id: format!("{}/x", chart.id),
            domain: x_domain,
            range: [plot[0], plot[2]],
            zero: false,
        },
        MirScale::Linear {
            id: format!("{}/y", chart.id),
            domain: y_domain,
            range: [plot[3], plot[1]],
            zero: false,
        },
    ];
    if let Some(scale) = color_scale {
        scales.push(scale);
    }

    Ok(MirChart {
        id: chart.id.clone(),
        title: chart.title.clone(),
        frame: chart.frame,
        scales,
        guides: chart_guides(chart, chart.color.as_ref()),
        mark: ChartMark::Symbol {
            x_scale: format!("{}/x", chart.id),
            y_scale: format!("{}/y", chart.id),
            color_scale: chart.color.as_ref().map(|_| format!("{}/color", chart.id)),
            size: chart.point_size,
            items,
        },
        provenance: vec![
            format!("x domain inferred from {} finite values", x_values.len()),
            format!("y domain inferred from {} finite values", y_values.len()),
            "linear domains expanded with deterministic nice-domain policy".to_owned(),
            "axes made explicit during chart dialect lowering".to_owned(),
        ],
    })
}

fn lower_line(document: &Document, chart: &LineChart) -> Result<MirChart, String> {
    let dataset = dataset(document, &chart.dataset)?;
    let plot = chart_plot_bounds(chart.frame);
    let x_values = numeric_values(&dataset.rows, &chart.x.field)?;
    let y_values = numeric_values(&dataset.rows, &chart.y.field)?;
    let x_domain = nice_domain(extent(&x_values), false);
    let y_domain = nice_domain(extent(&y_values), false);
    let color_scale = color_scale(&chart.id, chart.series.as_ref(), &dataset.rows)?;
    let mut grouped: BTreeMap<String, Vec<MirPointItem>> = BTreeMap::new();
    for row in &dataset.rows {
        let series =
            optional_category(row, chart.series.as_ref())?.unwrap_or_else(|| "series".to_owned());
        grouped
            .entry(series.clone())
            .or_default()
            .push(MirPointItem {
                key: key(row, &dataset.key)?,
                x: number(row, &chart.x.field)?,
                y: number(row, &chart.y.field)?,
                color_category: chart.series.as_ref().map(|_| series),
            });
    }
    let mut series = grouped
        .into_iter()
        .map(|(key, mut points)| {
            points.sort_by(|a, b| a.x.total_cmp(&b.x).then_with(|| a.key.cmp(&b.key)));
            MirSeries {
                color_category: chart.series.as_ref().map(|_| key.clone()),
                key,
                points,
            }
        })
        .collect::<Vec<_>>();
    series.sort_by(|a, b| a.key.cmp(&b.key));

    let mut scales = vec![
        MirScale::Linear {
            id: format!("{}/x", chart.id),
            domain: x_domain,
            range: [plot[0], plot[2]],
            zero: false,
        },
        MirScale::Linear {
            id: format!("{}/y", chart.id),
            domain: y_domain,
            range: [plot[3], plot[1]],
            zero: false,
        },
    ];
    if let Some(scale) = color_scale {
        scales.push(scale);
    }

    Ok(MirChart {
        id: chart.id.clone(),
        title: chart.title.clone(),
        frame: chart.frame,
        scales,
        guides: vec![
            MirGuide {
                kind: GuideKind::Axis,
                scale: format!("{}/x", chart.id),
                label: chart
                    .x
                    .label
                    .clone()
                    .unwrap_or_else(|| chart.x.field.clone()),
                orient: GuideOrient::Bottom,
            },
            MirGuide {
                kind: GuideKind::Axis,
                scale: format!("{}/y", chart.id),
                label: chart
                    .y
                    .label
                    .clone()
                    .unwrap_or_else(|| chart.y.field.clone()),
                orient: GuideOrient::Left,
            },
        ],
        mark: ChartMark::Line {
            x_scale: format!("{}/x", chart.id),
            y_scale: format!("{}/y", chart.id),
            color_scale: chart.series.as_ref().map(|_| format!("{}/color", chart.id)),
            line_width: chart.line_width,
            show_points: chart.show_points,
            series,
        },
        provenance: vec![
            "rows grouped by stable series value and sorted by x encoding".to_owned(),
            "linear scale domains inferred and expanded deterministically".to_owned(),
            "line topology remains in MIR; coordinates are unresolved".to_owned(),
        ],
    })
}

fn lower_bar(document: &Document, chart: &BarChart) -> Result<MirChart, String> {
    let dataset = dataset(document, &chart.dataset)?;
    let plot = chart_plot_bounds(chart.frame);
    let values = numeric_values(&dataset.rows, &chart.value.field)?;
    let categories = dataset
        .rows
        .iter()
        .map(|row| category(row, &chart.category.field))
        .collect::<Result<Vec<_>, _>>()?;
    let mut unique = BTreeSet::new();
    for category in &categories {
        if !unique.insert(category.clone()) {
            return Err(format!("bar category {category:?} is duplicated"));
        }
    }
    let color_scale = color_scale(&chart.id, chart.color.as_ref(), &dataset.rows)?;
    let mut items = Vec::with_capacity(dataset.rows.len());
    for row in &dataset.rows {
        items.push(MirBarItem {
            key: key(row, &dataset.key)?,
            category: category(row, &chart.category.field)?,
            value: number(row, &chart.value.field)?,
            color_category: optional_category(row, chart.color.as_ref())?,
        });
    }
    let raw = extent(&values);
    let domain = nice_domain([raw[0].min(0.0), raw[1].max(0.0)], true);
    let mut scales = vec![
        MirScale::Band {
            id: format!("{}/category", chart.id),
            domain: categories,
            range: [plot[0], plot[2]],
            padding: 0.22,
        },
        MirScale::Linear {
            id: format!("{}/value", chart.id),
            domain,
            range: [plot[3], plot[1]],
            zero: true,
        },
    ];
    if let Some(scale) = color_scale {
        scales.push(scale);
    }

    Ok(MirChart {
        id: chart.id.clone(),
        title: chart.title.clone(),
        frame: chart.frame,
        scales,
        guides: vec![
            MirGuide {
                kind: GuideKind::Axis,
                scale: format!("{}/category", chart.id),
                label: chart
                    .category
                    .label
                    .clone()
                    .unwrap_or_else(|| chart.category.field.clone()),
                orient: GuideOrient::Bottom,
            },
            MirGuide {
                kind: GuideKind::Axis,
                scale: format!("{}/value", chart.id),
                label: chart
                    .value
                    .label
                    .clone()
                    .unwrap_or_else(|| chart.value.field.clone()),
                orient: GuideOrient::Left,
            },
        ],
        mark: ChartMark::Bar {
            category_scale: format!("{}/category", chart.id),
            value_scale: format!("{}/value", chart.id),
            color_scale: chart.color.as_ref().map(|_| format!("{}/color", chart.id)),
            items,
        },
        provenance: vec![
            "one bar generated per unique category".to_owned(),
            "quantitative domain includes zero to preserve bar-chart truth".to_owned(),
            "band placement remains unresolved until Scene2D construction".to_owned(),
        ],
    })
}

fn chart_guides(chart: &ScatterChart, color: Option<&ColorEncoding>) -> Vec<MirGuide> {
    let mut guides = vec![
        MirGuide {
            kind: GuideKind::Axis,
            scale: format!("{}/x", chart.id),
            label: chart
                .x
                .label
                .clone()
                .unwrap_or_else(|| chart.x.field.clone()),
            orient: GuideOrient::Bottom,
        },
        MirGuide {
            kind: GuideKind::Axis,
            scale: format!("{}/y", chart.id),
            label: chart
                .y
                .label
                .clone()
                .unwrap_or_else(|| chart.y.field.clone()),
            orient: GuideOrient::Left,
        },
    ];
    if let Some(color) = color {
        guides.push(MirGuide {
            kind: GuideKind::Legend,
            scale: format!("{}/color", chart.id),
            label: color.field.clone(),
            orient: GuideOrient::Right,
        });
    }
    guides
}

fn color_scale(
    chart_id: &str,
    encoding: Option<&ColorEncoding>,
    rows: &[BTreeMap<String, Value>],
) -> Result<Option<MirScale>, String> {
    let Some(encoding) = encoding else {
        return Ok(None);
    };
    let domain = rows
        .iter()
        .map(|row| category(row, &encoding.field))
        .collect::<Result<BTreeSet<_>, _>>()?
        .into_iter()
        .collect::<Vec<_>>();
    let palette = if encoding.palette.is_empty() {
        DEFAULT_PALETTE
            .iter()
            .map(|value| Color::hex(value))
            .collect::<Vec<_>>()
    } else {
        encoding.palette.clone()
    };
    let range = (0..domain.len())
        .map(|index| palette[index % palette.len()].clone())
        .collect();
    Ok(Some(MirScale::OrdinalColor {
        id: format!("{chart_id}/color"),
        domain,
        range,
    }))
}

fn chart_plot_bounds(frame: vizir_core::Frame) -> [f64; 4] {
    [
        frame.x + 64.0,
        frame.y + 50.0,
        frame.x + frame.width - 30.0,
        frame.y + frame.height - 62.0,
    ]
}

fn dataset<'a>(document: &'a Document, name: &str) -> Result<&'a vizir_core::Dataset, String> {
    document
        .datasets
        .get(name)
        .ok_or_else(|| format!("dataset {name:?} disappeared after validation"))
}

fn key(row: &BTreeMap<String, Value>, field: &str) -> Result<String, String> {
    row.get(field)
        .and_then(value_as_key)
        .ok_or_else(|| format!("stable key field {field:?} disappeared after validation"))
}

fn number(row: &BTreeMap<String, Value>, field: &str) -> Result<f64, String> {
    row.get(field)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("numeric field {field:?} disappeared after validation"))
}

fn category(row: &BTreeMap<String, Value>, field: &str) -> Result<String, String> {
    row.get(field)
        .and_then(value_as_key)
        .ok_or_else(|| format!("category field {field:?} disappeared after validation"))
}

fn optional_category(
    row: &BTreeMap<String, Value>,
    encoding: Option<&ColorEncoding>,
) -> Result<Option<String>, String> {
    encoding
        .map(|encoding| category(row, &encoding.field))
        .transpose()
}

fn numeric_values(rows: &[BTreeMap<String, Value>], field: &str) -> Result<Vec<f64>, String> {
    rows.iter().map(|row| number(row, field)).collect()
}

fn extent(values: &[f64]) -> [f64; 2] {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for value in values {
        min = min.min(*value);
        max = max.max(*value);
    }
    [min, max]
}

fn nice_domain(domain: [f64; 2], include_zero: bool) -> [f64; 2] {
    let mut min = domain[0];
    let mut max = domain[1];
    if include_zero {
        min = min.min(0.0);
        max = max.max(0.0);
    }
    if (max - min).abs() < f64::EPSILON {
        let delta = max.abs().max(1.0) * 0.1;
        return [min - delta, max + delta];
    }
    let span = max - min;
    let power = 10_f64.powf(span.log10().floor());
    let normalized = span / power;
    let step = if normalized < 2.0 {
        power / 5.0
    } else if normalized < 5.0 {
        power / 2.0
    } else {
        power
    };
    [(min / step).floor() * step, (max / step).ceil() * step]
}

fn lowering_error(message: String) -> VizError {
    VizError::Diagnostic(format!("VIZ-LOWER-0001: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_domain_is_deterministic_and_expands() {
        assert_eq!(nice_domain([46.0, 230.0], false), [40.0, 240.0]);
        assert_eq!(nice_domain([2.0, 9.0], true), [0.0, 9.0]);
    }
}
