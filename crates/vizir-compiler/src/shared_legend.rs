//! A fixed, composition-owned categorical guide. Member scales remain authoritative.
use std::collections::BTreeSet;

use vizir_core::{
    ChartMark, Color, ColorEncoding, Document, FontWeight, MirChart, MirScale, MirSharedLegend,
    MirSharedLegendMember, MirView, Origin, Point, Rect, ResolvedStyle, SceneNode, TextAnchor,
    Transform2D, View, VizError, VizMir, VizResult,
};

use crate::ResolvedThemeDefaults;
use crate::chart_layout::{header_text_width, legend_domain};
use crate::materialize::Budget;
use crate::text::TextSession;

const PADDING: f64 = 8.;
const SWATCH: f64 = 10.;
const LABEL_GAP: f64 = 6.;
const ENTRY_GAP: f64 = 20.;
const ROW_GAP: f64 = 6.;
const TITLE_GAP: f64 = 6.;
const LABEL_SIZE: f64 = 10.5;
const TITLE_SIZE: f64 = 12.5;

fn error(detail: impl std::fmt::Display) -> VizError {
    VizError::Diagnostic(format!(
        "VIZ-LEGEND-0004: {detail}; enlarge the authored shared legend strip or shorten its labels"
    ))
}

pub(crate) fn hir_owns(document: &Document, id: &str) -> bool {
    document
        .shared_legend
        .as_ref()
        .is_some_and(|owner| owner.members.iter().any(|m| m == id))
}

pub(crate) fn mir_owns(mir: &VizMir, id: &str) -> bool {
    mir.shared_legend
        .as_ref()
        .is_some_and(|owner| owner.members.iter().any(|m| m.view == id))
}

pub(crate) fn local_domain<'a>(
    document: &Document,
    id: &str,
    scale: Option<&'a MirScale>,
) -> &'a [String] {
    if hir_owns(document, id) {
        &[]
    } else {
        legend_domain(scale)
    }
}

fn color_encoding(view: &View) -> Option<&ColorEncoding> {
    match view {
        View::Scatter(c) => c.color.as_ref(),
        View::Line(c) => c.series.as_ref(),
        View::Area(c) => c.series.as_ref(),
        View::Bar(c) => c.color.as_ref(),
        _ => None,
    }
}

fn color_scale(chart: &MirChart) -> Option<&str> {
    match &chart.mark {
        ChartMark::Symbol { color, .. }
        | ChartMark::Line { color, .. }
        | ChartMark::Area { color, .. }
        | ChartMark::Bar { color, .. } => color.as_ref().map(|b| b.scale.as_str()),
        _ => None,
    }
}

pub(crate) fn lower_owner(
    document: &Document,
    views: &[MirView],
) -> VizResult<Option<MirSharedLegend>> {
    let Some(owner) = &document.shared_legend else {
        return Ok(None);
    };
    let members = owner
        .members
        .iter()
        .map(|id| {
            let Some(MirView::Chart(chart)) = views.iter().find(|view| view.id() == id) else {
                return Err(error("shared legend member does not resolve to a chart"));
            };
            let scale = color_scale(chart)
                .ok_or_else(|| error("shared legend member has no categorical color binding"))?;
            Ok(MirSharedLegendMember {
                view: id.clone(),
                scale: scale.to_owned(),
            })
        })
        .collect::<VizResult<Vec<_>>>()?;
    Ok(Some(MirSharedLegend {
        id: owner.id.clone(),
        title: owner.title.clone(),
        placement: owner.placement,
        frame: owner.frame,
        members,
    }))
}

fn preflight_owner(
    id: &str,
    title: Option<&str>,
    count: usize,
    budget: &mut Budget,
) -> VizResult<()> {
    if !(2..=64).contains(&count) || id.len() > 16_384 || title.is_some_and(|t| t.len() > 16_384) {
        return Err(error(
            "shared legend owner exceeds bounded member or text limits",
        ));
    }
    budget.charge((id.len() + title.map_or(0, str::len) + count) as u64, id)
}

fn preflight_domain(domain: &[String], budget: &mut Budget, id: &str) -> VizResult<()> {
    if !(1..=256).contains(&domain.len()) {
        return Err(error("shared legend requires 1..=256 categories"));
    }
    let mut bytes = 0usize;
    for label in domain {
        bytes = bytes
            .checked_add(label.len())
            .ok_or_else(|| error("shared legend domain-byte overflow"))?;
        if label.len() > 16_384 || bytes > 1_048_576 {
            return Err(error("shared legend domain exceeds label-byte limits"));
        }
        // Account for reference equality, bounded UTF-8 validation and output copying.
        budget.charge(label.len() as u64 + 1, id)?;
    }
    Ok(())
}

