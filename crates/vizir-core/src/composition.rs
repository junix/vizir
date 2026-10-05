//! Versioned frame-free panel composition, resolved to ordinary VizHIR 0.2 through 0.5.
//!
//! A grid allocates equal cells in panel order. It does not scale or clip panel
//! contents, introduce scene groups, or change existing HIR/MIR/Scene contracts.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::hir::{
    default_background, default_diagram_layout, default_line_width, default_point_size,
    default_true,
};
use crate::{
    AreaChart, AreaOrder, BarChart, CategoryEncoding, Color, ColorEncoding, Dataset, Diagnostic,
    DiagramEdge, DiagramGraph, DiagramLayout, DiagramNode, Document, FieldEncoding, Frame,
    GeometryNode, GeometryScene, HeatmapChart, LineChart, QuantizeColorEncoding, ScatterChart,
    View, VizError, VizResult, validate_document,
};

/// Independent source contract; this is not a new HIR or MIR version.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum CompositionSchema {
    #[serde(rename = "vizir-composition/0.1")]
    V1,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields, try_from = "CompositionV1Wire")]
#[schemars(!try_from)]
pub struct CompositionV1 {
    pub schema: CompositionSchema,
    pub id: String,
    #[schemars(extend("exclusiveMinimum" = 0))]
    pub width: f64,
    #[schemars(extend("exclusiveMinimum" = 0))]
    pub height: f64,
    #[serde(default = "default_background")]
    pub background: Color,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub datasets: BTreeMap<String, Dataset>,
    pub layout: PanelLayout,
    #[schemars(length(min = 1))]
    pub panels: Vec<Panel>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompositionV1Wire {
    pub schema: CompositionSchema,
    pub id: String,
    pub width: f64,
    pub height: f64,
    #[serde(default = "default_background")]
    pub background: Color,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub datasets: BTreeMap<String, Dataset>,
    pub layout: PanelLayout,
    pub panels: Vec<Panel>,
}

impl TryFrom<CompositionV1Wire> for CompositionV1 {
    type Error = VizError;

