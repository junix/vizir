use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::{
    ChartMark, Color, Diagnostic, DiagramLayout, Document, GeometryNode, MirChart, MirScale,
    MirView, PathCommand, Point, ShapeStyle, TypeEnvironment, ValueType, View, VizError, VizMir,
    VizResult, type_expression,
};

pub fn parse_document(path: impl AsRef<Path>) -> VizResult<Document> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|source| VizError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let document = if extension.eq_ignore_ascii_case("json") {
        serde_json::from_str(&source)?
    } else {
        serde_yaml::from_str(&source)?
    };
    Ok(document)
}

pub fn validate_document(document: &Document) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();

    if document.version != "0.1" {
        diagnostics.push(
            Diagnostic::new(
                "VIZ-SCHEMA-0001",
                format!("unsupported VizHIR version {:?}", document.version),
            )
            .at("version")
            .with_help("use version \"0.1\" or run a schema migration"),
        );
    }

    validate_id(&document.id, "id", &mut diagnostics);
    validate_positive(document.width, "width", &mut diagnostics);
    validate_positive(document.height, "height", &mut diagnostics);
    validate_color(&document.background, "background", &mut diagnostics);

    for (name, dataset) in &document.datasets {
        validate_id(name, &format!("datasets.{name}"), &mut diagnostics);
        let mut keys = BTreeSet::new();
        for (row_index, row) in dataset.rows.iter().enumerate() {
            let row_source = format!("datasets.{name}.rows[{row_index}]");
            match row.get(&dataset.key).and_then(value_as_key) {
                Some(key) if keys.insert(key.clone()) => {}
                Some(key) => diagnostics.push(
                    Diagnostic::new("VIZ-VALIDATE-0102", format!("duplicate data key {key:?}"))
                        .at(format!("{row_source}.{}", dataset.key)),
                ),
                None => diagnostics.push(
                    Diagnostic::new(
                        "VIZ-VALIDATE-0103",
                        format!("missing or invalid stable key field {:?}", dataset.key),
                    )
                    .at(row_source),
                ),
            }
        }
        if dataset.rows.is_empty() {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-VALIDATE-0106",
                    "inline dataset without an explicit schema cannot be empty",
                )
                .at(format!("datasets.{name}.rows")),
            );
        }
    }

    let mut view_ids = HashSet::new();
    for (index, view) in document.views.iter().enumerate() {
        let source = format!("views[{index}]");
        validate_id(view.id(), &format!("{source}.id"), &mut diagnostics);
        if !view_ids.insert(view.id()) {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-VALIDATE-0002",
                    format!("duplicate view id {:?}", view.id()),
                )
                .at(format!("{source}.id")),
            );
        }
        validate_frame(view.frame(), &format!("{source}.frame"), &mut diagnostics);
        match view {
            View::Scatter(chart) => {
                validate_chart_fields(
                    document,
                    &chart.dataset,
                    &[&chart.x.field, &chart.y.field],
                    &chart
                        .color
                        .as_ref()
                        .map(|value| vec![value.field.as_str()])
                        .unwrap_or_default(),
                    &source,
                    &mut diagnostics,
                );
                validate_positive(
                    chart.point_size,
                    &format!("{source}.point_size"),
                    &mut diagnostics,
                );
                validate_palette(
                    chart.color.as_ref().map(|value| &value.palette),
                    &source,
                    &mut diagnostics,
                );
            }
            View::Line(chart) => {
                validate_chart_fields(
                    document,
                    &chart.dataset,
                    &[&chart.x.field, &chart.y.field],
                    &chart
                        .series
                        .as_ref()
                        .map(|value| vec![value.field.as_str()])
                        .unwrap_or_default(),
                    &source,
                    &mut diagnostics,
                );
                validate_positive(
                    chart.line_width,
                    &format!("{source}.line_width"),
                    &mut diagnostics,
                );
                validate_palette(
                    chart.series.as_ref().map(|value| &value.palette),
                    &source,
                    &mut diagnostics,
                );
            }
            View::Bar(chart) => {
                validate_chart_fields(
                    document,
                    &chart.dataset,
                    &[&chart.value.field],
                    &{
                        let mut fields = vec![chart.category.field.as_str()];
                        if let Some(color) = &chart.color {
                            fields.push(color.field.as_str());
                        }
                        fields
                    },
                    &source,
                    &mut diagnostics,
                );
                validate_palette(
                    chart.color.as_ref().map(|value| &value.palette),
                    &source,
                    &mut diagnostics,
                );
            }
            View::Diagram(diagram) => {
                let mut node_ids = HashSet::new();
                for (node_index, node) in diagram.nodes.iter().enumerate() {
                    let node_source = format!("{source}.nodes[{node_index}]");
                    validate_id(&node.id, &format!("{node_source}.id"), &mut diagnostics);
                    validate_style(
                        &node.style,
                        &format!("{node_source}.style"),
                        &mut diagnostics,
                    );
                    if !node_ids.insert(node.id.as_str()) {
                        diagnostics.push(
                            Diagnostic::new(
                                "VIZ-VALIDATE-0201",
                                format!("duplicate diagram node id {:?}", node.id),
                            )
                            .at(format!("{node_source}.id")),
                        );
                    }
                    if diagram.layout == DiagramLayout::Manual && node.position.is_none() {
                        diagnostics.push(
                            Diagnostic::new(
                                "VIZ-LAYOUT-0202",
                                format!("manual layout requires a position for node {:?}", node.id),
                            )
                            .at(&node_source),
                        );
                    }
                    if let Some(position) = node.position {
                        validate_point(
                            position,
                            &format!("{node_source}.position"),
                            &mut diagnostics,
                        );
                    }
                }
                for (edge_index, edge) in diagram.edges.iter().enumerate() {
                    let edge_source = format!("{source}.edges[{edge_index}]");
                    if !node_ids.contains(edge.from.as_str()) {
                        diagnostics.push(
                            Diagnostic::new(
                                "VIZ-VALIDATE-0203",
                                format!("edge references unknown source node {:?}", edge.from),
                            )
                            .at(format!("{edge_source}.from")),
                        );
                    }
                    if !node_ids.contains(edge.to.as_str()) {
                        diagnostics.push(
                            Diagnostic::new(
                                "VIZ-VALIDATE-0204",
                                format!("edge references unknown target node {:?}", edge.to),
                            )
                            .at(format!("{edge_source}.to")),
                        );
                    }
                    validate_style(
                        &edge.style,
                        &format!("{edge_source}.style"),
                        &mut diagnostics,
                    );
                }
            }
            View::Geometry(geometry) => {
                let mut ids = HashSet::new();
                for (node_index, node) in geometry.children.iter().enumerate() {
                    validate_geometry_node(
                        node,
                        &format!("{source}.children[{node_index}]"),
                        &mut ids,
                        &mut diagnostics,
                    );
                }
            }
        }
    }

    if document.views.is_empty() {
        diagnostics.push(Diagnostic::new("VIZ-VALIDATE-0003", "document has no views").at("views"));
    }

    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

