use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{LoweringFidelity, Scene2D, SceneNode, VizError, VizResult};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum UnsupportedPolicy {
    Error,
    Report,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BackendCapabilities {
    pub backend: String,
    pub version: String,
    pub accepted_ir: String,
    pub supports: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub unsupported: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub limits: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lowering: BTreeMap<String, CapabilityStatus>,
    pub unsupported_policy: UnsupportedPolicy,
}

impl BackendCapabilities {
    pub fn supports(&self, feature: &str) -> bool {
        self.supports.contains(feature)
    }

    pub fn validate(&self) -> VizResult<()> {
        if self.backend.is_empty() || self.version.is_empty() || self.accepted_ir.is_empty() {
            return Err(VizError::Diagnostic(
                "VIZ-CAP-0003: backend, version, and accepted_ir must be non-empty".to_owned(),
            ));
        }
        if let Some(feature) = self.supports.intersection(&self.unsupported).next() {
            return Err(VizError::Diagnostic(format!(
                "VIZ-CAP-0004: feature {feature:?} is both supported and unsupported"
            )));
        }
        if let Some(feature) = self
            .lowering
            .keys()
            .find(|feature| !self.supports.contains(*feature))
        {
            return Err(VizError::Diagnostic(format!(
                "VIZ-CAP-0005: lowering strategy references undeclared feature {feature:?}"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRequirement {
    pub source: String,
    pub feature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityStatus {
    Exact,
    Equivalent,
    Approximate,
    Rasterized,
    Dropped,
    Error,
}

impl CapabilityStatus {
    pub fn fidelity(&self) -> LoweringFidelity {
        match self {
            Self::Exact => LoweringFidelity::Lossless,
            Self::Equivalent => LoweringFidelity::SemanticallyEquivalent,
            Self::Approximate => LoweringFidelity::VisuallyApproximate,
            Self::Rasterized => LoweringFidelity::Rasterized,
            Self::Dropped | Self::Error => LoweringFidelity::Dropped,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDecision {
    pub source: String,
    pub feature: String,
    pub status: CapabilityStatus,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityReport {
    pub backend: String,
    pub backend_version: String,
    pub document_id: String,
    pub accepted_ir: String,
    pub decisions: Vec<CapabilityDecision>,
}

impl CapabilityReport {
    pub fn is_accepted(&self) -> bool {
        self.decisions
            .iter()
            .all(|decision| decision.status != CapabilityStatus::Error)
    }

    pub fn require_accepted(&self) -> VizResult<()> {
        if self.is_accepted() {
            return Ok(());
        }
        let failures = self
            .decisions
            .iter()
            .filter(|decision| decision.status == CapabilityStatus::Error)
            .map(|decision| format!("{} requires {}", decision.source, decision.feature))
            .collect::<Vec<_>>()
            .join(", ");
        Err(VizError::Diagnostic(format!(
            "VIZ-CAP-0002: backend {:?} cannot lower: {failures}",
            self.backend
        )))
    }
}

pub fn scene_capability_requirements(scene: &Scene2D) -> Vec<CapabilityRequirement> {
    let mut requirements = BTreeSet::new();
    requirements.insert(CapabilityRequirement {
        source: scene.document_id.clone(),
        feature: "scene.2d".to_owned(),
    });
    if scene.background.0 == "transparent" || scene.background.0.len() == 9 {
        requirements.insert(CapabilityRequirement {
            source: scene.document_id.clone(),
            feature: "paint.alpha".to_owned(),
        });
    }
    for node in &scene.nodes {
        collect_node_requirements(node, &mut requirements);
    }
    requirements.into_iter().collect()
}

pub fn negotiate_scene(
    scene: &Scene2D,
    capabilities: &BackendCapabilities,
) -> VizResult<CapabilityReport> {
    capabilities.validate()?;
    let decisions = scene_capability_requirements(scene)
        .into_iter()
        .map(|requirement| {
            if capabilities.supports(&requirement.feature) {
                let status = capabilities
                    .lowering
                    .get(&requirement.feature)
                    .cloned()
                    .unwrap_or(CapabilityStatus::Exact);
                CapabilityDecision {
                    source: requirement.source,
                    feature: requirement.feature,
                    reason: match status {
                        CapabilityStatus::Exact => "backend declares exact support".to_owned(),
                        _ => "backend declares an explicit lowering strategy".to_owned(),
                    },
                    status,
                }
            } else {
                CapabilityDecision {
                    source: requirement.source,
                    feature: requirement.feature,
                    status: CapabilityStatus::Error,
                    reason: "backend does not declare support and no fallback was selected"
                        .to_owned(),
                }
            }
        })
        .collect();
    Ok(CapabilityReport {
        backend: capabilities.backend.clone(),
        backend_version: capabilities.version.clone(),
        document_id: scene.document_id.clone(),
        accepted_ir: capabilities.accepted_ir.clone(),
        decisions,
    })
}

pub fn capability_schema() -> serde_json::Value {
    with_meta_schema(
        serde_json::to_value(schemars::schema_for!(BackendCapabilities))
            .expect("capability schema must serialize"),
    )
}

fn with_meta_schema(mut schema: serde_json::Value) -> serde_json::Value {
    schema
        .as_object_mut()
        .expect("root schema must be an object")
        .insert(
            "$schema".to_owned(),
            serde_json::Value::String("https://json-schema.org/draft/2020-12/schema".to_owned()),
        );
    schema
}

fn collect_node_requirements(node: &SceneNode, requirements: &mut BTreeSet<CapabilityRequirement>) {
    let feature = match node {
        SceneNode::Group { .. } => "scene.2d.group",
        SceneNode::Rect { .. } => "scene.2d.rect",
        SceneNode::Circle { .. } => "scene.2d.circle",
        SceneNode::Line { .. } => "scene.2d.line",
        SceneNode::Path { .. } => "scene.2d.path",
        SceneNode::Text { .. } => "scene.2d.text",
    };
    requirements.insert(CapabilityRequirement {
        source: node.id().to_owned(),
        feature: feature.to_owned(),
    });
    let requires_alpha = match node {
        SceneNode::Group { opacity, .. } => *opacity < 1.0,
        SceneNode::Rect { style, .. }
        | SceneNode::Circle { style, .. }
        | SceneNode::Line { style, .. }
        | SceneNode::Path { style, .. } => {
            style.opacity < 1.0
                || color_has_alpha(&style.fill.0)
                || color_has_alpha(&style.stroke.0)
        }
        SceneNode::Text { color, .. } => color_has_alpha(&color.0),
    };
    if requires_alpha {
        requirements.insert(CapabilityRequirement {
            source: node.id().to_owned(),
            feature: "paint.alpha".to_owned(),
        });
    }
    match node {
        SceneNode::Group {
            transform,
            children,
            ..
        } => {
            if transform != &crate::Transform2D::default() {
                requirements.insert(CapabilityRequirement {
                    source: node.id().to_owned(),
                    feature: "scene.2d.transform".to_owned(),
                });
            }
            for child in children {
                collect_node_requirements(child, requirements);
            }
        }
        SceneNode::Line {
            marker_end: true, ..
        }
        | SceneNode::Path {
            marker_end: true, ..
        } => {
            requirements.insert(CapabilityRequirement {
                source: node.id().to_owned(),
                feature: "paint.marker-end".to_owned(),
            });
        }
        _ => {}
    }
}

fn color_has_alpha(value: &str) -> bool {
    value == "transparent" || value.len() == 9
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, FontWeight, Origin, Point, Rect, ResolvedStyle, TextAnchor, Transform2D};

    fn minimal_scene() -> Scene2D {
        Scene2D {
            document_id: "doc".to_owned(),
            width: 10.0,
            height: 10.0,
            background: Color::transparent(),
            nodes: vec![SceneNode::Rect {
                id: "rect".to_owned(),
                bounds: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                origin: Origin {
                    hir_node: "rect".to_owned(),
                    mir_node: "rect".to_owned(),
                    data_key: None,
                    data_lineage: Vec::new(),
                    generated_by: "test".to_owned(),
                    explanation: "test".to_owned(),
                },
                radius: 0.0,
                style: ResolvedStyle {
                    fill: Color::hex("#000000"),
                    stroke: Color::transparent(),
                    stroke_width: 0.0,
                    opacity: 1.0,
                },
            }],
            losses: Vec::new(),
        }
    }

    #[test]
    fn missing_capabilities_are_explicit_errors() {
        let scene = minimal_scene();
        let capabilities = BackendCapabilities {
            backend: "empty".to_owned(),
            version: "1".to_owned(),
            accepted_ir: "scene2d".to_owned(),
            supports: BTreeSet::new(),
            unsupported: BTreeSet::new(),
            limits: BTreeMap::new(),
            lowering: BTreeMap::new(),
            unsupported_policy: UnsupportedPolicy::Error,
        };
        let report = negotiate_scene(&scene, &capabilities).unwrap();
        assert!(!report.is_accepted());
        assert!(
            report
                .decisions
                .iter()
                .any(|decision| decision.feature == "scene.2d.rect"
                    && decision.status == CapabilityStatus::Error)
        );
    }

    #[test]
    fn contradictory_capability_profile_is_rejected() {
        let capabilities = BackendCapabilities {
            backend: "broken".to_owned(),
            version: "1".to_owned(),
            accepted_ir: "scene2d".to_owned(),
            supports: BTreeSet::from(["scene.2d".to_owned()]),
            unsupported: BTreeSet::from(["scene.2d".to_owned()]),
            limits: BTreeMap::new(),
            lowering: BTreeMap::new(),
            unsupported_policy: UnsupportedPolicy::Error,
        };
        assert!(capabilities.validate().is_err());
    }

    fn backend_with(supports: &[&str]) -> BackendCapabilities {
        BackendCapabilities {
            backend: "test-backend".to_owned(),
            version: "1".to_owned(),
            accepted_ir: "scene2d".to_owned(),
            supports: supports.iter().map(|feature| feature.to_string()).collect(),
            unsupported: BTreeSet::new(),
            limits: BTreeMap::new(),
            lowering: BTreeMap::new(),
            unsupported_policy: UnsupportedPolicy::Error,
        }
    }

    #[test]
    fn validate_rejects_blank_identity_and_undeclared_lowering_features() {
        let blank_identity = "VIZ-CAP-0003: backend, version, and accepted_ir must be non-empty";
        for blank in [
            (
                "backend",
                (|capabilities: &mut BackendCapabilities| {
                    capabilities.backend = String::new();
                }) as fn(&mut BackendCapabilities),
            ),
            ("version", |capabilities: &mut BackendCapabilities| {
                capabilities.version = String::new();
            }),
            ("accepted_ir", |capabilities: &mut BackendCapabilities| {
                capabilities.accepted_ir = String::new();
            }),
        ] {
            let (field, blank_it) = blank;
            let mut capabilities = backend_with(&["scene.2d"]);
            blank_it(&mut capabilities);
            assert_eq!(
                capabilities.validate().unwrap_err().to_string(),
                blank_identity,
                "{field}"
            );
        }

        let mut undeclared = backend_with(&["scene.2d"]);
        undeclared
            .lowering
            .insert("paint.alpha".to_owned(), CapabilityStatus::Rasterized);
        assert_eq!(
            undeclared.validate().unwrap_err().to_string(),
            "VIZ-CAP-0005: lowering strategy references undeclared feature \"paint.alpha\""
        );
    }

    #[test]
    fn negotiation_reports_declared_lowering_and_support_fidelity() {
        let scene = minimal_scene();
        let mut capabilities = backend_with(&["scene.2d", "paint.alpha", "scene.2d.rect"]);
        capabilities
            .lowering
            .insert("scene.2d.rect".to_owned(), CapabilityStatus::Rasterized);
        let report = negotiate_scene(&scene, &capabilities)
            .expect("declared features should negotiate without errors");

        assert_eq!(report.backend, "test-backend");
        assert_eq!(report.backend_version, "1");
        assert_eq!(report.document_id, "doc");
        assert_eq!(report.accepted_ir, "scene2d");
        // Decisions stay in deterministic (source, feature) order; an explicit
        // lowering strategy replaces the default Exact status and its reason.
        assert_eq!(
            report.decisions,
            vec![
                CapabilityDecision {
                    source: "doc".to_owned(),
                    feature: "paint.alpha".to_owned(),
                    status: CapabilityStatus::Exact,
                    reason: "backend declares exact support".to_owned(),
                },
                CapabilityDecision {
                    source: "doc".to_owned(),
                    feature: "scene.2d".to_owned(),
                    status: CapabilityStatus::Exact,
                    reason: "backend declares exact support".to_owned(),
                },
                CapabilityDecision {
                    source: "rect".to_owned(),
                    feature: "paint.alpha".to_owned(),
                    status: CapabilityStatus::Exact,
                    reason: "backend declares exact support".to_owned(),
                },
                CapabilityDecision {
                    source: "rect".to_owned(),
                    feature: "scene.2d.rect".to_owned(),
                    status: CapabilityStatus::Rasterized,
                    reason: "backend declares an explicit lowering strategy".to_owned(),
                },
            ]
        );
        assert!(report.is_accepted());
        assert!(report.require_accepted().is_ok());
    }

    #[test]
    fn require_accepted_names_every_unmet_requirement_in_order() {
        let scene = minimal_scene();
        let mut capabilities = backend_with(&[]);
        capabilities.backend = "empty".to_owned();
        let report = negotiate_scene(&scene, &capabilities)
            .expect("negotiation reports errors instead of failing");
        assert!(!report.is_accepted());
        assert_eq!(
            report.require_accepted().unwrap_err().to_string(),
            "VIZ-CAP-0002: backend \"empty\" cannot lower: \
             doc requires paint.alpha, doc requires scene.2d, \
             rect requires paint.alpha, rect requires scene.2d.rect"
        );
    }

    #[test]
    fn scene_requirements_collect_alpha_transform_and_marker_features() {
        let opaque = ResolvedStyle {
            fill: Color::hex("#112233"),
            stroke: Color::hex("#000000"),
            stroke_width: 0.0,
            opacity: 1.0,
        };
        let origin = Origin {
            hir_node: "node".to_owned(),
            mir_node: "node".to_owned(),
            data_key: None,
            data_lineage: Vec::new(),
            generated_by: "test".to_owned(),
            explanation: "test".to_owned(),
        };
        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        // An opaque 6-digit background must not request paint.alpha, while
        // group opacity, a 9-digit text color, a transform, and a marker-end
        // each add their own requirement, deduplicated and sorted by
        // (source, feature).
        let scene = Scene2D {
            document_id: "doc".to_owned(),
            width: 10.0,
            height: 10.0,
            background: Color::hex("#112233"),
            nodes: vec![SceneNode::Group {
                id: "g".to_owned(),
                bounds,
                origin: origin.clone(),
                transform: Transform2D {
                    translate: Point { x: 1.0, y: 0.0 },
                    rotate_degrees: 0.0,
                    scale: Point { x: 1.0, y: 1.0 },
                },
                opacity: 0.5,
                children: vec![
                    SceneNode::Line {
                        id: "l".to_owned(),
                        bounds,
                        origin: origin.clone(),
                        from: Point { x: 0.0, y: 0.0 },
                        to: Point { x: 1.0, y: 1.0 },
                        style: opaque,
                        marker_end: true,
                    },
                    SceneNode::Text {
                        id: "t".to_owned(),
                        bounds,
                        origin,
                        position: Point { x: 0.0, y: 0.0 },
                        text: "t".to_owned(),
                        font_size: 1.0,
                        anchor: TextAnchor::default(),
                        color: Color::hex("#11223344"),
                        weight: FontWeight::default(),
                    },
                ],
            }],
            losses: Vec::new(),
        };

        let requirement = |source: &str, feature: &str| CapabilityRequirement {
            source: source.to_owned(),
            feature: feature.to_owned(),
        };
        assert_eq!(
            scene_capability_requirements(&scene),
            vec![
                requirement("doc", "scene.2d"),
                requirement("g", "paint.alpha"),
                requirement("g", "scene.2d.group"),
                requirement("g", "scene.2d.transform"),
                requirement("l", "paint.marker-end"),
                requirement("l", "scene.2d.line"),
                requirement("t", "paint.alpha"),
                requirement("t", "scene.2d.text"),
            ]
        );
    }
}