    fn try_from(wire: CompositionV1Wire) -> Result<Self, Self::Error> {
        let composition = Self {
            schema: wire.schema,
            id: wire.id,
            width: wire.width,
            height: wire.height,
            background: wire.background,
            title: wire.title,
            datasets: wire.datasets,
            layout: wire.layout,
            panels: wire.panels,
        };
        check_panel_capabilities(&composition.panels, "0.2")?;
        Ok(composition)
    }
}

/// Versioned composition entry point. The original V1 API remains a 0.1 adapter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields, try_from = "CompositionWire")]
pub struct Composition {
    pub schema: CompositionVersion,
    pub id: String,
    pub width: f64,
    pub height: f64,
    #[serde(default = "default_background")]
    pub background: Color,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub datasets: BTreeMap<String, Dataset>,
    pub layout: PanelLayout,
    pub panels: Vec<Panel>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub enum CompositionVersion {
    #[serde(rename = "vizir-composition/0.1")]
    V1,
    #[serde(rename = "vizir-composition/0.2")]
    V2,
    #[serde(rename = "vizir-composition/0.3")]
    #[schemars(skip)]
    V3,
    #[serde(rename = "vizir-composition/0.4")]
    #[schemars(skip)]
    V4,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompositionWire {
    pub schema: CompositionVersion,
    pub id: String,
    pub width: f64,
    pub height: f64,
    #[serde(default = "default_background")]
    pub background: Color,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub datasets: BTreeMap<String, Dataset>,
    pub layout: PanelLayout,
    pub panels: Vec<Panel>,
}

impl TryFrom<CompositionWire> for Composition {
    type Error = VizError;

    fn try_from(wire: CompositionWire) -> Result<Self, Self::Error> {
        let composition = Self {
            schema: wire.schema,
            id: wire.id,
            width: wire.width,
            height: wire.height,
            background: wire.background,
            title: wire.title,
            datasets: wire.datasets,
            layout: wire.layout,
            panels: wire.panels,
        };
        check_panel_capabilities(
            &composition.panels,
            match composition.schema {
                CompositionVersion::V1 => "0.2",
                CompositionVersion::V2 => "0.3",
                CompositionVersion::V3 => "0.4",
                CompositionVersion::V4 => "0.5",
            },
        )?;
        Ok(composition)
    }
}

// Borrow shared payloads so the legacy adapter does not add a second clone of
// potentially large datasets before grid validation and HIR construction.
struct CompositionInput<'a> {
    hir_version: &'static str,
    id: &'a str,
    width: f64,
    height: f64,
    background: &'a Color,
    title: &'a Option<String>,
    datasets: &'a BTreeMap<String, Dataset>,
    layout: PanelLayout,
    panels: &'a [Panel],
}

fn check_panel_capabilities(panels: &[Panel], hir_version: &str) -> VizResult<()> {
    let diagnostics: Vec<_> = panels
        .iter()
        .enumerate()
        .filter_map(|(index, panel)| {
            let message = match panel {
                Panel::Area(_) if !matches!(hir_version, "0.3" | "0.4" | "0.5") => {
                    "chart.area requires composition schema vizir-composition/0.2, /0.3, or /0.4"
                }
                Panel::Heatmap(_) if !matches!(hir_version, "0.4" | "0.5") => {
                    "chart.heatmap requires composition schema vizir-composition/0.3 or /0.4"
                }
                Panel::Heatmap(chart) if chart.value_labels.is_some() && hir_version != "0.5" => {
                    "heatmap value_labels requires composition schema vizir-composition/0.4"
                }
                _ => return None,
            };
            Some(Diagnostic::new("VIZ-COMPOSE-0005", message).at(format!("panels[{index}]")))
        })
        .collect();
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(VizError::validation(&diagnostics))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PanelLayout {
    Grid {
        #[serde(deserialize_with = "deserialize_columns")]
        #[schemars(range(min = 1, max = 4294967295_u64))]
        columns: u32,
        #[serde(default)]
        #[schemars(range(min = 0))]
        gap: f64,
        #[serde(default)]
        #[schemars(range(min = 0))]
        padding: f64,
    },
}

/// Panel payloads deliberately have no frame. An authored frame is an error,
/// rather than an ignored value or an ambiguous override of the grid.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Panel {
    #[serde(rename = "chart.scatter")]
    Scatter(ScatterPanel),
    #[serde(rename = "chart.line")]
    Line(LinePanel),
    #[serde(rename = "chart.area")]
    #[schemars(skip)]
    Area(AreaPanel),
    #[serde(rename = "chart.heatmap")]
    #[schemars(skip)]
    Heatmap(HeatmapPanel),
    #[serde(rename = "chart.bar")]
    Bar(BarPanel),
    #[serde(rename = "diagram.graph")]
    Diagram(DiagramPanel),
    #[serde(rename = "geometry.scene")]
    Geometry(GeometryPanel),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScatterPanel {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub dataset: String,
    pub x: FieldEncoding,
    pub y: FieldEncoding,
    #[serde(default)]
    pub color: Option<ColorEncoding>,
    #[serde(default = "default_point_size")]
    pub point_size: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LinePanel {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AreaPanel {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub dataset: String,
    pub x: FieldEncoding,
    pub y: FieldEncoding,
    #[serde(default)]
    pub series: Option<ColorEncoding>,
    pub baseline: f64,
    pub order: AreaOrder,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "HeatmapPanelV04")]
pub struct HeatmapPanel {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub dataset: String,
    pub x: CategoryEncoding,
    pub y: CategoryEncoding,
    pub color: QuantizeColorEncoding,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::hir::deserialize_present"
    )]
    #[schemars(with = "crate::HeatmapValueLabels")]
    pub value_labels: Option<crate::HeatmapValueLabels>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BarPanel {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub dataset: String,
    pub category: FieldEncoding,
    pub value: FieldEncoding,
    #[serde(default)]
    pub color: Option<ColorEncoding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DiagramPanel {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default = "default_diagram_layout")]
    pub layout: DiagramLayout,
    pub nodes: Vec<DiagramNode>,
    #[serde(default)]
    pub edges: Vec<DiagramEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GeometryPanel {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub children: Vec<GeometryNode>,
}

impl Panel {
    fn with_frame(&self, frame: Frame) -> View {
        match self {
            Self::Scatter(panel) => View::Scatter(ScatterChart {
                id: panel.id.clone(),
                title: panel.title.clone(),
                frame,
                dataset: panel.dataset.clone(),
                x: panel.x.clone(),
                y: panel.y.clone(),
                color: panel.color.clone(),
                point_size: panel.point_size,
            }),
            Self::Line(panel) => View::Line(LineChart {
                id: panel.id.clone(),
                title: panel.title.clone(),
                frame,
                dataset: panel.dataset.clone(),
                x: panel.x.clone(),
                y: panel.y.clone(),
                series: panel.series.clone(),
                line_width: panel.line_width,
                show_points: panel.show_points,
            }),
            Self::Area(panel) => View::Area(AreaChart {
                id: panel.id.clone(),
                title: panel.title.clone(),
                frame,
                dataset: panel.dataset.clone(),
                x: panel.x.clone(),
                y: panel.y.clone(),
                series: panel.series.clone(),
                baseline: panel.baseline,
                order: panel.order,
            }),
            Self::Heatmap(panel) => View::Heatmap(HeatmapChart {
                id: panel.id.clone(),
                title: panel.title.clone(),
                frame,
                dataset: panel.dataset.clone(),
                x: panel.x.clone(),
                y: panel.y.clone(),
                color: panel.color.clone(),
                value_labels: panel.value_labels.clone(),
            }),
            Self::Bar(panel) => View::Bar(BarChart {
                id: panel.id.clone(),
                title: panel.title.clone(),
                frame,
                dataset: panel.dataset.clone(),
                category: panel.category.clone(),
                value: panel.value.clone(),
                color: panel.color.clone(),
            }),
            Self::Diagram(panel) => View::Diagram(DiagramGraph {
                id: panel.id.clone(),
                title: panel.title.clone(),
                frame,
                layout: panel.layout.clone(),
                nodes: panel.nodes.clone(),
                edges: panel.edges.clone(),
            }),
            Self::Geometry(panel) => View::Geometry(GeometryScene {
                id: panel.id.clone(),
                title: panel.title.clone(),
                frame,
                children: panel.children.clone(),
            }),
        }
    }
}

/// Read a composition as JSON (`.json`) or YAML. Use `compose` to validate it and
/// resolve its panel frames; existing `parse_document` remains HIR-only.
pub fn parse_composition(path: impl AsRef<Path>) -> VizResult<CompositionV1> {
    read_composition(path)
}

/// Read either supported source version; `compose_versioned` resolves it.
pub fn parse_versioned_composition(path: impl AsRef<Path>) -> VizResult<Composition> {
    read_composition(path)
}

fn read_composition<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> VizResult<T> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|source| VizError::Read {
        path: path.display().to_string(),
        source,
    })?;
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("json"))
    {
        Ok(serde_json::from_str(&source)?)
    } else {
        Ok(serde_yaml::from_str(&source)?)
    }
}

