use serde::{Deserialize, Serialize};

use crate::{Color, DiagramEdge, DiagramLayout, DiagramNode, Frame, GeometryNode, Point};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VizMir {
    pub version: String,
    pub document_id: String,
    pub width: f64,
    pub height: f64,
    pub background: Color,
    pub views: Vec<MirView>,
    pub losses: Vec<LossRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "dialect", rename_all = "kebab-case")]
pub enum MirView {
    Chart(MirChart),
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MirChart {
    pub id: String,
    pub title: Option<String>,
    pub frame: Frame,
    pub scales: Vec<MirScale>,
    pub guides: Vec<MirGuide>,
    pub mark: ChartMark,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MirScale {
    Linear {
        id: String,
        domain: [f64; 2],
        range: [f64; 2],
        zero: bool,
    },
    Band {
        id: String,
        domain: Vec<String>,
        range: [f64; 2],
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MirGuide {
    pub kind: GuideKind,
    pub scale: String,
    pub label: String,
    pub orient: GuideOrient,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GuideKind {
    Axis,
    Legend,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GuideOrient {
    Bottom,
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ChartMark {
    Symbol {
        x_scale: String,
        y_scale: String,
        color_scale: Option<String>,
        size: f64,
        items: Vec<MirPointItem>,
    },
    Line {
        x_scale: String,
        y_scale: String,
        color_scale: Option<String>,
        line_width: f64,
        show_points: bool,
        series: Vec<MirSeries>,
    },
    Bar {
        category_scale: String,
        value_scale: String,
        color_scale: Option<String>,
        items: Vec<MirBarItem>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MirPointItem {
    pub key: String,
    pub x: f64,
    pub y: f64,
    pub color_category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MirSeries {
    pub key: String,
    pub color_category: Option<String>,
    pub points: Vec<MirPointItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MirBarItem {
    pub key: String,
    pub category: String,
    pub value: f64,
    pub color_category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MirDiagram {
    pub id: String,
    pub title: Option<String>,
    pub frame: Frame,
    pub nodes: Vec<DiagramNode>,
    pub edges: Vec<DiagramEdge>,
    pub layout_request: LayoutRequest,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutRequest {
    pub id: String,
    pub algorithm: DiagramLayout,
    pub node_ids: Vec<String>,
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MirGeometry {
    pub id: String,
    pub title: Option<String>,
    pub frame: Frame,
    pub children: Vec<GeometryNode>,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LoweringFidelity {
    Lossless,
    SemanticallyEquivalent,
    VisuallyApproximate,
    Rasterized,
    Dropped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LossRecord {
    pub source: String,
    pub target: String,
    pub fidelity: LoweringFidelity,
    pub reason: String,
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
