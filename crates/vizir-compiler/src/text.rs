//! Explicit, bounded exact-face font shaping and font-independent outlines.
//! Resource bytes are supplied by the caller; this module never opens files.
use cosmic_text::{
    Attrs, Buffer, Fallback, Family, FontSystem, Hinting, Metrics, Shaping, Weight, Wrap, fontdb,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
    outline::{DrawSettings, OutlinePen},
    raw::types::GlyphId,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use vizir_core::{
    Color, FontWeight, LossRecord, LoweringFidelity, PathCommand, Point, Rect, ResolvedStyle,
    Scene2D, SceneNode, TextAnchor, Transform2D, VizError, VizResult,
};

use crate::text_layout::{
    self, DiagramTextLayoutTarget, SemanticTextLayoutTarget, TextLayoutContext, TextLayoutRole,
    TextLayoutTarget,
};

pub const TEXT_PROFILE: &str = "vizir-text-outlines/1";
pub const TEXT_ENGINE: &str = "cosmic-text/0.19.0;harfrust/0.5.2;skrifa/0.40.0";
pub const TEXT_MAX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
pub const TEXT_MAX_FONT_BYTES: usize = 32 * 1024 * 1024;
pub const TEXT_MAX_FONT_TOTAL_BYTES: usize = 128 * 1024 * 1024;
const MAX_COORDINATE: f64 = 1_000_000.0;
const MIN_SIZE: f64 = 0.25;
const MAX_SIZE: f64 = 4096.0;

fn error(code: &str, detail: impl std::fmt::Display) -> VizError {
    VizError::Diagnostic(format!("VIZ-TEXT-{code}: {detail}"))
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn valid_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn valid_locale(s: &str) -> bool {
    let mut parts = s.split('-');
    let first = parts.next().unwrap_or_default();
    (2..=8).contains(&first.len())
        && first.bytes().all(|b| b.is_ascii_alphabetic())
        && parts.all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FontFace {
    pub sha256: String,
    #[serde(deserialize_with = "deserialize_u32_number")]
    pub face_index: u32,
    #[serde(deserialize_with = "deserialize_weight")]
    pub weight: u16,
}
impl FontFace {
    pub fn new(sha256: impl Into<String>, face_index: u32, weight: u16) -> Self {
        Self {
            sha256: sha256.into(),
            face_index,
            weight,
        }
    }
}
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TextFaces {
    pub regular: FontFace,
    pub medium: FontFace,
    pub bold: FontFace,
}
impl TextFaces {
    pub fn new(regular: FontFace, medium: FontFace, bold: FontFace) -> Self {
        Self {
            regular,
            medium,
            bold,
        }
    }
    fn roles(&self) -> [(FontWeight, &FontFace); 3] {
        [
            (FontWeight::Regular, &self.regular),
            (FontWeight::Medium, &self.medium),
            (FontWeight::Bold, &self.bold),
        ]
    }
}
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TextContext {
    pub profile: String,
    pub engine: String,
    pub declared_locale: String,
    pub shaping_language: String,
    pub faces: TextFaces,
}
impl TextContext {
    pub fn new(declared_locale: impl Into<String>, faces: TextFaces) -> Self {
        Self {
            profile: TEXT_PROFILE.into(),
            engine: TEXT_ENGINE.into(),
            declared_locale: declared_locale.into(),
            shaping_language: "default".into(),
            faces,
        }
    }
    pub fn validate(&self) -> VizResult<()> {
        if self.profile != TEXT_PROFILE
            || self.engine != TEXT_ENGINE
            || self.shaping_language != "default"
        {
            return Err(error(
                "0001",
                "unsupported text profile, shaping engine pin or active shaping language; only fixed default-language shaping is supported",
            ));
        }
        if self.declared_locale.len() > 32 || !valid_locale(&self.declared_locale) {
            return Err(error(
                "0001",
                "locale must be a well-formed explicit language tag of at most 32 bytes",
            ));
        }
        for (role, face) in self.faces.roles() {
            if !valid_hash(&face.sha256) || face.face_index >= 32 || face.weight != weight(role) {
                return Err(error(
                    "0001",
                    format!(
                        "invalid {role:?} face: require lowercase SHA256, index below 32 and exact weight {}",
                        weight(role)
                    ),
                ));
            }
        }
        Ok(())
    }
}
/// Owned immutable bytes, keyed by verified SHA256. No filesystem or network IO.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct FontResources {
    fonts: BTreeMap<String, Arc<Vec<u8>>>,
    bytes: usize,
}
impl FontResources {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }
    pub fn len(&self) -> usize {
        self.fonts.len()
    }
    pub fn insert(&mut self, expected_sha256: &str, bytes: Vec<u8>) -> VizResult<()> {
        if !valid_hash(expected_sha256) {
            return Err(error(
                "0002",
                "font resource key must be a lowercase SHA256",
            ));
        }
        if bytes.len() > TEXT_MAX_FONT_BYTES
            || self.fonts.len() >= 16
            || self
                .bytes
                .checked_add(bytes.len())
                .is_none_or(|n| n > TEXT_MAX_FONT_TOTAL_BYTES)
        {
            return Err(error("0003", "font resource byte/count limit exceeded"));
        }
        if self.fonts.contains_key(expected_sha256) {
            return Err(error("0002", "duplicate font resource SHA256"));
        }
        if digest(&bytes) != expected_sha256 {
            return Err(error(
                "0002",
                format!("font bytes do not match SHA256 {expected_sha256}"),
            ));
        }
        self.bytes += bytes.len();
        self.fonts.insert(expected_sha256.into(), Arc::new(bytes));
        Ok(())
    }
}
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextLimits {
    pub max_label_bytes: usize,
    pub max_text_bytes: usize,
    pub max_labels: usize,
    pub max_glyphs: usize,
    pub max_outline_commands: usize,
    pub max_cache_bytes: usize,
    pub max_collision_checks: usize,
    pub max_output_bytes: usize,
    pub max_layout_lines: usize,
    pub max_wrap_candidates: usize,
}
impl Default for TextLimits {
    fn default() -> Self {
        Self {
            max_label_bytes: 16 * 1024,
            max_text_bytes: 1024 * 1024,
            max_labels: 4096,
            max_glyphs: 65536,
            max_outline_commands: 1_000_000,
            max_cache_bytes: 16 * 1024 * 1024,
            max_collision_checks: 1_000_000,
            max_output_bytes: TEXT_MAX_OUTPUT_BYTES,
            max_layout_lines: 4096,
            max_wrap_candidates: 4096,
        }
    }
}
impl TextLimits {
    pub fn new() -> Self {
        Self::default()
    }
    fn bounded(self) -> Self {
        let d = Self::default();
        Self {
            max_label_bytes: self.max_label_bytes.min(d.max_label_bytes),
            max_text_bytes: self.max_text_bytes.min(d.max_text_bytes),
            max_labels: self.max_labels.min(d.max_labels),
            max_glyphs: self.max_glyphs.min(d.max_glyphs),
            max_outline_commands: self.max_outline_commands.min(d.max_outline_commands),
            max_cache_bytes: self.max_cache_bytes.min(d.max_cache_bytes),
            max_collision_checks: self.max_collision_checks.min(d.max_collision_checks),
            max_output_bytes: self.max_output_bytes.min(d.max_output_bytes),
            max_layout_lines: self.max_layout_lines.min(d.max_layout_lines),
            max_wrap_candidates: self.max_wrap_candidates.min(d.max_wrap_candidates),
        }
    }
}
fn weight(w: FontWeight) -> u16 {
    match w {
        FontWeight::Regular => 400,
        FontWeight::Medium => 500,
        FontWeight::Bold => 700,
    }
}
struct NoFallback;
impl Fallback for NoFallback {
    fn common_fallback(&self) -> &[&'static str] {
        &[]
    }
    fn forbidden_fallback(&self) -> &[&'static str] {
        &[]
    }
    fn script_fallback(&self, _: unicode_script::Script, _: &str) -> &[&'static str] {
        &[]
    }
}
#[derive(Debug)]
struct Run {
    advance: f64,
    bounds: Rect,
    commands: Vec<PathCommand>,
    glyphs: usize,
    coverage_complete: bool,
}
impl Run {
    fn width(&self) -> f64 {
        // Symmetric advance envelope also contains negative bearings.
        let center = self.advance / 2.;
        2. * center
            .max(center - self.bounds.x)
            .max(self.bounds.x + self.bounds.width - center)
    }
}
struct RawWrappedLine {
    source: std::ops::Range<usize>,
    separator: std::ops::Range<usize>,
    offset: f64,
    run: Arc<Run>,
}
struct WrappedLine {
    source: std::ops::Range<usize>,
    separator: std::ops::Range<usize>,
    baseline: f64,
    run: Arc<Run>,
    commands: Vec<PathCommand>,
    bounds: Rect,
    logical: Rect,
}
struct WrappedOutline {
    commands: Vec<PathCommand>,
    bounds: Rect,
    logical: Rect,
    details: String,
    glyphs: usize,
}
fn union_rect(previous: Option<Rect>, next: Rect) -> Rect {
    if let Some(p) = previous {
        let x = p.x.min(next.x);
        let y = p.y.min(next.y);
        Rect {
            x,
            y,
            width: (p.x + p.width).max(next.x + next.width) - x,
            height: (p.y + p.height).max(next.y + next.height) - y,
        }
    } else {
        next
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TitlePlanKey {
    view: String,
    source: String,
    text_identity: String,
    layout_profile: String,
    dimensions: [u64; 7],
    max_lines: u32,
    weight: u16,
}
struct TitlePlan {
    block: WrappedOutline,
    losses: Vec<LossRecord>,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CategoryPlanKey {
    role: TextLayoutRole,
    view: String,
    sources: Vec<String>,
    axis_title: String,
    text_identity: String,
    layout_profile: String,
    dimensions: [u64; 10],
    max_lines: u32,
}
struct CategoryBlock {
    source: String,
    raw_lines: Vec<RawWrappedLine>,
    outline: WrappedOutline,
    position: Point,
    cell: [f64; 2],
    losses: Vec<LossRecord>,
}
struct CategoryPlan {
    role: TextLayoutRole,
    blocks: Vec<CategoryBlock>,
    plot_bottom: f64,
}
pub(crate) struct CategoryAllocation {
    pub plot_bottom: f64,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DiagramPlanKey {
    view: String,
    node: String,
    source: String,
    text_identity: String,
    layout_profile: String,
    dimensions: [u64; 8],
    max_lines: u32,
}
struct DiagramPlan {
    source: String,
    raw_lines: Vec<RawWrappedLine>,
    block: WrappedOutline,
    position: Point,
    interior: Rect,
    losses: Vec<LossRecord>,
}
/// The serialized advance endpoints lie on the 1e-4 integer grid. Subtract
/// integers before scaling so a translated exact 60-unit span stays exactly 60.
/// Analytic cubic ink extrema are NOT rounded: retain every real overhang.
/// This mode is selected only by the new heatmap semantic role.
fn serialized_advance_envelope_width(left: f64, right: f64, ink: Option<Rect>) -> f64 {
    let width =
        ((right * 10_000.).round() as i64 - (left * 10_000.).round() as i64) as f64 / 10_000.;
    if let Some(ink) = ink {
        width + (left - ink.x).max(0.) + (ink.x + ink.width - right).max(0.)
    } else {
        width
    }
}

struct State {
    system: FontSystem,
    selected: BTreeMap<u16, (fontdb::ID, String, u16)>,
    cache: BTreeMap<(String, u64, u16), Arc<Run>>,
    text_bytes: usize,
    labels: usize,
    glyphs: usize,
    commands: usize,
    cache_bytes: usize,
    emitted_glyphs: usize,
    emitted_commands: usize,
    collision_checks: usize,
    layout_lines: usize,
    wrap_candidates: usize,
    used_targets: BTreeSet<(String, String)>,
    title_plans: BTreeMap<TitlePlanKey, Arc<TitlePlan>>,
    title_nodes: BTreeMap<String, (String, Arc<TitlePlan>)>,
    used_title_targets: BTreeSet<String>,
    category_plans: BTreeMap<CategoryPlanKey, Arc<CategoryPlan>>,
    category_views: BTreeMap<String, Arc<CategoryPlan>>,
    category_nodes: BTreeMap<String, (String, usize, Arc<CategoryPlan>)>,
    used_categories: BTreeMap<String, BTreeSet<usize>>,
    diagram_plans: BTreeMap<DiagramPlanKey, Arc<DiagramPlan>>,
    diagram_nodes: BTreeMap<String, (String, String, Arc<DiagramPlan>)>,
    used_diagram_targets: BTreeSet<(String, String)>,
}
pub(crate) struct TextSession {
    limits: TextLimits,
    state: RefCell<State>,
    targets: BTreeMap<String, BTreeMap<String, TextLayoutTarget>>,
    face_metrics: BTreeMap<u16, (f64, f64)>,
    title_targets: BTreeMap<String, SemanticTextLayoutTarget>,
    category_targets: BTreeMap<String, SemanticTextLayoutTarget>,
    diagram_targets: BTreeMap<String, BTreeMap<String, DiagramTextLayoutTarget>>,
    text_identity: String,
    layout_profile: String,
}
impl TextSession {
    pub(crate) fn new_with_layout(
        context: &TextContext,
        resources: &FontResources,
        limits: TextLimits,
        layout: Option<&TextLayoutContext>,
    ) -> VizResult<Self> {
        context.validate()?;
        let mut diagram_targets: BTreeMap<String, BTreeMap<String, DiagramTextLayoutTarget>> =
            BTreeMap::new();
        let mut title_targets = BTreeMap::new();
        let mut category_targets = BTreeMap::new();
        let mut targets: BTreeMap<String, BTreeMap<String, TextLayoutTarget>> = BTreeMap::new();
        if let Some(layout) = layout {
            layout.validate()?;
            if let Some(semantic) = &layout.semantic_targets {
                for t in semantic {
                    match t.role {
                        TextLayoutRole::ChartTitle => {
                            title_targets.insert(t.view_id.clone(), t.clone());
                        }
                        TextLayoutRole::BarCategoryLabels
                        | TextLayoutRole::HeatmapXCategoryLabels => {
                            if category_targets
                                .insert(t.view_id.clone(), t.clone())
                                .is_some()
                            {
                                return Err(text_layout::error(
                                    "one view cannot target both bar and heatmap categories",
                                ));
                            }
                        }
                    }
                }
            }
            for t in layout.diagram_targets.as_deref().unwrap_or_default() {
                diagram_targets
                    .entry(t.view_id.clone())
                    .or_default()
                    .insert(t.node_id.clone(), t.clone());
            }
            for t in &layout.targets {
                targets
                    .entry(t.view_id.clone())
                    .or_default()
                    .insert(t.node_id.clone(), t.clone());
            }
        }
        let mut face_metrics = BTreeMap::new();
        let mut db = fontdb::Database::new();
        let mut loaded = BTreeMap::new();
        let mut selected = BTreeMap::new();
        for (role, resource) in context.faces.roles() {
            let bytes = resources.fonts.get(&resource.sha256).ok_or_else(|| {
                error(
                    "0002",
                    format!(
                        "missing font resource {}; supply its exact bytes",
                        resource.sha256
                    ),
                )
            })?;
            if bytes.starts_with(b"ttcf")
                && (bytes.len() < 12
                    || u32::from_be_bytes(bytes[8..12].try_into().expect("four bytes")) > 32)
            {
                return Err(error("0003", "font collection exceeds 32 faces"));
            }
            let glyph_count = validate_font_structure(bytes, resource.face_index)?;
            let raw = FontRef::from_index(bytes.as_slice(), resource.face_index)
                .map_err(|_| error("0002", "invalid font or face index"))?;
            let metrics = raw.metrics(Size::unscaled(), LocationRef::default());
            face_metrics.insert(
                weight(role),
                (
                    f64::from(metrics.ascent) / f64::from(metrics.units_per_em),
                    f64::from(metrics.descent) / f64::from(metrics.units_per_em),
                ),
            );
            if !raw.axes().is_empty() {
                return Err(error(
                    "0002",
                    "variable fonts are outside the static exact-face profile",
                ));
            }
            if !loaded.contains_key(&resource.sha256) {
                let before: BTreeSet<_> = db.faces().map(|f| f.id).collect();
                db.load_font_source(fontdb::Source::Binary(bytes.clone()));
                let ids: Vec<_> = db
                    .faces()
                    .filter(|f| !before.contains(&f.id))
                    .map(|f| (f.index, f.id))
                    .collect();
                loaded.insert(resource.sha256.clone(), ids);
            }
            let id = loaded[&resource.sha256]
                .iter()
                .find(|(i, _)| *i == resource.face_index)
                .map(|(_, id)| *id)
                .ok_or_else(|| error("0002", "font database could not load the requested face"))?;
            let info = db.face(id).expect("loaded face");
            if info.weight.0 != weight(role)
                || info.style != fontdb::Style::Normal
                || info.stretch != fontdb::Stretch::Normal
            {
                return Err(error(
                    "0002",
                    format!(
                        "{role:?} needs a normal static face of weight {}; actual weight {}",
                        weight(role),
                        info.weight.0
                    ),
                ));
            }
            let family = info
                .families
                .first()
                .map(|f| f.0.clone())
                .ok_or_else(|| error("0002", "font has no family name"))?;
            selected.insert(weight(role), (id, family, glyph_count));
        }
        let keep: BTreeSet<_> = selected.values().map(|(id, _, _)| *id).collect();
        let remove: Vec<_> = db
            .faces()
            .filter(|f| !keep.contains(&f.id))
            .map(|f| f.id)
            .collect();
        for id in remove {
            db.remove_face(id);
        }
        let system = FontSystem::new_with_locale_and_db_and_fallback(
            context.declared_locale.clone(),
            db,
            NoFallback,
        );
        Ok(Self {
            limits: limits.bounded(),
            targets,
            face_metrics,
            title_targets,
            category_targets,
            diagram_targets,
            text_identity: digest(&serde_json::to_vec(context)?),
            layout_profile: layout.map(|l| l.profile.clone()).unwrap_or_default(),
            state: RefCell::new(State {
                system,
                selected,
                cache: BTreeMap::new(),
                text_bytes: 0,
                labels: 0,
                glyphs: 0,
                commands: 0,
                cache_bytes: 0,
                emitted_glyphs: 0,
                emitted_commands: 0,
                collision_checks: 0,
                layout_lines: 0,
                wrap_candidates: 0,
                used_targets: BTreeSet::new(),
                title_plans: BTreeMap::new(),
                title_nodes: BTreeMap::new(),
                used_title_targets: BTreeSet::new(),
                category_plans: BTreeMap::new(),
                category_views: BTreeMap::new(),
                category_nodes: BTreeMap::new(),
                used_categories: BTreeMap::new(),
                diagram_plans: BTreeMap::new(),
                diagram_nodes: BTreeMap::new(),
                used_diagram_targets: BTreeSet::new(),
            }),
        })
    }
    pub(crate) fn preflight_document(&self, document: &vizir_core::Document) -> VizResult<()> {
        vizir_core::validate_document_capabilities(document)
            .map_err(|diagnostics| VizError::validation(&diagnostics))?;
        let mut strings = SourceText::new(self.limits);
        if let Some(owner) = &document.shared_legend {
            strings.add_all(owner.title.as_deref())?;
            // Include authored categories absent from every member's data.
            if let Some(id) = owner.members.first()
                && let Some(view) = document.views.iter().find(|v| v.id() == id)
            {
                let encoding = match view {
                    vizir_core::View::Scatter(c) => c.color.as_ref(),
                    vizir_core::View::Line(c) => c.series.as_ref(),
                    vizir_core::View::Area(c) => c.series.as_ref(),
                    vizir_core::View::Bar(c) => c.color.as_ref(),
                    _ => None,
                };
                if let Some(domain) = encoding.and_then(|e| e.domain.as_deref()) {
                    strings.add_all(domain.iter().map(String::as_str))?;
                }
            }
        }
        let mut geometry = Vec::new();
        let mut found_targets = BTreeSet::new();
        let mut found_titles = BTreeSet::new();
        let mut found_categories = BTreeSet::new();
        let mut found_diagrams = BTreeSet::new();
        for view in &document.views {
            match view {
                vizir_core::View::Scatter(c) => {
                    self.preflight_title(
                        &mut strings,
                        &c.id,
                        c.title.as_deref(),
                        &mut found_titles,
                    )?;
                    strings.add(c.x.label.as_deref().unwrap_or(&c.x.field))?;
                    strings.add(c.y.label.as_deref().unwrap_or(&c.y.field))?;
                    if let Some(color) = &c.color
                        && let Some(data) = document.datasets.get(&c.dataset)
                    {
                        for row in &data.rows {
                            if let Some(serde_json::Value::String(s)) = row.get(&color.field) {
                                strings.add(s)?;
                            }
                        }
                    }
                }
                vizir_core::View::Line(c) => {
                    self.preflight_title(
                        &mut strings,
                        &c.id,
                        c.title.as_deref(),
                        &mut found_titles,
                    )?;
                    strings.add(c.x.label.as_deref().unwrap_or(&c.x.field))?;
                    strings.add(c.y.label.as_deref().unwrap_or(&c.y.field))?;
                    if let Some(color) = &c.series
                        && let Some(data) = document.datasets.get(&c.dataset)
                    {
                        for row in &data.rows {
                            if let Some(serde_json::Value::String(s)) = row.get(&color.field) {
                                strings.add(s)?;
                            }
                        }
                    }
                }
                vizir_core::View::Area(c) => {
                    self.preflight_title(
                        &mut strings,
                        &c.id,
                        c.title.as_deref(),
                        &mut found_titles,
                    )?;
                    strings.add(c.x.label.as_deref().unwrap_or(&c.x.field))?;
                    strings.add(c.y.label.as_deref().unwrap_or(&c.y.field))?;
                    if let Some(color) = &c.series
                        && let Some(data) = document.datasets.get(&c.dataset)
                    {
                        for row in &data.rows {
                            if let Some(serde_json::Value::String(s)) = row.get(&color.field) {
                                strings.add(s)?;
                            }
                        }
                    }
                }
                vizir_core::View::Heatmap(c) => {
                    self.preflight_title(
                        &mut strings,
                        &c.id,
                        c.title.as_deref(),
                        &mut found_titles,
                    )?;
                    strings.add(c.x.label.as_deref().unwrap_or(&c.x.field))?;
                    strings.add(c.y.label.as_deref().unwrap_or(&c.y.field))?;
                    strings.add(c.color.label.as_deref().unwrap_or(&c.color.field))?;
                    let category_target = self.category_targets.get(&c.id);
                    if let Some(target) = category_target
                        && (target.role != TextLayoutRole::HeatmapXCategoryLabels
                            || !found_categories.insert(c.id.clone()))
                    {
                        return Err(text_layout::error(
                            "heatmap x category target must resolve to one heatmap source view",
                        ));
                    }
                    for (index, encoding) in [&c.x, &c.y].into_iter().enumerate() {
                        let lines = if index == 0 {
                            category_target.map(|t| t.max_lines)
                        } else {
                            None
                        };
                        if let Some(domain) = &encoding.domain {
                            for value in domain {
                                strings.add_scoped(value, lines)?;
                            }
                        } else if let Some(data) = document.datasets.get(&c.dataset) {
                            let mut seen = BTreeSet::new();
                            for row in &data.rows {
                                if let Some(serde_json::Value::String(value)) =
                                    row.get(&encoding.field)
                                    && seen.insert(value.as_str())
                                {
                                    strings.add_scoped(value, lines)?;
                                }
                            }
                        }
                    }
                }
                vizir_core::View::Bar(c) => {
                    self.preflight_title(
                        &mut strings,
                        &c.id,
                        c.title.as_deref(),
                        &mut found_titles,
                    )?;
                    strings.add(c.category.label.as_deref().unwrap_or(&c.category.field))?;
                    strings.add(c.value.label.as_deref().unwrap_or(&c.value.field))?;
                    let category_target = self.category_targets.get(&c.id);
                    if category_target.is_some_and(|t| t.role != TextLayoutRole::BarCategoryLabels)
                        || (category_target.is_some() && !found_categories.insert(c.id.clone()))
                    {
                        return Err(text_layout::error(
                            "ambiguous bar.category_labels source view",
                        ));
                    }
                    if let Some(data) = document.datasets.get(&c.dataset) {
                        for row in &data.rows {
                            if let Some(serde_json::Value::String(s)) = row.get(&c.category.field) {
                                strings.add_scoped(s, category_target.map(|t| t.max_lines))?;
                            }
                            // A shared category/color field still has an independent
                            // untargeted legend use; wrapping permission is not global.
                            if let Some(color) = &c.color
                                && let Some(serde_json::Value::String(s)) = row.get(&color.field)
                            {
                                strings.add(s)?;
                            }
                        }
                    }
                }
                vizir_core::View::Diagram(c) => {
                    strings.add_all(c.title.as_deref())?;
                    for n in &c.nodes {
                        let target = self.diagram_target(&c.id, &n.id);
                        if target.is_some() && !found_diagrams.insert((c.id.clone(), n.id.clone()))
                        {
                            return Err(text_layout::error("ambiguous diagram node source target"));
                        }
                        strings.add_scoped(&n.label, target.map(|t| t.max_lines))?;
                    }
                    for e in &c.edges {
                        strings.add_all(e.label.as_deref())?;
                    }
                }
                vizir_core::View::Geometry(c) => {
                    strings.add_all(c.title.as_deref())?;
                    if c.children.len() > 65536usize.saturating_sub(geometry.len()) {
                        return Err(error("0003", "text source traversal limit exceeded"));
                    }
                    geometry.extend(c.children.iter().map(|n| (n, 0usize, c.id.as_str())));
                }
            }
        }
        let mut count = 0usize;
        while let Some((node, depth, view_id)) = geometry.pop() {
            count += 1;
            if count > 65536 || depth > 64 {
                return Err(error("0003", "text source traversal limit exceeded"));
            }
            let target = self.target(view_id, node.id());
            if target.is_some()
                && (!matches!(node, vizir_core::GeometryNode::Text { .. })
                    || !found_targets.insert((view_id.to_owned(), node.id().to_owned())))
            {
                return Err(text_layout::error(
                    "wrapping target is non-text or ambiguous",
                ));
            }
            match node {
                vizir_core::GeometryNode::Group { children, .. } => {
                    if children.len() > 65536usize.saturating_sub(geometry.len()) {
                        return Err(error("0003", "text source traversal limit exceeded"));
                    }
                    geometry.extend(children.iter().map(|n| (n, depth + 1, view_id)))
                }
                vizir_core::GeometryNode::Text { text, .. } => {
                    strings.add_scoped(text, target.map(|t| t.max_lines))?
                }
                _ => {}
            }
        }
        self.check_targets(&found_targets)?;
        self.check_title_targets(&found_titles)?;
        self.check_category_targets(&found_categories)?;
        self.check_diagram_targets(&found_diagrams)?;
        Ok(())
    }
    pub(crate) fn preflight_mir(&self, mir: &vizir_core::VizMir) -> VizResult<()> {
        vizir_core::validate_mir_capabilities(mir)
            .map_err(|diagnostics| VizError::validation(&diagnostics))?;
        let mut strings = SourceText::new(self.limits);
        if let Some(owner) = &mir.shared_legend {
            strings.add_all(owner.title.as_deref())?;
        }
        let mut geometry = Vec::new();
        let mut found_targets = BTreeSet::new();
        let mut found_titles = BTreeSet::new();
        let mut found_categories = BTreeSet::new();
        let mut found_diagrams = BTreeSet::new();
        for view in &mir.views {
            match view {
                vizir_core::MirView::Chart(c) => {
                    self.preflight_title(
                        &mut strings,
                        &c.id,
                        c.title.as_deref(),
                        &mut found_titles,
                    )?;
                    for guide in &c.guides {
                        strings.add(&guide.label)?;
                    }
                    let category_scale = self.category_scale(c)?;
                    if category_scale.is_some() && !found_categories.insert(c.id.clone()) {
                        return Err(text_layout::error(
                            "ambiguous bar.category_labels source view",
                        ));
                    }
                    for scale in &c.scales {
                        match scale {
                            vizir_core::MirScale::Band { domain, .. }
                                if category_scale
                                    .is_some_and(|selected| selected.id() == scale.id()) =>
                            {
                                for label in domain {
                                    strings.add_scoped(
                                        label,
                                        self.category_targets.get(&c.id).map(|t| t.max_lines),
                                    )?;
                                }
                            }
                            vizir_core::MirScale::Band { domain, .. }
                            | vizir_core::MirScale::OrdinalColor { domain, .. } => {
                                strings.add_all(domain.iter().map(String::as_str))?
                            }
                            _ => {}
                        }
                    }
                }
                vizir_core::MirView::Diagram(c) => {
                    strings.add_all(c.title.as_deref())?;
                    for n in &c.nodes {
                        let target = self.diagram_target(&c.id, &n.id);
                        if target.is_some() && !found_diagrams.insert((c.id.clone(), n.id.clone()))
                        {
                            return Err(text_layout::error("ambiguous diagram node source target"));
                        }
                        strings.add_scoped(&n.label, target.map(|t| t.max_lines))?;
                    }
                    for e in &c.edges {
                        strings.add_all(e.label.as_deref())?;
                    }
                }
                vizir_core::MirView::Geometry(c) => {
                    strings.add_all(c.title.as_deref())?;
                    if c.children.len() > 65536usize.saturating_sub(geometry.len()) {
                        return Err(error("0003", "text source traversal limit exceeded"));
                    }
                    geometry.extend(c.children.iter().map(|n| (n, 0usize, c.id.as_str())));
                }
            }
        }
        let mut count = 0usize;
        while let Some((node, depth, view_id)) = geometry.pop() {
            count += 1;
            if count > 65536 || depth > 64 {
                return Err(error("0003", "text source traversal limit exceeded"));
            }
            let target = self.target(view_id, node.id());
            if target.is_some()
                && (!matches!(node, vizir_core::MirGeometryNode::Text { .. })
                    || !found_targets.insert((view_id.to_owned(), node.id().to_owned())))
            {
                return Err(text_layout::error(
                    "wrapping target is non-text or ambiguous",
                ));
            }
            match node {
                vizir_core::MirGeometryNode::Group { children, .. } => {
                    if children.len() > 65536usize.saturating_sub(geometry.len()) {
                        return Err(error("0003", "text source traversal limit exceeded"));
                    }
                    geometry.extend(children.iter().map(|n| (n, depth + 1, view_id)))
                }
                vizir_core::MirGeometryNode::Text { text, .. } => {
                    strings.add_scoped(text, target.map(|t| t.max_lines))?
                }
                _ => {}
            }
        }
        self.check_targets(&found_targets)?;
        self.check_title_targets(&found_titles)?;
        self.check_category_targets(&found_categories)?;
        self.check_diagram_targets(&found_diagrams)?;
        Ok(())
    }
    pub(crate) fn has_diagram_layout(&self, view: &str, node: &str) -> bool {
        self.diagram_target(view, node).is_some()
    }
    fn diagram_target(&self, view: &str, node: &str) -> Option<&DiagramTextLayoutTarget> {
        self.diagram_targets
            .get(view)
            .and_then(|targets| targets.get(node))
    }
    fn check_diagram_targets(&self, found: &BTreeSet<(String, String)>) -> VizResult<()> {
        for (view, nodes) in &self.diagram_targets {
            for node in nodes.keys() {
                if !found.contains(&(view.clone(), node.clone())) {
                    return Err(text_layout::error(format!(
                        "diagram target ({view:?}, {node:?}) must name one existing source diagram node"
                    )));
                }
            }
        }
        Ok(())
    }
    fn diagram_plan(
        &self,
        view: &str,
        node: &str,
        source: &str,
        center: Point,
        bounds: Rect,
    ) -> VizResult<Option<Arc<DiagramPlan>>> {
        let Some(target) = self.diagram_target(view, node) else {
            return Ok(None);
        };
        if source.len() > self.limits.max_label_bytes {
            return Err(error("0003", "label byte limit exceeded"));
        }
        // Repeated plan lookups are charged even if all shaping is cached.
        {
            let mut state = self.state.borrow_mut();
            state.labels = state
                .labels
                .checked_add(1)
                .filter(|n| *n <= self.limits.max_labels)
                .ok_or_else(|| {
                    error(
                        "0003",
                        "whole-call label operation limit exceeded by diagram lookup",
                    )
                })?;
            state.text_bytes = state
                .text_bytes
                .checked_add(source.len())
                .filter(|n| *n <= self.limits.max_text_bytes)
                .ok_or_else(|| {
                    error(
                        "0003",
                        "whole-call text byte limit exceeded by diagram lookup",
                    )
                })?;
        }
        coordinate(center.x)?;
        coordinate(center.y)?;
        // The interior derives from the actual emitted node rectangle, whose x/y
        // and width/height are serialized independently by SVG.
        let rect = serialized_rect(bounds);
        let interior = Rect {
            x: rect.x + 8.,
            y: rect.y + 4.,
            width: rect.width - 16.,
            height: rect.height - 8.,
        };
        let key = DiagramPlanKey {
            view: view.into(),
            node: node.into(),
            source: source.into(),
            text_identity: self.text_identity.clone(),
            layout_profile: self.layout_profile.clone(),
            dimensions: [
                center.x.to_bits(),
                center.y.to_bits(),
                interior.x.to_bits(),
                interior.y.to_bits(),
                interior.width.to_bits(),
                interior.height.to_bits(),
                target.max_width.to_bits(),
                target.line_height.to_bits(),
            ],
            max_lines: target.max_lines,
        };
        if let Some(plan) = self.state.borrow().diagram_plans.get(&key) {
            return Ok(Some(plan.clone()));
        }
        let layout = TextLayoutTarget::new(
            view,
            node,
            target.max_width,
            target.max_lines,
            target.line_height,
        );
        let mut raw = Vec::new();
        self.wrapped_outline(
            &layout,
            source,
            13.,
            FontWeight::Medium,
            Point { x: center.x, y: 0. },
            TextAnchor::Middle,
            node,
            &mut Vec::new(),
            Some([interior.x, interior.x + interior.width]),
            Some(&mut raw),
            false,
        )?;
        let (ascent, descent) = self.face_metrics[&500];
        let mut top = f64::INFINITY;
        let mut bottom = f64::NEG_INFINITY;
        for line in &raw {
            let mut t = line.offset - ascent * 13.;
            let mut b = line.offset - descent * 13.;
            if !line.run.commands.is_empty() {
                t = t.min(line.offset + line.run.bounds.y);
                b = b.max(line.offset + line.run.bounds.y + line.run.bounds.height);
            }
            top = top.min(t);
            bottom = bottom.max(b);
        }
        // Center the complete unrounded allocation envelope, including empty
        // lines. Each line keeps its own advance centered horizontally.
        let position = Point {
            x: center.x,
            y: center.y - (top + bottom) / 2.,
        };
        coordinate(position.y)?;
        let (block, losses) = self.project_centered_lines(
            &raw,
            source,
            position,
            [interior.x, interior.x + interior.width],
            target.max_width,
            node,
            13.,
            FontWeight::Medium,
            "diagram",
            false,
        )?;
        contain_exact(block.logical, interior, node)?;
        let retained = block
            .commands
            .len()
            .checked_mul(std::mem::size_of::<PathCommand>())
            .and_then(|n| n.checked_add(raw.len() * std::mem::size_of::<RawWrappedLine>()))
            .and_then(|n| {
                n.checked_add(
                    block.details.len()
                        + source.len() * 2
                        + view.len()
                        + node.len()
                        + key.text_identity.len()
                        + key.layout_profile.len()
                        + std::mem::size_of::<DiagramPlanKey>()
                        + std::mem::size_of::<DiagramPlan>()
                        + 128,
                )
            })
            .and_then(|n| {
                n.checked_add(
                    losses
                        .iter()
                        .map(|l| {
                            l.source.len()
                                + l.target.len()
                                + l.reason.len()
                                + std::mem::size_of::<LossRecord>()
                        })
                        .sum::<usize>(),
                )
            })
            .ok_or_else(|| error("0003", "diagram plan cache size overflow"))?;
        let mut state = self.state.borrow_mut();
        state.cache_bytes = state
            .cache_bytes
            .checked_add(retained)
            .filter(|n| *n <= self.limits.max_cache_bytes)
            .ok_or_else(|| {
                error(
                    "0003",
                    "whole-call text cache limit exceeded by diagram plan",
                )
            })?;
        let plan = Arc::new(DiagramPlan {
            source: source.into(),
            raw_lines: raw,
            block,
            position,
            interior,
            losses,
        });
        state.diagram_plans.insert(key, plan.clone());
        Ok(Some(plan))
    }
    pub(crate) fn register_diagram_label(
        &self,
        view: &str,
        node: &str,
        source: &str,
        center: Point,
        bounds: Rect,
        scene_id: &str,
    ) -> VizResult<Option<Point>> {
        let Some(plan) = self.diagram_plan(view, node, source, center, bounds)? else {
            return Ok(None);
        };
        let mut state = self.state.borrow_mut();
        if state.diagram_nodes.contains_key(scene_id) {
            return Err(text_layout::error("ambiguous diagram label scene identity"));
        }
        state.cache_bytes = state
            .cache_bytes
            .checked_add(scene_id.len() + view.len() + node.len() + 128)
            .filter(|n| *n <= self.limits.max_cache_bytes)
            .ok_or_else(|| {
                error(
                    "0003",
                    "whole-call text cache limit exceeded by diagram registration",
                )
            })?;
        let position = plan.position;
        state
            .diagram_nodes
            .insert(scene_id.into(), (view.into(), node.into(), plan));
        Ok(Some(position))
    }
    pub(crate) fn has_category_layout(&self, view: &str) -> bool {
        self.category_targets.contains_key(view)
    }
    fn check_category_targets(&self, found: &BTreeSet<String>) -> VizResult<()> {
        for (view, target) in &self.category_targets {
            if !found.contains(view) {
                return Err(text_layout::error(format!(
                    "{} target {view:?} must resolve to one matching source view",
                    target.role.name()
                )));
            }
        }
        Ok(())
    }
    fn category_scale<'a>(
        &self,
        chart: &'a vizir_core::MirChart,
    ) -> VizResult<Option<&'a vizir_core::MirScale>> {
        if !self.has_category_layout(&chart.id) {
            return Ok(None);
        }
        let target = &self.category_targets[&chart.id];
        let category = match (&chart.mark, target.role) {
            (vizir_core::ChartMark::Bar { category, .. }, TextLayoutRole::BarCategoryLabels) => {
                category
            }
            (vizir_core::ChartMark::Heatmap { x, .. }, TextLayoutRole::HeatmapXCategoryLabels) => x,
            _ => {
                return Err(text_layout::error(format!(
                    "{} requires its matching chart mark",
                    target.role.name()
                )));
            }
        };
        let bottom: Vec<_> = chart
            .guides
            .iter()
            .filter(|g| {
                g.kind == vizir_core::GuideKind::Axis && g.orient == vizir_core::GuideOrient::Bottom
            })
            .collect();
        if bottom.len() != 1 || bottom[0].scale != category.scale {
            return Err(text_layout::error(
                "category wrapping requires the unique bottom Axis to reference the category binding",
            ));
        }
        let scale = chart
            .scales
            .iter()
            .find(|s| s.id() == category.scale)
            .ok_or_else(|| text_layout::error("category scale is missing"))?;
        if !matches!(scale, vizir_core::MirScale::Band {domain,..} if !domain.is_empty()) {
            return Err(text_layout::error(
                "category wrapping requires a nonempty actual Band domain",
            ));
        }
        Ok(Some(scale))
    }
    pub(crate) fn category_band_range(
        &self,
        chart: &vizir_core::MirChart,
    ) -> VizResult<Option<[f64; 2]>> {
        Ok(self.category_scale(chart)?.and_then(|s| match s {
            vizir_core::MirScale::Band { range, .. } => Some(*range),
            _ => None,
        }))
    }
    pub(crate) fn category_allocation(
        &self,
        view: &str,
        frame: vizir_core::Frame,
        plot: [f64; 4],
        labels: &[String],
        axis_title: &str,
    ) -> VizResult<Option<CategoryAllocation>> {
        let Some(target) = self.category_targets.get(view) else {
            return Ok(None);
        };
        if labels.is_empty() {
            return Err(text_layout::error(
                "category wrapping requires a nonempty actual Band domain",
            ));
        }
        if labels.len() > self.limits.max_layout_lines || labels.len() > self.limits.max_labels {
            return Err(error(
                "0003",
                "category domain exceeds whole-call line/label budget before allocation",
            ));
        }
        let bytes = labels
            .iter()
            .try_fold(0usize, |n, s| n.checked_add(s.len()))
            .ok_or_else(|| error("0003", "category source byte overflow"))?;
        {
            let mut state = self.state.borrow_mut();
            state.labels = state
                .labels
                .checked_add(1)
                .filter(|n| *n <= self.limits.max_labels)
                .ok_or_else(|| {
                    error(
                        "0003",
                        "whole-call label operation limit exceeded by category lookup",
                    )
                })?;
            state.text_bytes = state
                .text_bytes
                .checked_add(bytes)
                .filter(|n| *n <= self.limits.max_text_bytes)
                .ok_or_else(|| {
                    error(
                        "0003",
                        "whole-call text byte limit exceeded by category lookup",
                    )
                })?;
        }
        let key = CategoryPlanKey {
            role: target.role,
            view: view.into(),
            sources: labels.to_vec(),
            axis_title: axis_title.into(),
            text_identity: self.text_identity.clone(),
            layout_profile: self.layout_profile.clone(),
            dimensions: [
                frame.x.to_bits(),
                frame.y.to_bits(),
                frame.width.to_bits(),
                frame.height.to_bits(),
                plot[0].to_bits(),
                plot[1].to_bits(),
                plot[2].to_bits(),
                plot[3].to_bits(),
                target.max_width.to_bits(),
                target.line_height.to_bits(),
            ],
            max_lines: target.max_lines,
        };
        let cached = { self.state.borrow().category_plans.get(&key).cloned() };
        if let Some(plan) = cached {
            let result = CategoryAllocation {
                plot_bottom: plan.plot_bottom,
            };
            // Same source view is checked again in Scene construction; preserve the
            // exact stored Band endpoints used by that lookup.
            self.state
                .borrow_mut()
                .category_views
                .insert(view.into(), plan);
            return Ok(Some(result));
        }
        let step = (plot[2] - plot[0]) / labels.len() as f64;
        if !step.is_finite() || step <= 8. {
            return Err(text_layout::error(
                "category bands must leave a positive interior after two 4px side gaps",
            ));
        }
        let role = target.role;
        let target = TextLayoutTarget::new(
            view,
            role.name(),
            target.max_width,
            target.max_lines,
            target.line_height,
        );
        let (ascent, descent) = self.face_metrics[&400];
        let ascent = ascent * 10.;
        let descent = descent * 10.;
        let heatmap_bands = if role == TextLayoutRole::HeatmapXCategoryLabels {
            Some(crate::heatmap::band_boundaries(
                [plot[0], plot[2]],
                labels.len(),
            )?)
        } else {
            None
        };
        let mut raw_blocks = Vec::with_capacity(labels.len());
        let mut minimum = f64::INFINITY;
        let mut maximum = f64::NEG_INFINITY;
        for (i, source) in labels.iter().enumerate() {
            let (x, cell) = if let Some(bands) = &heatmap_bands {
                ((bands[i] + bands[i + 1]) / 2., [bands[i], bands[i + 1]])
            } else {
                (
                    plot[0] + step * (i as f64 + 0.5),
                    [
                        svg_number(plot[0] + step * i as f64),
                        svg_number(plot[0] + step * (i + 1) as f64),
                    ],
                )
            };
            let mut raw = Vec::new();
            let mut ignored_losses = Vec::new();
            // Width selection uses the final x anchor; raw runs retain y geometry
            // so final placement never translates an already-rounded path.
            self.wrapped_outline(
                &target,
                source,
                10.,
                FontWeight::Regular,
                Point { x, y: 0. },
                TextAnchor::Middle,
                view,
                &mut ignored_losses,
                Some([cell[0] + 4., cell[1] - 4.]),
                Some(&mut raw),
                role == TextLayoutRole::HeatmapXCategoryLabels,
            )?;
            for line in &raw {
                let mut top = line.offset - ascent;
                let mut bottom = line.offset - descent;
                if !line.run.commands.is_empty() {
                    top = top.min(line.offset + line.run.bounds.y);
                    bottom = bottom.max(line.offset + line.run.bounds.y + line.run.bounds.height);
                }
                minimum = minimum.min(top);
                maximum = maximum.max(bottom);
            }
            raw_blocks.push((x, cell, raw));
        }
        let axis = self.single_line_box_anchored(
            axis_title,
            12.5,
            FontWeight::Medium,
            Point {
                x: (plot[0] + plot[2]) / 2.,
                y: frame.y + frame.height - 16.,
            },
            TextAnchor::Middle,
        )?;
        // Each rounded Bezier y coordinate perturbs its curve by <=0.00005.
        // 0.0001 is the sum for two independently serialized envelopes, reserved
        // at each vertical gap. It is clearance, not a post-hoc fit tolerance.
        const CLEARANCE: f64 = 0.0001;
        let height = maximum - minimum;
        let bottom = plot[3].min(axis.y - 8. - height - 8. - 2. * CLEARANCE);
        let baseline = bottom + 8. - minimum + CLEARANCE;
        coordinate(bottom)?;
        coordinate(baseline)?;
        let mut blocks = Vec::with_capacity(labels.len());
        let frame = serialized_rect(Rect {
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height,
        });
        for ((x, cell, raw), source) in raw_blocks.into_iter().zip(labels) {
            let (outline, losses) = self.project_category(
                &raw,
                source,
                Point { x, y: baseline },
                cell,
                target.max_width,
                view,
                role == TextLayoutRole::HeatmapXCategoryLabels,
            )?;
            if outline.logical.y < svg_number(bottom) + 8.
                || outline.logical.y + outline.logical.height > axis.y - 8.
            {
                return Err(text_layout::error(
                    "serialized category block cannot preserve its exact 8px plot/axis-title gaps",
                ));
            }
            contain_exact(outline.logical, frame, view)?;
            blocks.push(CategoryBlock {
                source: source.clone(),
                raw_lines: raw,
                outline,
                position: Point { x, y: baseline },
                cell,
                losses,
            });
        }
        let metadata = key.view.len()
            + key.axis_title.len()
            + key.text_identity.len()
            + key.layout_profile.len()
            + bytes
            + std::mem::size_of_val(labels)
            + std::mem::size_of::<CategoryPlanKey>()
            + std::mem::size_of::<CategoryPlan>()
            + 128;
        let retained = blocks
            .iter()
            .try_fold(metadata, |n, b| {
                n.checked_add(
                    b.outline.commands.len() * std::mem::size_of::<PathCommand>()
                        + b.raw_lines.len() * std::mem::size_of::<RawWrappedLine>()
                        + b.source.len()
                        + b.outline.details.len()
                        + std::mem::size_of::<CategoryBlock>()
                        + b.losses
                            .iter()
                            .map(|l| {
                                l.source.len()
                                    + l.target.len()
                                    + l.reason.len()
                                    + std::mem::size_of::<LossRecord>()
                            })
                            .sum::<usize>(),
                )
            })
            .ok_or_else(|| error("0003", "category plan cache size overflow"))?;
        let mut state = self.state.borrow_mut();
        state.cache_bytes = state
            .cache_bytes
            .checked_add(retained)
            .filter(|n| *n <= self.limits.max_cache_bytes)
            .ok_or_else(|| {
                error(
                    "0003",
                    "whole-call text cache limit exceeded by category plan",
                )
            })?;
        let plan = Arc::new(CategoryPlan {
            role,
            blocks,
            plot_bottom: bottom,
        });
        state.category_plans.insert(key, plan.clone());
        state.category_views.insert(view.into(), plan);
        Ok(Some(CategoryAllocation {
            plot_bottom: bottom,
        }))
    }
    #[allow(clippy::too_many_arguments)]
    fn project_category(
        &self,
        lines: &[RawWrappedLine],
        source: &str,
        position: Point,
        cell: [f64; 2],
        max_width: f64,
        view: &str,
        exact_advance_grid: bool,
    ) -> VizResult<(WrappedOutline, Vec<LossRecord>)> {
        self.project_centered_lines(
            lines,
            source,
            position,
            [cell[0] + 4., cell[1] - 4.],
            max_width,
            view,
            10.,
            FontWeight::Regular,
            "category",
            exact_advance_grid,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn project_centered_lines(
        &self,
        lines: &[RawWrappedLine],
        source: &str,
        position: Point,
        horizontal: [f64; 2],
        max_width: f64,
        owner: &str,
        size: f64,
        weight: FontWeight,
        role: &str,
        exact_advance_grid: bool,
    ) -> VizResult<(WrappedOutline, Vec<LossRecord>)> {
        let (ascent, descent) = self.face_metrics[&crate::text::weight(weight)];
        let ascent = ascent * size;
        let descent = descent * size;
        let mut logical = None;
        let mut commands = Vec::new();
        let mut details = Vec::new();
        let mut glyphs = 0;
        let mut losses = Vec::new();
        let mut previous: Option<Rect> = None;
        let mut cursor = 0;
        for line in lines {
            if line.source.start != cursor || line.source.end != line.separator.start {
                return Err(text_layout::error(format!(
                    "{role} source coverage is not contiguous"
                )));
            }
            cursor = line.separator.end;
            let baseline = position.y + line.offset;
            let (path, ink) = self.project(
                &line.run,
                Point {
                    x: position.x,
                    y: baseline,
                },
                TextAnchor::Middle,
            )?;
            let origin = position.x - line.run.advance / 2.;
            let mut left = svg_number(origin);
            let mut right = svg_number(origin + line.run.advance);
            let mut top = svg_number(baseline - ascent);
            let mut bottom = svg_number(baseline - descent);
            if !path.is_empty() {
                left = left.min(ink.x);
                right = right.max(ink.x + ink.width);
                top = top.min(ink.y);
                bottom = bottom.max(ink.y + ink.height);
            }
            let width = if exact_advance_grid {
                serialized_advance_envelope_width(
                    svg_number(origin),
                    svg_number(origin + line.run.advance),
                    (!path.is_empty()).then_some(ink),
                )
            } else {
                right - left
            };
            if width > max_width || left < horizontal[0] || right > horizontal[1] {
                return Err(text_layout::error(format!(
                    "serialized {role} advance/ink exceeds explicit width or its available interior"
                )));
            }
            let before = dimensional_contours(&line.run.commands);
            let after = dimensional_contours_grid(&path);
            if before > 0 && after == 0 {
                return Err(error(
                    "0004",
                    format!("{role} line contours collapse at SVG four-decimal precision"),
                ));
            }
            if after < before {
                losses.push(LossRecord {
                    source: owner.into(),
                    target: "scene2d".into(),
                    fidelity: LoweringFidelity::VisuallyApproximate,
                    reason: format!(
                        "{} {role} line sub-contours collapse at four-decimal precision",
                        before - after
                    ),
                });
            }
            if !path.is_empty() {
                if let Some(previous) = previous
                    && previous.y + previous.height > ink.y
                {
                    return Err(text_layout::error(format!(
                        "{role} line ink overlaps; increase explicit line_height"
                    )));
                }
                previous = Some(ink);
            }
            logical = Some(union_rect(
                logical,
                Rect {
                    x: left,
                    y: top,
                    width: right - left,
                    height: bottom - top,
                },
            ));
            glyphs += line.run.glyphs;
            details.push(format!(
                "{}..{} separator {}..{} baseline {} advance {}",
                line.source.start,
                line.source.end,
                line.separator.start,
                line.separator.end,
                svg_number(baseline),
                line.run.advance
            ));
            commands.extend(path);
        }
        if cursor != source.len() {
            return Err(text_layout::error(format!(
                "{role} source coverage is incomplete"
            )));
        }
        let bounds = path_bounds(&commands);
        Ok((
            WrappedOutline {
                commands,
                bounds,
                logical: logical.ok_or_else(|| {
                    text_layout::error(format!("{role} requires at least one logical line"))
                })?,
                details: details.join("; "),
                glyphs,
            },
            losses,
        ))
    }
    pub(crate) fn category_bounds(&self, view: &str, index: usize) -> VizResult<Rect> {
        self.state
            .borrow()
            .category_views
            .get(view)
            .and_then(|p| p.blocks.get(index))
            .map(|b| b.outline.logical)
            .ok_or_else(|| text_layout::error("missing measured category bounds"))
    }
    pub(crate) fn category_position(&self, view: &str, index: usize) -> VizResult<Option<Point>> {
        if !self.has_category_layout(view) {
            return Ok(None);
        }
        self.state
            .borrow()
            .category_views
            .get(view)
            .and_then(|p| p.blocks.get(index))
            .map(|b| Some(b.position))
            .ok_or_else(|| text_layout::error("missing measured category domain item"))
    }
    pub(crate) fn register_category_node(
        &self,
        view: &str,
        index: usize,
        node: &SceneNode,
    ) -> VizResult<()> {
        if !self.has_category_layout(view) {
            return Ok(());
        }
        let SceneNode::Text {
            id,
            text,
            position,
            font_size,
            anchor,
            weight,
            ..
        } = node
        else {
            return Err(text_layout::error("category registration requires text"));
        };
        let plan = self
            .state
            .borrow()
            .category_views
            .get(view)
            .cloned()
            .ok_or_else(|| text_layout::error("missing measured category domain"))?;
        let block = plan
            .blocks
            .get(index)
            .ok_or_else(|| text_layout::error("extra category emission outside complete domain"))?;
        if text != &block.source
            || *position != block.position
            || *font_size != 10.
            || *weight != FontWeight::Regular
            || *anchor != TextAnchor::Middle
            || block
                .raw_lines
                .last()
                .is_none_or(|l| l.separator.end != text.len())
        {
            return Err(text_layout::error(
                "category emission differs from its measured domain item",
            ));
        }
        let mut state = self.state.borrow_mut();
        if state.category_nodes.contains_key(id) {
            return Err(text_layout::error("ambiguous category scene identity"));
        }
        state.cache_bytes = state
            .cache_bytes
            .checked_add(id.len() + view.len() + 128)
            .filter(|n| *n <= self.limits.max_cache_bytes)
            .ok_or_else(|| {
                error(
                    "0003",
                    "whole-call text cache limit exceeded by category registration",
                )
            })?;
        state
            .category_nodes
            .insert(id.clone(), (view.into(), index, plan));
        Ok(())
    }
    fn registered_category(&self, id: &str) -> Option<(String, usize, Arc<CategoryPlan>)> {
        self.state.borrow().category_nodes.get(id).cloned()
    }
    fn check_category_emission(&self) -> VizResult<()> {
        let state = self.state.borrow();
        for view in self.category_targets.keys() {
            let plan = state
                .category_views
                .get(view)
                .ok_or_else(|| text_layout::error("unused category target"))?;
            let used = state
                .used_categories
                .get(view)
                .ok_or_else(|| text_layout::error("category target emitted no domain items"))?;
            if used.len() != plan.blocks.len() || !(0..plan.blocks.len()).all(|i| used.contains(&i))
            {
                return Err(text_layout::error(
                    "category emission did not cover every domain item exactly once",
                ));
            }
        }
        Ok(())
    }

    fn preflight_title(
        &self,
        strings: &mut SourceText,
        view: &str,
        title: Option<&str>,
        found: &mut BTreeSet<String>,
    ) -> VizResult<()> {
        if let Some(target) = self.title_targets.get(view) {
            let title = title.ok_or_else(|| {
                text_layout::error(format!(
                    "chart.title target {view:?} requires an existing source title"
                ))
            })?;
            if !found.insert(view.to_owned()) {
                return Err(text_layout::error("ambiguous chart.title source view"));
            }
            strings.add_scoped(title, Some(target.max_lines))
        } else {
            strings.add_all(title)
        }
    }
    fn check_title_targets(&self, found: &BTreeSet<String>) -> VizResult<()> {
        for view in self.title_targets.keys() {
            if !found.contains(view) {
                return Err(text_layout::error(format!(
                    "chart.title target {view:?} must name one existing title in a bar, line or scatter source view, an area source view under HIR/MIR 0.3/0.4/0.5, or a heatmap under HIR/MIR 0.4/0.5"
                )));
            }
        }
        Ok(())
    }
    fn emit_glyphs(&self, count: usize) -> VizResult<()> {
        let mut state = self.state.borrow_mut();
        state.emitted_glyphs = state
            .emitted_glyphs
            .checked_add(count)
            .filter(|n| *n <= self.limits.max_glyphs)
            .ok_or_else(|| error("0003", "whole-call emitted glyph limit exceeded"))?;
        Ok(())
    }
    /// Reserve bounded heatmap layout comparisons before exact-face layout.
    pub(crate) fn reserve_heatmap_collisions(&self, labels: usize) -> VizResult<()> {
        let checks = labels
            .checked_mul(labels)
            .ok_or_else(|| error("0003", "heatmap label comparison count overflow"))?;
        let mut state = self.state.borrow_mut();
        state.collision_checks = state
            .collision_checks
            .checked_add(checks)
            .filter(|n| *n <= self.limits.max_collision_checks)
            .ok_or_else(|| {
                error(
                    "0003",
                    "whole-call label collision-check limit exceeded by heatmap layout",
                )
            })?;
        Ok(())
    }

    pub(crate) fn single_line_box(
        &self,
        source: &str,
        size: f64,
        weight: FontWeight,
        position: Point,
    ) -> VizResult<Rect> {
        self.single_line_box_anchored(source, size, weight, position, TextAnchor::Start)
    }
    pub(crate) fn single_line_box_anchored(
        &self,
        source: &str,
        size: f64,
        weight: FontWeight,
        position: Point,
        anchor: TextAnchor,
    ) -> VizResult<Rect> {
        let run = self.shape(source, size, weight)?;
        let (commands, ink) = self.project(&run, position, anchor)?;
        let origin = position.x
            + match anchor {
                TextAnchor::Start => 0.,
                TextAnchor::Middle => -run.advance / 2.,
                TextAnchor::End => -run.advance,
            };
        let (ascent, descent) = self.face_metrics[&crate::text::weight(weight)];
        let mut logical = Rect {
            x: svg_number(origin),
            y: svg_number(position.y - ascent * size),
            width: svg_number(origin + run.advance) - svg_number(origin),
            height: svg_number(position.y - descent * size)
                - svg_number(position.y - ascent * size),
        };
        if !commands.is_empty() {
            logical = union_rect(Some(logical), ink);
        }
        Ok(logical)
    }

    pub(crate) fn chart_title_bounds(
        &self,
        view: &str,
        title: Option<&str>,
        frame: vizir_core::Frame,
    ) -> VizResult<Option<Rect>> {
        Ok(self
            .chart_title_plan(view, title, frame)?
            .map(|plan| plan.block.logical))
    }
    fn chart_title_plan(
        &self,
        view: &str,
        title: Option<&str>,
        frame: vizir_core::Frame,
    ) -> VizResult<Option<Arc<TitlePlan>>> {
        let Some(target) = self.title_targets.get(view) else {
            return Ok(None);
        };
        let source =
            title.ok_or_else(|| text_layout::error("chart.title target has no source title"))?;
        if source.len() > self.limits.max_label_bytes {
            return Err(error("0003", "label byte limit exceeded"));
        }
        // Repeated lookups remain bounded work, even when the resolved block is cached.
        {
            let mut state = self.state.borrow_mut();
            state.labels = state
                .labels
                .checked_add(1)
                .filter(|n| *n <= self.limits.max_labels)
                .ok_or_else(|| error("0003", "whole-call label operation limit exceeded"))?;
            state.text_bytes = state
                .text_bytes
                .checked_add(source.len())
                .filter(|n| *n <= self.limits.max_text_bytes)
                .ok_or_else(|| error("0003", "whole-call text byte limit exceeded"))?;
        }
        let key = TitlePlanKey {
            view: view.into(),
            source: source.into(),
            text_identity: self.text_identity.clone(),
            layout_profile: self.layout_profile.clone(),
            dimensions: [
                frame.x.to_bits(),
                frame.y.to_bits(),
                frame.width.to_bits(),
                frame.height.to_bits(),
                target.max_width.to_bits(),
                target.line_height.to_bits(),
                18.0f64.to_bits(),
            ],
            max_lines: target.max_lines,
            weight: 700,
        };
        if let Some(plan) = self.state.borrow().title_plans.get(&key) {
            return Ok(Some(plan.clone()));
        }
        let bounds = TextLayoutTarget::new(
            view,
            "chart.title",
            target.max_width,
            target.max_lines,
            target.line_height,
        );
        let mut losses = Vec::new();
        let block = self.wrapped_outline(
            &bounds,
            source,
            18.,
            FontWeight::Bold,
            Point {
                x: frame.x + 18.,
                y: frame.y + 28.,
            },
            TextAnchor::Start,
            view,
            &mut losses,
            None,
            None,
            false,
        )?;
        contain(
            block.logical,
            serialized_rect(Rect {
                x: frame.x,
                y: frame.y,
                width: frame.width,
                height: frame.height,
            }),
            view,
        )?;
        let bytes = block
            .commands
            .len()
            .checked_mul(std::mem::size_of::<PathCommand>())
            .and_then(|n| n.checked_add(block.details.len()))
            .and_then(|n| {
                n.checked_add(
                    source.len()
                        + view.len()
                        + self.text_identity.len()
                        + self.layout_profile.len()
                        + std::mem::size_of::<TitlePlan>()
                        + std::mem::size_of::<TitlePlanKey>()
                        + 128,
                )
            })
            .and_then(|n| {
                n.checked_add(
                    losses
                        .iter()
                        .map(|l| {
                            l.source.len()
                                + l.target.len()
                                + l.reason.len()
                                + std::mem::size_of::<LossRecord>()
                        })
                        .sum::<usize>(),
                )
            })
            .ok_or_else(|| error("0003", "title plan cache size overflow"))?;
        let mut state = self.state.borrow_mut();
        state.cache_bytes = state
            .cache_bytes
            .checked_add(bytes)
            .filter(|n| *n <= self.limits.max_cache_bytes)
            .ok_or_else(|| {
                error(
                    "0003",
                    "whole-call text outline cache limit exceeded by title plan",
                )
            })?;
        let plan = Arc::new(TitlePlan { block, losses });
        state.title_plans.insert(key, plan.clone());
        Ok(Some(plan))
    }
    /// Register the semantic role where the chart builder creates it. Neither
    /// user selectors nor later passes parse a generated Scene ID.
    pub(crate) fn register_chart_title(
        &self,
        view: &str,
        node: &SceneNode,
        frame: vizir_core::Frame,
    ) -> VizResult<()> {
        let SceneNode::Text {
            id,
            text,
            position,
            font_size,
            anchor,
            weight,
            ..
        } = node
        else {
            return Err(text_layout::error("chart title registration requires text"));
        };
        let Some(plan) = self.chart_title_plan(view, Some(text), frame)? else {
            return Ok(());
        };
        if *font_size != 18.
            || *weight != FontWeight::Bold
            || *anchor != TextAnchor::Start
            || position.x != frame.x + 18.
            || position.y != frame.y + 28.
        {
            return Err(text_layout::error(
                "chart title emission differs from its measured source role",
            ));
        }
        let mut state = self.state.borrow_mut();
        if state.title_nodes.contains_key(id) {
            return Err(text_layout::error(
                "chart title scene identity is ambiguous",
            ));
        }
        state.cache_bytes = state
            .cache_bytes
            .checked_add(id.len() + view.len() + 128)
            .filter(|n| *n <= self.limits.max_cache_bytes)
            .ok_or_else(|| {
                error(
                    "0003",
                    "whole-call text cache limit exceeded by title registration",
                )
            })?;
        state.title_nodes.insert(id.clone(), (view.into(), plan));
        Ok(())
    }
    fn registered_title(&self, id: &str) -> Option<(String, Arc<TitlePlan>)> {
        self.state.borrow().title_nodes.get(id).cloned()
    }

    fn target(&self, view: &str, node: &str) -> Option<&TextLayoutTarget> {
        self.targets.get(view)?.get(node)
    }
    fn check_targets(&self, found: &BTreeSet<(String, String)>) -> VizResult<()> {
        for (view, nodes) in &self.targets {
            for node in nodes.keys() {
                if !found.contains(&(view.clone(), node.clone())) {
                    return Err(text_layout::error(format!(
                        "wrapping target ({view:?}, {node:?}) must name exactly one geometry.scene text source"
                    )));
                }
            }
        }
        Ok(())
    }
    fn shape(&self, text: &str, size: f64, w: FontWeight) -> VizResult<Arc<Run>> {
        if !size.is_finite() || !(MIN_SIZE..=MAX_SIZE).contains(&size) {
            return Err(error(
                "0004",
                "font size must be finite and between 0.25 and 4096 scene units",
            ));
        }
        if text.len() > self.limits.max_label_bytes {
            return Err(error("0003", "label byte limit exceeded"));
        }
        if text
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        {
            return Err(error(
                "0004",
                "single-line text does not support newlines, tabs or control characters; edit the original HIR",
            ));
        }
        let mut state = self.state.borrow_mut();
        state.text_bytes = state
            .text_bytes
            .checked_add(text.len())
            .filter(|n| *n <= self.limits.max_text_bytes)
            .ok_or_else(|| error("0003", "whole-call text byte limit exceeded"))?;
        state.labels = state
            .labels
            .checked_add(1)
            .filter(|n| *n <= self.limits.max_labels)
            .ok_or_else(|| error("0003", "whole-call label operation limit exceeded"))?;
        let key = (text.to_owned(), size.to_bits(), weight(w));
        if let Some(run) = state.cache.get(&key) {
            return Ok(run.clone());
        }
        let (font_id, family, glyph_count) = state.selected[&weight(w)].clone();
        let attrs = Attrs::new()
            .family(Family::Name(&family))
            .weight(Weight(weight(w)));
        let mut buffer = Buffer::new(
            &mut state.system,
            Metrics::new(size as f32, (size * 1.4) as f32),
        );
        buffer.set_hinting(Hinting::Disabled);
        buffer.set_wrap(Wrap::None);
        buffer.set_size(None, None);
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut state.system, false);
        let mut pen = Pen::new(
            self.limits
                .max_outline_commands
                .saturating_sub(state.commands),
        );
        let mut advance = 0f64;
        let mut glyphs = 0usize;
        let mut coverage = vec![false; text.len()];
        for line in buffer.layout_runs() {
            advance = advance.max(f64::from(line.line_w));
            for glyph in line.glyphs {
                glyphs += 1;
                if state
                    .glyphs
                    .checked_add(glyphs)
                    .is_none_or(|n| n > self.limits.max_glyphs)
                {
                    return Err(error("0003", "whole-call shaped glyph limit exceeded"));
                }
                if glyph.glyph_id == 0 || glyph.glyph_id >= glyph_count || glyph.font_id != font_id
                {
                    return Err(error(
                        "0005",
                        format!(
                            "selected {w:?} face has missing glyph coverage at UTF-8 bytes {}..{}; no fallback or synthesis is allowed",
                            glyph.start, glyph.end
                        ),
                    ));
                }
                let font = state
                    .system
                    .get_font(font_id, Weight(weight(w)))
                    .ok_or_else(|| error("0002", "selected font could not be shaped"))?;
                let index = state.system.db().face(font_id).expect("selected").index;
                let raw = FontRef::from_index(font.data(), index)
                    .map_err(|_| error("0002", "invalid selected font"))?;
                let gid = GlyphId::new(u32::from(glyph.glyph_id));
                // Draw unscaled font geometry, then scale in f64. Avoid fixed-point
                // scaled metrics rounding and accidental global-coordinate f32 loss.
                let units = raw
                    .metrics(Size::unscaled(), LocationRef::default())
                    .units_per_em;
                if units == 0 {
                    return Err(error("0002", "font has zero units per em"));
                }
                pen.scale = size / f64::from(units);
                pen.tx = f64::from(glyph.x) + size * f64::from(glyph.x_offset);
                pen.ty = f64::from(glyph.y) - size * f64::from(glyph.y_offset);
                let Some(outline) = raw.outline_glyphs().get(gid) else {
                    return Err(error(
                        "0005",
                        "selected glyph has no supported vector outline",
                    ));
                };
                let command_start = pen.commands.len();
                outline
                    .draw(
                        DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
                        &mut pen,
                    )
                    .map_err(|_| error("0005", "font outline extraction failed"))?;
                if pen.failed {
                    return Err(error(
                        "0003",
                        "outline command or coordinate limit exceeded",
                    ));
                }
                let drawn = &pen.commands[command_start..];
                let source = text
                    .get(glyph.start..glyph.end)
                    .ok_or_else(|| error("0005", "shaper returned an invalid source cluster"))?;
                coverage
                    .get_mut(glyph.start..glyph.end)
                    .ok_or_else(|| error("0005", "invalid source cluster range"))?
                    .fill(true);
                if dimensional_contours(drawn) == 0
                    && (!drawn.is_empty() || glyph.w > 0.0)
                    && source.chars().any(|c| !c.is_whitespace())
                {
                    return Err(error(
                        "0005",
                        "selected font extraction produces no filled two-dimensional ink for a non-whitespace glyph",
                    ));
                }
            }
        }
        if !advance.is_finite() || advance > MAX_COORDINATE {
            return Err(error(
                "0004",
                "measured advance exceeds the finite coordinate limit",
            ));
        }
        let bounds = pen.bounds.unwrap_or_default();
        let bytes = pen
            .commands
            .len()
            .checked_mul(std::mem::size_of::<PathCommand>())
            .and_then(|n| n.checked_add(text.len()))
            .ok_or_else(|| error("0003", "text cache size overflow"))?;
        state.cache_bytes = state
            .cache_bytes
            .checked_add(bytes)
            .filter(|n| *n <= self.limits.max_cache_bytes)
            .ok_or_else(|| error("0003", "whole-call text outline cache limit exceeded"))?;
        state.commands += pen.commands.len();
        state.glyphs += glyphs;
        let run = Arc::new(Run {
            advance,
            bounds,
            commands: pen.commands,
            glyphs,
            coverage_complete: coverage.into_iter().all(|covered| covered),
        });
        state.cache.insert(key, run.clone());
        Ok(run)
    }
    pub(crate) fn width(&self, text: &str, size: f64, w: FontWeight) -> VizResult<f64> {
        Ok(self.shape(text, size, w)?.width())
    }
    fn project(
        &self,
        run: &Run,
        position: Point,
        anchor: TextAnchor,
    ) -> VizResult<(Vec<PathCommand>, Rect)> {
        coordinate(position.x)?;
        coordinate(position.y)?;
        {
            let mut state = self.state.borrow_mut();
            state.emitted_commands = state
                .emitted_commands
                .checked_add(run.commands.len())
                .filter(|n| *n <= self.limits.max_outline_commands)
                .ok_or_else(|| error("0003", "whole-call outline projection limit exceeded"))?;
        }
        let dx = position.x
            + match anchor {
                TextAnchor::Start => 0.,
                TextAnchor::Middle => -run.advance / 2.,
                TextAnchor::End => -run.advance,
            };
        let commands = run
            .commands
            .iter()
            .map(|c| translate(*c, dx, position.y))
            .collect::<VizResult<Vec<_>>>()?;
        let bounds = path_bounds(&commands);
        Ok((commands, bounds))
    }
    pub(crate) fn check_text_box(&self, node: &SceneNode, region: Rect) -> VizResult<()> {
        if let SceneNode::Text {
            id,
            text,
            font_size,
            weight,
            position,
            anchor,
            ..
        } = node
        {
            let run = self.shape(text, *font_size, *weight)?;
            let (_, bounds) = self.project(&run, *position, *anchor)?;
            contain(bounds, serialized_rect(region), id)?;
        }
        Ok(())
    }
    pub(crate) fn check_chart(
        &self,
        nodes: &[SceneNode],
        frame: vizir_core::Frame,
        plot: [f64; 4],
    ) -> VizResult<()> {
        let plot = plot.map(svg_number);
        let region = serialized_rect(Rect {
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height,
        });
        let mut labels = Vec::new();
        let mut pending: Vec<_> = nodes.iter().collect();
        while let Some(node) = pending.pop() {
            match node {
                SceneNode::Text {
                    id,
                    text,
                    font_size,
                    weight,
                    position,
                    anchor,
                    ..
                } => {
                    let b = if let Some((_, index, plan)) = self.registered_category(id) {
                        plan.blocks[index].outline.logical
                    } else if let Some((_, plan)) = self.registered_title(id) {
                        plan.block.logical
                    } else {
                        let r = self.shape(text, *font_size, *weight)?;
                        self.project(&r, *position, *anchor)?.1
                    };
                    if b.x < plot[2]
                        && b.x + b.width > plot[0]
                        && b.y < plot[3]
                        && b.y + b.height > plot[1]
                    {
                        return Err(error(
                            "0006",
                            format!(
                                "chart label {id:?} overlaps the plot; enlarge the frame or choose another exact font"
                            ),
                        ));
                    }
                    contain(b, region, id)?;
                    if b.width > 0. && b.height > 0. {
                        labels.push((id, b));
                    }
                }
                SceneNode::Group { children, .. } => pending.extend(children),
                _ => {}
            }
        }
        labels.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
        for (i, (id, a)) in labels.iter().enumerate() {
            for (other, b) in &labels[i + 1..] {
                if b.x >= a.x + a.width {
                    break;
                }
                let mut state = self.state.borrow_mut();
                state.collision_checks += 1;
                if state.collision_checks > self.limits.max_collision_checks {
                    return Err(error(
                        "0003",
                        "whole-call label collision-check limit exceeded",
                    ));
                }
                drop(state);
                if a.y < b.y + b.height && b.y < a.y + a.height {
                    return Err(error(
                        "0006",
                        format!(
                            "chart labels {id:?} and {other:?} collide; enlarge the frame or shorten source labels"
                        ),
                    ));
                }
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn wrapped_outline(
        &self,
        target: &TextLayoutTarget,
        source: &str,
        size: f64,
        weight: FontWeight,
        position: Point,
        anchor: TextAnchor,
        id: &str,
        losses: &mut Vec<LossRecord>,
        cell: Option<[f64; 2]>,
        mut raw_lines: Option<&mut Vec<RawWrappedLine>>,
        exact_advance_grid: bool,
    ) -> VizResult<WrappedOutline> {
        let paragraphs = text_layout::paragraphs(source)?;
        if paragraphs.len() > target.max_lines as usize {
            return Err(text_layout::error("hard breaks exceed target max_lines"));
        }
        // Empty and terminal logical lines consume work before any shaping.
        self.reserve_lines(paragraphs.len())?;
        let (ascent, descent) = self.face_metrics[&crate::text::weight(weight)];
        let ascent = ascent * size;
        let descent = descent * size;
        if !ascent.is_finite()
            || !descent.is_finite()
            || ascent < 0.
            || descent > 0.
            || ascent - descent <= 0.
            || target.line_height < ascent - descent
        {
            return Err(text_layout::error(format!(
                "line_height {} must be at least the exact selected-face ascent minus descent {} at size {size}",
                target.line_height,
                ascent - descent
            )));
        }
        let mut lines: Vec<WrappedLine> = Vec::new();
        for paragraph in &paragraphs {
            let text = &source[paragraph.text.clone()];
            let opportunities = text_layout::opportunities(text);
            let mut start = paragraph.text.start;
            loop {
                if lines.len() >= target.max_lines as usize {
                    return Err(text_layout::error(format!(
                        "text {id:?} exceeds max_lines; no text is dropped"
                    )));
                }
                let baseline = position.y + lines.len() as f64 * target.line_height;
                coordinate(baseline)?;
                let mut selected = None;
                let ends: Vec<usize> = if text.is_empty() {
                    vec![start]
                } else {
                    opportunities
                        .iter()
                        .map(|i| i + paragraph.text.start)
                        .filter(|end| *end > start)
                        .collect()
                };
                for end in ends {
                    {
                        let mut state = self.state.borrow_mut();
                        state.wrap_candidates = state
                            .wrap_candidates
                            .checked_add(1)
                            .filter(|n| *n <= self.limits.max_wrap_candidates)
                            .ok_or_else(|| {
                                error("0003", "whole-call wrap candidate limit exceeded")
                            })?;
                    }
                    let run = self.shape(&source[start..end], size, weight)?;
                    if !run.coverage_complete {
                        return Err(text_layout::error(
                            "shaping omitted source coverage; wrapping will not drop whitespace or graphemes",
                        ));
                    }
                    let pos = Point {
                        x: position.x,
                        y: baseline,
                    };
                    let (commands, bounds) = self.project(&run, pos, anchor)?;
                    let origin = position.x
                        + match anchor {
                            TextAnchor::Start => 0.,
                            TextAnchor::Middle => -run.advance / 2.,
                            TextAnchor::End => -run.advance,
                        };
                    let mut left = svg_number(origin);
                    let mut right = svg_number(origin + run.advance);
                    if !commands.is_empty() {
                        left = left.min(bounds.x);
                        right = right.max(bounds.x + bounds.width);
                    }
                    // Bounds include actual cubic extrema of serialized coordinates.
                    let width = if exact_advance_grid {
                        serialized_advance_envelope_width(
                            svg_number(origin),
                            svg_number(origin + run.advance),
                            (!commands.is_empty()).then_some(bounds),
                        )
                    } else {
                        right - left
                    };
                    if width > target.max_width
                        || cell.is_some_and(|cell| left < cell[0] || right > cell[1])
                    {
                        // Explicit first-overflow greedy policy, not an assumption
                        // that arbitrary shaped prefix widths are monotone.
                        break;
                    }
                    let mut top = svg_number(baseline - ascent);
                    let mut bottom = svg_number(baseline - descent);
                    if !commands.is_empty() {
                        top = top.min(bounds.y);
                        bottom = bottom.max(bounds.y + bounds.height);
                    }
                    let logical = Rect {
                        x: left,
                        y: top,
                        width: right - left,
                        height: bottom - top,
                    };
                    selected = Some(WrappedLine {
                        source: start..end,
                        separator: end..end,
                        baseline: svg_number(baseline),
                        run,
                        commands,
                        bounds,
                        logical,
                    });
                }
                let mut selected = selected.ok_or_else(|| text_layout::error(format!("text {id:?} has an unbreakable segment exceeding max_width {}; no emergency grapheme split or shrink is allowed", target.max_width)))?;
                let end = selected.source.end;
                let last = end == paragraph.text.end;
                if last {
                    selected.separator = paragraph.separator.clone();
                } else {
                    self.reserve_lines(1)?;
                }
                let before = dimensional_contours(&selected.run.commands);
                let after = dimensional_contours_grid(&selected.commands);
                if raw_lines.is_none() && before > 0 && after == 0 {
                    return Err(error(
                        "0004",
                        "nonempty wrapped line contours collapse at SVG four-decimal precision",
                    ));
                }
                if raw_lines.is_none() && after < before {
                    losses.push(LossRecord { source: id.into(), target: "scene2d".into(), fidelity: LoweringFidelity::VisuallyApproximate, reason: format!("{} wrapped-line sub-contours collapse at four-decimal outline precision", before-after) });
                }
                if raw_lines.is_none()
                    && let Some(previous) =
                        lines.iter().rev().find(|line| !line.commands.is_empty())
                    && !selected.commands.is_empty()
                    && previous.bounds.y + previous.bounds.height > selected.bounds.y
                {
                    return Err(text_layout::error(format!(
                        "text {id:?} has vertically overlapping line ink; increase explicit line_height"
                    )));
                }
                lines.push(selected);
                start = end;
                if last {
                    break;
                }
            }
        }
        let mut cursor = 0;
        let mut logical = None;
        let mut commands = Vec::new();
        let mut details = Vec::new();
        let mut glyphs = 0;
        for (i, line) in lines.into_iter().enumerate() {
            if let Some(raw) = raw_lines.as_deref_mut() {
                raw.push(RawWrappedLine {
                    source: line.source.clone(),
                    separator: line.separator.clone(),
                    offset: i as f64 * target.line_height,
                    run: line.run.clone(),
                });
            }
            glyphs += line.run.glyphs;
            if line.source.start != cursor || line.source.end != line.separator.start {
                return Err(text_layout::error(
                    "internal wrapping source coverage is not contiguous",
                ));
            }
            cursor = line.separator.end;
            logical = Some(union_rect(logical, line.logical));
            details.push(format!(
                "{}..{} separator {}..{} baseline {} advance {}",
                line.source.start,
                line.source.end,
                line.separator.start,
                line.separator.end,
                line.baseline,
                line.run.advance
            ));
            commands.extend(line.commands);
        }
        if cursor != source.len() {
            return Err(text_layout::error(
                "internal wrapping source coverage is incomplete",
            ));
        }
        let bounds = path_bounds(&commands);
        Ok(WrappedOutline {
            commands,
            bounds,
            logical: logical.expect("one or more logical lines"),
            details: details.join("; "),
            glyphs,
        })
    }
    fn reserve_lines(&self, count: usize) -> VizResult<()> {
        let mut state = self.state.borrow_mut();
        state.layout_lines = state
            .layout_lines
            .checked_add(count)
            .filter(|n| *n <= self.limits.max_layout_lines)
            .ok_or_else(|| error("0003", "whole-call visual line limit exceeded"))?;
        Ok(())
    }

    pub(crate) fn outline_scene(&self, mut scene: Scene2D) -> VizResult<Scene2D> {
        if !scene.width.is_finite()
            || !scene.height.is_finite()
            || scene.width > MAX_COORDINATE
            || scene.height > MAX_COORDINATE
        {
            return Err(error(
                "0004",
                "measured text document dimensions exceed 1000000 scene units",
            ));
        }
        scene.width = svg_number(scene.width);
        scene.height = svg_number(scene.height);
        if scene.width <= 0. || scene.height <= 0. {
            return Err(error(
                "0004",
                "document viewport collapses at SVG precision",
            ));
        }
        let mut contour_losses = Vec::new();
        let canvas = Rect {
            x: 0.,
            y: 0.,
            width: scene.width,
            height: scene.height,
        };
        for node in &mut scene.nodes {
            let view_id = node.id().to_owned();
            let frame = match node {
                SceneNode::Group { bounds, .. } => {
                    *bounds = serialized_rect(*bounds);
                    *bounds
                }
                _ => canvas,
            };
            self.outline_node(
                node,
                &view_id,
                Affine::IDENTITY,
                frame,
                canvas,
                0,
                &mut contour_losses,
            )?;
        }
        self.check_targets(&self.state.borrow().used_targets)?;
        self.check_title_targets(&self.state.borrow().used_title_targets)?;
        self.check_category_emission()?;
        self.check_diagram_targets(&self.state.borrow().used_diagram_targets)?;
        scene.losses.extend(contour_losses);
        scene.losses.push(LossRecord{source:"text".into(),target:"scene2d".into(),fidelity:LoweringFidelity::VisuallyApproximate,reason:"Opt-in measured text was converted to font-independent outlines at SVG four-decimal precision (path coordinates and group-transform scalars round by at most 0.00005; transformed error depends on the transform). Original strings remain in source/MIR; SVG text selection, search and text editing are unavailable.".into()});
        vizir_core::validate_scene(&scene).map_err(|d| VizError::validation(&d))?;
        let mut writer = OutputBudget(self.limits.max_output_bytes);
        serde_json::to_writer_pretty(&mut writer, &scene).map_err(|_| {
            error(
                "0003",
                "outlined Scene exceeds the serialized output-byte limit",
            )
        })?;
        Ok(scene)
    }
    #[allow(clippy::too_many_arguments)]
    fn outline_node(
        &self,
        node: &mut SceneNode,
        view_id: &str,
        parent: Affine,
        frame: Rect,
        canvas: Rect,
        depth: usize,
        contour_losses: &mut Vec<LossRecord>,
    ) -> VizResult<()> {
        if depth > 64 {
            return Err(error("0003", "text traversal depth limit exceeded"));
        }
        if let SceneNode::Group {
            transform,
            children,
            ..
        } = node
        {
            *transform = serialized_transform(*transform)?;
            let matrix = parent.then(*transform)?;
            for child in children {
                self.outline_node(
                    child,
                    view_id,
                    matrix,
                    frame,
                    canvas,
                    depth + 1,
                    contour_losses,
                )?;
            }
            return Ok(());
        }
        let SceneNode::Text {
            id,
            origin,
            position,
            text,
            font_size,
            anchor,
            color,
            weight,
            ..
        } = node
        else {
            return Ok(());
        };
        coordinate(position.x)?;
        coordinate(position.y)?;
        let diagram_plan = { self.state.borrow().diagram_nodes.get(id).cloned() };
        let (commands, bounds, explanation) = if let Some((owner, source_node, plan)) = diagram_plan
        {
            if owner != view_id
                || !self
                    .state
                    .borrow_mut()
                    .used_diagram_targets
                    .insert((owner, source_node))
            {
                return Err(text_layout::error(
                    "diagram label emitted with wrong or repeated semantic owner",
                ));
            }
            if text != &plan.source
                || *position != plan.position
                || *font_size != 13.
                || *weight != FontWeight::Medium
                || *anchor != TextAnchor::Middle
                || plan
                    .raw_lines
                    .last()
                    .is_none_or(|line| line.separator.end != text.len())
            {
                return Err(text_layout::error(
                    "diagram emission differs from its measured source node",
                ));
            }
            self.emit_glyphs(plan.block.glyphs)?;
            contain_exact(plan.block.logical, plan.interior, id)?;
            let logical = parent.bounds(plan.block.logical)?;
            contain_exact(logical, frame, id)?;
            contain_exact(logical, canvas, id)?;
            contour_losses.extend(plan.losses.clone());
            (
                plan.block.commands.clone(),
                plan.block.bounds,
                format!(
                    "exact-face bounded diagram node wrapping {}; fixed 13px Medium; per-line advance centered; full logical/ink block centered before four-decimal projection; source byte coverage [{}]; original text: {text}",
                    self.layout_profile, plan.block.details
                ),
            )
        } else if let Some((owner, index, plan)) = self.registered_category(id) {
            if owner != view_id
                || !self
                    .state
                    .borrow_mut()
                    .used_categories
                    .entry(owner)
                    .or_default()
                    .insert(index)
            {
                return Err(text_layout::error(
                    "category item has wrong or repeated semantic owner",
                ));
            }
            let block = &plan.blocks[index];
            self.emit_glyphs(block.outline.glyphs)?;
            let logical = parent.bounds(block.outline.logical)?;
            contain_exact(logical, frame, id)?;
            contain_exact(logical, canvas, id)?;
            if block.outline.logical.x < block.cell[0] + 4.
                || block.outline.logical.x + block.outline.logical.width > block.cell[1] - 4.
            {
                return Err(text_layout::error(
                    "category lost its explicit Band-cell containment",
                ));
            }
            contour_losses.extend(block.losses.clone());
            (
                block.outline.commands.clone(),
                block.outline.bounds,
                format!(
                    "exact-face bounded {} wrapping {}; domain index {index}; source byte coverage [{}]; original text: {text}",
                    plan.role.name(),
                    self.layout_profile,
                    block.outline.details
                ),
            )
        } else if let Some((owner, plan)) = self.registered_title(id) {
            if owner != view_id || !self.state.borrow_mut().used_title_targets.insert(owner) {
                return Err(text_layout::error(
                    "chart title emitted with wrong or repeated semantic owner",
                ));
            }
            self.emit_glyphs(plan.block.glyphs)?;
            let logical = parent.bounds(plan.block.logical)?;
            contain(logical, frame, id)?;
            contain(logical, canvas, id)?;
            contour_losses.extend(plan.losses.clone());
            (
                plan.block.commands.clone(),
                plan.block.bounds,
                format!(
                    "exact-face bounded chart.title wrapping {}; source byte coverage [{}]; original text: {text}",
                    self.layout_profile, plan.block.details
                ),
            )
        } else if let Some(target) = self.target(view_id, &origin.hir_node)
            && id == &format!("{view_id}/{}", target.node_id)
        {
            if !self
                .state
                .borrow_mut()
                .used_targets
                .insert((view_id.to_owned(), target.node_id.clone()))
            {
                return Err(text_layout::error(
                    "wrapping target was emitted more than once",
                ));
            }
            let wrapped = self.wrapped_outline(
                target,
                text,
                *font_size,
                *weight,
                *position,
                *anchor,
                id,
                contour_losses,
                None,
                None,
                false,
            )?;
            self.emit_glyphs(wrapped.glyphs)?;
            let logical = parent.bounds(wrapped.logical)?;
            contain(logical, frame, id)?;
            contain(logical, canvas, id)?;
            (
                wrapped.commands,
                wrapped.bounds,
                format!(
                    "exact-face bounded wrapping {}; source byte coverage [{}]; original text: {text}",
                    text_layout::TEXT_LAYOUT_PROFILE,
                    wrapped.details
                ),
            )
        } else {
            let run = self.shape(text, *font_size, *weight)?;
            {
                let mut state = self.state.borrow_mut();
                state.emitted_glyphs = state
                    .emitted_glyphs
                    .checked_add(run.glyphs)
                    .filter(|n| *n <= self.limits.max_glyphs)
                    .ok_or_else(|| error("0003", "whole-call emitted glyph limit exceeded"))?;
            }
            let (commands, bounds) = self.project(&run, *position, *anchor)?;
            let before = dimensional_contours(&run.commands);
            let after = dimensional_contours_grid(&commands);
            if before > 0 && after == 0 {
                return Err(error(
                    "0004",
                    "nonempty text contours collapse at SVG four-decimal precision",
                ));
            }
            if after < before {
                contour_losses.push(LossRecord{source:id.clone(),target:"scene2d".into(),fidelity:LoweringFidelity::VisuallyApproximate,reason:format!("{} sub-contours collapse at four-decimal outline precision; remaining label ink is retained",before-after)});
            }
            if run.bounds.width > 0.
                && run.bounds.height > 0.
                && (bounds.width == 0. || bounds.height == 0.)
            {
                return Err(error(
                    "0004",
                    "nonempty text ink collapses at SVG four-decimal precision",
                ));
            }
            (
                commands,
                bounds,
                format!("exact-face single-line outlines; original text: {text}"),
            )
        };
        let world = parent.bounds(bounds)?;
        contain(world, frame, id)?;
        contain(world, canvas, id)?;
        let mut path_origin = origin.clone();
        path_origin.generated_by = "shape-measured-text".into();
        path_origin.explanation = format!("{}; {explanation}", origin.explanation);
        *node = SceneNode::Path {
            id: id.clone(),
            bounds,
            origin: path_origin,
            commands,
            style: ResolvedStyle {
                fill: color.clone(),
                stroke: Color::transparent(),
                stroke_width: 0.,
                opacity: 1.,
            },
            marker_end: false,
        };
        Ok(())
    }
}
fn coordinate(value: f64) -> VizResult<()> {
    if !value.is_finite() || value.abs() > MAX_COORDINATE {
        Err(error(
            "0004",
            "text coordinate exceeds finite +/-1000000 scene-unit range",
        ))
    } else {
        Ok(())
    }
}
fn contain_exact(b: Rect, frame: Rect, id: &str) -> VizResult<()> {
    if b.x < frame.x
        || b.y < frame.y
        || b.x + b.width > frame.x + frame.width
        || b.y + b.height > frame.y + frame.height
    {
        return Err(text_layout::error(format!(
            "text block {id:?} overflows its serialized container/view/canvas"
        )));
    }
    Ok(())
}
fn contain(b: Rect, frame: Rect, id: &str) -> VizResult<()> {
    if b.width == 0. && b.height == 0. {
        return Ok(());
    }
    if b.x < frame.x - 1e-6
        || b.y < frame.y - 1e-6
        || b.x + b.width > frame.x + frame.width + 1e-6
        || b.y + b.height > frame.y + frame.height + 1e-6
    {
        return Err(error(
            "0006",
            format!(
                "text {id:?} overflows its view/canvas; enlarge the frame or shorten the source text"
            ),
        ));
    }
    Ok(())
}
fn translate(command: PathCommand, x: f64, y: f64) -> VizResult<PathCommand> {
    let point = |p: Point| -> VizResult<Point> {
        let p = Point {
            x: p.x + x,
            y: p.y + y,
        };
        coordinate(p.x)?;
        coordinate(p.y)?;
        Ok(Point {
            x: svg_number(p.x),
            y: svg_number(p.y),
        })
    };
    Ok(match command {
        PathCommand::Move { to } => PathCommand::Move { to: point(to)? },
        PathCommand::Line { to } => PathCommand::Line { to: point(to)? },
        PathCommand::Cubic {
            control1,
            control2,
            to,
        } => PathCommand::Cubic {
            control1: point(control1)?,
            control2: point(control2)?,
            to: point(to)?,
        },
        PathCommand::Close => PathCommand::Close,
    })
}
#[derive(Clone, Copy)]
struct Affine([f64; 6]);
impl Affine {
    const IDENTITY: Self = Self([1., 0., 0., 1., 0., 0.]);
    fn then(self, t: Transform2D) -> VizResult<Self> {
        for n in [
            t.translate.x,
            t.translate.y,
            t.scale.x,
            t.scale.y,
            t.rotate_degrees,
        ] {
            coordinate(n)?;
            if svg_number(n) != n {
                return Err(error(
                    "0004",
                    "measured text group transforms must be exactly representable at SVG four-decimal precision",
                ));
            }
        }
        if t.scale.x == 0. || t.scale.y == 0. {
            return Err(error(
                "0004",
                "measured text does not support a collapsing zero scale",
            ));
        }
        let (s, c) = t.rotate_degrees.to_radians().sin_cos();
        let q = [
            c * t.scale.x,
            s * t.scale.x,
            -s * t.scale.y,
            c * t.scale.y,
            t.translate.x,
            t.translate.y,
        ];
        let p = self.0;
        let r = Self([
            p[0] * q[0] + p[2] * q[1],
            p[1] * q[0] + p[3] * q[1],
            p[0] * q[2] + p[2] * q[3],
            p[1] * q[2] + p[3] * q[3],
            p[0] * q[4] + p[2] * q[5] + p[4],
            p[1] * q[4] + p[3] * q[5] + p[5],
        ]);
        for n in r.0 {
            coordinate(n)?;
        }
        Ok(r)
    }
    fn bounds(self, b: Rect) -> VizResult<Rect> {
        let p = self.0;
        let mut out = None;
        for (x, y) in [
            (b.x, b.y),
            (b.x + b.width, b.y),
            (b.x, b.y + b.height),
            (b.x + b.width, b.y + b.height),
        ] {
            let v = Point {
                x: p[0] * x + p[2] * y + p[4],
                y: p[1] * x + p[3] * y + p[5],
            };
            coordinate(v.x)?;
            coordinate(v.y)?;
            include(&mut out, v);
        }
        Ok(out.unwrap_or_default())
    }
}
fn include(bounds: &mut Option<Rect>, p: Point) {
    *bounds = Some(match *bounds {
        None => Rect {
            x: p.x,
            y: p.y,
            width: 0.,
            height: 0.,
        },
        Some(r) => {
            let x = r.x.min(p.x);
            let y = r.y.min(p.y);
            Rect {
                x,
                y,
                width: (r.x + r.width).max(p.x) - x,
                height: (r.y + r.height).max(p.y) - y,
            }
        }
    });
}
/// Bounds from emitted cubic geometry, including derivative roots, not character estimates.
struct Pen {
    commands: Vec<PathCommand>,
    bounds: Option<Rect>,
    current: Point,
    start: Point,
    scale: f64,
    tx: f64,
    ty: f64,
    limit: usize,
    failed: bool,
}
impl Pen {
    fn new(limit: usize) -> Self {
        Self {
            commands: Vec::new(),
            bounds: None,
            current: Point::default(),
            start: Point::default(),
            scale: 1.,
            tx: 0.,
            ty: 0.,
            limit,
            failed: false,
        }
    }
    fn p(&self, x: f32, y: f32) -> Point {
        Point {
            x: self.tx + f64::from(x) * self.scale,
            y: self.ty - f64::from(y) * self.scale,
        }
    }
    fn push(&mut self, c: PathCommand) {
        if self.commands.len() >= self.limit {
            self.failed = true;
        } else if !self.failed {
            self.commands.push(c);
        }
    }
    fn vertex(&mut self, p: Point) {
        if coordinate(p.x).is_err() || coordinate(p.y).is_err() {
            self.failed = true;
        }
        include(&mut self.bounds, p);
    }
    fn cubic(&mut self, a: Point, b: Point, to: Point) {
        for p in [self.current, a, b, to] {
            if coordinate(p.x).is_err() || coordinate(p.y).is_err() {
                self.failed = true;
            }
        }
        include_cubic(&mut self.bounds, self.current, a, b, to);
        self.push(PathCommand::Cubic {
            control1: a,
            control2: b,
            to,
        });
        self.current = to;
    }
}
impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.current = p;
        self.start = p;
        self.vertex(p);
        self.push(PathCommand::Move { to: p });
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.vertex(p);
        self.current = p;
        self.push(PathCommand::Line { to: p });
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let c = self.p(cx, cy);
        let p = self.p(x, y);
        let a = Point {
            x: self.current.x + (c.x - self.current.x) * 2. / 3.,
            y: self.current.y + (c.y - self.current.y) * 2. / 3.,
        };
        let b = Point {
            x: p.x + (c.x - p.x) * 2. / 3.,
            y: p.y + (c.y - p.y) * 2. / 3.,
        };
        self.cubic(a, b, p);
    }
    fn curve_to(&mut self, ax: f32, ay: f32, bx: f32, by: f32, x: f32, y: f32) {
        self.cubic(self.p(ax, ay), self.p(bx, by), self.p(x, y));
    }
    fn close(&mut self) {
        self.current = self.start;
        self.push(PathCommand::Close);
    }
}

/// Count bytes through a bounded sink before publishing a serialized artifact.
pub(crate) fn check_serialized_output<T: serde::Serialize>(
    value: &T,
    limit: usize,
) -> VizResult<()> {
    serde_json::to_writer_pretty(OutputBudget(limit), value)
        .map_err(|_| error("0003", "serialized output exceeds the output-byte limit"))
}

struct OutputBudget(usize);
impl std::io::Write for OutputBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("output limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn svg_number(n: f64) -> f64 {
    let n = if n.abs() < 0.000_000_1 { 0. } else { n };
    format!("{n:.4}")
        .parse()
        .expect("finite numeric formatting")
}
fn include_cubic(bounds: &mut Option<Rect>, from: Point, a: Point, b: Point, to: Point) {
    include(bounds, from);
    include(bounds, to);
    for axis in 0..2 {
        let v = |p: Point| if axis == 0 { p.x } else { p.y };
        let (p0, p1, p2, p3) = (v(from), v(a), v(b), v(to));
        let aa = -p0 + 3. * p1 - 3. * p2 + p3;
        let bb = 2. * (p0 - 2. * p1 + p2);
        let cc = p1 - p0;
        let mut roots = [f64::NAN; 2];
        if aa.abs() < 1e-12 {
            if bb.abs() > 1e-12 {
                roots[0] = -cc / bb;
            }
        } else {
            let d = bb * bb - 4. * aa * cc;
            if d >= 0. {
                roots[0] = (-bb + d.sqrt()) / (2. * aa);
                roots[1] = (-bb - d.sqrt()) / (2. * aa);
            }
        }
        for t in roots {
            if t > 0. && t < 1. {
                let u = 1. - t;
                include(
                    bounds,
                    Point {
                        x: u * u * u * from.x
                            + 3. * u * u * t * a.x
                            + 3. * u * t * t * b.x
                            + t * t * t * to.x,
                        y: u * u * u * from.y
                            + 3. * u * u * t * a.y
                            + 3. * u * t * t * b.y
                            + t * t * t * to.y,
                    },
                );
            }
        }
    }
}
fn path_bounds(commands: &[PathCommand]) -> Rect {
    let mut bounds = None;
    let mut current = Point::default();
    for c in commands {
        match *c {
            PathCommand::Move { to } | PathCommand::Line { to } => {
                include(&mut bounds, to);
                current = to;
            }
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => {
                include_cubic(&mut bounds, current, control1, control2, to);
                current = to;
            }
            PathCommand::Close => {}
        }
    }
    bounds.unwrap_or_default()
}
struct SourceText {
    bytes: usize,
    count: usize,
    limits: TextLimits,
}
impl SourceText {
    fn new(limits: TextLimits) -> Self {
        Self {
            bytes: 0,
            count: 0,
            limits,
        }
    }
    fn add(&mut self, s: &str) -> VizResult<()> {
        self.add_scoped(s, None)
    }
    fn add_scoped(&mut self, s: &str, target: Option<u32>) -> VizResult<()> {
        self.count += 1;
        if self.count > 1_000_000 || s.len() > self.limits.max_label_bytes {
            return Err(error(
                "0003",
                "source label count/byte limit exceeded before normalization",
            ));
        }
        self.bytes = self
            .bytes
            .checked_add(s.len())
            .filter(|n| *n <= self.limits.max_text_bytes)
            .ok_or_else(|| error("0003", "source text exceeds whole-call byte limit"))?;
        if let Some(target) = target {
            if text_layout::paragraphs(s)?.len() > target as usize {
                return Err(text_layout::error("hard breaks exceed target max_lines"));
            }
            return Ok(());
        }
        if s.chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        {
            return Err(error(
                "0004",
                "single-line text does not support newlines, tabs or controls; edit original HIR",
            ));
        }
        Ok(())
    }
    fn add_all<'a>(&mut self, values: impl IntoIterator<Item = &'a str>) -> VizResult<()> {
        for s in values {
            self.add(s)?;
        }
        Ok(())
    }
}

fn serialized_transform(t: Transform2D) -> VizResult<Transform2D> {
    for n in [
        t.translate.x,
        t.translate.y,
        t.rotate_degrees,
        t.scale.x,
        t.scale.y,
    ] {
        coordinate(n)?;
    }
    let q = Transform2D {
        translate: Point {
            x: svg_number(t.translate.x),
            y: svg_number(t.translate.y),
        },
        rotate_degrees: svg_number(t.rotate_degrees),
        scale: Point {
            x: svg_number(t.scale.x),
            y: svg_number(t.scale.y),
        },
    };
    if q.scale.x == 0. || q.scale.y == 0. {
        return Err(error(
            "0004",
            "text transform scale collapses under SVG serialization",
        ));
    }
    Ok(q)
}
// A contour whose control hull is collinear has no filled two-dimensional ink.
// Detect whole-label collapse; retain a loss record for disappearing sub-contours.
fn dimensional_contours(commands: &[PathCommand]) -> usize {
    let mut count = 0usize;
    let mut first = None;
    let mut second = None;
    let mut dimensional = false;
    for command in commands {
        if matches!(command, PathCommand::Move { .. }) {
            if dimensional {
                count += 1;
            }
            first = None;
            second = None;
            dimensional = false;
        }
        let mut add = |p: Point| {
            if let Some(a) = first {
                if let Some(b) = second {
                    let a: Point = a;
                    let b: Point = b;
                    if (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x) != 0. {
                        dimensional = true;
                    }
                } else if a != p {
                    second = Some(p);
                }
            } else {
                first = Some(p);
            }
        };
        match *command {
            PathCommand::Move { to } | PathCommand::Line { to } => add(to),
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => {
                add(control1);
                add(control2);
                add(to);
            }
            PathCommand::Close => {}
        }
    }
    if dimensional {
        count += 1;
    }
    count
}

fn serialized_rect(r: Rect) -> Rect {
    Rect {
        x: svg_number(r.x),
        y: svg_number(r.y),
        width: svg_number(r.width),
        height: svg_number(r.height),
    }
}

// Validate bounded directory spans for every declared face, and the horizontal
// metric/glyph tables used by the selected face. This is not validation of all
// unused OpenType tables or a hostile-font execution sandbox.
fn validate_font_structure(bytes: &[u8], index: u32) -> VizResult<u16> {
    let bad = || {
        error(
            "0002",
            "invalid required font table, glyph count or metric consistency",
        )
    };
    let u16at = |b: &[u8], i: usize| -> VizResult<u16> {
        Ok(u16::from_be_bytes(
            b.get(i..i + 2)
                .ok_or_else(bad)?
                .try_into()
                .expect("two bytes"),
        ))
    };
    let u32at = |b: &[u8], i: usize| -> VizResult<u32> {
        Ok(u32::from_be_bytes(
            b.get(i..i + 4)
                .ok_or_else(bad)?
                .try_into()
                .expect("four bytes"),
        ))
    };
    let offsets = if bytes.starts_with(b"ttcf") {
        let count = u32at(bytes, 8)? as usize;
        if count == 0 || count > 32 || index as usize >= count {
            return Err(bad());
        }
        (0..count)
            .map(|i| u32at(bytes, 12 + i * 4).map(|n| n as usize))
            .collect::<VizResult<Vec<_>>>()?
    } else {
        if index != 0 {
            return Err(bad());
        }
        vec![0]
    };
    for offset in offsets {
        let header = bytes.get(offset..).ok_or_else(bad)?;
        let tables = u16at(header, 4)? as usize;
        if tables == 0 || tables > 256 {
            return Err(bad());
        }
        let directory = header.get(12..12 + tables * 16).ok_or_else(bad)?;
        let mut tags = BTreeSet::new();
        for record in directory.as_chunks::<16>().0 {
            if !tags.insert(&record[..4]) {
                return Err(bad());
            }
            let start = u32at(record, 8)? as usize;
            let len = u32at(record, 12)? as usize;
            if start.checked_add(len).is_none_or(|end| end > bytes.len()) {
                return Err(bad());
            }
        }
    }
    let face = ttf_parser::Face::parse(bytes, index).map_err(|_| bad())?;
    let table = |tag: &[u8; 4]| {
        face.raw_face()
            .table(ttf_parser::Tag::from_bytes(tag))
            .ok_or_else(bad)
    };
    for tag in [b"COLR", b"CBDT", b"CBLC", b"sbix", b"SVG "] {
        if face
            .raw_face()
            .table(ttf_parser::Tag::from_bytes(tag))
            .is_some()
        {
            return Err(error(
                "0002",
                "color/bitmap/SVG font tables are outside the static monochrome outline profile",
            ));
        }
    }
    let head = table(b"head")?;
    let hhea = table(b"hhea")?;
    let maxp = table(b"maxp")?;
    let hmtx = table(b"hmtx")?;
    if head.len() < 54
        || u32at(head, 0)? != 0x00010000
        || u32at(head, 12)? != 0x5f0f3cf5
        || !(16..=16384).contains(&face.units_per_em())
        || hhea.len() < 36
        || u32at(hhea, 0)? != 0x00010000
        || u16at(hhea, 32)? != 0
    {
        return Err(bad());
    }
    let count = face.number_of_glyphs();
    let metrics = u16at(hhea, 34)?;
    let maxp_version = u32at(maxp, 0)?;
    if count == 0
        || metrics == 0
        || metrics > count
        || !matches!(maxp_version, 0x00005000 | 0x00010000)
        || (maxp_version == 0x00010000 && maxp.len() < 32)
    {
        return Err(bad());
    }
    if hmtx.len() < usize::from(metrics) * 4 + usize::from(count - metrics) * 2
        || face.tables().cmap.is_none()
    {
        return Err(bad());
    }
    if let Some(cff) = face.tables().cff.as_ref() {
        if cff.number_of_glyphs() != count || maxp_version != 0x00005000 {
            return Err(bad());
        }
    } else if face.tables().glyf.is_some() {
        if maxp_version != 0x00010000 {
            return Err(bad());
        }
        let glyf = table(b"glyf")?;
        let loca = table(b"loca")?;
        let format = u16at(head, 50)?;
        let mut previous = 0usize;
        for i in 0..=usize::from(count) {
            let offset = match format {
                0 => usize::from(u16at(loca, i * 2)?) * 2,
                1 => u32at(loca, i * 4)? as usize,
                _ => return Err(bad()),
            };
            if offset < previous || offset > glyf.len() {
                return Err(bad());
            }
            previous = offset;
        }
    } else {
        return Err(error(
            "0002",
            "selected font needs supported static TrueType or CFF1 outlines",
        ));
    }
    Ok(count)
}

pub(crate) fn deserialize_u32_number<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<u32, D::Error> {
    struct Integer;
    impl serde::de::Visitor<'_> for Integer {
        type Value = u32;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("an integer-valued unsigned 32-bit number")
        }
        fn visit_u64<E: serde::de::Error>(self, n: u64) -> Result<u32, E> {
            u32::try_from(n).map_err(|_| E::custom("integer out of range"))
        }
        fn visit_i64<E: serde::de::Error>(self, n: i64) -> Result<u32, E> {
            u32::try_from(n).map_err(|_| E::custom("integer out of range"))
        }
        fn visit_f64<E: serde::de::Error>(self, n: f64) -> Result<u32, E> {
            if n.is_finite() && n.fract() == 0. && (0.0..=u32::MAX as f64).contains(&n) {
                Ok(n as u32)
            } else {
                Err(E::custom(
                    "expected a finite integer-valued unsigned number",
                ))
            }
        }
    }
    d.deserialize_any(Integer)
}
fn deserialize_weight<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u16, D::Error> {
    u16::try_from(deserialize_u32_number(d)?).map_err(serde::de::Error::custom)
}

// Post-serialization coordinates lie on an exact 1/10000 grid. Integer
// cross products avoid binary-float noise inventing area in collinear contours.
fn dimensional_contours_grid(commands: &[PathCommand]) -> usize {
    let mut count = 0usize;
    let mut first: Option<(i128, i128)> = None;
    let mut second = None;
    let mut dimensional = false;
    for command in commands {
        if matches!(command, PathCommand::Move { .. }) {
            if dimensional {
                count += 1;
            }
            first = None;
            second = None;
            dimensional = false;
        }
        let mut add = |p: Point| {
            let p = (
                (p.x * 10000.).round() as i128,
                (p.y * 10000.).round() as i128,
            );
            if let Some(a) = first {
                if let Some(b) = second {
                    let b: (i128, i128) = b;
                    if (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0) != 0 {
                        dimensional = true;
                    }
                } else if p != a {
                    second = Some(p);
                }
            } else {
                first = Some(p);
            }
        };
        match *command {
            PathCommand::Move { to } | PathCommand::Line { to } => add(to),
            PathCommand::Cubic {
                control1,
                control2,
                to,
            } => {
                add(control1);
                add(control2);
                add(to);
            }
            PathCommand::Close => {}
        }
    }
    if dimensional {
        count += 1;
    }
    count
}

#[cfg(test)]
mod precision_tests {
    use super::*;
    #[test]
    fn heatmap_exact_grid_width_keeps_unrounded_ink_overhangs() {
        for left in [-900_000.123_4, -0.1234, 0.1234, 83.8833, 900_000.123_4] {
            let right = svg_number(left + 60.);
            assert_eq!(serialized_advance_envelope_width(left, right, None), 60.);
            assert_eq!(
                serialized_advance_envelope_width(
                    left,
                    right,
                    Some(Rect {
                        x: left + 1.,
                        y: 0.,
                        width: 58.,
                        height: 10.,
                    })
                ),
                60.
            );
            assert!(
                serialized_advance_envelope_width(
                    left,
                    right,
                    Some(Rect {
                        x: left - 0.0000001,
                        y: 0.,
                        width: 60.0000002,
                        height: 10.,
                    })
                ) > 60.
            );
        }
    }

    #[test]
    fn cubic_extrema_and_tiny_contour_loss_are_detected() {
        let c = vec![
            PathCommand::Move {
                to: Point { x: 0., y: 0. },
            },
            PathCommand::Cubic {
                control1: Point { x: 0., y: 1. },
                control2: Point { x: 1., y: 1. },
                to: Point { x: 1., y: 0. },
            },
            PathCommand::Close,
        ];
        assert_eq!(
            path_bounds(&c),
            Rect {
                x: 0.,
                y: 0.,
                width: 1.,
                height: 0.75
            }
        );
        assert_eq!(dimensional_contours(&c), 1);
        let tiny = vec![
            PathCommand::Move {
                to: Point { x: 0., y: 0. },
            },
            PathCommand::Line {
                to: Point { x: 0.00001, y: 0. },
            },
            PathCommand::Line {
                to: Point { x: 0., y: 0.00001 },
            },
            PathCommand::Close,
        ];
        assert_eq!(dimensional_contours(&tiny), 1);
        let rounded = tiny
            .into_iter()
            .map(|c| translate(c, 0., 0.).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(dimensional_contours_grid(&rounded), 0);
        let diagonal = vec![
            PathCommand::Move {
                to: Point { x: 40.025, y: 70.0 },
            },
            PathCommand::Line {
                to: Point { x: 40.125, y: 69.9 },
            },
            PathCommand::Line {
                to: Point {
                    x: 40.075,
                    y: 69.95,
                },
            },
            PathCommand::Close,
        ];
        assert_eq!(dimensional_contours_grid(&diagonal), 0);
    }
    #[test]
    fn title_measurements_reuse_plan_without_charging_emitted_glyphs() {
        let context: TextContext = serde_json::from_str(include_str!(
            "../../../examples/text/wrapping-font-profile.json"
        ))
        .unwrap();
        let mut resources = FontResources::new();
        for (face, bytes) in [
            (
                &context.faces.regular,
                include_bytes!(
                    "../tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-Regular.otf"
                )
                .as_slice(),
            ),
            (
                &context.faces.medium,
                include_bytes!(
                    "../tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-Medium.otf"
                )
                .as_slice(),
            ),
            (
                &context.faces.bold,
                include_bytes!("../tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-Bold.otf")
                    .as_slice(),
            ),
        ] {
            resources.insert(&face.sha256, bytes.to_vec()).unwrap();
        }
        let layout = TextLayoutContext::new(vec![]).with_semantic_targets(vec![
            SemanticTextLayoutTarget::chart_title("c", 180., 5, 30.),
        ]);
        let mut limits = TextLimits::new();
        limits.max_wrap_candidates = 1;
        limits.max_layout_lines = 1;
        let session =
            TextSession::new_with_layout(&context, &resources, limits, Some(&layout)).unwrap();
        let frame = vizir_core::Frame {
            x: 0.,
            y: 0.,
            width: 600.,
            height: 400.,
        };
        let first = session
            .chart_title_plan("c", Some("AV"), frame)
            .unwrap()
            .unwrap();
        let second = session
            .chart_title_plan("c", Some("AV"), frame)
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(session.state.borrow().emitted_glyphs, 0);
        assert_eq!(session.state.borrow().layout_lines, 1);
        assert_eq!(session.state.borrow().wrap_candidates, 1);
        assert_eq!(first.block.glyphs, 2);
        session.emit_glyphs(first.block.glyphs).unwrap();
        assert_eq!(session.state.borrow().emitted_glyphs, 2);
        // Cache hits consume label-operation work and cannot be repeated forever.
        for _ in 0..4096 {
            if session.chart_title_plan("c", Some("AV"), frame).is_err() {
                return;
            }
        }
        panic!("unbounded title cache hits");
    }
    #[test]
    fn category_plan_reuse_is_bounded_and_emission_is_deferred() {
        category_cache(false);
    }
    #[test]
    fn heatmap_category_plan_reuse_is_bounded_and_emission_is_deferred() {
        category_cache(true);
    }
    fn category_cache(heatmap: bool) {
        let context: TextContext = serde_json::from_str(include_str!(
            "../../../examples/text/wrapping-font-profile.json"
        ))
        .unwrap();
        let mut resources = FontResources::new();
        for (face, bytes) in [
            (
                &context.faces.regular,
                include_bytes!(
                    "../tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-Regular.otf"
                )
                .as_slice(),
            ),
            (
                &context.faces.medium,
                include_bytes!(
                    "../tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-Medium.otf"
                )
                .as_slice(),
            ),
            (
                &context.faces.bold,
                include_bytes!("../tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-Bold.otf")
                    .as_slice(),
            ),
        ] {
            resources.insert(&face.sha256, bytes.to_vec()).unwrap();
        }
        let layout = if heatmap {
            TextLayoutContext::new(vec![]).with_heatmap_x_labels(vec![
                SemanticTextLayoutTarget::heatmap_x_category_labels("c", 100., 4, 16.),
            ])
        } else {
            TextLayoutContext::new(vec![]).with_category_labels(vec![
                SemanticTextLayoutTarget::bar_category_labels("c", 100., 4, 16.),
            ])
        };
        let mut limits = TextLimits::new();
        limits.max_wrap_candidates = 2;
        limits.max_layout_lines = 2;
        let session =
            TextSession::new_with_layout(&context, &resources, limits, Some(&layout)).unwrap();
        let frame = vizir_core::Frame {
            x: 0.,
            y: 0.,
            width: 600.,
            height: 400.,
        };
        let plot = [64., 50., 570., 338.];
        let labels = vec!["A".into(), "B".into()];
        session
            .category_allocation("c", frame, plot, &labels, "category")
            .unwrap();
        let first = session.state.borrow().category_views["c"].clone();
        session
            .category_allocation("c", frame, plot, &labels, "category")
            .unwrap();
        let second = session.state.borrow().category_views["c"].clone();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(session.state.borrow().layout_lines, 2);
        assert_eq!(session.state.borrow().wrap_candidates, 2);
        assert_eq!(session.state.borrow().emitted_glyphs, 0);
        let origin = vizir_core::Origin {
            hir_node: "c".into(),
            mir_node: "c".into(),
            data_key: None,
            data_lineage: vec![],
            generated_by: "test".into(),
            explanation: "semantic role test".into(),
        };
        let mut children = Vec::new();
        for (i, block) in first.blocks.iter().enumerate() {
            let node = SceneNode::Text {
                id: format!("opaque-label-{i}"),
                bounds: Rect::default(),
                origin: origin.clone(),
                position: block.position,
                text: block.source.clone(),
                font_size: 10.,
                anchor: TextAnchor::Middle,
                color: Color::hex("#000000"),
                weight: FontWeight::Regular,
            };
            session.register_category_node("c", i, &node).unwrap();
            children.push(node);
        }
        let scene = Scene2D {
            document_id: "test".into(),
            width: 600.,
            height: 400.,
            background: Color::transparent(),
            nodes: vec![SceneNode::Group {
                id: "c".into(),
                bounds: Rect {
                    x: 0.,
                    y: 0.,
                    width: 600.,
                    height: 400.,
                },
                origin,
                transform: Transform2D::default(),
                opacity: 1.,
                children,
            }],
            losses: vec![],
        };
        session.outline_scene(scene).unwrap();
        assert_eq!(session.state.borrow().emitted_glyphs, 2);
        // Every cached whole-domain lookup remains a metered operation.
        for _ in 0..4096 {
            if session
                .category_allocation("c", frame, plot, &labels, "category")
                .is_err()
            {
                return;
            }
        }
        panic!("unbounded category cache lookups");
    }
    #[test]
    fn diagram_plan_reuse_is_bounded_and_emission_is_deferred() {
        let context: TextContext = serde_json::from_str(include_str!(
            "../../../examples/text/wrapping-font-profile.json"
        ))
        .unwrap();
        let mut resources = FontResources::new();
        for (face, bytes) in [
            (
                &context.faces.regular,
                include_bytes!(
                    "../tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-Regular.otf"
                )
                .as_slice(),
            ),
            (
                &context.faces.medium,
                include_bytes!(
                    "../tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-Medium.otf"
                )
                .as_slice(),
            ),
            (
                &context.faces.bold,
                include_bytes!("../tests/fixtures/wrapping-fonts/VizIRWrappingFixtureSC-Bold.otf")
                    .as_slice(),
            ),
        ] {
            resources.insert(&face.sha256, bytes.to_vec()).unwrap();
        }
        let layout = TextLayoutContext::new(vec![])
            .with_diagram_targets(vec![DiagramTextLayoutTarget::new("d", "n", 134., 2, 20.)]);
        let mut limits = TextLimits::new();
        limits.max_wrap_candidates = 2;
        limits.max_layout_lines = 2;
        let session =
            TextSession::new_with_layout(&context, &resources, limits, Some(&layout)).unwrap();
        let center = Point {
            x: 200.123456,
            y: 180.654321,
        };
        let bounds = Rect {
            x: center.x - 75.,
            y: center.y - 31.,
            width: 150.,
            height: 62.,
        };
        let first = session
            .diagram_plan("d", "n", "A\nB", center, bounds)
            .unwrap()
            .unwrap();
        let commands = session.state.borrow().emitted_commands;
        let second = session
            .diagram_plan("d", "n", "A\nB", center, bounds)
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(session.state.borrow().layout_lines, 2);
        assert_eq!(session.state.borrow().wrap_candidates, 2);
        assert_eq!(session.state.borrow().emitted_glyphs, 0);
        assert_eq!(session.state.borrow().emitted_commands, commands);
        let position = session
            .register_diagram_label("d", "n", "A\nB", center, bounds, "opaque-label")
            .unwrap()
            .unwrap();
        let origin = vizir_core::Origin {
            hir_node: "n".into(),
            mir_node: "n".into(),
            data_key: None,
            data_lineage: vec![],
            generated_by: "test".into(),
            explanation: "semantic role test".into(),
        };
        let label = SceneNode::Text {
            id: "opaque-label".into(),
            bounds: Rect::default(),
            origin: origin.clone(),
            position,
            text: "A\nB".into(),
            font_size: 13.,
            anchor: TextAnchor::Middle,
            color: Color::hex("#000000"),
            weight: FontWeight::Medium,
        };
        let scene = Scene2D {
            document_id: "test".into(),
            width: 600.,
            height: 400.,
            background: Color::transparent(),
            nodes: vec![SceneNode::Group {
                id: "d".into(),
                bounds: Rect {
                    x: 0.,
                    y: 0.,
                    width: 600.,
                    height: 400.,
                },
                origin,
                transform: Transform2D::default(),
                opacity: 1.,
                children: vec![label],
            }],
            losses: vec![],
        };
        let scene = session.outline_scene(scene).unwrap();
        assert!(
            matches!(&scene.nodes[0], SceneNode::Group { children, .. } if matches!(&children[0], SceneNode::Path { id, .. } if id == "opaque-label"))
        );
        assert_eq!(session.state.borrow().emitted_glyphs, 2);
        assert_eq!(session.state.borrow().emitted_commands, commands);
        for _ in 0..4096 {
            if session
                .diagram_plan("d", "n", "A\nB", center, bounds)
                .is_err()
            {
                return;
            }
        }
        panic!("unbounded diagram cache lookups");
    }
}