/// Resolve a grid into a fully validated, ordinary VizHIR 0.2 document.
///
/// Only frames are assigned. Geometry children and manual diagram positions
/// remain local; normal lowering applies their view translation exactly once.
/// Downstream MIR correctly records `source_hir_version: "0.2"`.
pub fn compose(composition: &CompositionV1) -> VizResult<Document> {
    compose_grid(CompositionInput {
        hir_version: "0.2",
        id: &composition.id,
        width: composition.width,
        height: composition.height,
        background: &composition.background,
        title: &composition.title,
        datasets: &composition.datasets,
        layout: composition.layout,
        panels: &composition.panels,
    })
}

/// Resolve composition 0.1/0.2/0.3/0.4 to HIR 0.2/0.3/0.4/0.5 using the same grid.
pub fn compose_versioned(composition: &Composition) -> VizResult<Document> {
    compose_grid(CompositionInput {
        hir_version: match composition.schema {
            CompositionVersion::V1 => "0.2",
            CompositionVersion::V2 => "0.3",
            CompositionVersion::V3 => "0.4",
            CompositionVersion::V4 => "0.5",
        },
        id: &composition.id,
        width: composition.width,
        height: composition.height,
        background: &composition.background,
        title: &composition.title,
        datasets: &composition.datasets,
        layout: composition.layout,
        panels: &composition.panels,
    })
}

