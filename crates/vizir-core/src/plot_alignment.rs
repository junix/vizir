//! Explicit, opt-in uniform plot-area alignment.
//!
//! Members retain their own numeric domains and scale identities. The group
//! coordinates guide insets only; it does not share scales or modify data.
use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ChartMark, CoordinateSpaceKind, Diagnostic, Document, Frame, GuideKind, GuideOrient, MirScale,
    MirView, SpatialUnit, Transform2D, View, VizMir,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PlotAlignmentMode {
    Uniform,
}

/// Composition and HIR reference stable view IDs in authored member order.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(transform = alignment_member_schema)]
pub struct PlotAlignment {
    #[schemars(length(min = 1, max = 16384), regex(pattern = "^[A-Za-z0-9_/-]+$"))]
    #[serde(deserialize_with = "deserialize_id")]
    pub id: String,
    pub mode: PlotAlignmentMode,
    #[serde(deserialize_with = "deserialize_member_ids")]
    #[schemars(length(min = 2, max = 64), extend("uniqueItems" = true))]
    pub members: Vec<String>,
}

/// MIR records exact references to each member's independent numeric scales.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MirPlotAlignment {
    #[schemars(length(min = 1, max = 16384), regex(pattern = "^[A-Za-z0-9_/-]+$"))]
    #[serde(deserialize_with = "deserialize_id")]
    pub id: String,
    pub mode: PlotAlignmentMode,
    #[schemars(length(min = 2, max = 64), extend("uniqueItems" = true))]
    pub members: Vec<MirPlotAlignmentMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MirPlotAlignmentMember {
    #[schemars(length(min = 1, max = 16384), regex(pattern = "^[A-Za-z0-9_/-]+$"))]
    #[serde(deserialize_with = "deserialize_id")]
    pub view: String,
    #[schemars(length(min = 1, max = 16384), regex(pattern = "^[A-Za-z0-9_/-]+$"))]
    #[serde(deserialize_with = "deserialize_id")]
    pub x_scale: String,
    #[schemars(length(min = 1, max = 16384), regex(pattern = "^[A-Za-z0-9_/-]+$"))]
    #[serde(deserialize_with = "deserialize_id")]
    pub y_scale: String,
}

fn alignment_member_schema(schema: &mut schemars::Schema) {
    // Bound each authored reference as well as the member array itself.
    schema
        .as_object_mut()
        .expect("plot alignment schema object")
        .get_mut("properties")
        .expect("plot alignment properties")["members"]["items"] = serde_json::json!({"type":"string", "minLength":1, "maxLength":16384, "pattern":"^[A-Za-z0-9_/-]+$"});
}

fn deserialize_id<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(id) => Ok(id),
        _ => Err(serde::de::Error::custom(
            "plot alignment IDs and references must be strings",
        )),
    }
}

fn deserialize_member_ids<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    Vec::<serde_json::Value>::deserialize(deserializer)?
        .into_iter()
        .map(|value| match value {
            serde_json::Value::String(id) => Ok(id),
            _ => Err(serde::de::Error::custom(
                "plot alignment member IDs must be strings",
            )),
        })
        .collect()
}

fn validate_id(id: &str, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    if id.is_empty()
        || id.len() > 16_384
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'/'))
    {
        diagnostics.push(Diagnostic::new("VIZ-ALIGN-0002", "plot alignment IDs must contain 1..=16384 ASCII letters, digits, hyphens, underscores, or slashes").at(source));
    }
}

fn validate_members<'a>(
    id: &str,
    members: impl Iterator<Item = &'a str>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let initial = diagnostics.len();
    validate_id(id, "plot_alignment.id", diagnostics);
    let mut ids = BTreeSet::new();
    let mut count = 0;
    for (index, member) in members.take(65).enumerate() {
        count += 1;
        let source = format!("plot_alignment.members[{index}]");
        validate_id(member, &source, diagnostics);
        if !ids.insert(member) {
            diagnostics.push(
                Diagnostic::new("VIZ-ALIGN-0002", "plot alignment members must be unique")
                    .at(source),
            );
        }
    }
    if !(2..=64).contains(&count) {
        diagnostics.push(
            Diagnostic::new(
                "VIZ-ALIGN-0002",
                "plot alignment requires 2..=64 unique members",
            )
            .at("plot_alignment.members"),
        );
    }
    diagnostics.len() == initial
}

