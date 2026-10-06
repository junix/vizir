use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn default_version() -> String {
    "0.1".to_owned()
}

pub(crate) fn default_background() -> Color {
    Color::transparent()
}

pub(crate) fn default_point_size() -> f64 {
    7.0
}

pub(crate) fn default_line_width() -> f64 {
    2.5
}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn default_diagram_layout() -> DiagramLayout {
    DiagramLayout::Layered
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, try_from = "DocumentWire")]
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::hir::deserialize_present"
    )]
    pub shared_legend: Option<crate::SharedLegend>,
}

// Decode only the new capability boundary here. General semantic validation
// remains explicit, so legacy generic serde behavior is unchanged.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentWire {
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::hir::deserialize_present"
    )]
    pub shared_legend: Option<crate::SharedLegend>,
}

impl TryFrom<DocumentWire> for Document {
    type Error = crate::VizError;

    fn try_from(wire: DocumentWire) -> Result<Self, Self::Error> {
        let document = Self {
            version: wire.version,
            id: wire.id,
            width: wire.width,
            height: wire.height,
            background: wire.background,
            title: wire.title,
            datasets: wire.datasets,
            views: wire.views,
            shared_legend: wire.shared_legend,
        };
        crate::validate::validate_document_capabilities(&document)
            .map_err(|diagnostics| crate::VizError::validation(&diagnostics))?;
        Ok(document)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
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
    #[serde(rename = "chart.area")]
    Area(AreaChart),
    #[serde(rename = "chart.heatmap")]
    Heatmap(HeatmapChart),
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
            Self::Area(view) => &view.id,
            Self::Heatmap(view) => &view.id,
            Self::Bar(view) => &view.id,
            Self::Diagram(view) => &view.id,
            Self::Geometry(view) => &view.id,
        }
    }

    pub(crate) fn categorical_color(&self) -> Option<(&str, &str, &ColorEncoding)> {
        let (dataset, field, encoding) = match self {
            Self::Scatter(chart) => (&chart.dataset, "color", &chart.color),
            Self::Line(chart) => (&chart.dataset, "series", &chart.series),
            Self::Area(chart) => (&chart.dataset, "series", &chart.series),
            Self::Bar(chart) => (&chart.dataset, "color", &chart.color),
            _ => return None,
        };
        encoding
            .as_ref()
            .map(|encoding| (dataset.as_str(), field, encoding))
    }

    pub(crate) fn numeric_encodings(&self) -> Option<(&str, Vec<(&str, &FieldEncoding)>)> {
        match self {
            Self::Scatter(c) => Some((&c.dataset, vec![("x", &c.x), ("y", &c.y)])),
            Self::Line(c) => Some((&c.dataset, vec![("x", &c.x), ("y", &c.y)])),
            Self::Area(c) => Some((&c.dataset, vec![("x", &c.x), ("y", &c.y)])),
            Self::Bar(c) => Some((&c.dataset, vec![("value", &c.value)])),
            _ => None,
        }
    }

    pub fn frame(&self) -> &Frame {
        match self {
            Self::Scatter(view) => &view.frame,
            Self::Line(view) => &view.frame,
            Self::Area(view) => &view.frame,
            Self::Heatmap(view) => &view.frame,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
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
    #[schemars(with = "AxisOptions")]
    pub axis: Option<AxisOptions>,
    /// Exact ascending numeric bounds in VizHIR 0.7; outliers are rejected.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_numeric_domain"
    )]
    #[schemars(skip)]
    pub domain: Option<[f64; 2]>,
}

/// New schema-only dependency; the published FieldEncoding closure stays frozen.
#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct FieldEncodingV07 {
    pub field: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "AxisOptions")]
    pub axis: Option<AxisOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "[f64; 2]")]
    pub domain: Option<[f64; 2]>,
}

fn deserialize_numeric_domain<'de, D>(deserializer: D) -> Result<Option<[f64; 2]>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values = Vec::<Value>::deserialize(deserializer)?;
    let domain = match values.as_slice() {
        [lo, hi] => [lo.as_f64(), hi.as_f64()],
        _ => {
            return Err(serde::de::Error::custom(
                "numeric domain must contain exactly two numbers",
            ));
        }
    };
    let [Some(lo), Some(hi)] = domain else {
        return Err(serde::de::Error::custom(
            "numeric domain endpoints must be numbers",
        ));
    };
    validate_numeric_domain([lo, hi]).map_err(serde::de::Error::custom)?;
    Ok(Some([lo, hi]))
}

