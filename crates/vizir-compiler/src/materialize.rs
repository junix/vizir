//! Bounded static chart execution. Stored instances are assertions, not inputs.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use vizir_core::{
    ChartMark, Document, Expression, LiteralValue, MAX_HEATMAP_CATEGORIES, MAX_HEATMAP_CELLS,
    MAX_HEATMAP_CELLS_PER_CALL, MAX_HEATMAP_DOMAIN_BYTES, MAX_HEATMAP_GRID_CELLS,
    MAX_HEATMAP_LABEL_BYTES, MirBarItem, MirChart, MirDataNode, MirDataOperator, MirGeometryNode,
    MirHeatmapCell, MirPointItem, MirScale, MirSeries, MirView, TypedExpression, UpdateMode,
    ValueType, View, VizError, VizMir, VizResult, canonical_quantize_thresholds,
    quantize_color_index, validate_heatmap_category, value_as_key,
};

/// Whole-call limits, checked before recursive expression typing or cloning.
/// These bound materialization after parsing, not input decoding or process memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterializationLimits {
    pub max_expression_depth: usize,
    pub max_expression_nodes: usize,
    pub max_evaluation_steps: u64,
}

impl Default for MaterializationLimits {
    fn default() -> Self {
        Self {
            max_expression_depth: 64,
            max_expression_nodes: 65_536,
            max_evaluation_steps: 10_000_000,
        }
    }
}

pub(crate) struct Budget {
    limits: MaterializationLimits,
    remaining: u64,
    value_label_count: usize,
    value_label_bytes: usize,
}

impl Budget {
    pub(crate) fn new(limits: MaterializationLimits) -> Self {
        Self {
            limits,
            remaining: limits.max_evaluation_steps,
            value_label_count: 0,
            value_label_bytes: 0,
        }
    }

    pub(crate) fn step(&mut self, context: &str) -> VizResult<()> {
        self.charge(1, context)
    }

    pub(crate) fn charge(&mut self, steps: u64, context: &str) -> VizResult<()> {
        self.remaining = self.remaining.checked_sub(steps).ok_or_else(|| {
            error(
                "VIZ-MATERIALIZE-0001",
                context,
                "whole-call evaluation work limit exceeded",
            )
        })?;
        Ok(())
    }

    fn depth(&self, depth: usize, context: &str) -> VizResult<()> {
        // A configurable limit may tighten the stack-safety ceiling, never relax it.
        if depth > self.limits.max_expression_depth.min(64) {
            return Err(error(
                "VIZ-MATERIALIZE-0001",
                context,
                "expression/schema/value depth limit exceeded (maximum 64)",
            ));
        }
        Ok(())
    }
}

fn error(code: &str, context: &str, message: impl std::fmt::Display) -> VizError {
    VizError::Diagnostic(format!("{code}: {context}: {message}"))
}

/// Deliberately refresh mark caches after editing authoritative MIR inputs.
/// Explicit scale domains/ranges and all other plan fields remain unchanged.
pub fn rematerialize_mir(mir: &VizMir) -> VizResult<VizMir> {
    rematerialize_mir_with_limits(mir, MaterializationLimits::default())
}

pub fn rematerialize_mir_with_limits(
    mir: &VizMir,
    limits: MaterializationLimits,
) -> VizResult<VizMir> {
    let marks = materialize_mir_marks(mir, limits, false)?;
    // All recursively cloned data, types, expressions and geometry were bounded first.
    let views = mir
        .views
        .iter()
        .zip(marks)
        .map(|(view, mark)| match (view, mark) {
            (MirView::Chart(chart), Some(mark)) => MirView::Chart(Box::new(vizir_core::MirChart {
                id: chart.id.clone(),
                title: chart.title.clone(),
                frame: chart.frame,
                space: chart.space.clone(),
                source: chart.source.clone(),
                row_variable: chart.row_variable.clone(),
                key_expression: chart.key_expression.clone(),
                scales: chart.scales.clone(),
                guides: chart.guides.clone(),
                mark,
                provenance: chart.provenance.clone(),
            })),
            _ => view.clone(),
        })
        .collect();
    let result = VizMir {
        version: mir.version.clone(),
        source_hir_version: mir.source_hir_version.clone(),
        document_id: mir.document_id.clone(),
        width: mir.width,
        height: mir.height,
        background: mir.background.clone(),
        spaces: mir.spaces.clone(),
        data: mir.data.clone(),
        expressions: mir.expressions.clone(),
        views,
        losses: mir.losses.clone(),
    };
    crate::heatmap::check_mir_output(&result)?;
    Ok(result)
}

pub(crate) fn materialize_mir_marks(
    mir: &VizMir,
    limits: MaterializationLimits,
    verify: bool,
) -> VizResult<Vec<Option<ChartMark>>> {
    let mut budget = Budget::new(limits);
    preflight_mir(mir, &mut budget)?;
    crate::heatmap::check_mir_output(mir)?;
    // This validator clones and recursively types referenced expressions: preflight first.
    vizir_core::validate_mir(mir).map_err(|d| VizError::validation(&d))?;
    mir.views.iter().map(|view| {
        let MirView::Chart(chart) = view else { return Ok(None) };
        let source = &mir.data[&chart.source];
        let mark = materialize_mark(source, &chart.row_variable, &chart.key_expression,
            &chart.mark, &mir.expressions, &chart.id, &mut budget)?;
        check_scale_membership(chart, &mark, &mut budget)?;
        if verify && !same_cache(&chart.mark, &mark) {
            return Err(error("VIZ-MATERIALIZE-0002", &chart.id,
                "stored mark instances/series differ from data and expressions; call rematerialize_mir to refresh caches (explicit scales are preserved)"));
        }
        Ok(Some(mark))
    }).collect()
}

pub(crate) fn preflight_document(document: &Document, budget: &mut Budget) -> VizResult<()> {
    preflight_heatmap_document(document, budget)?;
    for (name, data) in &document.datasets {
        let mut fields = BTreeSet::new();
        for row in &data.rows {
            budget.step(name)?;
            for (field, value) in row {
                fields.insert(field);
                preflight_value(value, budget, name)?;
            }
        }
        // Schema inference visits every row for each unioned field, including missing cells.
        let cells = (data.rows.len() as u64)
            .checked_mul(fields.len() as u64)
            .ok_or_else(|| {
                error(
                    "VIZ-MATERIALIZE-0001",
                    name,
                    "schema inference work overflow",
                )
            })?;
        budget.charge(cells, name)?;
    }
    Ok(())
}

// Heatmap-specific hard ceilings are checked before schema inference, validation,
// expression typing, or refreshing clones. They do not change the public limits API.
fn check_heatmap_row_count(count: usize, context: &str) -> VizResult<()> {
    if count == 0 || count > MAX_HEATMAP_CELLS {
        return Err(error(
            "VIZ-MATERIALIZE-0011",
            context,
            "heatmap requires 1..=16384 source rows per chart",
        ));
    }
    Ok(())
}

fn add_heatmap_call_count(total: &mut usize, count: usize, context: &str) -> VizResult<()> {
    *total = total.checked_add(count).ok_or_else(|| {
        error(
            "VIZ-MATERIALIZE-0001",
            context,
            "heatmap whole-call count overflow",
        )
    })?;
    if *total > MAX_HEATMAP_CELLS_PER_CALL {
        return Err(error(
            "VIZ-MATERIALIZE-0001",
            context,
            "heatmap whole-call source-row or supplied-cache limit exceeded (65536)",
        ));
    }
    Ok(())
}