pub fn validate_mir(mir: &VizMir) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    if mir.version != "0.1" {
        diagnostics.push(
            Diagnostic::new(
                "VIZ-MIR-0001",
                format!("unsupported VizMIR version {:?}", mir.version),
            )
            .at("version"),
        );
    }

    for (id, space) in &mir.spaces {
        if id != &space.id {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-MIR-0002",
                    format!(
                        "coordinate-space map key {id:?} does not match id {:?}",
                        space.id
                    ),
                )
                .at(format!("spaces.{id}")),
            );
        }
        if let Some(parent) = &space.parent
            && !mir.spaces.contains_key(parent)
        {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-RESOLVE-0001",
                    format!("unknown parent coordinate space {parent:?}"),
                )
                .at(format!("spaces.{id}.parent")),
            );
        }
    }

    for (id, data) in &mir.data {
        if id != &data.id {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-MIR-0003",
                    format!("data map key {id:?} does not match id {:?}", data.id),
                )
                .at(format!("data.{id}")),
            );
        }
        if !data.schema.fields.contains_key(&data.schema.key) {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-RESOLVE-0002",
                    format!(
                        "data key field {:?} is absent from the schema",
                        data.schema.key
                    ),
                )
                .at(format!("data.{id}.schema.key")),
            );
        }
    }

    let mut view_ids = BTreeSet::new();
    for (index, view) in mir.views.iter().enumerate() {
        let source = format!("views[{index}]");
        if !view_ids.insert(view.id()) {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-MIR-0004",
                    format!("duplicate MIR view id {:?}", view.id()),
                )
                .at(format!("{source}.id")),
            );
        }
        if !mir.spaces.contains_key(view.space()) {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-RESOLVE-0003",
                    format!("unknown view coordinate space {:?}", view.space()),
                )
                .at(format!("{source}.space")),
            );
        }
        if let MirView::Chart(chart) = view {
            validate_mir_chart(mir, chart, &source, &mut diagnostics);
        }
    }

    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn validate_mir_chart(
    mir: &VizMir,
    chart: &MirChart,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(data) = mir.data.get(&chart.source) else {
        diagnostics.push(
            Diagnostic::new(
                "VIZ-RESOLVE-0004",
                format!("unknown chart data source {:?}", chart.source),
            )
            .at(format!("{source}.source")),
        );
        return;
    };
    let environment = TypeEnvironment {
        rows: BTreeMap::from([(chart.row_variable.clone(), data.schema.fields.clone())]),
        ..TypeEnvironment::default()
    };
    validate_expression_reference(
        mir,
        &chart.key_expression,
        &environment,
        &format!("{source}.key_expression"),
        diagnostics,
    );

    let scale_ids = chart
        .scales
        .iter()
        .map(|scale| scale.id())
        .collect::<BTreeSet<_>>();
    if scale_ids.len() != chart.scales.len() {
        diagnostics.push(
            Diagnostic::new("VIZ-MIR-0005", "chart scale IDs must be unique")
                .at(format!("{source}.scales")),
        );
    }
    for (index, scale) in chart.scales.iter().enumerate() {
        if let Some(space) = scale.range_space()
            && !mir.spaces.contains_key(space)
        {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-RESOLVE-0005",
                    format!("scale references unknown range space {space:?}"),
                )
                .at(format!("{source}.scales[{index}].range_space")),
            );
        }
    }
    let guide_ids = chart
        .guides
        .iter()
        .map(|guide| guide.id.as_str())
        .collect::<BTreeSet<_>>();
    if guide_ids.len() != chart.guides.len() {
        diagnostics.push(
            Diagnostic::new("VIZ-MIR-0006", "chart guide IDs must be unique")
                .at(format!("{source}.guides")),
        );
    }
    for (index, guide) in chart.guides.iter().enumerate() {
        if !scale_ids.contains(guide.scale.as_str()) {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-RESOLVE-0006",
                    format!("guide references unknown scale {:?}", guide.scale),
                )
                .at(format!("{source}.guides[{index}].scale")),
            );
        }
    }
    for (index, binding) in chart.mark.bindings().iter().enumerate() {
        if !scale_ids.contains(binding.scale.as_str()) {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-RESOLVE-0007",
                    format!("mark binding references unknown scale {:?}", binding.scale),
                )
                .at(format!("{source}.mark.bindings[{index}].scale")),
            );
        }
        validate_expression_reference(
            mir,
            &binding.expression,
            &environment,
            &format!("{source}.mark.bindings[{index}].expression"),
            diagnostics,
        );
        if let (Some(scale), Some(expression)) = (
            chart
                .scales
                .iter()
                .find(|scale| scale.id() == binding.scale),
            mir.expressions.get(&binding.expression),
        ) {
            let compatible = match scale {
                MirScale::Linear { .. } => matches!(
                    expression.result_type,
                    ValueType::Int64 | ValueType::Float64
                ),
                MirScale::Band { .. } | MirScale::OrdinalColor { .. } => matches!(
                    expression.result_type,
                    ValueType::Bool | ValueType::Int64 | ValueType::Float64 | ValueType::String
                ),
            };
            if !compatible {
                diagnostics.push(
                    Diagnostic::new(
                        "VIZ-TYPE-0202",
                        format!(
                            "expression {:?} of type {:?} is incompatible with scale {:?}",
                            binding.expression, expression.result_type, binding.scale
                        ),
                    )
                    .at(format!("{source}.mark.bindings[{index}]")),
                );
            }
        }
    }
    if let ChartMark::Line {
        group_expression,
        order_expression,
        ..
    } = &chart.mark
    {
        if let Some(expression) = group_expression {
            validate_expression_reference(
                mir,
                expression,
                &environment,
                &format!("{source}.mark.group_expression"),
                diagnostics,
            );
        }
        validate_expression_reference(
            mir,
            order_expression,
            &environment,
            &format!("{source}.mark.order_expression"),
            diagnostics,
        );
    }
}

