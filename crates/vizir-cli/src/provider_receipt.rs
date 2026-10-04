//! Path-free typed evidence. Native semantics are verified here, independently
//! from any future Hub verification of the fixed artifact relation core.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Write};
use vizir_compiler::{CompilationContext, CompiledMir};
use vizir_core::{CapabilityReport, Color, LossRecord, Scene2D, VizResult};

pub(crate) const MAX_RECEIPT_BYTES: usize = 8 * 1024 * 1024;

pub(crate) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct InputReceipt {
    pub role: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Primary {
    artifact_id: String,
    role: String,
    argument: String,
    kind: String,
    sha256: String,
    bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ArtifactReceipt {
    schema_version: String,
    inputs: Vec<InputReceipt>,
    primary: Primary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Provider {
    id: String,
    version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: String,
    profile: String,
    provider: Provider,
    artifact_receipt: ArtifactReceipt,
    document_id: String,
    source_ir_version: String,
    source_context_format: String,
    compilation_context: CompilationContext,
    background: Color,
    capability_report: CapabilityReport,
    losses: Vec<LossRecord>,
}

struct BoundedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("receipt byte budget exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialize_checked(receipt: &Receipt, limit: usize) -> VizResult<Vec<u8>> {
    let mut output = BoundedBuffer {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer_pretty(&mut output, receipt)?;
    // Decode from the actual bytes to ensure no typed field was dropped during
    // serialization. Equality includes every native context, decision and loss.
    let decoded: Receipt = serde_json::from_slice(&output.bytes)?;
    if &decoded != receipt {
        return Err(super::provider_render::error("receipt self-check failed"));
    }
    Ok(output.bytes)
}

pub(crate) fn build(
    inputs: Vec<InputReceipt>,
    mir: &CompiledMir,
    scene: &Scene2D,
    capability_report: CapabilityReport,
    svg: &[u8],
) -> VizResult<Vec<u8>> {
    let receipt = Receipt {
        schema_version: "vizir.render-receipt/v1".into(),
        profile: super::PROFILE.into(),
        provider: Provider {
            id: super::PROVIDER_ID.into(),
            version: super::version().into(),
        },
        artifact_receipt: ArtifactReceipt {
            schema_version: "plot.artifact-receipt-core/v1".into(),
            inputs,
            primary: Primary {
                artifact_id: "figure".into(),
                role: "primary".into(),
                argument: "output".into(),
                kind: "svg".into(),
                sha256: digest(svg),
                bytes: svg.len() as u64,
            },
        },
        document_id: mir.mir.document_id.clone(),
        source_ir_version: mir.mir.source_hir_version.clone(),
        source_context_format: mir.format.clone(),
        compilation_context: mir.context.clone(),
        background: scene.background.clone(),
        capability_report,
        losses: scene.losses.clone(),
    };
    serialize_checked(&receipt, MAX_RECEIPT_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_writer_never_retains_over_budget_bytes() {
        let mut output = BoundedBuffer {
            bytes: Vec::new(),
            limit: 3,
        };
        output.write_all(b"abc").unwrap();
        assert!(output.write_all(b"d").is_err());
        assert_eq!(output.bytes, b"abc");
    }

    #[test]
    fn receipt_core_is_closed_and_nonnullable() {
        for value in [
            r#"{"role":"input","sha256":"x","bytes":1,"path":"/tmp/input"}"#,
            r#"{"role":"input","role":"font_1","sha256":"x","bytes":1}"#,
            r#"{"role":null,"sha256":"x","bytes":1}"#,
        ] {
            assert!(serde_json::from_str::<InputReceipt>(value).is_err());
        }
    }
}
