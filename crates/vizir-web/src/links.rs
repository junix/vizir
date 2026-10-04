//! Explicit, immutable-snapshot selection links. No inferred data joins.
use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vizir_core::{SceneNode, VizResult};

use crate::{
    BoundedBytes, CameraPolicy, ContextFields, ExplorerOptions, InteractionContext,
    MAX_METADATA_BYTES, MAX_SOURCE_STRING_BYTES, NodeIdentity, Viewport, error, validate_common,
};

pub const LINKS_FORMAT: &str = "vizir-selection-links/1";
pub const LINKED_FORMAT: &str = "vizir-interaction/2";
pub const LINKED_PROFILE: &str = "explorer-linked-v1";
pub const MAX_LINK_SPEC_BYTES: usize = 1024 * 1024;
pub const MAX_LINK_GROUPS: usize = 256;
pub const MAX_GROUP_MEMBERS: usize = 256;
pub const MAX_LINK_MEMBERS: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SelectionGroup {
    pub id: String,
    pub members: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SelectionLinks {
    pub format: String,
    pub document_id: String,
    pub scene_sha256: String,
    pub instance_key: String,
    pub groups: Vec<SelectionGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LinkedInteractionContext {
    pub format: String,
    pub profile: String,
    pub document_id: String,
    pub scene_sha256: String,
    pub runtime_payload_sha256: String,
    pub instance_key: String,
    pub home: Viewport,
    pub camera: CameraPolicy,
    pub nodes: Vec<NodeIdentity>,
    pub link_groups: Vec<SelectionGroup>,
}

pub(crate) struct LinkBudget {
    pub source_bytes: usize,
    pub json_bytes: usize,
}

fn group_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Reject membership ambiguity before copying, indexing, or serializing groups.
fn group_budget(groups: &[SelectionGroup]) -> VizResult<LinkBudget> {
    if groups.is_empty() || groups.len() > MAX_LINK_GROUPS {
        return Err(error("0101", "links require 1..256 groups"));
    }
    let mut count = 0usize;
    let mut bytes = 0usize;
    let mut group_ids = BTreeSet::new();
    let mut members = BTreeSet::new();
    for group in groups {
        if !group_id(&group.id) || !group_ids.insert(group.id.as_str()) {
            return Err(error("0101", "invalid or duplicate link group ID"));
        }
        if !(2..=MAX_GROUP_MEMBERS).contains(&group.members.len()) {
            return Err(error("0101", "each link group requires 2..256 members"));
        }
        count += group.members.len();
        if count > MAX_LINK_MEMBERS {
            return Err(error("0102", "aggregate link members exceed 4096"));
        }
        bytes += group.id.len();
        for member in &group.members {
            if member.len() > crate::MAX_STRING_BYTES || member.trim().is_empty() {
                return Err(error("0101", "invalid exact Scene-node link member"));
            }
            bytes += member.len();
            if bytes > MAX_LINK_SPEC_BYTES {
                return Err(error("0102", "link source strings exceed 1 MiB"));
            }
            if !members.insert(member.as_str()) {
                return Err(error(
                    "0101",
                    "duplicate or overlapping link member; membership must be disjoint",
                ));
            }
        }
    }
    let mut writer = BoundedBytes::new(MAX_LINK_SPEC_BYTES);
    serde_json::to_writer(&mut writer, groups)
        .map_err(|_| error("0102", "serialized link groups exceed 1 MiB"))?;
    Ok(LinkBudget {
        source_bytes: bytes,
        json_bytes: writer.bytes.len(),
    })
}

impl SelectionLinks {
    pub fn parse(bytes: &[u8]) -> VizResult<Self> {
        if bytes.len() > MAX_LINK_SPEC_BYTES {
            return Err(error("0102", "selection-link input exceeds 1 MiB"));
        }
        let result: Self = serde_json::from_slice(bytes)?;
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> VizResult<()> {
        self.budget().map(|_| ())
    }
    pub(crate) fn budget(&self) -> VizResult<LinkBudget> {
        if self.format != LINKS_FORMAT
            || self.document_id.len() > crate::MAX_STRING_BYTES
            || self.document_id.trim().is_empty()
            || !digest(&self.scene_sha256)
        {
            return Err(error(
                "0101",
                "invalid selection-link format or snapshot identity",
            ));
        }
        vizir_backend_svg::SvgRenderContext::new(&self.instance_key)?;
        let budget = group_budget(&self.groups)?;
        let mut writer = BoundedBytes::new(MAX_LINK_SPEC_BYTES);
        serde_json::to_writer(&mut writer, self)
            .map_err(|_| error("0102", "selection-link specification exceeds 1 MiB"))?;
        Ok(budget)
    }
    pub(crate) fn resolve(
        &self,
        document_id: &str,
        scene_sha256: &str,
        options: &ExplorerOptions,
        nodes: &[(&SceneNode, Option<&str>)],
    ) -> VizResult<Vec<SelectionGroup>> {
        // Callers have validated/bounded the spec before preparing the scene.
        if self.document_id != document_id
            || self.scene_sha256 != scene_sha256
            || self.instance_key != options.instance_key
        {
            return Err(error(
                "0103",
                "selection links do not match the exact exported document, scene digest and instance",
            ));
        }
        let indices: BTreeMap<_, _> = nodes
            .iter()
            .enumerate()
            .map(|(index, (node, _))| (node.id(), index))
            .collect();
        for group in &self.groups {
            if group
                .members
                .iter()
                .any(|member| !indices.contains_key(member.as_str()))
            {
                return Err(error(
                    "0103",
                    "selection link member is absent from the exact scene",
                ));
            }
        }
        derived_budget(
            document_id,
            scene_sha256,
            &options.instance_key,
            nodes.iter().map(|(node, _)| node.id()),
            &self.groups,
        )?;
        let mut groups = self.groups.clone();
        groups.sort_by(|a, b| a.id.cmp(&b.id));
        for group in &mut groups {
            group.members.sort_by_key(|id| indices[id.as_str()]);
        }
        Ok(groups)
    }
}

impl LinkedInteractionContext {
    pub(crate) fn new(context: InteractionContext, link_groups: Vec<SelectionGroup>) -> Self {
        let InteractionContext {
            document_id,
            scene_sha256,
            runtime_payload_sha256,
            instance_key,
            home,
            camera,
            nodes,
            ..
        } = context;
        Self {
            format: LINKED_FORMAT.into(),
            profile: LINKED_PROFILE.into(),
            document_id,
            scene_sha256,
            runtime_payload_sha256,
            instance_key,
            home,
            camera,
            nodes,
            link_groups,
        }
    }
    pub fn parse(bytes: &[u8]) -> VizResult<Self> {
        if bytes.len() > MAX_METADATA_BYTES {
            return Err(error("0102", "linked metadata input exceeds 4 MiB"));
        }
        let result: Self = serde_json::from_slice(bytes)?;
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> VizResult<()> {
        let source_bytes = validate_common(
            ContextFields {
                format: &self.format,
                profile: &self.profile,
                document_id: &self.document_id,
                scene_sha256: &self.scene_sha256,
                runtime_payload_sha256: &self.runtime_payload_sha256,
                instance_key: &self.instance_key,
                home: &self.home,
                camera: &self.camera,
                nodes: &self.nodes,
            },
            LINKED_FORMAT,
            LINKED_PROFILE,
        )?;
        let budget = group_budget(&self.link_groups)?;
        if source_bytes + budget.source_bytes > MAX_SOURCE_STRING_BYTES {
            return Err(error(
                "0102",
                "combined scene/link metadata strings exceed 4 MiB",
            ));
        }
        let indices: BTreeMap<_, _> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.scene_node_id.as_str(), i))
            .collect();
        let mut previous_group: Option<&str> = None;
        for group in &self.link_groups {
            if previous_group.is_some_and(|previous| previous >= group.id.as_str()) {
                return Err(error(
                    "0101",
                    "link metadata groups must be in canonical ASCII ID order",
                ));
            }
            previous_group = Some(&group.id);
            let mut previous_member = None;
            for member in &group.members {
                let index = *indices.get(member.as_str()).ok_or_else(|| {
                    error("0103", "link metadata member is absent from the snapshot")
                })?;
                if previous_member.is_some_and(|previous| previous >= index) {
                    return Err(error(
                        "0101",
                        "link metadata members must be in canonical Scene preorder",
                    ));
                }
                previous_member = Some(index);
            }
        }
        derived_budget(
            &self.document_id,
            &self.scene_sha256,
            &self.instance_key,
            self.nodes.iter().map(|n| n.scene_node_id.as_str()),
            &self.link_groups,
        )?;
        let mut writer = BoundedBytes::new(MAX_METADATA_BYTES);
        serde_json::to_writer(&mut writer, self)
            .map_err(|_| error("0102", "linked metadata exceeds 4 MiB"))?;
        let normalized_extra = self
            .nodes
            .iter()
            .filter(|n| n.origin.data_lineage.is_empty())
            .count()
            * 18;
        if writer.bytes.len() + normalized_extra > MAX_METADATA_BYTES {
            return Err(error("0102", "normalized linked metadata exceeds 4 MiB"));
        }
        Ok(())
    }
}

pub fn selection_links_schema() -> serde_json::Value {
    let mut schema =
        serde_json::to_value(schemars::schema_for!(SelectionLinks)).expect("schema serializes");
    schema["properties"]["format"]["const"] = LINKS_FORMAT.into();
    schema["properties"]["scene_sha256"]["pattern"] = r"^[0-9a-f]{64}$(?![\s\S])".into();
    schema["properties"]["instance_key"]["pattern"] = r"^[a-z][a-z0-9-]{0,31}$(?![\s\S])".into();
    apply_group_schema(&mut schema, "groups");
    schema
}
pub fn linked_interaction_schema() -> serde_json::Value {
    let schema = serde_json::to_value(schemars::schema_for!(LinkedInteractionContext))
        .expect("schema serializes");
    let mut schema = crate::context_schema(schema, LINKED_FORMAT, LINKED_PROFILE);
    apply_group_schema(&mut schema, "link_groups");
    schema
}
fn apply_group_schema(schema: &mut serde_json::Value, field: &str) {
    schema["properties"][field]["minItems"] = 1.into();
    schema["properties"][field]["maxItems"] = MAX_LINK_GROUPS.into();
    schema["$defs"]["SelectionGroup"]["properties"]["id"]["pattern"] =
        r"^[a-z][a-z0-9-]{0,63}$(?![\s\S])".into();
    schema["$defs"]["SelectionGroup"]["properties"]["members"]["minItems"] = 2.into();
    schema["$defs"]["SelectionGroup"]["properties"]["members"]["maxItems"] =
        MAX_GROUP_MEMBERS.into();
    schema["$defs"]["SelectionGroup"]["properties"]["members"]["uniqueItems"] = true.into();
}

// B is the escaped compact JSON size of the qualified binding. R(node) adds
// ,"scene_node_id": plus the escaped ID. The same explicit ceilings are used
// by the JavaScript v2 reader before constructing qualified references/state.
fn derived_budget<'a>(
    document_id: &str,
    scene_sha256: &str,
    instance_key: &str,
    nodes: impl Iterator<Item = &'a str>,
    groups: &[SelectionGroup],
) -> VizResult<()> {
    #[derive(Serialize)]
    struct Binding<'a> {
        document_id: &'a str,
        scene_sha256: &'a str,
        instance_key: &'a str,
    }
    let binding = json_size(&Binding {
        document_id,
        scene_sha256,
        instance_key,
    })?;
    let mut sizes = BTreeMap::new();
    let mut total = 0usize;
    let mut largest = 0usize;
    for id in nodes {
        let size = binding + 17 + json_size(id)?;
        total += size;
        if total > MAX_METADATA_BYTES {
            return Err(error(
                "0102",
                "qualified link-reference expansion exceeds 4 MiB",
            ));
        }
        largest = largest.max(size);
        sizes.insert(id, size);
    }
    for group in groups {
        let mut envelope = binding + 3 * largest + 2048;
        for member in &group.members {
            envelope += sizes
                .get(member.as_str())
                .ok_or_else(|| error("0103", "link member absent during state budget check"))?
                + 1;
            if envelope > MAX_METADATA_BYTES {
                return Err(error(
                    "0102",
                    "retained linked-state envelope exceeds 4 MiB",
                ));
            }
        }
    }
    Ok(())
}
fn json_size(value: &(impl Serialize + ?Sized)) -> VizResult<usize> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_METADATA_BYTES.saturating_sub(self.0) {
                return Err(std::io::Error::other("JSON size budget"));
            }
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value)
        .map_err(|_| error("0102", "derived JSON byte budget exceeded"))?;
    Ok(counter.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escaped_json_reference_sizes_and_retained_state_envelope_are_bounded() {
        assert_eq!(
            json_size("a\t\"漢").unwrap(),
            serde_json::to_vec("a\t\"漢").unwrap().len()
        );
        let ids: Vec<String> = (0..168).map(|i| format!("node-{i}")).collect();
        let groups = vec![SelectionGroup {
            id: "group".into(),
            members: ids.clone(),
        }];
        let document = "\u{1}".repeat(4096);
        let err = derived_budget(
            &document,
            &"0".repeat(64),
            "main",
            ids.iter().map(String::as_str),
            &groups,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("retained linked-state envelope"), "{err}");
    }
}