fn validate_expression_reference(
    mir: &VizMir,
    id: &str,
    environment: &TypeEnvironment,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(expression) = mir.expressions.get(id) else {
        diagnostics.push(
            Diagnostic::new(
                "VIZ-RESOLVE-0008",
                format!("unknown typed expression {id:?}"),
            )
            .at(source),
        );
        return;
    };
    match type_expression(expression.expression.clone(), environment) {
        Ok(checked) if checked.result_type == expression.result_type => {}
        Ok(checked) => diagnostics.push(
            Diagnostic::new(
                "VIZ-TYPE-0201",
                format!(
                    "expression {id:?} declares {:?} but checks as {:?}",
                    expression.result_type, checked.result_type
                ),
            )
            .at(source),
        ),
        Err(mut diagnostic) => {
            diagnostic.source = Some(source.to_owned());
            diagnostics.push(diagnostic);
        }
    }
}

fn validate_chart_fields(
    document: &Document,
    dataset_name: &str,
    numeric_fields: &[&str],
    category_fields: &[&str],
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(dataset) = document.datasets.get(dataset_name) else {
        diagnostics.push(
            Diagnostic::new(
                "VIZ-VALIDATE-0101",
                format!("unknown dataset {dataset_name:?}"),
            )
            .at(format!("{source}.dataset")),
        );
        return;
    };

    for (row_index, row) in dataset.rows.iter().enumerate() {
        let row_source = format!("datasets.{dataset_name}.rows[{row_index}]");
        for field in numeric_fields {
            match row.get(*field).and_then(Value::as_f64) {
                Some(value) if value.is_finite() => {}
                _ => diagnostics.push(
                    Diagnostic::new(
                        "VIZ-TYPE-0104",
                        format!("field {field:?} must contain finite numbers"),
                    )
                    .at(format!("{row_source}.{field}")),
                ),
            }
        }
        for field in category_fields {
            if row.get(*field).and_then(value_as_key).is_none() {
                diagnostics.push(
                    Diagnostic::new(
                        "VIZ-TYPE-0105",
                        format!("field {field:?} must contain string-like category values"),
                    )
                    .at(format!("{row_source}.{field}")),
                );
            }
        }
    }
}