fn heatmap_category<'a>(
    value: &'a str,
    values: &mut BTreeSet<&'a str>,
    bytes: &mut usize,
    budget: &mut Budget,
    context: &str,
) -> VizResult<bool> {
    budget.charge(value.len() as u64 + 1, context)?;
    validate_heatmap_category(value)?;
    let fresh = values.insert(value);
    if fresh {
        *bytes = bytes.checked_add(value.len()).ok_or_else(|| {
            error(
                "VIZ-MATERIALIZE-0001",
                context,
                "heatmap category byte count overflow",
            )
        })?;
        if values.len() > MAX_HEATMAP_CATEGORIES || *bytes > MAX_HEATMAP_DOMAIN_BYTES {
            return Err(error(
                "VIZ-MATERIALIZE-0011",
                context,
                "heatmap domains require at most 256 categories per axis and 1048576 combined UTF-8 bytes",
            ));
        }
    }
    Ok(fresh)
}

fn heatmap_domain<'a>(
    domain: &'a [String],
    bytes: &mut usize,
    budget: &mut Budget,
    context: &str,
) -> VizResult<BTreeSet<&'a str>> {
    if domain.is_empty() || domain.len() > MAX_HEATMAP_CATEGORIES {
        return Err(error(
            "VIZ-MATERIALIZE-0011",
            context,
            "heatmap axis domains require 1..=256 categories",
        ));
    }
    let mut values = BTreeSet::new();
    for value in domain {
        if !heatmap_category(value, &mut values, bytes, budget, context)? {
            return Err(error(
                "VIZ-MATERIALIZE-0011",
                context,
                "heatmap axis domain contains a duplicate category",
            ));
        }
    }
    Ok(values)
}

fn check_heatmap_grid(x: usize, y: usize, context: &str) -> VizResult<()> {
    if x.checked_mul(y)
        .is_none_or(|cells| cells > MAX_HEATMAP_GRID_CELLS)
    {
        return Err(error(
            "VIZ-MATERIALIZE-0011",
            context,
            "heatmap axis-domain product exceeds 65536",
        ));
    }
    Ok(())
}

fn preflight_heatmap_document(document: &Document, budget: &mut Budget) -> VizResult<()> {
    let mut total = 0;
    let mut labels = 0;
    // Count every chart's source, even when charts share one dataset.
    for view in &document.views {
        let View::Heatmap(chart) = view else { continue };
        budget.step(&chart.id)?;
        if let Some(data) = document.datasets.get(&chart.dataset) {
            check_heatmap_row_count(data.rows.len(), &chart.id)?;
            add_heatmap_call_count(&mut total, data.rows.len(), &chart.id)?;
            if chart.value_labels.is_some() {
                vizir_core::reserve_heatmap_value_labels(&mut labels, data.rows.len())?;
            }
        }
    }
    for view in &document.views {
        let View::Heatmap(chart) = view else { continue };
        for label in [
            chart.x.label.as_deref().unwrap_or(&chart.x.field),
            chart.y.label.as_deref().unwrap_or(&chart.y.field),
            chart.color.label.as_deref().unwrap_or(&chart.color.field),
        ] {
            if label.len() > MAX_HEATMAP_LABEL_BYTES {
                return Err(error(
                    "VIZ-MATERIALIZE-0011",
                    &chart.id,
                    "heatmap labels cannot exceed 16384 UTF-8 bytes",
                ));
            }
            budget.charge(label.len() as u64 + 1, &chart.id)?;
        }
        let Some(data) = document.datasets.get(&chart.dataset) else {
            continue;
        };
        let mut bytes = 0;
        let mut x_domain = chart
            .x
            .domain
            .as_ref()
            .map(|domain| heatmap_domain(domain, &mut bytes, budget, &chart.id))
            .transpose()?
            .unwrap_or_default();
        let mut y_domain = chart
            .y
            .domain
            .as_ref()
            .map(|domain| heatmap_domain(domain, &mut bytes, budget, &chart.id))
            .transpose()?
            .unwrap_or_default();
        let mut coordinates = BTreeSet::new();
        for row in &data.rows {
            budget.step(&chart.id)?;
            if let Some(Value::String(key)) = row.get(&data.key) {
                budget.charge(key.len() as u64, &chart.id)?;
            }
            let category = |field: &str| -> VizResult<&str> {
                match row.get(field) {
                    Some(Value::String(value)) => Ok(value),
                    _ => Err(error(
                        "VIZ-MATERIALIZE-0011",
                        &chart.id,
                        format!(
                            "heatmap category field {field:?} requires exact non-null String values"
                        ),
                    )),
                }
            };
            let x = category(&chart.x.field)?;
            let y = category(&chart.y.field)?;
            for (value, explicit, domain) in [
                (x, chart.x.domain.is_some(), &mut x_domain),
                (y, chart.y.domain.is_some(), &mut y_domain),
            ] {
                if explicit {
                    budget.charge(value.len() as u64 + 1, &chart.id)?;
                    validate_heatmap_category(value)?;
                    if !domain.contains(value) {
                        return Err(error(
                            "VIZ-MATERIALIZE-0011",
                            &chart.id,
                            "heatmap source category is absent from its explicit axis domain",
                        ));
                    }
                } else {
                    heatmap_category(value, domain, &mut bytes, budget, &chart.id)?;
                }
            }
            if !coordinates.insert((x, y)) {
                return Err(error(
                    "VIZ-MATERIALIZE-0011",
                    &chart.id,
                    "duplicate heatmap (x, y) category pair",
                ));
            }
        }
        check_heatmap_grid(x_domain.len(), y_domain.len(), &chart.id)?;
    }
    Ok(())
}