fn validate_group_id<'a>(
    id: &str,
    views: impl Iterator<Item = &'a str>,
    legend_id: Option<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if views.into_iter().any(|view| view == id) || legend_id == Some(id) {
        diagnostics.push(
            Diagnostic::new(
                "VIZ-ALIGN-0002",
                "plot alignment ID must be distinct from every view ID and the shared legend ID",
            )
            .at("plot_alignment.id"),
        );
    }
}

/// Equal-cell composition subtracts independently anchored floating-point edges.
/// Permit only that operand-rounding noise, never a visual/layout tolerance.
fn equal_extent(left: f64, right: f64, left_origin: f64, right_origin: f64) -> bool {
    left.is_finite()
        && right.is_finite()
        && (left - right).abs()
            <= 4.0
                * f64::EPSILON
                * left
                    .abs()
                    .max(right.abs())
                    .max(left_origin.abs())
                    .max(right_origin.abs())
}

fn validate_frames(
    members: &[(usize, &Frame)],
    width: f64,
    height: f64,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for &(view_index, frame) in members {
        let source = format!("views[{view_index}].frame");
        let right = frame.x + frame.width;
        let bottom = frame.y + frame.height;
        if !width.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || height <= 0.0
            || !frame.x.is_finite()
            || !frame.y.is_finite()
            || !frame.width.is_finite()
            || !frame.height.is_finite()
            || frame.width <= 0.0
            || frame.height <= 0.0
            || frame.x < 0.0
            || frame.y < 0.0
            || !right.is_finite()
            || !bottom.is_finite()
            || right <= frame.x
            || bottom <= frame.y
            || right > width
            || bottom > height
        {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-ALIGN-0003",
                    "aligned frames must fit the canvas with finite positive representable extents",
                )
                .at(&source),
            );
            continue;
        }
        for &(other_index, other) in members {
            // Compare every pair so tolerance acceptance never depends on the
            // first authored member or the direction of its order.
            if other_index > view_index
                && (!equal_extent(frame.width, other.width, frame.x, other.x)
                    || !equal_extent(frame.height, other.height, frame.y, other.y))
            {
                diagnostics.push(
                    Diagnostic::new(
                        "VIZ-ALIGN-0003",
                        "uniform plot alignment requires equal frame widths and heights",
                    )
                    .at(&source),
                );
            }
            if other_index > view_index
                && frame.x < other.x + other.width
                && other.x < right
                && frame.y < other.y + other.height
                && other.y < bottom
            {
                diagnostics.push(
                    Diagnostic::new(
                        "VIZ-ALIGN-0003",
                        format!("aligned frame must not overlap view {other_index}"),
                    )
                    .at(&source),
                );
            }
        }
    }
}

pub(crate) fn validate_document_alignment(document: &Document, diagnostics: &mut Vec<Diagnostic>) {
    let Some(owner) = &document.plot_alignment else {
        return;
    };
    if !validate_members(
        &owner.id,
        owner.members.iter().map(String::as_str),
        diagnostics,
    ) {
        return;
    }
    validate_group_id(
        &owner.id,
        document.views.iter().map(View::id),
        document.shared_legend.as_ref().map(|v| v.id.as_str()),
        diagnostics,
    );
    let mut frames = Vec::new();
    for (member_index, member) in owner.members.iter().enumerate() {
        let source = format!("plot_alignment.members[{member_index}]");
        let resolved: Vec<_> = document
            .views
            .iter()
            .enumerate()
            .filter(|(_, view)| view.id() == member)
            .collect();
        if resolved.len() != 1 {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-ALIGN-0004",
                    format!("plot alignment member {member:?} must resolve to exactly one view"),
                )
                .at(source),
            );
            continue;
        }
        let (view_index, view) = resolved[0];
        if !matches!(view, View::Scatter(_) | View::Line(_) | View::Area(_)) {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-ALIGN-0004",
                    "plot alignment members must be numeric scatter, line, or area charts",
                )
                .at(source),
            );
            continue;
        }
        frames.push((view_index, view.frame()));
    }
    validate_frames(&frames, document.width, document.height, diagnostics);
}