pub fn value_as_key(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn validate_palette(palette: Option<&Vec<Color>>, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(palette) = palette {
        for (index, color) in palette.iter().enumerate() {
            validate_color(color, &format!("{source}.palette[{index}]"), diagnostics);
        }
    }
}

fn validate_frame(frame: &crate::Frame, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    validate_finite(frame.x, &format!("{source}.x"), diagnostics);
    validate_finite(frame.y, &format!("{source}.y"), diagnostics);
    validate_positive(frame.width, &format!("{source}.width"), diagnostics);
    validate_positive(frame.height, &format!("{source}.height"), diagnostics);
}

fn validate_geometry_node<'a>(
    node: &'a GeometryNode,
    source: &str,
    ids: &mut HashSet<&'a str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !ids.insert(node.id()) {
        diagnostics.push(
            Diagnostic::new(
                "VIZ-VALIDATE-0301",
                format!("duplicate geometry node id {:?}", node.id()),
            )
            .at(format!("{source}.id")),
        );
    }
    match node {
        GeometryNode::Group {
            transform,
            opacity,
            children,
            ..
        } => {
            validate_point(
                transform.translate,
                &format!("{source}.transform.translate"),
                diagnostics,
            );
            validate_point(
                transform.scale,
                &format!("{source}.transform.scale"),
                diagnostics,
            );
            validate_finite(
                transform.rotate_degrees,
                &format!("{source}.transform.rotate_degrees"),
                diagnostics,
            );
            if let Some(opacity) = opacity {
                validate_unit(*opacity, &format!("{source}.opacity"), diagnostics);
            }
            for (index, child) in children.iter().enumerate() {
                validate_geometry_node(
                    child,
                    &format!("{source}.children[{index}]"),
                    ids,
                    diagnostics,
                );
            }
        }
        GeometryNode::Rect {
            x,
            y,
            width,
            height,
            radius,
            style,
            ..
        } => {
            validate_finite(*x, &format!("{source}.x"), diagnostics);
            validate_finite(*y, &format!("{source}.y"), diagnostics);
            validate_positive(*width, &format!("{source}.width"), diagnostics);
            validate_positive(*height, &format!("{source}.height"), diagnostics);
            validate_non_negative(*radius, &format!("{source}.radius"), diagnostics);
            validate_style(style, &format!("{source}.style"), diagnostics);
        }
        GeometryNode::Circle {
            cx,
            cy,
            radius,
            style,
            ..
        } => {
            validate_finite(*cx, &format!("{source}.cx"), diagnostics);
            validate_finite(*cy, &format!("{source}.cy"), diagnostics);
            validate_positive(*radius, &format!("{source}.radius"), diagnostics);
            validate_style(style, &format!("{source}.style"), diagnostics);
        }
        GeometryNode::Line {
            from, to, style, ..
        } => {
            validate_point(*from, &format!("{source}.from"), diagnostics);
            validate_point(*to, &format!("{source}.to"), diagnostics);
            validate_style(style, &format!("{source}.style"), diagnostics);
        }
        GeometryNode::Path {
            commands, style, ..
        } => {
            if commands.is_empty() {
                diagnostics
                    .push(Diagnostic::new("VIZ-VALIDATE-0302", "path has no commands").at(source));
            }
            for (index, command) in commands.iter().enumerate() {
                let command_source = format!("{source}.commands[{index}]");
                match command {
                    PathCommand::Move { to } | PathCommand::Line { to } => {
                        validate_point(*to, &command_source, diagnostics)
                    }
                    PathCommand::Cubic {
                        control1,
                        control2,
                        to,
                    } => {
                        validate_point(*control1, &command_source, diagnostics);
                        validate_point(*control2, &command_source, diagnostics);
                        validate_point(*to, &command_source, diagnostics);
                    }
                    PathCommand::Close => {}
                }
            }
            validate_style(style, &format!("{source}.style"), diagnostics);
        }
        GeometryNode::Text {
            x,
            y,
            font_size,
            color,
            ..
        } => {
            validate_finite(*x, &format!("{source}.x"), diagnostics);
            validate_finite(*y, &format!("{source}.y"), diagnostics);
            validate_positive(*font_size, &format!("{source}.font_size"), diagnostics);
            if let Some(color) = color {
                validate_color(color, &format!("{source}.color"), diagnostics);
            }
        }
    }
}