fn preflight_heatmap_mir(mir: &VizMir, budget: &mut Budget) -> VizResult<()> {
    let mut total_rows = 0;
    let mut total_caches = 0;
    let mut label_sources = 0;
    let mut label_caches = 0;
    let mut label_bytes = 0;
    for view in &mir.views {
        let MirView::Chart(chart) = view else {
            continue;
        };
        let ChartMark::Heatmap {
            instances,
            value_labels,
            ..
        } = &chart.mark
        else {
            continue;
        };
        budget.step(&chart.id)?;
        if instances.len() > MAX_HEATMAP_CELLS {
            return Err(error(
                "VIZ-MATERIALIZE-0001",
                &chart.id,
                "heatmap supplied-cache limit exceeded (16384 per chart)",
            ));
        }
        add_heatmap_call_count(&mut total_caches, instances.len(), &chart.id)?;
        if let Some(labels) = value_labels {
            vizir_core::reserve_heatmap_value_labels(&mut label_caches, labels.instances.len())?;
            for label in &labels.instances {
                vizir_core::reserve_heatmap_value_label_bytes(&mut label_bytes, &label.text)?;
                budget.charge(
                    label.key.len() as u64 + label.text.len() as u64 + 1,
                    &chart.id,
                )?;
            }
        }
        if let Some(source) = mir.data.get(&chart.source) {
            let MirDataOperator::Inline { rows } = &source.operator;
            check_heatmap_row_count(rows.len(), &chart.id)?;
            add_heatmap_call_count(&mut total_rows, rows.len(), &chart.id)?;
            if value_labels.is_some() {
                vizir_core::reserve_heatmap_value_labels(&mut label_sources, rows.len())?;
            }
        }
    }
    for view in &mir.views {
        let MirView::Chart(chart) = view else {
            continue;
        };
        let ChartMark::Heatmap {
            x, y, instances, ..
        } = &chart.mark
        else {
            continue;
        };
        for guide in &chart.guides {
            if guide.label.len() > MAX_HEATMAP_LABEL_BYTES {
                return Err(error(
                    "VIZ-MATERIALIZE-0011",
                    &chart.id,
                    "heatmap labels cannot exceed 16384 UTF-8 bytes",
                ));
            }
            budget.charge(guide.label.len() as u64 + 1, &chart.id)?;
        }
        // Inspect all supplied scales before core validation can clone them.
        let mut bytes = 0;
        for scale in &chart.scales {
            budget.step(&chart.id)?;
            match scale {
                MirScale::Band { domain, .. } => {
                    heatmap_domain(domain, &mut bytes, budget, &chart.id)?;
                }
                MirScale::QuantizeColor {
                    thresholds, range, ..
                } => {
                    if thresholds.len() > 8 || !(2..=9).contains(&range.len()) {
                        return Err(error(
                            "VIZ-MATERIALIZE-0011",
                            &chart.id,
                            "heatmap quantize scales require 2..=9 colors and at most 8 thresholds",
                        ));
                    }
                    budget.charge((thresholds.len() + range.len()) as u64, &chart.id)?;
                }
                _ => {}
            }
        }
        for cell in instances {
            if cell.x.len() > MAX_HEATMAP_LABEL_BYTES || cell.y.len() > MAX_HEATMAP_LABEL_BYTES {
                return Err(error(
                    "VIZ-MATERIALIZE-0011",
                    &chart.id,
                    "heatmap cached category exceeds 16384 UTF-8 bytes",
                ));
            }
            // Reserve string clone/comparison work even when refreshing stale caches.
            budget.charge(
                (cell.key.len() + cell.x.len() + cell.y.len()) as u64 + 1,
                &chart.id,
            )?;
        }
        let Some(source) = mir.data.get(&chart.source) else {
            continue;
        };
        let MirDataOperator::Inline { rows } = &source.operator;
        for row in rows {
            budget.step(&chart.id)?;
            if let Some(Value::String(key)) = row.get(&source.schema.key) {
                budget.charge(key.len() as u64, &chart.id)?;
            }
        }
        // Valid String programs are fields or literals. Inspect their category
        // sizes without cloning, before the core type checker clones each AST.
        for binding in [x, y] {
            let Some(typed) = mir.expressions.get(&binding.expression) else {
                continue;
            };
            match &typed.expression {
                Expression::Field { field, .. } => {
                    for row in rows {
                        budget.step(&chart.id)?;
                        if let Some(Value::String(value)) = row.get(field) {
                            budget.charge(value.len() as u64, &chart.id)?;
                            validate_heatmap_category(value)?;
                        }
                    }
                }
                Expression::Literal {
                    value: LiteralValue::String(value),
                } => {
                    budget.charge(value.len() as u64 + 1, &chart.id)?;
                    validate_heatmap_category(value)?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

pub(crate) fn preflight_hir_bindings(source: &MirDataNode, budget: &mut Budget) -> VizResult<()> {
    // HIR emits at most four field expressions, each cloning the complete type environment.
    for _ in 0..4 {
        for ty in source.schema.fields.values() {
            preflight_type(ty, budget, &source.id)?;
        }
    }
    Ok(())
}

fn preflight_mir(mir: &VizMir, budget: &mut Budget) -> VizResult<()> {
    preflight_heatmap_mir(mir, budget)?;
    let has_heatmap = mir.views.iter().any(|view| {
        matches!(view, MirView::Chart(chart) if matches!(chart.mark, ChartMark::Heatmap { .. }))
    });
    let mut count = 0usize;
    let mut expression_costs = BTreeMap::new();
    let mut schema_costs = BTreeMap::new();
    // Bound even unreferenced entries: the refresh API clones the complete MIR.
    for (id, typed) in &mir.expressions {
        let before = budget.remaining;
        preflight_type(&typed.result_type, budget, id)?;
        let mut pending = vec![(&typed.expression, 1usize)];
        while let Some((expr, depth)) = pending.pop() {
            budget.depth(depth, id)?;
            budget.step(id)?;
            count = count.checked_add(1).ok_or_else(|| {
                error("VIZ-MATERIALIZE-0001", id, "expression node count overflow")
            })?;
            if count > budget.limits.max_expression_nodes {
                return Err(error(
                    "VIZ-MATERIALIZE-0001",
                    id,
                    "whole-call expression node limit exceeded",
                ));
            }
            let next = depth + 1;
            if has_heatmap
                && let Expression::Literal {
                    value: LiteralValue::String(value),
                } = expr
            {
                budget.charge(value.len() as u64, id)?;
            }
            match expr {
                Expression::Literal { .. }
                | Expression::Field { .. }
                | Expression::Parameter { .. }
                | Expression::Signal { .. } => {}
                Expression::Array { items }
                | Expression::And { args: items }
                | Expression::Or { args: items }
                | Expression::Call { args: items, .. } => {
                    if items.len()
                        > budget
                            .limits
                            .max_expression_nodes
                            .saturating_sub(count + pending.len())
                    {
                        return Err(error(
                            "VIZ-MATERIALIZE-0001",
                            id,
                            "whole-call expression node limit exceeded",
                        ));
                    }
                    for item in items {
                        pending.push((item, next));
                    }
                }
                Expression::Record { fields } => {
                    if fields.len()
                        > budget
                            .limits
                            .max_expression_nodes
                            .saturating_sub(count + pending.len())
                    {
                        return Err(error(
                            "VIZ-MATERIALIZE-0001",
                            id,
                            "whole-call expression node limit exceeded",
                        ));
                    }
                    for item in fields.values() {
                        pending.push((item, next));
                    }
                }
                Expression::Add { left, right }
                | Expression::Subtract { left, right }
                | Expression::Multiply { left, right }
                | Expression::Divide { left, right }
                | Expression::Equal { left, right }
                | Expression::LessThan { left, right } => {
                    pending.push((right, next));
                    pending.push((left, next));
                }
                Expression::Not { arg } | Expression::IsNull { arg } => pending.push((arg, next)),
                Expression::If {
                    condition,
                    then_value,
                    else_value,
                } => {
                    pending.push((else_value, next));
                    pending.push((then_value, next));
                    pending.push((condition, next));
                }
                Expression::Convert { value, to } => {
                    preflight_type(to, budget, id)?;
                    pending.push((value, next));
                }
            }
        }
        expression_costs.insert(id.as_str(), before - budget.remaining);
    }
    for (id, data) in &mir.data {
        let before = budget.remaining;
        for ty in data.schema.fields.values() {
            preflight_type(ty, budget, id)?;
        }
        schema_costs.insert(id.as_str(), before - budget.remaining);
        let MirDataOperator::Inline { rows } = &data.operator;
        for row in rows {
            budget.step(id)?;
            for value in row.values() {
                preflight_value(value, budget, id)?;
            }
        }
    }
    for view in &mir.views {
        if let MirView::Chart(chart) = view {
            // Charge repeated schema clones and expression typing before validate_mir starts.
            budget.charge(
                schema_costs
                    .get(chart.source.as_str())
                    .copied()
                    .unwrap_or(0),
                &chart.id,
            )?;
            let mut ids = vec![chart.key_expression.as_str()];
            ids.extend(
                chart
                    .mark
                    .bindings()
                    .iter()
                    .map(|binding| binding.expression.as_str()),
            );
            if let ChartMark::Line {
                group_expression,
                order_expression,
                ..
            }
            | ChartMark::Area {
                group_expression,
                order_expression,
                ..
            } = &chart.mark
            {
                ids.extend(group_expression.as_deref());
                ids.push(order_expression);
            }
            for id in ids {
                budget.charge(expression_costs.get(id).copied().unwrap_or(0), &chart.id)?;
                if let Some(expression) = mir.expressions.get(id) {
                    ensure_executable(&expression.expression, id)?;
                }
            }
        }
        if let MirView::Geometry(geometry) = view {
            let mut pending = geometry
                .children
                .iter()
                .map(|n| (n, 1usize))
                .collect::<Vec<_>>();
            while let Some((node, depth)) = pending.pop() {
                budget.depth(depth, &geometry.id)?;
                budget.step(&geometry.id)?;
                if let MirGeometryNode::Group { children, .. } = node {
                    for child in children {
                        pending.push((child, depth + 1));
                    }
                }
            }
        }
    }
    Ok(())
}

// Full referenced AST support is checked even when a source has zero rows.
// The preceding iterative preflight has already bounded this traversal.
fn ensure_executable(expression: &Expression, id: &str) -> VizResult<()> {
    let mut pending = vec![expression];
    while let Some(expression) = pending.pop() {
        match expression {
            Expression::Field { .. } => {}
            Expression::Literal {
                value: LiteralValue::Int64(_) | LiteralValue::Bool(_) | LiteralValue::String(_),
            } => {}
            Expression::Literal {
                value: LiteralValue::Float64(v),
            } if v.is_finite() => {}
            Expression::Add { left, right }
            | Expression::Subtract { left, right }
            | Expression::Multiply { left, right } => {
                pending.push(right);
                pending.push(left);
            }
            _ => {
                return Err(error(
                    "VIZ-MATERIALIZE-0006",
                    id,
                    "unsupported static materialization expression; only Field, finite scalar Literal, Add, Subtract, Multiply execute",
                ));
            }
        }
    }
    Ok(())
}

fn preflight_type(value: &ValueType, budget: &mut Budget, context: &str) -> VizResult<()> {
    let mut pending = vec![(value, 1usize)];
    while let Some((value, depth)) = pending.pop() {
        budget.depth(depth, context)?;
        budget.step(context)?;
        match value {
            ValueType::Array { items } | ValueType::Option { item: items } => {
                pending.push((items, depth + 1))
            }
            ValueType::Record { fields } => {
                for item in fields.values() {
                    pending.push((item, depth + 1));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn preflight_value(value: &Value, budget: &mut Budget, context: &str) -> VizResult<()> {
    let mut pending = vec![(value, 1usize)];
    while let Some((value, depth)) = pending.pop() {
        budget.depth(depth, context)?;
        budget.step(context)?;
        match value {
            Value::Array(items) => {
                for item in items {
                    pending.push((item, depth + 1));
                }
            }
            Value::Object(fields) => {
                for item in fields.values() {
                    pending.push((item, depth + 1));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn check_value(
    value: Option<&Value>,
    ty: &ValueType,
    budget: &mut Budget,
    path: &str,
) -> VizResult<()> {
    budget.step(path)?;
    if let ValueType::Option { item } = ty {
        return match value {
            None | Some(Value::Null) => Ok(()),
            Some(value) => check_value(Some(value), item, budget, path),
        };
    }
    let valid = match (value, ty) {
        (Some(Value::Null), ValueType::Null) => true,
        (Some(Value::Bool(_)), ValueType::Bool) | (Some(Value::String(_)), ValueType::String) => {
            true
        }
        (Some(Value::Number(n)), ValueType::Int64) => n.as_i64().is_some(),
        (Some(Value::Number(n)), ValueType::Float64) => n.as_f64().is_some_and(f64::is_finite),
        (Some(Value::String(s)), ValueType::Color) => {
            s == "transparent"
                || (matches!(s.len(), 7 | 9)
                    && s.starts_with('#')
                    && s[1..].bytes().all(|b| b.is_ascii_hexdigit()))
        }
        (Some(Value::Array(values)), ValueType::Array { items }) => {
            for (i, value) in values.iter().enumerate() {
                check_value(Some(value), items, budget, &format!("{path}[{i}]"))?;
            }
            true
        }
        (Some(Value::Object(values)), ValueType::Record { fields }) => {
            check_row(
                values.iter().map(|(k, v)| (k.as_str(), v)).collect(),
                fields,
                budget,
                path,
            )?;
            true
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(error(
            "VIZ-MATERIALIZE-0003",
            path,
            format!("value does not conform to declared type {ty:?}"),
        ))
    }
}

fn check_row(
    values: BTreeMap<&str, &Value>,
    fields: &BTreeMap<String, ValueType>,
    budget: &mut Budget,
    path: &str,
) -> VizResult<()> {
    for key in values.keys() {
        budget.step(path)?;
        if !fields.contains_key(*key) {
            return Err(error(
                "VIZ-MATERIALIZE-0003",
                path,
                format!("undeclared field {key:?}"),
            ));
        }
    }
    for (field, ty) in fields {
        check_value(
            values.get(field.as_str()).copied(),
            ty,
            budget,
            &format!("{path}.{field}"),
        )?;
    }
    Ok(())
}

#[derive(Clone)]
enum Scalar {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
}

struct LineGroup {
    color: Option<String>,
    points: Vec<(Scalar, MirPointItem)>,
}

fn compare_order(a: &Scalar, b: &Scalar) -> std::cmp::Ordering {
    match (a, b) {
        (Scalar::Int(a), Scalar::Int(b)) => a.cmp(b),
        (Scalar::Float(a), Scalar::Float(b)) => a.total_cmp(b),
        // Typed scalar expressions have the same numeric type on every row.
        _ => unreachable!("line order was checked and evaluated against a single schema"),
    }
}

struct Evaluated {
    value: Scalar,
    // Preserve legacy category spelling for mixed numeric inline columns.
    category: Option<String>,
}

impl Evaluated {
    fn exact_string(self, path: &str) -> VizResult<String> {
        match self.value {
            Scalar::String(value) => Ok(value),
            _ => Err(error(
                "VIZ-MATERIALIZE-0011",
                path,
                "heatmap x/y channels require exact non-null String values",
            )),
        }
    }

    fn number(&self, path: &str) -> VizResult<f64> {
        match self.value {
            Scalar::Int(v) => Ok(v as f64),
            Scalar::Float(v) if v.is_finite() => Ok(v),
            _ => Err(error(
                "VIZ-MATERIALIZE-0004",
                path,
                "numeric channel requires a finite number",
            )),
        }
    }

    fn category(&self) -> String {
        self.category.clone().unwrap_or_else(|| match &self.value {
            Scalar::Int(v) => v.to_string(),
            Scalar::Float(v) => serde_json::Number::from_f64(*v)
                .expect("finite scalar")
                .to_string(),
            Scalar::Bool(v) => v.to_string(),
            Scalar::String(v) => v.clone(),
        })
    }
}

struct Evaluator<'a, 'b> {
    row: &'a BTreeMap<String, Value>,
    schema: &'a BTreeMap<String, ValueType>,
    row_variable: &'a str,
    budget: &'b mut Budget,
    heatmap: bool,
}

impl Evaluator<'_, '_> {
    fn evaluate(&mut self, expr: &Expression, path: &str) -> VizResult<Evaluated> {
        self.budget.step(path)?;
        let value = match expr {
            Expression::Field { row, field } => {
                if row != self.row_variable {
                    return Err(error(
                        "VIZ-MATERIALIZE-0004",
                        path,
                        "field references a different row scope",
                    ));
                }
                let raw = self.row.get(field).ok_or_else(|| {
                    error(
                        "VIZ-MATERIALIZE-0004",
                        path,
                        format!("missing field {field:?}"),
                    )
                })?;
                if self.heatmap
                    && let Value::String(value) = raw
                {
                    self.budget.charge(value.len() as u64, path)?;
                }
                let scalar = match (self.schema.get(field), raw) {
                    (Some(ValueType::Int64), Value::Number(n)) => Scalar::Int(
                        n.as_i64()
                            .ok_or_else(|| error("VIZ-MATERIALIZE-0003", path, "expected Int64"))?,
                    ),
                    (Some(ValueType::Float64), Value::Number(n)) => {
                        Scalar::Float(n.as_f64().filter(|n| n.is_finite()).ok_or_else(|| {
                            error("VIZ-MATERIALIZE-0003", path, "expected finite Float64")
                        })?)
                    }
                    (Some(ValueType::Bool), Value::Bool(v)) => Scalar::Bool(*v),
                    (Some(ValueType::String), Value::String(v)) => Scalar::String(v.clone()),
                    _ => {
                        return Err(error(
                            "VIZ-MATERIALIZE-0004",
                            path,
                            "field is not a supported non-null scalar",
                        ));
                    }
                };
                return Ok(Evaluated {
                    value: scalar,
                    category: if self.heatmap {
                        None
                    } else {
                        value_as_key(raw)
                    },
                });
            }
            Expression::Literal { value } => match value {
                LiteralValue::Int64(v) => Scalar::Int(*v),
                LiteralValue::Float64(v) if v.is_finite() => Scalar::Float(*v),
                LiteralValue::Bool(v) => Scalar::Bool(*v),
                LiteralValue::String(v) => {
                    if self.heatmap {
                        self.budget.charge(v.len() as u64, path)?;
                    }
                    Scalar::String(v.clone())
                }
                _ => {
                    return Err(error(
                        "VIZ-MATERIALIZE-0004",
                        path,
                        "literal is not a supported finite non-null scalar",
                    ));
                }
            },
            Expression::Add { left, right }
            | Expression::Subtract { left, right }
            | Expression::Multiply { left, right } => {
                let left = self.evaluate(left, &format!("{path}.left"))?;
                let right = self.evaluate(right, &format!("{path}.right"))?;
                match (&left.value, &right.value) {
                    (Scalar::Int(a), Scalar::Int(b)) => {
                        let result = match expr {
                            Expression::Add { .. } => a.checked_add(*b),
                            Expression::Subtract { .. } => a.checked_sub(*b),
                            _ => a.checked_mul(*b),
                        }
                        .ok_or_else(|| {
                            error("VIZ-MATERIALIZE-0005", path, "Int64 arithmetic overflow")
                        })?;
                        Scalar::Int(result)
                    }
                    _ => {
                        let (a, b) = (left.number(path)?, right.number(path)?);
                        let result = match expr {
                            Expression::Add { .. } => a + b,
                            Expression::Subtract { .. } => a - b,
                            _ => a * b,
                        };
                        if !result.is_finite() {
                            return Err(error(
                                "VIZ-MATERIALIZE-0005",
                                path,
                                "Float64 arithmetic produced a non-finite result",
                            ));
                        }
                        Scalar::Float(result)
                    }
                }
            }
            _ => {
                return Err(error(
                    "VIZ-MATERIALIZE-0006",
                    path,
                    "unsupported static materialization expression; only Field, scalar Literal, Add, Subtract, Multiply execute",
                ));
            }
        };
        Ok(Evaluated {
            value,
            category: None,
        })
    }
}

fn check_role(
    expressions: &BTreeMap<String, TypedExpression>,
    id: &str,
    numeric: bool,
    context: &str,
    budget: &mut Budget,
) -> VizResult<()> {
    budget.step(context)?;
    let ty = &expressions
        .get(id)
        .ok_or_else(|| {
            error(
                "VIZ-MATERIALIZE-0004",
                context,
                format!("unknown expression {id:?}"),
            )
        })?
        .result_type;
    if matches!(ty, ValueType::Int64 | ValueType::Float64)
        || (!numeric && matches!(ty, ValueType::String | ValueType::Bool))
    {
        return Ok(());
    }
    Err(error(
        "VIZ-MATERIALIZE-0004",
        context,
        format!(
            "expression {id:?} requires {}, received {ty:?}",
            if numeric {
                "numeric type"
            } else {
                "non-null scalar type"
            }
        ),
    ))
}

fn check_string_role(
    expressions: &BTreeMap<String, TypedExpression>,
    id: &str,
    context: &str,
    budget: &mut Budget,
) -> VizResult<()> {
    budget.step(context)?;
    if expressions
        .get(id)
        .is_some_and(|expression| expression.result_type == ValueType::String)
    {
        Ok(())
    } else {
        Err(error(
            "VIZ-MATERIALIZE-0011",
            context,
            format!("heatmap category expression {id:?} requires non-null String type"),
        ))
    }
}

/// Validate fresh cells against authored scale plans, without filling sparse gaps.
/// Called both after HIR lowering and during MIR replay/refresh.
pub(crate) fn check_heatmap_membership(
    chart: &MirChart,
    mark: &ChartMark,
    budget: &mut Budget,
) -> VizResult<()> {
    let ChartMark::Heatmap {
        x,
        y,
        color,
        instances,
        ..
    } = mark
    else {
        return Ok(());
    };
    check_heatmap_row_count(instances.len(), &chart.id)?;
    let mut bytes = 0;
    let mut domains = Vec::with_capacity(2);
    for binding in [x, y] {
        let scale = chart
            .scales
            .iter()
            .find(|scale| scale.id() == binding.scale);
        let Some(MirScale::Band {
            domain,
            range,
            padding,
            ..
        }) = scale
        else {
            return Err(error(
                "VIZ-MATERIALIZE-0011",
                &chart.id,
                "heatmap x/y bindings require band scales",
            ));
        };
        if *padding != 0.0 {
            return Err(error(
                "VIZ-MATERIALIZE-0011",
                &chart.id,
                "heatmap band scales require zero padding",
            ));
        }
        let values = heatmap_domain(domain, &mut bytes, budget, &chart.id)?;
        budget.charge(domain.len() as u64 + 1, &chart.id)?;
        crate::heatmap::band_boundaries(*range, domain.len())?;
        domains.push(values);
    }
    check_heatmap_grid(domains[0].len(), domains[1].len(), &chart.id)?;
    let Some(MirScale::QuantizeColor {
        domain,
        thresholds,
        range,
        ..
    }) = chart.scales.iter().find(|scale| scale.id() == color.scale)
    else {
        return Err(error(
            "VIZ-MATERIALIZE-0011",
            &chart.id,
            "heatmap color binding requires a quantize-color scale",
        ));
    };
    budget.charge(range.len() as u64 + 1, &chart.id)?;
    let canonical = canonical_quantize_thresholds(*domain, range.len())?;
    let labels = (domains[0].len() + domains[1].len() + 2 * range.len() + 4) as u64;
    let layout_work = labels.checked_mul(labels).ok_or_else(|| {
        error(
            "VIZ-MATERIALIZE-0001",
            &chart.id,
            "heatmap label-layout work overflow",
        )
    })?;
    budget.charge(layout_work, &chart.id)?;
    if thresholds.len() != canonical.len() {
        return Err(error(
            "VIZ-MATERIALIZE-0011",
            &chart.id,
            "heatmap quantize thresholds do not match the color domain and palette",
        ));
    }
    for cell in instances {
        budget.charge(
            (cell.x.len() + cell.y.len() + range.len()) as u64 + 1,
            &chart.id,
        )?;
        if !domains[0].contains(cell.x.as_str()) || !domains[1].contains(cell.y.as_str()) {
            return Err(error(
                "VIZ-MATERIALIZE-0011",
                &chart.id,
                "materialized heatmap category is absent from explicit axis domain; update the scale deliberately",
            ));
        }
        quantize_color_index(cell.value, *domain, thresholds, range.len())?;
    }
    Ok(())
}

pub(crate) fn check_scale_membership(
    chart: &MirChart,
    mark: &ChartMark,
    budget: &mut Budget,
) -> VizResult<()> {
    check_numeric_membership(chart, mark, budget)?;
    if matches!(mark, ChartMark::Heatmap { .. }) {
        return check_heatmap_membership(chart, mark, budget);
    }
    let mut domains = BTreeMap::new();
    for scale in &chart.scales {
        let (id, domain) = match scale {
            MirScale::OrdinalColor { id, domain, range } => {
                if domain.len() != range.len() {
                    return Err(error(
                        "VIZ-MATERIALIZE-0009",
                        &chart.id,
                        format!("ordinal scale {id:?} requires one color per domain value"),
                    ));
                }
                (id, domain)
            }
            MirScale::Band { id, domain, .. } => (id, domain),
            _ => continue,
        };
        let mut values = BTreeSet::new();
        for value in domain {
            budget.step(&chart.id)?;
            if !values.insert(value.as_str()) {
                return Err(error(
                    "VIZ-MATERIALIZE-0009",
                    &chart.id,
                    format!("scale {id:?} has duplicate domain value {value:?}"),
                ));
            }
        }
        domains.insert(id.as_str(), values);
    }
    let color = match mark {
        ChartMark::Symbol { color, .. }
        | ChartMark::Line { color, .. }
        | ChartMark::Area { color, .. }
        | ChartMark::Bar { color, .. } => color,
        ChartMark::Heatmap { .. } => unreachable!("heatmap handled above"),
    };
    if let Some(color) = color
        && !chart
            .scales
            .iter()
            .any(|s| matches!(s, MirScale::OrdinalColor { id, .. } if id == &color.scale))
    {
        return Err(error(
            "VIZ-MATERIALIZE-0009",
            &chart.id,
            "color bindings require an ordinal-color scale",
        ));
    }
    let mut check = |scale: &str, category: &str| -> VizResult<()> {
        budget.step(&chart.id)?;
        if !domains
            .get(scale)
            .is_some_and(|domain| domain.contains(category))
        {
            return Err(error(
                "VIZ-MATERIALIZE-0009",
                &chart.id,
                format!(
                    "materialized category {category:?} is absent from explicit scale {scale:?}; update the scale deliberately"
                ),
            ));
        }
        Ok(())
    };
    match mark {
        ChartMark::Symbol { instances, .. } => {
            if let Some(color) = color {
                for item in instances {
                    check(
                        &color.scale,
                        item.color_category.as_deref().expect("color evaluated"),
                    )?;
                }
            }
        }
        ChartMark::Line { series, .. } | ChartMark::Area { series, .. } => {
            if let Some(color) = color {
                for item in series {
                    check(
                        &color.scale,
                        item.color_category.as_deref().expect("color evaluated"),
                    )?;
                }
            }
        }
        ChartMark::Bar {
            category,
            instances,
            ..
        } => {
            if !chart
                .scales
                .iter()
                .any(|s| matches!(s, MirScale::Band { id, .. } if id == &category.scale))
            {
                return Err(error(
                    "VIZ-MATERIALIZE-0009",
                    &chart.id,
                    "bar category bindings require a band scale",
                ));
            }
            for item in instances {
                check(&category.scale, &item.category)?;
                if let Some(color) = color {
                    check(
                        &color.scale,
                        item.color_category.as_deref().expect("color evaluated"),
                    )?;
                }
            }
        }
        ChartMark::Heatmap { .. } => unreachable!("heatmap handled above"),
    }
    check_area_projection(chart, mark, budget)
}

/// Check fresh materialized coordinates before replay or transactional refresh.
fn check_numeric_membership(
    chart: &MirChart,
    mark: &ChartMark,
    budget: &mut Budget,
) -> VizResult<()> {
    let domains: BTreeMap<_, _> = chart
        .scales
        .iter()
        .filter_map(|scale| {
            if let MirScale::Linear {
                id,
                domain,
                out_of_domain: Some(_),
                ..
            } = scale
            {
                Some((id.as_str(), *domain))
            } else {
                None
            }
        })
        .collect();
    if domains.is_empty() {
        return Ok(());
    }
    let mut check = |binding: &vizir_core::ScaleBinding, value: f64| -> VizResult<()> {
        if let Some(domain) = domains.get(binding.scale.as_str()) {
            budget.step(&chart.id)?;
            if !value.is_finite() || !(domain[0]..=domain[1]).contains(&value) {
                return Err(error(
                    "VIZ-DOMAIN-0002",
                    &chart.id,
                    format!(
                        "numeric value {value} is outside explicit scale {:?}; outliers are rejected",
                        binding.scale
                    ),
                ));
            }
        }
        Ok(())
    };
    match mark {
        ChartMark::Symbol {
            x, y, instances, ..
        } => {
            for point in instances {
                check(x, point.x)?;
                check(y, point.y)?;
            }
        }
        ChartMark::Line { x, y, series, .. } | ChartMark::Area { x, y, series, .. } => {
            if let ChartMark::Area { baseline, .. } = mark {
                check(y, *baseline)?;
            }
            for series in series {
                for point in &series.points {
                    check(x, point.x)?;
                    check(y, point.y)?;
                }
            }
        }
        ChartMark::Bar {
            value, instances, ..
        } => {
            check(value, 0.0)?;
            for item in instances {
                check(value, item.value)?;
            }
        }
        ChartMark::Heatmap { .. } => {}
    }
    Ok(())
}

/// Area's strict-x contract also applies after the explicit scale projection.
/// Use fresh materialized values, both during HIR lowering and MIR refresh/build.
pub(crate) fn check_area_projection(
    chart: &MirChart,
    mark: &ChartMark,
    budget: &mut Budget,
) -> VizResult<()> {
    let ChartMark::Area {
        x,
        y,
        baseline,
        series,
        ..
    } = mark
    else {
        return Ok(());
    };
    let linear = |binding: &vizir_core::ScaleBinding| -> VizResult<([f64; 2], [f64; 2])> {
        chart
            .scales
            .iter()
            .find_map(|scale| match scale {
                MirScale::Linear {
                    id, domain, range, ..
                } if id == &binding.scale => Some((*domain, *range)),
                _ => None,
            })
            .ok_or_else(|| {
                error(
                    "VIZ-MATERIALIZE-0010",
                    &chart.id,
                    "area x/y bindings require linear scales",
                )
            })
    };
    let xs = linear(x)?;
    let ys = linear(y)?;
    for (domain, range) in [xs, ys] {
        if !domain.into_iter().chain(range).all(f64::is_finite)
            || !(domain[1] - domain[0]).is_finite()
            || !(range[1] - range[0]).is_finite()
        {
            return Err(error(
                "VIZ-MATERIALIZE-0010",
                &chart.id,
                "area linear scale domains and ranges require finite endpoints and spans; rescale the inputs",
            ));
        }
    }
    let baseline_y = vizir_core::map_linear(*baseline, ys.0, ys.1);
    for series in series {
        let mut previous = None;
        for point in &series.points {
            budget.step(&chart.id)?;
            let px = vizir_core::map_linear(point.x, xs.0, xs.1);
            let py = vizir_core::map_linear(point.y, ys.0, ys.1);
            if !baseline_y.is_finite()
                || !px.is_finite()
                || !py.is_finite()
                || previous.is_some_and(|last| last >= px)
            {
                return Err(error(
                    "VIZ-MATERIALIZE-0010",
                    &chart.id,
                    format!(
                        "area series {:?} key {:?} requires finite geometry and strictly increasing projected x positions; rescale inputs or enlarge the frame",
                        series.key, point.key
                    ),
                ));
            }
            previous = Some(px);
        }
    }
    Ok(())
}

pub(crate) fn materialize_mark(
    source: &MirDataNode,
    row_variable: &str,
    key_expression: &str,
    mark: &ChartMark,
    expressions: &BTreeMap<String, TypedExpression>,
    chart_id: &str,
    budget: &mut Budget,
) -> VizResult<ChartMark> {
    if source.update_mode != UpdateMode::Replace || !source.deterministic {
        return Err(error(
            "VIZ-MATERIALIZE-0006",
            chart_id,
            "static execution requires deterministic replace-mode inline data",
        ));
    }
    let key_expr = expressions
        .get(key_expression)
        .ok_or_else(|| error("VIZ-MATERIALIZE-0004", chart_id, "unknown key expression"))?;
    if !matches!(&key_expr.expression, Expression::Field { row, field } if row == row_variable && field == &source.schema.key)
    {
        return Err(error(
            "VIZ-MATERIALIZE-0007",
            chart_id,
            "key_expression must directly reference the source schema's stable key field",
        ));
    }
    check_role(expressions, key_expression, false, chart_id, budget)?;
    match mark {
        ChartMark::Heatmap { x, y, color, .. } => {
            check_string_role(expressions, &x.expression, chart_id, budget)?;
            check_string_role(expressions, &y.expression, chart_id, budget)?;
            check_role(expressions, &color.expression, true, chart_id, budget)?;
        }
        ChartMark::Symbol { x, y, color, .. }
        | ChartMark::Line { x, y, color, .. }
        | ChartMark::Area { x, y, color, .. } => {
            check_role(expressions, &x.expression, true, chart_id, budget)?;
            check_role(expressions, &y.expression, true, chart_id, budget)?;
            if let Some(color) = color {
                check_role(expressions, &color.expression, false, chart_id, budget)?;
            }
        }
        ChartMark::Bar {
            category,
            value,
            color,
            ..
        } => {
            check_role(expressions, &category.expression, false, chart_id, budget)?;
            check_role(expressions, &value.expression, true, chart_id, budget)?;
            if let Some(color) = color {
                check_role(expressions, &color.expression, false, chart_id, budget)?;
            }
        }
    }
    if let ChartMark::Line {
        group_expression,
        order_expression,
        ..
    }
    | ChartMark::Area {
        group_expression,
        order_expression,
        ..
    } = mark
    {
        check_role(expressions, order_expression, true, chart_id, budget)?;
        if let Some(group) = group_expression {
            check_role(expressions, group, false, chart_id, budget)?;
        }
    }
    if let ChartMark::Area {
        x,
        order_expression,
        baseline,
        ..
    } = mark
        && (order_expression != &x.expression || !baseline.is_finite())
    {
        return Err(error(
            "VIZ-MATERIALIZE-0010",
            chart_id,
            "area requires a finite explicit baseline and ascending order by its x expression",
        ));
    }
    let MirDataOperator::Inline { rows } = &source.operator;
    if matches!(mark, ChartMark::Heatmap { .. }) {
        check_heatmap_row_count(rows.len(), chart_id)?;
    }
    let mut cell_labels = Vec::new();
    if matches!(
        mark,
        ChartMark::Heatmap {
            value_labels: Some(_),
            ..
        }
    ) {
        vizir_core::reserve_heatmap_value_labels(&mut budget.value_label_count, rows.len())?;
    }
    let mut cells = Vec::new();
    let mut cell_coordinates = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut points = Vec::new();
    let mut bars = Vec::new();
    let mut groups: BTreeMap<String, LineGroup> = BTreeMap::new();
    let mut categories = BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        budget.step(chart_id)?;
        let key = row
            .get(&source.schema.key)
            .and_then(value_as_key)
            .ok_or_else(|| {
                error(
                    "VIZ-MATERIALIZE-0007",
                    chart_id,
                    format!("{} row {index}: missing or invalid stable key", source.id),
                )
            })?;
        let context = format!(
            "chart {chart_id:?}, data {:?}, row {index} key {key:?}",
            source.id
        );
        if !keys.insert(key.clone()) {
            return Err(error(
                "VIZ-MATERIALIZE-0007",
                &context,
                "duplicate stable data key",
            ));
        }
        check_row(
            row.iter().map(|(k, v)| (k.as_str(), v)).collect(),
            &source.schema.fields,
            budget,
            &context,
        )?;
        let mut evaluator = Evaluator {
            row,
            schema: &source.schema.fields,
            row_variable,
            budget,
            heatmap: matches!(mark, ChartMark::Heatmap { .. }),
        };
        let mut eval = |id: &str| -> VizResult<Evaluated> {
            let expression = expressions.get(id).ok_or_else(|| {
                error(
                    "VIZ-MATERIALIZE-0004",
                    &context,
                    format!("unknown expression {id:?}"),
                )
            })?;
            evaluator.evaluate(
                &expression.expression,
                &format!("{context}, expression {id:?}"),
            )
        };
        match mark {
            ChartMark::Heatmap {
                x,
                y,
                color,
                value_labels,
                ..
            } => {
                let x = eval(&x.expression)?.exact_string(&context)?;
                let y = eval(&y.expression)?.exact_string(&context)?;
                let typed = eval(&color.expression)?;
                // Format the authoritative typed scalar before paint loses Int64 precision.
                if let Some(options) = value_labels {
                    let text = match typed.value {
                        Scalar::Int(value) => crate::tick_format::format_integer(
                            value,
                            options.number_format.as_ref(),
                        ),
                        Scalar::Float(value) if value.is_finite() => {
                            crate::tick_format::format_exact_float(
                                value,
                                options.number_format.as_ref(),
                            )
                        }
                        _ => {
                            return Err(error(
                                "VIZ-MATERIALIZE-0011",
                                &context,
                                "heatmap value labels require finite numeric values",
                            ));
                        }
                    };
                    vizir_core::reserve_heatmap_value_label_bytes(
                        &mut budget.value_label_bytes,
                        &text,
                    )?;
                    budget.charge(key.len() as u64 + text.len() as u64 + 1, &context)?;
                    cell_labels.push(vizir_core::MirHeatmapValueLabel {
                        key: key.clone(),
                        text,
                    });
                }
                let value = typed.number(&context)?;
                budget.charge((x.len() + y.len() + key.len()) as u64 + 1, &context)?;
                validate_heatmap_category(&x)?;
                validate_heatmap_category(&y)?;
                if !cell_coordinates.insert((x.clone(), y.clone())) {
                    return Err(error(
                        "VIZ-MATERIALIZE-0011",
                        &context,
                        "duplicate heatmap (x, y) category pair",
                    ));
                }
                cells.push(MirHeatmapCell { key, x, y, value });
            }
            ChartMark::Symbol { x, y, color, .. } => {
                points.push(MirPointItem {
                    key,
                    x: eval(&x.expression)?.number(&context)?,
                    y: eval(&y.expression)?.number(&context)?,
                    color_category: color
                        .as_ref()
                        .map(|c| eval(&c.expression).map(|v| v.category()))
                        .transpose()?,
                });
            }
            ChartMark::Line {
                x,
                y,
                color,
                group_expression,
                order_expression,
                ..
            }
            | ChartMark::Area {
                x,
                y,
                color,
                group_expression,
                order_expression,
                ..
            } => {
                let group = group_expression
                    .as_ref()
                    .map(|id| eval(id).map(|v| v.category()))
                    .transpose()?
                    .unwrap_or_else(|| "series".into());
                let color = color
                    .as_ref()
                    .map(|c| eval(&c.expression).map(|v| v.category()))
                    .transpose()?;
                let order = eval(order_expression)?;
                order.number(&context)?;
                let point = MirPointItem {
                    key,
                    x: eval(&x.expression)?.number(&context)?,
                    y: eval(&y.expression)?.number(&context)?,
                    color_category: color.clone(),
                };
                let entry = groups.entry(group).or_insert_with(|| LineGroup {
                    color: color.clone(),
                    points: Vec::new(),
                });
                if entry.color != color {
                    return Err(error(
                        "VIZ-MATERIALIZE-0008",
                        &context,
                        if matches!(mark, ChartMark::Area { .. }) {
                            "one area group must have a uniform color category"
                        } else {
                            "one line group must have a uniform color category"
                        },
                    ));
                }
                entry.points.push((order.value, point));
            }
            ChartMark::Bar {
                category,
                value,
                color,
                ..
            } => {
                let category = eval(&category.expression)?.category();
                if !categories.insert(category.clone()) {
                    return Err(error(
                        "VIZ-MATERIALIZE-0008",
                        &context,
                        format!("bar category {category:?} is duplicated"),
                    ));
                }
                bars.push(MirBarItem {
                    key,
                    category,
                    value: eval(&value.expression)?.number(&context)?,
                    color_category: color
                        .as_ref()
                        .map(|c| eval(&c.expression).map(|v| v.category()))
                        .transpose()?,
                });
            }
        }
    }
    // Clone only plan fields, never the supplied caches.
    Ok(match mark {
        ChartMark::Heatmap {
            id,
            x,
            y,
            color,
            value_labels,
            ..
        } => ChartMark::Heatmap {
            id: id.clone(),
            x: x.clone(),
            y: y.clone(),
            color: color.clone(),
            instances: cells,
            value_labels: value_labels
                .as_ref()
                .map(|options| vizir_core::MirHeatmapValueLabels {
                    number_format: options.number_format,
                    color: options.color.clone(),
                    instances: cell_labels,
                }),
        },
        ChartMark::Symbol {
            id,
            x,
            y,
            color,
            size,
            ..
        } => ChartMark::Symbol {
            id: id.clone(),
            x: x.clone(),
            y: y.clone(),
            color: color.clone(),
            size: *size,
            instances: points,
        },
        ChartMark::Bar {
            id,
            category,
            value,
            color,
            ..
        } => ChartMark::Bar {
            id: id.clone(),
            category: category.clone(),
            value: value.clone(),
            color: color.clone(),
            instances: bars,
        },
        ChartMark::Area {
            id,
            x,
            y,
            color,
            group_expression,
            order_expression,
            baseline,
            ..
        } => {
            if groups.is_empty() {
                return Err(error(
                    "VIZ-MATERIALIZE-0010",
                    chart_id,
                    "area requires at least one series with two distinct x values",
                ));
            }
            let mut low = *baseline;
            let mut high = *baseline;
            let mut series = Vec::with_capacity(groups.len());
            for (
                key,
                LineGroup {
                    color: color_category,
                    mut points,
                },
            ) in groups
            {
                let count = points.len() as u64;
                // Reserve sorting and adjacency-check work before doing either.
                budget.charge(
                    count.saturating_mul(u64::from(count.max(1).ilog2()) + 2),
                    chart_id,
                )?;
                if points.len() < 2 {
                    return Err(error(
                        "VIZ-MATERIALIZE-0010",
                        chart_id,
                        format!("area series {key:?} requires at least two points"),
                    ));
                }
                points
                    .sort_by(|a, b| compare_order(&a.0, &b.0).then_with(|| a.1.key.cmp(&b.1.key)));
                for pair in points.windows(2) {
                    if pair[0].1.x >= pair[1].1.x {
                        return Err(error(
                            "VIZ-MATERIALIZE-0010",
                            chart_id,
                            format!(
                                "area series {key:?} keys {:?} and {:?} require strictly increasing representable x values; duplicates and coordinate precision collisions are unsupported",
                                pair[0].1.key, pair[1].1.key
                            ),
                        ));
                    }
                }
                for (_, point) in &points {
                    low = low.min(point.y);
                    high = high.max(point.y);
                }
                series.push(MirSeries {
                    key,
                    color_category,
                    points: points.into_iter().map(|(_, point)| point).collect(),
                });
            }
            if !(high - low).is_finite() {
                return Err(error(
                    "VIZ-MATERIALIZE-0010",
                    chart_id,
                    "area y values and baseline have a nonfinite combined span; rescale the inputs",
                ));
            }
            ChartMark::Area {
                id: id.clone(),
                x: x.clone(),
                y: y.clone(),
                color: color.clone(),
                group_expression: group_expression.clone(),
                order_expression: order_expression.clone(),
                baseline: *baseline,
                series,
            }
        }
        ChartMark::Line {
            id,
            x,
            y,
            color,
            group_expression,
            order_expression,
            line_width,
            show_points,
            ..
        } => {
            let series = groups
                .into_iter()
                .map(
                    |(
                        key,
                        LineGroup {
                            color: color_category,
                            mut points,
                        },
                    )| {
                        points.sort_by(|a, b| {
                            compare_order(&a.0, &b.0).then_with(|| a.1.key.cmp(&b.1.key))
                        });
                        MirSeries {
                            key,
                            color_category,
                            points: points.into_iter().map(|(_, p)| p).collect(),
                        }
                    },
                )
                .collect();
            ChartMark::Line {
                id: id.clone(),
                x: x.clone(),
                y: y.clone(),
                color: color.clone(),
                group_expression: group_expression.clone(),
                order_expression: order_expression.clone(),
                line_width: *line_width,
                show_points: *show_points,
                series,
            }
        }
    })
}

fn same_points(a: &[MirPointItem], b: &[MirPointItem]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.key == b.key
                && a.color_category == b.color_category
                && a.x.to_bits() == b.x.to_bits()
                && a.y.to_bits() == b.y.to_bits()
        })
}

fn same_cache(a: &ChartMark, b: &ChartMark) -> bool {
    match (a, b) {
        (
            ChartMark::Heatmap {
                instances: a,
                value_labels: al,
                ..
            },
            ChartMark::Heatmap {
                instances: b,
                value_labels: bl,
                ..
            },
        ) => {
            al == bl
                && a.len() == b.len()
                && a.iter().zip(b).all(|(a, b)| {
                    a.key == b.key
                        && a.x == b.x
                        && a.y == b.y
                        && a.value.to_bits() == b.value.to_bits()
                })
        }
        (ChartMark::Symbol { instances: a, .. }, ChartMark::Symbol { instances: b, .. }) => {
            same_points(a, b)
        }
        (ChartMark::Bar { instances: a, .. }, ChartMark::Bar { instances: b, .. }) => {
            a.len() == b.len()
                && a.iter().zip(b).all(|(a, b)| {
                    a.key == b.key
                        && a.category == b.category
                        && a.color_category == b.color_category
                        && a.value.to_bits() == b.value.to_bits()
                })
        }
        (ChartMark::Line { series: a, .. }, ChartMark::Line { series: b, .. })
        | (ChartMark::Area { series: a, .. }, ChartMark::Area { series: b, .. }) => {
            a.len() == b.len()
                && a.iter().zip(b).all(|(a, b)| {
                    a.key == b.key
                        && a.color_category == b.color_category
                        && same_points(&a.points, &b.points)
                })
        }
        _ => false,
    }
}
