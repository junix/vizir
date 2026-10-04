use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Color, DiagramEdge, DiagramLayout, DiagramNode, FontWeight, Frame, NumberFormat, PathCommand,
    Point, SpatialUnit, TextAnchor, Transform2D, TypedExpression, ValueType,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, try_from = "VizMirWire")]
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VizMirWire {
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

impl TryFrom<VizMirWire> for VizMir {
    type Error = crate::VizError;

    fn try_from(wire: VizMirWire) -> Result<Self, Self::Error> {
        let mir = Self {
            version: wire.version,
            source_hir_version: wire.source_hir_version,
            document_id: wire.document_id,
            width: wire.width,
            height: wire.height,
            background: wire.background,
            spaces: wire.spaces,
            data: wire.data,
            expressions: wire.expressions,
            views: wire.views,
            losses: wire.losses,
        };
        crate::validate::validate_mir_capabilities(&mir)
            .map_err(|diagnostics| crate::VizError::validation(&diagnostics))?;
        Ok(mir)
    }
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
    // Preserve the exact pre-0.4 schema dependency closure.
    #[schemars(skip)]
    QuantizeColor {
        id: String,
        domain: [f64; 2],
        thresholds: Vec<f64>,
        range: Vec<Color>,
    },
}

impl MirScale {
    pub fn id(&self) -> &str {
        match self {
            Self::Linear { id, .. }
            | Self::Band { id, .. }
            | Self::OrdinalColor { id, .. }
            | Self::QuantizeColor { id, .. } => id,
        }
    }

    pub fn range_space(&self) -> Option<&str> {
        match self {
            Self::Linear { range_space, .. } | Self::Band { range_space, .. } => Some(range_space),
            Self::OrdinalColor { .. } | Self::QuantizeColor { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(transform = numeric_guide_schema)]
pub struct MirGuide {
    pub id: String,
    pub kind: GuideKind,
    pub scale: String,
    pub label: String,
    pub orient: GuideOrient,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::hir::deserialize_present"
    )]
    #[schemars(with = "NumberFormat")]
    pub number_format: Option<NumberFormat>,
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
    // Kept out of the legacy ChartMark schema's entire dependency closure.
    #[schemars(skip)]
    Area {
        id: String,
        x: ScaleBinding,
        y: ScaleBinding,
        color: Option<ScaleBinding>,
        group_expression: Option<String>,
        order_expression: String,
        baseline: f64,
        series: Vec<MirSeries>,
    },
    #[schemars(skip)]
    Heatmap {
        id: String,
        x: ScaleBinding,
        y: ScaleBinding,
        color: ScaleBinding,
        instances: Vec<MirHeatmapCell>,
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
            Self::Symbol { id, .. }
            | Self::Line { id, .. }
            | Self::Area { id, .. }
            | Self::Heatmap { id, .. }
            | Self::Bar { id, .. } => id,
        }
    }

    pub fn bindings(&self) -> Vec<&ScaleBinding> {
        match self {
            Self::Symbol { x, y, color, .. }
            | Self::Line { x, y, color, .. }
            | Self::Area { x, y, color, .. } => {
                let mut bindings = vec![x, y];
                bindings.extend(color.iter());
                bindings
            }
            Self::Heatmap { x, y, color, .. } => vec![x, y, color],
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
pub struct MirHeatmapCell {
    pub key: String,
    pub x: String,
    pub y: String,
    pub value: f64,
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

// These private schema-only types are not alternate runtime IRs. Preserve every
// legacy referenced definition, and add new versioned paths only for VizMIR 0.3.
#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = versioned_mir_schema)]
struct VizMirLegacy {
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

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = mir_v03_schema)]
struct VizMirV03 {
    pub version: String,
    pub source_hir_version: String,
    pub document_id: String,
    pub width: f64,
    pub height: f64,
    pub background: Color,
    pub spaces: BTreeMap<String, CoordinateSpace2D>,
    pub data: BTreeMap<String, MirDataNode>,
    pub expressions: BTreeMap<String, TypedExpression>,
    pub views: Vec<MirViewV03>,
    pub losses: Vec<LossRecord>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "dialect", rename_all = "kebab-case", deny_unknown_fields)]
enum MirViewV03 {
    Chart(Box<MirChartV03>),
    Diagram(MirDiagram),
    Geometry(MirGeometry),
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
struct MirChartV03 {
    pub id: String,
    pub title: Option<String>,
    pub frame: Frame,
    pub space: String,
    pub source: String,
    pub row_variable: String,
    pub key_expression: String,
    pub scales: Vec<MirScale>,
    pub guides: Vec<MirGuide>,
    pub mark: ChartMarkV03,
    pub provenance: Vec<String>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum ChartMarkV03 {
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
    Area {
        id: String,
        x: ScaleBinding,
        y: ScaleBinding,
        color: Option<ScaleBinding>,
        group_expression: Option<String>,
        order_expression: String,
        baseline: f64,
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

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = mir_v04_schema)]
struct VizMirV04 {
    pub version: String,
    pub source_hir_version: String,
    pub document_id: String,
    pub width: f64,
    pub height: f64,
    pub background: Color,
    pub spaces: BTreeMap<String, CoordinateSpace2D>,
    pub data: BTreeMap<String, MirDataNode>,
    pub expressions: BTreeMap<String, TypedExpression>,
    pub views: Vec<MirViewV04>,
    pub losses: Vec<LossRecord>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "dialect", rename_all = "kebab-case", deny_unknown_fields)]
enum MirViewV04 {
    Chart(Box<MirChartV04>),
    Diagram(MirDiagram),
    Geometry(MirGeometry),
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
struct MirChartV04 {
    pub id: String,
    pub title: Option<String>,
    pub frame: Frame,
    pub space: String,
    pub source: String,
    pub row_variable: String,
    pub key_expression: String,
    pub scales: Vec<MirScaleV04>,
    pub guides: Vec<MirGuideV04>,
    pub mark: ChartMarkV04,
    pub provenance: Vec<String>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum ChartMarkV04 {
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
    Area {
        id: String,
        x: ScaleBinding,
        y: ScaleBinding,
        color: Option<ScaleBinding>,
        group_expression: Option<String>,
        order_expression: String,
        baseline: f64,
        series: Vec<MirSeries>,
    },
    Heatmap {
        id: String,
        x: ScaleBinding,
        y: ScaleBinding,
        color: ScaleBinding,
        instances: Vec<MirHeatmapCell>,
    },
    Bar {
        id: String,
        category: ScaleBinding,
        value: ScaleBinding,
        color: Option<ScaleBinding>,
        instances: Vec<MirBarItem>,
    },
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum MirScaleV04 {
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
    // Preserve the exact pre-0.4 schema dependency closure.
    QuantizeColor {
        id: String,
        domain: [f64; 2],
        #[schemars(length(max = 8))]
        thresholds: Vec<f64>,
        #[schemars(length(min = 2, max = 9))]
        range: Vec<Color>,
    },
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
struct MirGuideV04 {
    pub id: String,
    pub kind: GuideKind,
    pub scale: String,
    pub label: String,
    pub orient: GuideOrient,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::hir::deserialize_present"
    )]
    #[schemars(with = "NumberFormat")]
    pub number_format: Option<NumberFormat>,
}

impl JsonSchema for VizMir {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "VizMir".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let legacy = VizMirLegacy::json_schema(generator);
        let current = generator.subschema_for::<VizMirV03>();
        let heatmap = generator.subschema_for::<VizMirV04>();
        schemars::json_schema!({ "oneOf": [legacy, current, heatmap] })
    }
}

fn mir_v03_schema(schema: &mut schemars::Schema) {
    let properties = schema
        .as_object_mut()
        .expect("MIR schema object")
        .get_mut("properties")
        .expect("MIR schema properties");
    properties["version"]["const"] = "0.3".into();
    properties["source_hir_version"]["const"] = "0.3".into();
}

fn mir_v04_schema(schema: &mut schemars::Schema) {
    let properties = schema
        .as_object_mut()
        .expect("MIR schema object")
        .get_mut("properties")
        .expect("MIR schema properties");
    properties["version"]["const"] = "0.4".into();
    properties["source_hir_version"]["const"] = "0.4".into();
}

fn numeric_guide_schema(schema: &mut schemars::Schema) {
    schema.insert(
        "allOf".to_owned(),
        serde_json::json!([{
            "if": { "required": ["number_format"] },
            "then": { "properties": { "kind": { "const": "axis" } } }
        }]),
    );
}

// Structural version boundary complements validate_mir's semantic reference
// checks. Keep this on the type so schema_for!(VizMir) has the same contract.
fn versioned_mir_schema(schema: &mut schemars::Schema) {
    schema
        .as_object_mut()
        .expect("VizMIR schema is an object")
        .get_mut("properties")
        .expect("VizMIR has properties")["version"]["enum"] = serde_json::json!(["0.1", "0.2"]);
    schema.insert(
        "allOf".to_owned(),
        serde_json::json!([{
            "if": { "properties": { "version": { "const": "0.1" } }, "required": ["version"] },
            "then": { "properties": { "views": { "items": { "properties": {
                "guides": { "items": { "not": { "required": ["number_format"] } } }
            } } } } }
        }]),
    );
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
    // Only an exactly constant domain maps to the range midpoint. Tiny
    // representable spans must preserve distinct datum positions.
    if span == 0.0 {
        return (range[0] + range[1]) / 2.0;
    }
    range[0] + (value - domain[0]) / span * (range[1] - range[0])
}

pub fn point(x: f64, y: f64) -> Point {
    Point { x, y }
}