fn validate_style(style: &ShapeStyle, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    if let Some(fill) = &style.fill {
        validate_color(fill, &format!("{source}.fill"), diagnostics);
    }
    if let Some(stroke) = &style.stroke {
        validate_color(stroke, &format!("{source}.stroke"), diagnostics);
    }
    validate_non_negative(
        style.stroke_width,
        &format!("{source}.stroke_width"),
        diagnostics,
    );
    validate_unit(style.opacity, &format!("{source}.opacity"), diagnostics);
}

fn validate_color(color: &Color, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    let value = color.0.as_str();
    let valid_hex = matches!(value.len(), 7 | 9)
        && value.starts_with('#')
        && value[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit());
    if value != "transparent" && !valid_hex {
        diagnostics.push(
            Diagnostic::new("VIZ-TYPE-0004", format!("invalid portable color {value:?}"))
                .at(source)
                .with_help("use transparent, #RRGGBB, or #RRGGBBAA"),
        );
    }
}

fn validate_id(id: &str, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    let valid = !id.is_empty()
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '/')
        });
    if !valid {
        diagnostics.push(
            Diagnostic::new("VIZ-TYPE-0001", format!("invalid stable id {id:?}"))
                .at(source)
                .with_help("use letters, digits, hyphen, underscore, or slash"),
        );
    }
}

fn validate_point(point: Point, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    validate_finite(point.x, &format!("{source}.x"), diagnostics);
    validate_finite(point.y, &format!("{source}.y"), diagnostics);
}

fn validate_positive(value: f64, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    if !value.is_finite() || value <= 0.0 {
        diagnostics.push(
            Diagnostic::new(
                "VIZ-TYPE-0002",
                "value must be finite and greater than zero",
            )
            .at(source),
        );
    }
}

fn validate_non_negative(value: f64, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    if !value.is_finite() || value < 0.0 {
        diagnostics.push(
            Diagnostic::new("VIZ-TYPE-0003", "value must be finite and non-negative").at(source),
        );
    }
}

fn validate_unit(value: f64, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        diagnostics.push(
            Diagnostic::new("VIZ-TYPE-0005", "value must be between zero and one").at(source),
        );
    }
}

