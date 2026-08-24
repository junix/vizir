use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Color, FontWeight, LossRecord, PathCommand, Point, TextAnchor, Transform2D};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Scene2D {
    pub document_id: String,
    pub width: f64,
    pub height: f64,
    pub background: Color,
    pub nodes: Vec<SceneNode>,
    pub losses: Vec<LossRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SceneNode {
    Group {
        id: String,
        bounds: Rect,
        origin: Origin,
        transform: Transform2D,
        opacity: f64,
        children: Vec<SceneNode>,
    },
    Rect {
        id: String,
        bounds: Rect,
        origin: Origin,
        radius: f64,
        style: ResolvedStyle,
    },
    Circle {
        id: String,
        bounds: Rect,
        origin: Origin,
        center: Point,
        radius: f64,
        style: ResolvedStyle,
    },
    Line {
        id: String,
        bounds: Rect,
        origin: Origin,
        from: Point,
        to: Point,
        style: ResolvedStyle,
        marker_end: bool,
    },
    Path {
        id: String,
        bounds: Rect,
        origin: Origin,
        commands: Vec<PathCommand>,
        style: ResolvedStyle,
        marker_end: bool,
    },
    Text {
        id: String,
        bounds: Rect,
        origin: Origin,
        position: Point,
        text: String,
        font_size: f64,
        anchor: TextAnchor,
        color: Color,
        weight: FontWeight,
    },
}

impl SceneNode {
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

    pub fn origin(&self) -> &Origin {
        match self {
            Self::Group { origin, .. }
            | Self::Rect { origin, .. }
            | Self::Circle { origin, .. }
            | Self::Line { origin, .. }
            | Self::Path { origin, .. }
            | Self::Text { origin, .. } => origin,
        }
    }

    pub fn children(&self) -> &[SceneNode] {
        match self {
            Self::Group { children, .. } => children,
            _ => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn from_points(a: Point, b: Point) -> Self {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        Self {
            x,
            y,
            width: (a.x - b.x).abs(),
            height: (a.y - b.y).abs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ResolvedStyle {
    pub fill: Color,
    pub stroke: Color,
    pub stroke_width: f64,
    pub opacity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Origin {
    pub hir_node: String,
    pub mir_node: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_key: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_lineage: Vec<String>,
    pub generated_by: String,
    pub explanation: String,
}

pub fn find_scene_node<'a>(nodes: &'a [SceneNode], id: &str) -> Option<&'a SceneNode> {
    for node in nodes {
        if node.id() == id {
            return Some(node);
        }
        if let Some(found) = find_scene_node(node.children(), id) {
            return Some(found);
        }
    }
    None
}
