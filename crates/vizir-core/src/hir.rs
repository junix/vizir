use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn default_version() -> String {
    "0.1".to_owned()
}

fn default_background() -> Color {
    Color::transparent()
}

fn default_point_size() -> f64 {
    7.0
}

fn default_line_width() -> f64 {
    2.5
}

fn default_true() -> bool {
    true
}

fn default_diagram_layout() -> DiagramLayout {
    DiagramLayout::Layered
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Document {
    #[serde(default = "default_version")]
    pub version: String,
    pub id: String,
    pub width: f64,
    pub height: f64,
    #[serde(default = "default_background")]
    pub background: Color,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub datasets: BTreeMap<String, Dataset>,
    pub views: Vec<View>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Dataset {
    pub key: String,
    pub rows: Vec<BTreeMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum View {
    #[serde(rename = "chart.scatter")]
    Scatter(ScatterChart),
    #[serde(rename = "chart.line")]
    Line(LineChart),
    #[serde(rename = "chart.bar")]
    Bar(BarChart),
    #[serde(rename = "diagram.graph")]
    Diagram(DiagramGraph),
    #[serde(rename = "geometry.scene")]
    Geometry(GeometryScene),
}

impl View {
    pub fn id(&self) -> &str {
        match self {
            Self::Scatter(view) => &view.id,
            Self::Line(view) => &view.id,
            Self::Bar(view) => &view.id,
            Self::Diagram(view) => &view.id,
            Self::Geometry(view) => &view.id,
        }
    }

    pub fn frame(&self) -> &Frame {
        match self {
            Self::Scatter(view) => &view.frame,
            Self::Line(view) => &view.frame,
            Self::Bar(view) => &view.frame,
            Self::Diagram(view) => &view.frame,
            Self::Geometry(view) => &view.frame,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FieldEncoding {
    pub field: String,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ColorEncoding {
    pub field: String,
    #[serde(default)]
    pub palette: Vec<Color>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScatterChart {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub frame: Frame,
    pub dataset: String,
    pub x: FieldEncoding,
    pub y: FieldEncoding,
    #[serde(default)]
    pub color: Option<ColorEncoding>,
    #[serde(default = "default_point_size")]
    pub point_size: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LineChart {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub frame: Frame,
    pub dataset: String,
    pub x: FieldEncoding,
    pub y: FieldEncoding,
    #[serde(default)]
    pub series: Option<ColorEncoding>,
    #[serde(default = "default_line_width")]
    pub line_width: f64,
    #[serde(default = "default_true")]
    pub show_points: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BarChart {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub frame: Frame,
    pub dataset: String,
    pub category: FieldEncoding,
    pub value: FieldEncoding,
    #[serde(default)]
    pub color: Option<ColorEncoding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DiagramLayout {
    Layered,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DiagramGraph {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub frame: Frame,
    #[serde(default = "default_diagram_layout")]
    pub layout: DiagramLayout,
    pub nodes: Vec<DiagramNode>,
    #[serde(default)]
    pub edges: Vec<DiagramEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DiagramNode {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub position: Option<Point>,
    #[serde(default)]
    pub style: ShapeStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DiagramEdge {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub style: ShapeStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GeometryScene {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub frame: Frame,
    pub children: Vec<GeometryNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum GeometryNode {
    Group {
        id: String,
        #[serde(default)]
        transform: Transform2D,
        #[serde(default)]
        opacity: Option<f64>,
        children: Vec<GeometryNode>,
    },
    Rect {
        id: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        #[serde(default)]
        radius: f64,
        #[serde(default)]
        style: ShapeStyle,
    },
    Circle {
        id: String,
        cx: f64,
        cy: f64,
        radius: f64,
        #[serde(default)]
        style: ShapeStyle,
    },
    Line {
        id: String,
        from: Point,
        to: Point,
        #[serde(default)]
        style: ShapeStyle,
    },
    Path {
        id: String,
        commands: Vec<PathCommand>,
        #[serde(default)]
        style: ShapeStyle,
    },
    Text {
        id: String,
        x: f64,
        y: f64,
        text: String,
        #[serde(default = "default_font_size")]
        font_size: f64,
        #[serde(default)]
        anchor: TextAnchor,
        #[serde(default)]
        color: Option<Color>,
        #[serde(default)]
        weight: FontWeight,
    },
}

impl GeometryNode {
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

fn default_font_size() -> f64 {
    16.0
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PathCommand {
    Move {
        to: Point,
    },
    Line {
        to: Point,
    },
    Cubic {
        control1: Point,
        control2: Point,
        to: Point,
    },
    Close,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Transform2D {
    #[serde(default)]
    pub translate: Point,
    #[serde(default)]
    pub rotate_degrees: f64,
    #[serde(default = "default_scale")]
    pub scale: Point,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self {
            translate: Point::default(),
            rotate_degrees: 0.0,
            scale: default_scale(),
        }
    }
}

fn default_scale() -> Point {
    Point { x: 1.0, y: 1.0 }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShapeStyle {
    #[serde(default)]
    pub fill: Option<Color>,
    #[serde(default)]
    pub stroke: Option<Color>,
    #[serde(default = "default_stroke_width")]
    pub stroke_width: f64,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
}

impl Default for ShapeStyle {
    fn default() -> Self {
        Self {
            fill: None,
            stroke: None,
            stroke_width: default_stroke_width(),
            opacity: default_opacity(),
        }
    }
}

fn default_stroke_width() -> f64 {
    1.5
}

fn default_opacity() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct Color(pub String);

impl Color {
    pub fn transparent() -> Self {
        Self("transparent".to_owned())
    }

    pub fn hex(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TextAnchor {
    Start,
    #[default]
    Middle,
    End,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum FontWeight {
    #[default]
    Regular,
    Medium,
    Bold,
}