fn compose_grid(composition: CompositionInput<'_>) -> VizResult<Document> {
    check_panel_capabilities(composition.panels, composition.hir_version)?;
    let mut heatmap_cells = 0usize;
    let mut value_labels = 0usize;
    for panel in composition.panels {
        if let Panel::Heatmap(panel) = panel {
            crate::validate::heatmap::preflight_source(
                composition.datasets.get(&panel.dataset),
                &panel.x,
                &panel.y,
                &panel.color,
                &mut heatmap_cells,
                panel.value_labels.as_ref(),
                &mut value_labels,
            )
            .map_err(|diagnostic| VizError::validation(&[diagnostic]))?;
        }
    }
    let PanelLayout::Grid {
        columns,
        gap,
        padding,
    } = composition.layout;
    let mut diagnostics = Vec::new();
    for (value, source, positive) in [
        (composition.width, "width", true),
        (composition.height, "height", true),
        (gap, "layout.gap", false),
        (padding, "layout.padding", false),
    ] {
        if !value.is_finite() || if positive { value <= 0.0 } else { value < 0.0 } {
            diagnostics.push(
                Diagnostic::new(
                    "VIZ-COMPOSE-0001",
                    if positive {
                        "must be finite and positive"
                    } else {
                        "must be finite and nonnegative"
                    },
                )
                .at(source),
            );
        }
    }
    let count = composition.panels.len();
    if count == 0 {
        diagnostics
            .push(Diagnostic::new("VIZ-COMPOSE-0002", "composition has no panels").at("panels"));
    }
    if columns == 0 || u64::from(columns) > count as u64 {
        diagnostics.push(
            Diagnostic::new(
                "VIZ-COMPOSE-0003",
                "grid columns must be between one and the number of panels",
            )
            .at("layout.columns"),
        );
    }
    if !diagnostics.is_empty() {
        return Err(VizError::validation(&diagnostics));
    }
    let columns = columns as usize;
    // Avoid overflow in the familiar (count + columns - 1) / columns formula.
    let rows = 1 + (count - 1) / columns;
    let horizontal = tracks(composition.width, columns, gap, padding, "width")?;
    let vertical = tracks(composition.height, rows, gap, padding, "height")?;
    let views = composition
        .panels
        .iter()
        .enumerate()
        .map(|(index, panel)| {
            let (x, width) = horizontal[index % columns];
            let (y, height) = vertical[index / columns];
            panel.with_frame(Frame {
                x,
                y,
                width,
                height,
            })
        })
        .collect();
    let document = Document {
        version: composition.hir_version.to_owned(),
        id: composition.id.to_owned(),
        width: composition.width,
        height: composition.height,
        background: composition.background.clone(),
        title: composition.title.clone(),
        datasets: composition.datasets.clone(),
        views,
    };
    validate_document(&document).map_err(|mut diagnostics| {
        for diagnostic in &mut diagnostics {
            if let Some(source) = &mut diagnostic.source
                && let Some(suffix) = source.strip_prefix("views")
            {
                *source = format!("panels{suffix}");
            }
        }
        VizError::validation(&diagnostics)
    })?;
    Ok(document)
}

