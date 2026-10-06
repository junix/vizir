//! Explicit uniform numeric plot insets. Stored ranges are assertions at replay.
use std::collections::BTreeMap;

use vizir_core::{
    ChartMark, Document, MirPlotAlignment, MirPlotAlignmentMember, MirScale, MirView, VizError,
    VizMir, VizResult,
};

use crate::materialize::Budget;
use crate::scene_builder::resolve_chart_layout;
use crate::text::TextSession;

fn error(detail: impl std::fmt::Display) -> VizError {
    VizError::Diagnostic(format!("VIZ-ALIGN-0004: {detail}"))
}

pub(crate) fn lower_group(
    document: &Document,
    views: &[MirView],
) -> VizResult<Option<MirPlotAlignment>> {
    let Some(group) = &document.plot_alignment else {
        return Ok(None);
    };
    let members = group
        .members
        .iter()
        .map(|id| {
            let Some(MirView::Chart(chart)) = views.iter().find(|view| view.id() == id) else {
                return Err(error(
                    "alignment member does not resolve to a numeric chart",
                ));
            };
            let (x, y) = match &chart.mark {
                ChartMark::Symbol { x, y, .. }
                | ChartMark::Line { x, y, .. }
                | ChartMark::Area { x, y, .. } => (x, y),
                _ => {
                    return Err(error(
                        "alignment requires numeric line, scatter, or area charts",
                    ));
                }
            };
            Ok(MirPlotAlignmentMember {
                view: id.clone(),
                x_scale: x.scale.clone(),
                y_scale: y.scale.clone(),
            })
        })
        .collect::<VizResult<Vec<_>>>()?;
    Ok(Some(MirPlotAlignment {
        id: group.id.clone(),
        mode: group.mode,
        members,
    }))
}

fn preflight_group(id: &str, count: usize, budget: &mut Budget) -> VizResult<()> {
    if !(2..=64).contains(&count) || id.len() > 16_384 {
        return Err(error("plot alignment exceeds bounded member or ID limits"));
    }
    budget.charge((id.len() + count) as u64, id)
}

fn reference(id: &str, group: &str, budget: &mut Budget) -> VizResult<()> {
    if id.len() > 16_384 {
        return Err(error("plot alignment reference exceeds 16384 bytes"));
    }
    budget.charge(id.len() as u64 + 1, group)
}

pub(crate) fn preflight_document(document: &Document, budget: &mut Budget) -> VizResult<()> {
    let Some(group) = &document.plot_alignment else {
        return Ok(());
    };
    preflight_group(&group.id, group.members.len(), budget)?;
    for member in &group.members {
        reference(member, &group.id, budget)?;
        // Covers capability validation, group lowering and reference lookups.
        for view in &document.views {
            budget.charge((view.id().len() as u64 + 1) * 4, &group.id)?;
        }
    }
    Ok(())
}

pub(crate) fn preflight_mir(mir: &VizMir, budget: &mut Budget) -> VizResult<()> {
    let Some(group) = &mir.plot_alignment else {
        return Ok(());
    };
    preflight_group(&group.id, group.members.len(), budget)?;
    for member in &group.members {
        for id in [&member.view, &member.x_scale, &member.y_scale] {
            reference(id, &group.id, budget)?;
        }
        for view in &mir.views {
            budget.charge((view.id().len() as u64 + 1) * 8, &group.id)?;
            if view.id() != member.view {
                continue;
            }
            if let MirView::Chart(chart) = view {
                // Covers repeated local-layout/final-fit/replay traversal, prior
                // to shaping or cloning. TextSession independently bounds glyphs.
                let charge_text = |label: &str, budget: &mut Budget| {
                    budget.charge((label.len() as u64 + 1) * 8, &group.id)
                };
                if let Some(title) = &chart.title {
                    charge_text(title, budget)?;
                }
                for guide in &chart.guides {
                    charge_text(&guide.id, budget)?;
                    charge_text(&guide.scale, budget)?;
                    charge_text(&guide.label, budget)?;
                    // Six numeric tick strings and measurement per axis.
                    budget.charge(6 * 128 * 8, &group.id)?;
                }
                for scale in &chart.scales {
                    charge_text(scale.id(), budget)?;
                    if let MirScale::OrdinalColor { domain, .. } = scale {
                        for label in domain {
                            charge_text(label, budget)?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Recompute requirements from semantic inputs only; never read stored ranges.
/// Members were bounded and reference-validated before this function is called.
pub(crate) fn resolve(
    mir: &VizMir,
    text: Option<&TextSession>,
) -> VizResult<BTreeMap<String, [f64; 4]>> {
    let Some(group) = &mir.plot_alignment else {
        return Ok(BTreeMap::new());
    };
    let mut requirements = Vec::with_capacity(group.members.len());
    let mut insets = [0.0_f64; 4];
    for member in &group.members {
        let Some(MirView::Chart(chart)) = mir.views.iter().find(|view| view.id() == member.view)
        else {
            return Err(error("unresolved plot alignment member"));
        };
        let layout = resolve_chart_layout(
            chart,
            text,
            crate::shared_legend::mir_owns(mir, &chart.id),
            true,
        )?;
        let [left, top, right, bottom] = layout.layout.plot;
        let frame = chart.frame;
        let local = [
            left - frame.x,
            top - frame.y,
            frame.x + frame.width - right,
            frame.y + frame.height - bottom,
        ];
        if local.iter().any(|inset| !inset.is_finite() || *inset < 0.) {
            return Err(error(
                "plot alignment requires finite nonnegative local insets",
            ));
        }
        for (common, local) in insets.iter_mut().zip(local) {
            *common = common.max(local);
        }
        requirements.push((chart, layout));
    }
    let mut plots = BTreeMap::new();
    for (chart, mut layout) in requirements {
        let frame = chart.frame;
        let plot = [
            frame.x + insets[0],
            frame.y + insets[1],
            frame.x + frame.width - insets[2],
            frame.y + frame.height - insets[3],
        ];
        layout.align(chart, plot, text)?;
        plots.insert(chart.id.clone(), plot);
    }
    Ok(plots)
}

/// Normalization owns the final range assignment. Replay only calls `resolve`
/// and compares the result in scene construction; it never repairs stale ranges.
pub(crate) fn finalize(mir: &mut VizMir, text: Option<&TextSession>) -> VizResult<()> {
    if mir.plot_alignment.is_none() {
        return Ok(());
    }
    vizir_core::validate_mir(mir).map_err(|diagnostics| VizError::validation(&diagnostics))?;
    let plots = resolve(mir, text)?;
    for view in &mut mir.views {
        let MirView::Chart(chart) = view else {
            continue;
        };
        let Some(plot) = plots.get(&chart.id) else {
            continue;
        };
        let (x, y) = match &chart.mark {
            ChartMark::Symbol { x, y, .. }
            | ChartMark::Line { x, y, .. }
            | ChartMark::Area { x, y, .. } => (&x.scale, &y.scale),
            _ => return Err(error("alignment member is not a numeric chart")),
        };
        for scale in &mut chart.scales {
            if let MirScale::Linear { id, range, .. } = scale {
                if id == x {
                    *range = [plot[0], plot[2]];
                }
                if id == y {
                    *range = [plot[3], plot[1]];
                }
            }
        }
    }
    Ok(())
}
