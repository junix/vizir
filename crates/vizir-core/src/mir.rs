use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Color, DiagramEdge, DiagramLayout, DiagramNode, FontWeight, Frame, PathCommand, Point,
    SpatialUnit, TextAnchor, Transform2D, TypedExpression, ValueType,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VizMir {
    pub version: String,
    pub source_hir_version: String,
    pub document_id: String,
    pub width: f64,
    pub height: f64,
    pub background: Color,
    pub spaces: BTreeMap<String, CoordinateSpace2D>,
    pub data: BTreeMap<String, MirDataNode>,
    pub expressions: BTreeMap<String, TypedExpression>,
    pub views: Vec<MirView>,
    pub losses: Vec<LossRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CoordinateSpaceKind {
    Document,
    ViewLocal,
    Plot,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CoordinateSpace2D {
    pub id: String,
    pub kind: CoordinateSpaceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub unit: SpatialUnit,
    pub transform_to_parent: Transform2D,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MirDataNode {
    pub id: String,
    pub schema: MirDataSchema,
    pub operator: MirDataOperator,
    pub update_mode: UpdateMode,
    pub deterministic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MirDataSchema {
    pub key: String,
    pub fields: BTreeMap<String, ValueType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MirDataOperator {
    Inline { rows: Vec<BTreeMap<String, Value>> },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateMode {
    Replace,
    Incremental,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "dialect", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MirView {
    Chart(Box<MirChart>),
    Diagram(MirDiagram),
    Geometry(MirGeometry),
}

impl MirView {
    pub fn id(&self) -> &str {
        match self {
            Self::Chart(view) => &view.id,
            Self::Diagram(view) => &view.id,
            Self::Geometry(view) => &view.id,
        }
    }

    pub fn space(&self) -> &str {
        match self {
            Self::Chart(view) => &view.space,
            Self::Diagram(view) => &view.space,
            Self::Geometry(view) => &view.space,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MirChart {
    pub id: String,
    pub title: Option<String>,
    pub frame: Frame,
    pub space: String,
    pub source: String,
    pub row_variable: String,
    pub key_expression: String,
    pub scales: Vec<MirScale>,
    pub guides: Vec<MirGuide>,
    pub mark: ChartMark,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MirScale {
    Linear {
        id: String,
        domain: [f64; 2],
        range: [f64; 2],
        range_space: String,
        zero: bool,
    },
    Band {
        id: String,
        domain: Vec<String>,
        range: [f64; 2],
        range_space: String,
        padding: f64,
    },
    OrdinalColor {
        id: String,
        domain: Vec<String>,
        range: Vec<Color>,
    },
}

impl MirScale {
    pub fn id(&self) -> &str {
        match self {
            Self::Linear { id, .. } | Self::Band { id, .. } | Self::OrdinalColor { id, .. } => id,
        }
    }

    pub fn range_space(&self) -> Option<&str> {
        match self {
            Self::Linear { range_space, .. } | Self::Band { range_space, .. } => Some(range_space),
            Self::OrdinalColor { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MirGuide {
    pub id: String,
    pub kind: GuideKind,
    pub scale: String,
    pub label: String,
    pub orient: GuideOrient,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GuideKind {
    Axis,
    Legend,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GuideOrient {
    Bottom,
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScaleBinding {
    pub scale: String,
    pub expression: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ChartMark {
    Symbol {
        id: String,
        x: ScaleBinding,
        y: ScaleBinding,
        color: Option<ScaleBinding>,
        size: f64,
        instances: Vec<MirPointItem>,
    },
    Line {
        id: String,
        x: ScaleBinding,
        y: ScaleBinding,
        color: Option<ScaleBinding>,
        group_expression: Option<String>,
        order_expression: String,
        line_width: f64,
        show_points: bool,
        series: Vec<MirSeries>,
    },
    Bar {
        id: String,
        category: ScaleBinding,
        value: ScaleBinding,
        color: Option<ScaleBinding>,
        instances: Vec<MirBarItem>,
    },
}

impl ChartMark {
    pub fn id(&self) -> &str {
        match self {
            Self::Symbol { id, .. } | Self::Line { id, .. } | Self::Bar { id, .. } => id,
        }
    }

    pub fn bindings(&self) -> Vec<&ScaleBinding> {
        match self {
            Self::Symbol { x, y, color, .. } | Self::Line { x, y, color, .. } => {
                let mut bindings = vec![x, y];
                bindings.extend(color.iter());
                bindings
            }
            Self::Bar {
                category,
                value,
                color,
                ..
            } => {
                let mut bindings = vec![category, value];
                bindings.extend(color.iter());
                bindings
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MirPointItem {
    pub key: String,
    pub x: f64,
    pub y: f64,
    pub color_category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MirSeries {
    pub key: String,
    pub color_category: Option<String>,
    pub points: Vec<MirPointItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MirBarItem {
    pub key: String,
    pub category: String,
    pub value: f64,
    pub color_category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MirDiagram {
    pub id: String,
    pub title: Option<String>,
    pub frame: Frame,
    pub space: String,
    pub nodes: Vec<DiagramNode>,
    pub edges: Vec<DiagramEdge>,
    pub layout_request: LayoutRequest,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LayoutRequest {
    pub id: String,
    pub algorithm: DiagramLayout,
    pub node_ids: Vec<String>,
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MirGeometry {
    pub id: String,
    pub title: Option<String>,
    pub frame: Frame,
    pub space: String,
    pub children: Vec<MirGeometryNode>,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MirShapeStyle {
    pub fill: Color,
    pub stroke: Color,
    pub stroke_width: f64,
    pub opacity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MirGeometryNode {
    Group {
        id: String,
        transform: Transform2D,
        opacity: f64,
        children: Vec<MirGeometryNode>,
    },
    Rect {
        id: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
        style: MirShapeStyle,
    },
    Circle {
        id: String,
        cx: f64,
        cy: f64,
        radius: f64,
        style: MirShapeStyle,
    },
    Line {
        id: String,
        from: Point,
        to: Point,
        style: MirShapeStyle,
    },
    Path {
        id: String,
        commands: Vec<PathCommand>,
        style: MirShapeStyle,
    },
    Text {
        id: String,
        x: f64,
        y: f64,
        text: String,
        font_size: f64,
        anchor: TextAnchor,
        color: Color,
        weight: FontWeight,
    },
}

impl MirGeometryNode {
    pub fn id(&self) -> &str {
        match self {
            Self::Group { id, .. }
            | Self::Rect { id, .. }
            | Self::Circle { id, .. }
            | Self::Line { id, .. }
            | Self::Path { id, .. }
            | Self::Text { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LoweringFidelity {
    Lossless,
    SemanticallyEquivalent,
    VisuallyApproximate,
    Rasterized,
    Dropped,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LossRecord {
    pub source: String,
    pub target: String,
    pub fidelity: LoweringFidelity,
    pub reason: String,
}

pub fn mir_schema() -> serde_json::Value {
    let mut schema =
        serde_json::to_value(schemars::schema_for!(VizMir)).expect("VizMIR schema must serialize");
    schema
        .as_object_mut()
        .expect("root schema must be an object")
        .insert(
            "$schema".to_owned(),
            serde_json::Value::String("https://json-schema.org/draft/2020-12/schema".to_owned()),
        );
    schema
}

pub fn map_linear(value: f64, domain: [f64; 2], range: [f64; 2]) -> f64 {
    let span = domain[1] - domain[0];
    if span.abs() < f64::EPSILON {
        return (range[0] + range[1]) / 2.0;
    }
    range[0] + (value - domain[0]) / span * (range[1] - range[0])
}

pub fn point(x: f64, y: f64) -> Point {
    Point { x, y }
}
