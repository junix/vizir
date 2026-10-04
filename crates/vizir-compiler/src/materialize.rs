//! Bounded static chart execution. Stored instances are assertions, not inputs.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use vizir_core::{
    ChartMark, Document, Expression, LiteralValue, MirBarItem, MirChart, MirDataNode,
    MirDataOperator, MirGeometryNode, MirPointItem, MirScale, MirSeries, MirView, TypedExpression,
    UpdateMode, ValueType, VizError, VizMir, VizResult, value_as_key,
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
}

impl Budget {
    pub(crate) fn new(limits: MaterializationLimits) -> Self {
        Self {
            limits,
            remaining: limits.max_evaluation_steps,
        }
    }

    fn step(&mut self, context: &str) -> VizResult<()> {
        self.charge(1, context)
    }

    fn charge(&mut self, steps: u64, context: &str) -> VizResult<()> {
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
    Ok(result)
}

pub(crate) fn materialize_mir_marks(
    mir: &VizMir,
    limits: MaterializationLimits,
    verify: bool,
) -> VizResult<Vec<Option<ChartMark>>> {
    let mut budget = Budget::new(limits);
    preflight_mir(mir, &mut budget)?;
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
                    category: value_as_key(raw),
                });
            }
            Expression::Literal { value } => match value {
                LiteralValue::Int64(v) => Scalar::Int(*v),
                LiteralValue::Float64(v) if v.is_finite() => Scalar::Float(*v),
                LiteralValue::Bool(v) => Scalar::Bool(*v),
                LiteralValue::String(v) => Scalar::String(v.clone()),
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

fn check_scale_membership(
    chart: &MirChart,
    mark: &ChartMark,
    budget: &mut Budget,
) -> VizResult<()> {
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
        | ChartMark::Bar { color, .. } => color,
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
        ChartMark::Line { series, .. } => {
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
        ChartMark::Symbol { x, y, color, .. } | ChartMark::Line { x, y, color, .. } => {
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
    } = mark
    {
        check_role(expressions, order_expression, true, chart_id, budget)?;
        if let Some(group) = group_expression {
            check_role(expressions, group, false, chart_id, budget)?;
        }
    }
    let MirDataOperator::Inline { rows } = &source.operator;
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
                        "one line group must have a uniform color category",
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
        (ChartMark::Line { series: a, .. }, ChartMark::Line { series: b, .. }) => {
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