pub(crate) fn validate_numeric_domain([lo, hi]: [f64; 2]) -> Result<(), &'static str> {
    if !lo.is_finite() || !hi.is_finite() || lo >= hi || !(hi - lo).is_finite() {
        return Err(
            "numeric domain requires finite strictly ascending endpoints and a finite nonzero span",
        );
    }
    Ok(())
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ColorEncoding {
    pub field: String,
    #[serde(default)]
    pub palette: Vec<Color>,
    /// Ordered categorical keys, available starting with VizHIR 0.6.
    /// Omission retains the legacy sorted observed-domain behavior.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_color_domain"
    )]
    #[schemars(skip)]
    pub domain: Option<Vec<String>>,
}

// Keep the published ColorEncoding schema closed. Only the new composition
// branch references this schema-only type; there is no alternate runtime IR.
#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ColorEncodingV06 {
    pub field: String,
    #[serde(default)]
    pub palette: Vec<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<String>", length(min = 1, max = 256), extend("uniqueItems" = true))]
    pub domain: Option<Vec<String>>,
}

fn deserialize_color_domain<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Going through Value rejects YAML's implicit number/bool-to-String
    // coercion, keeping the same strict string-key contract as JSON.
    let values = Vec::<Value>::deserialize(deserializer)?;
    let domain = values
        .into_iter()
        .map(|value| match value {
            Value::String(key) => Ok(key),
            _ => Err(serde::de::Error::custom(
                "categorical domain entries must be strings",
            )),
        })
        .collect::<Result<Vec<_>, D::Error>>()?;
    validate_color_domain(&domain).map_err(serde::de::Error::custom)?;
    Ok(Some(domain))
}

pub(crate) fn validate_color_domain(domain: &[String]) -> Result<(), &'static str> {
    if domain.is_empty() || domain.len() > 256 {
        return Err("categorical domain must contain 1..=256 keys");
    }
    let mut keys = std::collections::BTreeSet::new();
    let mut bytes = 0usize;
    for key in domain {
        if key.is_empty()
            || key.len() > 16_384
            || key
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}'))
        {
            return Err(
                "categorical domain keys must contain 1..=16384 UTF-8 bytes without control characters or line separators",
            );
        }
        bytes = bytes
            .checked_add(key.len())
            .ok_or("categorical domain byte count overflow")?;
        if bytes > 1_048_576 {
            return Err("categorical domain exceeds 1048576 UTF-8 bytes");
        }
        if !keys.insert(key) {
            return Err("categorical domain keys must be unique");
        }
    }
    Ok(())
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

/// A linear, unstacked area chart, available starting in VizHIR 0.3.
/// Baseline and order are required authored semantics; styling is fixed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AreaChart {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub frame: Frame,
    pub dataset: String,
    pub x: FieldEncoding,
    pub y: FieldEncoding,
    #[serde(default)]
    pub series: Option<ColorEncoding>,
    pub baseline: f64,
    pub order: AreaOrder,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AreaOrder {
    XAscending,
}

/// An exact, ordered string-category encoding for heatmap axes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CategoryEncoding {
    pub field: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "String")]
    pub label: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "Vec<String>", length(min = 1, max = 256), extend("uniqueItems" = true))]
    pub domain: Option<Vec<String>>,
}

/// A finite numeric encoding with a discrete, equally spaced color palette.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct QuantizeColorEncoding {
    pub field: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "String")]
    pub label: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "[f64; 2]")]
    pub domain: Option<[f64; 2]>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "Vec<Color>", length(min = 2, max = 9))]
    pub palette: Option<Vec<Color>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "NumberFormat")]
    pub number_format: Option<NumberFormat>,
}

/// A sparse rectangular heatmap, available starting in VizHIR 0.4.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HeatmapChart {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub frame: Frame,
    pub dataset: String,
    pub x: CategoryEncoding,
    pub y: CategoryEncoding,
    pub color: QuantizeColorEncoding,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "HeatmapValueLabels")]
    pub value_labels: Option<HeatmapValueLabels>,
}

/// Exact numeric labels for every present heatmap cell, starting in VizHIR 0.5.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HeatmapValueLabels {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "NumberFormat")]
    pub number_format: Option<NumberFormat>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_present"
    )]
    #[schemars(with = "Color")]
    pub color: Option<Color>,
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