fn tracks(
    extent: f64,
    count: usize,
    gap: f64,
    padding: f64,
    dimension: &str,
) -> VizResult<Vec<(f64, f64)>> {
    let fail = || {
        VizError::validation(&[Diagnostic::new(
            "VIZ-COMPOSE-0004",
            format!(
                "grid cannot allocate {count} positive, representable {dimension} tracks inside the padded canvas"
            ),
        )
        .at("layout")
        .with_help("enlarge the canvas or reduce columns, panels, gap, or padding")])
    };
    let limit = extent - padding;
    let available = extent - padding - padding - (count - 1) as f64 * gap;
    let cell = available / count as f64;
    if !available.is_finite()
        || !cell.is_finite()
        || cell <= 0.0
        || (padding > 0.0 && limit >= extent)
    {
        return Err(fail());
    }
    let mut result = Vec::with_capacity(count);
    let mut previous_end = padding;
    for index in 0..count {
        let start = padding + index as f64 * cell + index as f64 * gap;
        // Anchor the final edge to the padded boundary. Cells are equal in real
        // arithmetic; their stored spans can differ by floating-point rounding.
        let end = if index + 1 == count {
            limit
        } else {
            padding + (index + 1) as f64 * cell + index as f64 * gap
        };
        let span = end - start;
        if !start.is_finite()
            || !end.is_finite()
            || !span.is_finite()
            || span <= 0.0
            || start < padding
            || start < previous_end
            || (index > 0 && gap > 0.0 && start <= previous_end)
            || end > limit
            || start + span > limit
            || start + span <= start
        {
            return Err(fail());
        }
        result.push((start, span));
        previous_end = end;
    }
    Ok(result)
}

// JSON Schema integers include integer-valued numbers such as 2.0 and 2e0.
// Accept those spellings as well as ordinary u32 values, without truncation.
fn deserialize_columns<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct ColumnsVisitor;
    impl serde::de::Visitor<'_> for ColumnsVisitor {
        type Value = u32;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("an integer-valued columns number in the u32 range")
        }
        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<u32, E> {
            u32::try_from(value).map_err(E::custom)
        }
        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<u32, E> {
            u32::try_from(value).map_err(E::custom)
        }
        fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<u32, E> {
            if value.is_finite() && value.fract() == 0.0 && (0.0..=u32::MAX as f64).contains(&value)
            {
                Ok(value as u32)
            } else {
                Err(E::custom(
                    "columns must be an integer-valued number in the u32 range",
                ))
            }
        }
    }
    deserializer.deserialize_any(ColumnsVisitor)
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "HeatmapPanel")]
struct HeatmapPanelV03 {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    pub dataset: String,
    pub x: CategoryEncoding,
    pub y: CategoryEncoding,
    pub color: QuantizeColorEncoding,
}

// Schema-only version paths prevent the new mark from broadening old sources.
#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "kind", deny_unknown_fields)]
enum PanelV02 {
    #[serde(rename = "chart.scatter")]
    Scatter(ScatterPanel),
    #[serde(rename = "chart.line")]
    Line(LinePanel),
    #[serde(rename = "chart.area")]
    Area(AreaPanel),
    #[serde(rename = "chart.bar")]
    Bar(BarPanel),
    #[serde(rename = "diagram.graph")]
    Diagram(DiagramPanel),
    #[serde(rename = "geometry.scene")]
    Geometry(GeometryPanel),
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CompositionV2", transform = composition_v2_schema)]
struct CompositionV2Schema {
    pub schema: CompositionVersion,
    pub id: String,
    #[schemars(extend("exclusiveMinimum" = 0))]
    pub width: f64,
    #[schemars(extend("exclusiveMinimum" = 0))]
    pub height: f64,
    #[serde(default = "default_background")]
    pub background: Color,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub datasets: BTreeMap<String, Dataset>,
    pub layout: PanelLayout,
    #[schemars(length(min = 1))]
    pub panels: Vec<PanelV02>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "kind", deny_unknown_fields)]
