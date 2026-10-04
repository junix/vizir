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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    pub axis: Option<AxisOptions>,
}

/// Opt-in axis semantics, available starting with VizHIR 0.2.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AxisOptions {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "NumberFormat")]
    pub number_format: Option<NumberFormat>,
}

/// Locale-independent numeric tick formatting; precision is decimal places.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NumberFormat {
    pub notation: NumberNotation,
    #[serde(deserialize_with = "deserialize_precision")]
    #[schemars(range(min = 0, max = 12))]
    pub precision: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NumberNotation {
    Scientific,
    Fixed,
}

// JSON Schema integers include integral numeric spellings such as 2.0 and
// 2e0. Preserve the typed u8 API while accepting those equivalent values;
// semantic validation still supplies the precise 0..=12 diagnostic.
fn deserialize_precision<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct PrecisionVisitor;
    impl serde::de::Visitor<'_> for PrecisionVisitor {
        type Value = u8;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("an integer-valued precision number")
        }
        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<u8, E> {
            u8::try_from(value)
                .map_err(|_| E::custom("precision is outside the supported integer range"))
        }
        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<u8, E> {
            u8::try_from(value)
                .map_err(|_| E::custom("precision is outside the supported integer range"))
        }
        fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<u8, E> {
            if value.is_finite() && value.fract() == 0.0 && (0.0..=255.0).contains(&value) {
                Ok(value as u8)
            } else {
                Err(E::custom(
                    "precision must be a finite integer-valued number in the supported integer range",
                ))
            }
        }
    }
    deserializer.deserialize_any(PrecisionVisitor)
}

// Optional means absent, not an explicit null carrying no semantics. This also
// keeps generated schemas aligned with the strict wire reader.
pub(crate) fn deserialize_present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl FieldEncoding {
    pub fn number_format(&self) -> Option<&NumberFormat> {
        self.axis
            .as_ref()
            .and_then(|axis| axis.number_format.as_ref())
    }
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

#[cfg(test)]
mod number_format_tests {
    use super::*;

    #[test]
    fn json_and_yaml_integral_precision_spellings_are_equivalent() {
        for (spelling, expected) in [
            ("0", 0),
            ("0.0", 0),
            ("-0.0", 0),
            ("2", 2),
            ("2.0", 2),
            ("2e0", 2),
            ("12.0", 12),
        ] {
            let json = format!(r#"{{"notation":"fixed","precision":{spelling}}}"#);
            let yaml = format!("notation: fixed\nprecision: {spelling}\n");
            for format in [
                serde_json::from_str::<NumberFormat>(&json).unwrap(),
                serde_yaml::from_str::<NumberFormat>(&yaml).unwrap(),
            ] {
                assert_eq!(format.precision, expected, "{spelling}");
                let canonical = serde_json::to_value(format).unwrap();
                assert_eq!(canonical["precision"].as_u64(), Some(u64::from(expected)));
                assert!(!canonical["precision"].is_f64());
            }
        }
    }

    #[test]
    fn precision_rejects_nonintegral_nonnumeric_and_nonfinite_input() {
        for spelling in [
            "2.5", "-1", "-1.0", "256", "256.0", "true", "false", "null", "\"2\"", "NaN",
            "Infinity",
        ] {
            let json = format!(r#"{{"notation":"fixed","precision":{spelling}}}"#);
            assert!(
                serde_json::from_str::<NumberFormat>(&json).is_err(),
                "{spelling}"
            );
        }
        for spelling in [
            "2.5", "-1.0", "256.0", "true", "null", "\"2\"", ".nan", ".inf", "-.inf",
        ] {
            let yaml = format!("notation: fixed\nprecision: {spelling}\n");
            assert!(
                serde_yaml::from_str::<NumberFormat>(&yaml).is_err(),
                "{spelling}"
            );
        }
    }
}