pub(crate) fn validate_mir_alignment(mir: &VizMir, diagnostics: &mut Vec<Diagnostic>) {
    let Some(owner) = &mir.plot_alignment else {
        return;
    };
    if !validate_members(
        &owner.id,
        owner.members.iter().map(|m| m.view.as_str()),
        diagnostics,
    ) {
        return;
    }
    validate_group_id(
        &owner.id,
        mir.views.iter().map(MirView::id),
        mir.shared_legend.as_ref().map(|v| v.id.as_str()),
        diagnostics,
    );
    let mut frames = Vec::new();
    for (member_index, member) in owner.members.iter().enumerate() {
        let source = format!("plot_alignment.members[{member_index}]");
        validate_id(&member.x_scale, &format!("{source}.x_scale"), diagnostics);
        validate_id(&member.y_scale, &format!("{source}.y_scale"), diagnostics);
        let resolved: Vec<_> = mir
            .views
            .iter()
            .enumerate()
            .filter(|(_, view)| view.id() == member.view)
            .collect();
        if resolved.len() != 1 {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-ALIGN-0004",
                    format!(
                        "plot alignment member {:?} must resolve to exactly one view",
                        member.view
                    ),
                )
                .at(source),
            );
            continue;
        }
        let (view_index, MirView::Chart(chart)) = resolved[0] else {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-ALIGN-0004",
                    "plot alignment members must be numeric scatter, line, or area charts",
                )
                .at(source),
            );
            continue;
        };
        let (x, y) = match &chart.mark {
            ChartMark::Symbol { x, y, .. }
            | ChartMark::Line { x, y, .. }
            | ChartMark::Area { x, y, .. } => (x, y),
            _ => {
                diagnostics.push(
                    Diagnostic::new(
                        "VIZ-ALIGN-0004",
                        "plot alignment members must be numeric scatter, line, or area charts",
                    )
                    .at(source),
                );
                continue;
            }
        };
        frames.push((view_index, &chart.frame));
        if !mir.spaces.get(&chart.space).is_some_and(|space| {
            space.kind == CoordinateSpaceKind::Document
                && space.parent.is_none()
                && space.unit == SpatialUnit::SceneUnit
                && space.transform_to_parent == Transform2D::default()
        }) {
            diagnostics.push(Diagnostic::new("VIZ-ALIGN-0007", "plot alignment requires an unparented identity document coordinate space in scene units").at(format!("views[{view_index}].space")));
        }
        if x.scale != member.x_scale
            || y.scale != member.y_scale
            || member.x_scale == member.y_scale
        {
            diagnostics.push(Diagnostic::new("VIZ-ALIGN-0005", "plot alignment member must reference the mark's exact, distinct x and y scale bindings").at(&source));
        }
        for (axis, id) in [("x_scale", &member.x_scale), ("y_scale", &member.y_scale)] {
            let scales: Vec<_> = chart
                .scales
                .iter()
                .filter(|scale| scale.id() == id)
                .collect();
            if scales.len() != 1 || !matches!(scales[0], MirScale::Linear { .. }) {
                diagnostics.push(Diagnostic::new("VIZ-ALIGN-0005", "plot alignment requires exactly one existing linear scale for each numeric position binding").at(format!("{source}.{axis}")));
            } else if let MirScale::Linear {
                domain,
                range_space,
                ..
            } = scales[0]
            {
                if range_space != &chart.space {
                    diagnostics.push(
                        Diagnostic::new(
                            "VIZ-ALIGN-0007",
                            "aligned x and y scales must use their chart document coordinate space",
                        )
                        .at(format!("{source}.{axis}")),
                    );
                }
                let span = domain[1] - domain[0];
                if !domain.iter().all(|value| value.is_finite()) || !span.is_finite() || span < 0.0
                {
                    diagnostics.push(Diagnostic::new("VIZ-ALIGN-0005", "plot alignment numeric domains must have finite endpoints and a finite nonnegative span").at(format!("{source}.{axis}")));
                }
            }
        }
        let axes: Vec<_> = chart
            .guides
            .iter()
            .filter(|guide| guide.kind == GuideKind::Axis)
            .collect();
        if axes.len() != 2
            || axes
                .iter()
                .filter(|g| g.scale == member.x_scale && g.orient == GuideOrient::Bottom)
                .count()
                != 1
            || axes
                .iter()
                .filter(|g| g.scale == member.y_scale && g.orient == GuideOrient::Left)
                .count()
                != 1
        {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-ALIGN-0006",
                    "plot alignment requires exactly one bottom x axis and one left y axis",
                )
                .at(source),
            );
        }
    }
    validate_frames(&frames, mir.width, mir.height, diagnostics);
}
