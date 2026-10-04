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

use crate::text_layout::{self, TextLayoutContext, TextLayoutTarget};

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
}
pub(crate) struct TextSession {
    limits: TextLimits,
    state: RefCell<State>,
    targets: BTreeMap<String, BTreeMap<String, TextLayoutTarget>>,
    face_metrics: BTreeMap<u16, (f64, f64)>,
}
impl TextSession {
    pub(crate) fn new_with_layout(
        context: &TextContext,
        resources: &FontResources,
        limits: TextLimits,
        layout: Option<&TextLayoutContext>,
    ) -> VizResult<Self> {
        context.validate()?;
        let mut targets: BTreeMap<String, BTreeMap<String, TextLayoutTarget>> = BTreeMap::new();
        if let Some(layout) = layout {
            layout.validate()?;
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
            }),
        })
    }
    pub(crate) fn preflight_document(&self, document: &vizir_core::Document) -> VizResult<()> {
        let mut strings = SourceText::new(self.limits);
        let mut geometry = Vec::new();
        let mut found_targets = BTreeSet::new();
        for view in &document.views {
            match view {
                vizir_core::View::Scatter(c) => {
                    strings.add_all(c.title.as_deref())?;
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
                    strings.add_all(c.title.as_deref())?;
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
                vizir_core::View::Bar(c) => {
                    strings.add_all(c.title.as_deref())?;
                    strings.add(c.category.label.as_deref().unwrap_or(&c.category.field))?;
                    strings.add(c.value.label.as_deref().unwrap_or(&c.value.field))?;
                    if let Some(data) = document.datasets.get(&c.dataset) {
                        for row in &data.rows {
                            for field in std::iter::once(&c.category.field)
                                .chain(c.color.as_ref().map(|c| &c.field))
                            {
                                if let Some(serde_json::Value::String(s)) = row.get(field) {
                                    strings.add(s)?;
                                }
                            }
                        }
                    }
                }
                vizir_core::View::Diagram(c) => {
                    strings.add_all(c.title.as_deref())?;
                    for n in &c.nodes {
                        strings.add(&n.label)?;
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
                vizir_core::GeometryNode::Text { text, .. } => strings.add_scoped(text, target)?,
                _ => {}
            }
        }
        self.check_targets(&found_targets)?;
        Ok(())
    }
    pub(crate) fn preflight_mir(&self, mir: &vizir_core::VizMir) -> VizResult<()> {
        let mut strings = SourceText::new(self.limits);
        let mut geometry = Vec::new();
        let mut found_targets = BTreeSet::new();
        for view in &mir.views {
            match view {
                vizir_core::MirView::Chart(c) => {
                    strings.add_all(c.title.as_deref())?;
                    for guide in &c.guides {
                        strings.add(&guide.label)?;
                    }
                    for scale in &c.scales {
                        match scale {
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
                        strings.add(&n.label)?;
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
                    strings.add_scoped(text, target)?
                }
                _ => {}
            }
        }
        self.check_targets(&found_targets)?;
        Ok(())
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
                    let r = self.shape(text, *font_size, *weight)?;
                    let (_, b) = self.project(&r, *position, *anchor)?;
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
                    if right - left > target.max_width {
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
                if before > 0 && after == 0 {
                    return Err(error(
                        "0004",
                        "nonempty wrapped line contours collapse at SVG four-decimal precision",
                    ));
                }
                if after < before {
                    losses.push(LossRecord { source: id.into(), target: "scene2d".into(), fidelity: LoweringFidelity::VisuallyApproximate, reason: format!("{} wrapped-line sub-contours collapse at four-decimal outline precision", before-after) });
                }
                if let Some(previous) = lines.iter().rev().find(|line| !line.commands.is_empty())
                    && !selected.commands.is_empty()
                    && previous.bounds.y + previous.bounds.height > selected.bounds.y
                {
                    return Err(text_layout::error(format!(
                        "text {id:?} has vertically overlapping line ink; increase explicit line_height"
                    )));
                }
                {
                    let mut state = self.state.borrow_mut();
                    state.emitted_glyphs = state
                        .emitted_glyphs
                        .checked_add(selected.run.glyphs)
                        .filter(|n| *n <= self.limits.max_glyphs)
                        .ok_or_else(|| error("0003", "whole-call emitted glyph limit exceeded"))?;
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
        for line in lines {
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
        let (commands, bounds, explanation) = if let Some(target) =
            self.target(view_id, &origin.hir_node)
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
            )?;
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
    fn add_scoped(&mut self, s: &str, target: Option<&TextLayoutTarget>) -> VizResult<()> {
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
            if text_layout::paragraphs(s)?.len() > target.max_lines as usize {
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
}