enum PanelV03 {
    #[serde(rename = "chart.scatter")]
    Scatter(ScatterPanel),
    #[serde(rename = "chart.line")]
    Line(LinePanel),
    #[serde(rename = "chart.area")]
    Area(AreaPanel),
    #[serde(rename = "chart.heatmap")]
    Heatmap(HeatmapPanelV03),
    #[serde(rename = "chart.bar")]
    Bar(BarPanel),
    #[serde(rename = "diagram.graph")]
    Diagram(DiagramPanel),
    #[serde(rename = "geometry.scene")]
    Geometry(GeometryPanel),
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CompositionV3", transform = composition_v3_schema)]
struct CompositionV3Schema {
    pub schema: CompositionVersion,
    pub id: String,
    #[schemars(extend("exclusiveMinimum" = 0))]
    pub width: f64,
    #[schemars(extend("exclusiveMinimum" = 0))]
    pub height: f64,
    #[serde(default = "default_background")]
    pub background: Color,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub datasets: BTreeMap<String, Dataset>,
    pub layout: PanelLayout,
    #[schemars(length(min = 1))]
    pub panels: Vec<PanelV03>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "kind", deny_unknown_fields)]
enum PanelV04 {
    #[serde(rename = "chart.scatter")]
    Scatter(ScatterPanel),
    #[serde(rename = "chart.line")]
    Line(LinePanel),
    #[serde(rename = "chart.area")]
    Area(AreaPanel),
    #[serde(rename = "chart.heatmap")]
    Heatmap(HeatmapPanel),
    #[serde(rename = "chart.bar")]
    Bar(BarPanel),
    #[serde(rename = "diagram.graph")]
    Diagram(DiagramPanel),
    #[serde(rename = "geometry.scene")]
    Geometry(GeometryPanel),
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CompositionV4", transform = composition_v4_schema)]
struct CompositionV4Schema {
    pub schema: CompositionVersion,
    pub id: String,
    #[schemars(extend("exclusiveMinimum" = 0))]
    pub width: f64,
    #[schemars(extend("exclusiveMinimum" = 0))]
    pub height: f64,
    #[serde(default = "default_background")]
    pub background: Color,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub datasets: BTreeMap<String, Dataset>,
    pub layout: PanelLayout,
    #[schemars(length(min = 1))]
    pub panels: Vec<PanelV04>,
}

fn composition_v2_schema(schema: &mut schemars::Schema) {
    schema
        .as_object_mut()
        .expect("composition schema object")
        .get_mut("properties")
        .expect("composition properties")["schema"] =
        serde_json::json!({"type": "string", "const": "vizir-composition/0.2"});
}

fn composition_v3_schema(schema: &mut schemars::Schema) {
    schema
        .as_object_mut()
        .expect("composition schema object")
        .get_mut("properties")
        .expect("composition properties")["schema"] =
        serde_json::json!({"type": "string", "const": "vizir-composition/0.3"});
}

fn composition_v4_schema(schema: &mut schemars::Schema) {
    schema
        .as_object_mut()
        .expect("composition schema object")
        .get_mut("properties")
        .expect("composition properties")["schema"] =
        serde_json::json!({"type": "string", "const": "vizir-composition/0.4"});
}

impl JsonSchema for Composition {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Composition".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let legacy = generator.subschema_for::<CompositionV1>();
        let current = generator.subschema_for::<CompositionV2Schema>();
        let heatmap = generator.subschema_for::<CompositionV3Schema>();
        let labels = generator.subschema_for::<CompositionV4Schema>();
        schemars::json_schema!({"oneOf": [legacy, current, heatmap, labels]})
    }
}

/// Generated structural schema. Cross-field layout feasibility and existing
/// HIR semantics are checked by `compose`, rather than promised by JSON Schema.
pub fn composition_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Composition))
        .expect("composition schema must serialize")
}