pub(crate) fn preflight_document(document: &Document, budget: &mut Budget) -> VizResult<()> {
    let Some(owner) = &document.shared_legend else {
        return Ok(());
    };
    preflight_owner(
        &owner.id,
        owner.title.as_deref(),
        owner.members.len(),
        budget,
    )?;
    budget.charge(document.views.len() as u64, &owner.id)?;
    let mut entries = 0usize;
    let mut lineage_bytes = 0u64;
    for id in &owner.members {
        if id.len() > 16_384 {
            return Err(error("shared legend member ID exceeds 16384 bytes"));
        }
        budget.charge(id.len() as u64, &owner.id)?;
        for view in &document.views {
            budget.charge(view.id().len() as u64 + 1, &owner.id)?;
            if view.id() == id {
                if let Some(encoding) = color_encoding(view) {
                    if let Some(domain) = encoding.domain.as_deref() {
                        preflight_domain(domain, budget, &owner.id)?;
                        entries = entries.max(domain.len());
                    }
                    let dataset = match view {
                        View::Scatter(c) => &c.dataset,
                        View::Line(c) => &c.dataset,
                        View::Area(c) => &c.dataset,
                        View::Bar(c) => &c.dataset,
                        _ => unreachable!("color encoding identifies a supported chart"),
                    };
                    lineage_bytes = lineage_bytes
                        .checked_add(dataset.len() as u64 + 5)
                        .ok_or_else(|| error("shared legend lineage byte overflow"))?;
                    budget.charge(encoding.palette.len() as u64, &owner.id)?;
                    for color in &encoding.palette {
                        budget.charge(color.0.len() as u64, &owner.id)?;
                    }
                }
                break;
            }
        }
    }
    charge_output(entries, &owner.id, lineage_bytes, budget)
}

fn charge_output(
    entries: usize,
    owner: &str,
    lineage_bytes: u64,
    budget: &mut Budget,
) -> VizResult<()> {
    let nodes = (2 * entries + 2) as u64;
    let bytes = lineage_bytes
        .checked_add(owner.len() as u64 * 3 + 256)
        .and_then(|bytes| bytes.checked_mul(nodes))
        .ok_or_else(|| error("shared legend output byte overflow"))?;
    budget.charge(bytes, owner)
}

pub(crate) fn preflight_mir(mir: &VizMir, budget: &mut Budget) -> VizResult<()> {
    let Some(owner) = &mir.shared_legend else {
        return Ok(());
    };
    preflight_owner(
        &owner.id,
        owner.title.as_deref(),
        owner.members.len(),
        budget,
    )?;
    budget.charge(mir.views.len() as u64, &owner.id)?;
    let mut entries = 0usize;
    let mut lineage_bytes = 0u64;
    for member in &owner.members {
        if member.view.len() > 16_384 || member.scale.len() > 16_384 {
            return Err(error("shared legend member reference exceeds 16384 bytes"));
        }
        budget.charge((member.view.len() + member.scale.len()) as u64, &owner.id)?;
        for view in &mir.views {
            budget.charge(view.id().len() as u64 + 1, &owner.id)?;
            if view.id() == member.view {
                if let MirView::Chart(chart) = view {
                    lineage_bytes = lineage_bytes
                        .checked_add(chart.source.len() as u64)
                        .ok_or_else(|| error("shared legend lineage byte overflow"))?;
                    for scale in &chart.scales {
                        budget.charge(scale.id().len() as u64 + 1, &owner.id)?;
                        if scale.id() == member.scale
                            && let MirScale::OrdinalColor { domain, range, .. } = scale
                        {
                            preflight_domain(domain, budget, &owner.id)?;
                            entries = entries.max(domain.len());
                            if range.len() > 256 {
                                return Err(error("shared legend color range exceeds 256 entries"));
                            }
                            for color in range {
                                budget.charge(color.0.len() as u64 + 1, &owner.id)?;
                            }
                        }
                    }
                }
                break;
            }
        }
    }
    charge_output(entries, &owner.id, lineage_bytes, budget)
}