fn validate_finite(value: f64, source: &str, diagnostics: &mut Vec<Diagnostic>) {
    if !value.is_finite() {
        diagnostics.push(Diagnostic::new("VIZ-TYPE-0006", "value must be finite").at(source));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_validation_accepts_portable_values() {
        let mut diagnostics = Vec::new();
        validate_color(&Color("#12aBcDff".to_owned()), "color", &mut diagnostics);
        validate_color(&Color::transparent(), "color", &mut diagnostics);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn color_validation_rejects_css_names() {
        let mut diagnostics = Vec::new();
        validate_color(
            &Color("rebeccapurple".to_owned()),
            "color",
            &mut diagnostics,
        );
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = diagnostics.pop().unwrap();
        assert_eq!(diagnostic.code, "VIZ-TYPE-0004");
        assert_eq!(
            diagnostic.message,
            "invalid portable color \"rebeccapurple\""
        );
        assert_eq!(diagnostic.source.as_deref(), Some("color"));
        assert_eq!(
            diagnostic.help.as_deref(),
            Some("use transparent, #RRGGBB, or #RRGGBBAA")
        );
    }

    #[test]
    fn color_validation_enforces_exact_hex_lengths() {
        for (value, accepted) in [
            ("#12aBcD", true),     // #RRGGBB
            ("#12aBcDff", true),   // #RRGGBBAA
            ("#12aBc", false),     // one short of #RRGGBB
            ("#12aBcDf", false),   // between the two portable lengths
            ("#12aBcDff0", false), // one past #RRGGBBAA
            ("12aBcD", false),     // right shape, missing the '#' prefix
            ("#12aBGcD", false),   // right length, non-hex digit
        ] {
            let mut diagnostics = Vec::new();
            validate_color(&Color(value.to_owned()), "color", &mut diagnostics);
            assert_eq!(diagnostics.len(), usize::from(!accepted), "{value}");
            if !accepted {
                assert_eq!(diagnostics[0].code, "VIZ-TYPE-0004", "{value}");
            }
        }
    }

    #[test]
    fn unit_validation_enforces_the_inclusive_zero_to_one_range() {
        for value in [0.0, 1.0, 0.5] {
            let mut diagnostics = Vec::new();
            validate_unit(value, "opacity", &mut diagnostics);
            assert!(diagnostics.is_empty(), "{value}");
        }
        for value in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
            let mut diagnostics = Vec::new();
            validate_unit(value, "opacity", &mut diagnostics);
            assert_eq!(diagnostics.len(), 1, "{value}");
            assert_eq!(diagnostics[0].code, "VIZ-TYPE-0005", "{value}");
            assert_eq!(diagnostics[0].source.as_deref(), Some("opacity"), "{value}");
        }
    }

    #[test]
    fn finite_validation_rejects_nan_and_infinities_only() {
        for value in [0.0, -1.5, 1e300] {
            let mut diagnostics = Vec::new();
            validate_finite(value, "width", &mut diagnostics);
            assert!(diagnostics.is_empty(), "{value}");
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut diagnostics = Vec::new();
            validate_finite(value, "width", &mut diagnostics);
            assert_eq!(diagnostics.len(), 1, "{value}");
            assert_eq!(diagnostics[0].code, "VIZ-TYPE-0006", "{value}");
        }
    }

    #[test]
    fn validation_rejects_duplicate_data_keys() {
        let document: Document = serde_yaml::from_str(
            r##"
version: "0.1"
id: duplicate-keys
width: 400
height: 300
datasets:
  points:
    key: id
    rows:
      - {id: same, x: 1, y: 2}
      - {id: same, x: 2, y: 3}
views:
  - kind: chart.scatter
    id: points
    frame: {x: 0, y: 0, width: 400, height: 300}
    dataset: points
    x: {field: x}
    y: {field: y}
"##,
        )
        .unwrap();
        let diagnostics = validate_document(&document).unwrap_err();
        assert!(
            diagnostics
                .iter()
                .any(|value| value.code == "VIZ-VALIDATE-0102")
        );
    }

    #[test]
    fn validation_rejects_dangling_diagram_edges() {
        let document: Document = serde_yaml::from_str(
            r##"
version: "0.1"
id: dangling-edge
width: 400
height: 300
views:
  - kind: diagram.graph
    id: graph
    frame: {x: 0, y: 0, width: 400, height: 300}
    nodes:
      - {id: source, label: Source}
    edges:
      - {from: source, to: missing}
"##,
        )
        .unwrap();
        let diagnostics = validate_document(&document).unwrap_err();
        assert!(
            diagnostics
                .iter()
                .any(|value| value.code == "VIZ-VALIDATE-0204")
        );
    }
}
