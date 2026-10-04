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
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "deserialize_limits"
    )]
    pub limits: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lowering: BTreeMap<String, CapabilityStatus>,
    pub unsupported_policy: UnsupportedPolicy,
}

// Do not let a repeated key silently replace a stricter advertised bound.
// Keep the existing u64 value parser and public/map schema representation.
fn deserialize_limits<'de, D>(deserializer: D) -> Result<BTreeMap<String, u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct LimitsVisitor;

    impl<'de> serde::de::Visitor<'de> for LimitsVisitor {
        type Value = BTreeMap<String, u64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a map of unique limit names to unsigned 64-bit integers")
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: serde::de::MapAccess<'de>,
        {
            let mut limits = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, u64>()? {
                if limits.insert(key.clone(), value).is_some() {
                    return Err(serde::de::Error::custom(format!("duplicate limit {key:?}")));
                }
            }
            Ok(limits)
        }
    }

    deserializer.deserialize_map(LimitsVisitor)
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
    let mut decisions: Vec<_> = scene_capability_requirements(scene)
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
    if !matches!(
        capabilities.accepted_ir.as_str(),
        "scene2d" | "scene2d-through-svg"
    ) {
        decisions.push(CapabilityDecision {
            source: scene.document_id.clone(),
            feature: "accepted_ir".to_owned(),
            status: CapabilityStatus::Error,
            reason: format!(
                "backend accepts {:?}, not the current Scene2D 0.1 contract (scene2d or scene2d-through-svg)",
                capabilities.accepted_ir
            ),
        });
    }
    if !capabilities.limits.is_empty() {
        let usage = scene_usage(scene);
        for (key, limit) in &capabilities.limits {
            let observed = match key.as_str() {
                "max-nodes" => usage.nodes,
                "max-clip-depth" => usage.max_clip_depth,
                _ => {
                    decisions.push(CapabilityDecision {
                        source: scene.document_id.clone(),
                        feature: format!("limit.{key}"),
                        status: CapabilityStatus::Error,
                        reason: format!("unsupported limit {key:?} for Scene2D negotiation"),
                    });
                    continue;
                }
            };
            if observed > *limit {
                decisions.push(CapabilityDecision {
                    source: scene.document_id.clone(),
                    feature: format!("limit.{key}"),
                    status: CapabilityStatus::Error,
                    reason: format!(
                        "Scene2D observed {observed} exceeds backend limit {key:?} of {limit}"
                    ),
                });
            }
        }
    }
    decisions
        .sort_by(|left, right| (&left.source, &left.feature).cmp(&(&right.source, &right.feature)));
    Ok(CapabilityReport {
        backend: capabilities.backend.clone(),
        backend_version: capabilities.version.clone(),
        document_id: scene.document_id.clone(),
        accepted_ir: capabilities.accepted_ir.clone(),
        decisions,
    })
}

struct SceneUsage {
    nodes: u64,
    max_clip_depth: u64,
}

fn scene_usage(scene: &Scene2D) -> SceneUsage {
    let mut usage = SceneUsage {
        nodes: 0,
        // Scene2D 0.1 has no clip construct. Groups/transforms are not clips.
        // Keep the match below exhaustive so new node kinds require review.
        max_clip_depth: 0,
    };
    let mut pending = vec![scene.nodes.as_slice()];
    while let Some(nodes) = pending.pop() {
        for node in nodes {
            usage.nodes = usage.nodes.saturating_add(1);
            match node {
                SceneNode::Group { children, .. } => pending.push(children),
                SceneNode::Rect { .. }
                | SceneNode::Circle { .. }
                | SceneNode::Line { .. }
                | SceneNode::Path { .. }
                | SceneNode::Text { .. } => {}
            }
        }
    }
    usage
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
#[path = "capability_tests.rs"]
mod tests;