fn measure(
    source: &str,
    size: f64,
    weight: FontWeight,
    text: Option<&TextSession>,
) -> VizResult<Rect> {
    match text {
        Some(text) => text.single_line_box(source, size, weight, Point { x: 0., y: 0. }),
        None => Ok(Rect {
            x: 0.,
            y: -size,
            width: header_text_width(source, size),
            height: size * 1.25,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn text_node(
    id: String,
    label: &str,
    metrics: Rect,
    top_left: Point,
    size: f64,
    weight: FontWeight,
    color: Color,
    origin: Origin,
) -> SceneNode {
    SceneNode::Text {
        id,
        bounds: Rect {
            x: top_left.x,
            y: top_left.y,
            width: metrics.width,
            height: metrics.height,
        },
        position: Point {
            x: top_left.x - metrics.x,
            y: top_left.y - metrics.y,
        },
        text: label.to_owned(),
        font_size: size,
        anchor: TextAnchor::Start,
        color,
        weight,
        origin,
    }
}

/// Build from validated references. Layout is recomputed under the persisted
/// document context and can only consume the fixed owner frame.
pub(crate) fn build(
    mir: &VizMir,
    defaults: Option<&ResolvedThemeDefaults>,
    text: Option<&TextSession>,
) -> VizResult<Option<SceneNode>> {
    let Some(owner) = &mir.shared_legend else {
        return Ok(None);
    };
    let first = owner
        .members
        .first()
        .ok_or_else(|| error("shared legend has no members"))?;
    let chart = match mir.views.iter().find(|v| v.id() == first.view) {
        Some(MirView::Chart(c)) => c,
        _ => return Err(error("shared legend reference is unresolved")),
    };
    let Some(MirScale::OrdinalColor { domain, range, .. }) =
        chart.scales.iter().find(|s| s.id() == first.scale)
    else {
        return Err(error("shared legend scale is not ordinal color"));
    };
    let frame = owner.frame;
    let left = frame.x + PADDING;
    let right = frame.x + frame.width - PADDING;
    let bottom = frame.y + frame.height - PADDING;
    let mut top = frame.y + PADDING;
    if !(right > left
        && bottom > top
        && left > frame.x
        && top > frame.y
        && right < frame.x + frame.width
        && bottom < frame.y + frame.height)
    {
        return Err(error(
            "shared legend strip has no representable padded content area",
        ));
    }
    let prefix = format!("shared-legend:{}:{}", owner.id.len(), owner.id);
    let lineage: Vec<_> = owner
        .members
        .iter()
        .filter_map(|m| match mir.views.iter().find(|v| v.id() == m.view) {
            Some(MirView::Chart(c)) => Some(c.source.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let origin = Origin { hir_node: owner.id.clone(), mir_node: owner.id.clone(), data_key: None,
        data_lineage: lineage, generated_by: "build-shared-legend".into(),
        explanation: "composition-owned categorical color guide; exact ordered member scale mappings retained".into() };
    let mut children = Vec::with_capacity(domain.len() * 2 + usize::from(owner.title.is_some()));
    if let Some(title) = &owner.title {
        let metrics = measure(title, TITLE_SIZE, FontWeight::Medium, text)?;
        if metrics.width > right - left || top + metrics.height > bottom {
            return Err(error(
                "shared legend title does not fit its single-line strip",
            ));
        }
        children.push(text_node(
            format!("{prefix}/title"),
            title,
            metrics,
            Point { x: left, y: top },
            TITLE_SIZE,
            FontWeight::Medium,
            defaults
                .map(|d| d.ink.clone())
                .unwrap_or_else(|| Color::hex("#1C2736")),
            origin.clone(),
        ));
        top += metrics.height + TITLE_GAP;
    }
    let mut x = left;
    let mut row_height: f64 = 0.;
    for (index, (label, color)) in domain.iter().zip(range).enumerate() {
        let metrics = measure(label, LABEL_SIZE, FontWeight::Regular, text)?;
        let width = SWATCH + LABEL_GAP + metrics.width;
        let height = SWATCH.max(metrics.height);
        if !width.is_finite() || width > right - left {
            return Err(error(format!(
                "shared legend entry {index} cannot fit on one row"
            )));
        }
        if x > left && x + width > right {
            top += row_height + ROW_GAP;
            x = left;
            row_height = 0.;
        }
        if top + height > bottom
            || x + width <= x
            || top + height <= top
            || x + SWATCH <= x
            || x + SWATCH + LABEL_GAP <= x + SWATCH
            || x + width + ENTRY_GAP <= x + width
        {
            return Err(error("shared legend entries overflow the fixed strip"));
        }
        // Every entry shares the row top. Its measured envelope and swatch are
        // disjoint from adjacent entries; row height is the maximum full envelope.
        row_height = row_height.max(height);
        let bounds = Rect {
            x,
            y: top + (height - SWATCH) / 2.,
            width: SWATCH,
            height: SWATCH,
        };
        children.push(SceneNode::Rect {
            id: format!("{prefix}/entry/{index}/swatch"),
            bounds,
            origin: origin.clone(),
            radius: 0.,
            style: ResolvedStyle {
                fill: color.clone(),
                stroke: Color::transparent(),
                stroke_width: 0.,
                opacity: 1.,
            },
        });
        children.push(text_node(
            format!("{prefix}/entry/{index}/label"),
            label,
            metrics,
            Point {
                x: x + SWATCH + LABEL_GAP,
                y: top + (height - metrics.height) / 2.,
            },
            LABEL_SIZE,
            FontWeight::Regular,
            defaults
                .map(|d| d.muted.clone())
                .unwrap_or_else(|| Color::hex("#596579")),
            origin.clone(),
        ));
        x += width + ENTRY_GAP;
    }
    let region = Rect {
        x: frame.x,
        y: frame.y,
        width: frame.width,
        height: frame.height,
    };
    if let Some(text) = text {
        for node in &children {
            text.check_text_box(node, region)?;
        }
    }
    Ok(Some(SceneNode::Group {
        id: prefix,
        bounds: region,
        origin,
        transform: Transform2D::default(),
        opacity: 1.,
        children,
    }))
}
