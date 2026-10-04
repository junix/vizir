//! Independent test consumer of the public receipt shape. No provider module
//! is imported: recompute relations from actual bytes and native direct output.
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use vizir_compiler::{CompilationContext, parse_compiled_mir_json};
use vizir_core::{CapabilityReport, Color, LossRecord};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Provider {
    id: String,
    version: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    role: String,
    sha256: String,
    bytes: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Primary {
    artifact_id: String,
    role: String,
    argument: String,
    kind: String,
    sha256: String,
    bytes: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Core {
    schema_version: String,
    inputs: Vec<Input>,
    primary: Primary,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: String,
    profile: String,
    provider: Provider,
    artifact_receipt: Core,
    document_id: String,
    source_ir_version: String,
    source_context_format: String,
    compilation_context: CompilationContext,
    background: Color,
    capability_report: CapabilityReport,
    losses: Vec<LossRecord>,
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn verify(
    receipt: &[u8],
    resources: &BTreeMap<String, Vec<u8>>,
    svg: &[u8],
    native: &Value,
    provider_version: &str,
) -> Result<(), String> {
    if receipt.len() > 8 * 1024 * 1024 {
        return Err("receipt budget".into());
    }
    let r: Receipt = serde_json::from_slice(receipt).map_err(|e| e.to_string())?;
    let raw: Value = serde_json::from_slice(receipt).map_err(|e| e.to_string())?;
    if ["theme", "text", "text_layout"].iter().any(|field| {
        raw["compilation_context"]
            .get(field)
            .is_some_and(Value::is_null)
    }) {
        return Err("null context placeholder".into());
    }
    let compiled = parse_compiled_mir_json(&resources["input"]).map_err(|e| e.to_string())?;
    let core = &r.artifact_receipt;
    let p = &core.primary;
    if r.schema_version != "vizir.render-receipt/v1"
        || r.profile != "vizir-compiled-svg/1"
        || r.source_ir_version != "0.4"
        || r.source_context_format != "vizir-compiled-mir/1"
        || r.compilation_context.theme.is_none()
        || r.compilation_context.text.is_none()
        || r.provider.id != "plot-provider-vizir"
        || r.provider.version != provider_version
        || core.schema_version != "plot.artifact-receipt-core/v1"
        || p.artifact_id != "figure"
        || p.role != "primary"
        || p.argument != "output"
        || p.kind != "svg"
        || p.sha256 != digest(svg)
        || p.bytes != svg.len()
        || core.inputs.len() != resources.len()
    {
        return Err("identity or artifact relation mismatch".into());
    }
    for (input, (role, bytes)) in core.inputs.iter().zip(resources) {
        if input.role != *role || input.sha256 != digest(bytes) || input.bytes != bytes.len() {
            return Err("input relation or stable role order mismatch".into());
        }
    }
    if r.document_id != compiled.mir.document_id
        || r.source_ir_version != compiled.mir.source_hir_version
        || r.source_context_format != compiled.format
        || r.compilation_context != compiled.context
        || serde_json::to_value(&r.compilation_context).unwrap() != native["compilation_context"]
        || serde_json::to_value(&r.background).unwrap() != native["background"]
        || serde_json::to_value(&r.capability_report).unwrap() != native["capability_report"]
        || serde_json::to_value(&r.losses).unwrap() != native["losses"]
    {
        return Err("native context, capability or loss mismatch".into());
    }
    Ok(())
}
