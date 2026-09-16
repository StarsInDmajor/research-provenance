//! Private report ruleset identity, not authentication or binding success.
//! Hash source bytes, including this allowlist itself, never compiled output.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

macro_rules! input {
    ($path:literal) => {
        (
            $path,
            include_bytes!(concat!("../../../", $path)).as_slice(),
        )
    };
}

// Versioned exact allowlist. Paths are relative to the package, not the build host.
// Cargo.toml files bind dependency feature selection as well as Cargo.lock versions.
static INPUTS: &[(&str, &[u8])] = &[
    input!("Cargo.lock"),
    input!("Cargo.toml"),
    input!("crates/rp-core/Cargo.toml"),
    input!("crates/rp-core/src/access.rs"),
    input!("crates/rp-core/src/claim.rs"),
    input!("crates/rp-core/src/content.rs"),
    input!("crates/rp-core/src/content_observations.rs"),
    input!("crates/rp-core/src/digest.rs"),
    input!("crates/rp-core/src/execution.rs"),
    input!("crates/rp-core/src/finding.rs"),
    input!("crates/rp-core/src/findings.rs"),
    input!("crates/rp-core/src/lib.rs"),
    input!("crates/rp-core/src/model.rs"),
    input!("crates/rp-core/src/project.rs"),
    input!("crates/rp-core/src/report_acquisition.rs"),
    input!("crates/rp-core/src/report_binding.rs"),
    input!("crates/rp-core/src/report_fingerprint.rs"),
    input!("crates/rp-core/src/report_json.rs"),
    input!("crates/rp-core/src/report_subject.rs"),
    input!("crates/rp-core/src/schema/mod.rs"),
    input!("crates/rp-core/src/yaml/mod.rs"),
    input!("resources/schemas/v1/artifact-manifest.schema.json"),
    input!("resources/schemas/v1/assessment.schema.json"),
    input!("resources/schemas/v1/claim-chain-snapshot.schema.json"),
    input!("resources/schemas/v1/claim-chain-validation-report.schema.json"),
    input!("resources/schemas/v1/common.schema.json"),
    input!("resources/schemas/v1/external-reference.schema.json"),
    input!("resources/schemas/v1/finding-registry.yaml"),
    input!("resources/schemas/v1/finding.schema.json"),
    input!("resources/schemas/v1/freshness-policy.schema.json"),
    input!("resources/schemas/v1/layer-b.schema.json"),
    input!("resources/schemas/v1/node-revision.schema.json"),
    input!("resources/schemas/v1/presentation-order.yaml"),
    input!("resources/schemas/v1/project.schema.json"),
    input!("resources/schemas/v1/research-run.schema.json"),
    input!("resources/schemas/v1/research-thread.schema.json"),
    input!("resources/schemas/v1/resource-limits.yaml"),
    input!("resources/schemas/v1/schema-catalog.json"),
    input!("resources/schemas/v1/scientific-relation-revision.schema.json"),
    input!("resources/schemas/v1/thread-binding.schema.json"),
    input!("resources/schemas/v1/validation-stages.yaml"),
];

pub(crate) fn manifest() -> Value {
    let mut entries: Vec<_> = INPUTS.iter().map(|(path, bytes)| {
        json!({"logical_path": path, "raw_sha256": format!("sha256:{:x}", Sha256::digest(bytes))})
    }).collect();
    entries.sort_by(|a, b| a["logical_path"].as_str().cmp(&b["logical_path"].as_str()));
    json!({
        "algorithm": "rp/validator-fingerprint/v1",
        "allowlist": "rp/claim-chain-validator-inputs/v1",
        "ruleset": "claim-chain-subject-pre-binding/v1",
        "entries": entries,
    })
}

pub(crate) fn fingerprint() -> &'static str {
    static FINGERPRINT: OnceLock<String> = OnceLock::new();
    FINGERPRINT.get_or_init(|| {
        crate::jcs_sha256(&manifest()).expect("fixed string-only manifest is JCS serializable")
    })
}
