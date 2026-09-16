use crate::jcs_sha256;
use crate::report_fingerprint::{fingerprint, manifest};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

#[test]
fn report_fingerprint_manifest_matches_actual_allowlisted_sources_and_resources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = manifest();
    assert_eq!(manifest["algorithm"], "rp/validator-fingerprint/v1");
    assert_eq!(manifest["allowlist"], "rp/claim-chain-validator-inputs/v1");
    let entries = manifest["entries"]
        .as_array()
        .expect("logical manifest entries");
    let paths: Vec<_> = entries
        .iter()
        .map(|v| v["logical_path"].as_str().unwrap())
        .collect();
    assert!(paths.windows(2).all(|p| p[0] < p[1]));
    for required in [
        "Cargo.lock",
        "Cargo.toml",
        "crates/rp-core/Cargo.toml",
        "crates/rp-core/src/report_fingerprint.rs",
        "crates/rp-core/src/report_subject.rs",
        "crates/rp-core/src/project.rs",
        "crates/rp-core/src/claim.rs",
        "crates/rp-core/src/content.rs",
        "crates/rp-core/src/content_observations.rs",
        "crates/rp-core/src/access.rs",
        "crates/rp-core/src/schema/mod.rs",
        "crates/rp-core/src/yaml/mod.rs",
        "crates/rp-core/src/report_json.rs",
        "crates/rp-core/src/report_acquisition.rs",
        "crates/rp-core/src/report_binding.rs",
        "resources/schemas/v1/claim-chain-validation-report.schema.json",
        "resources/schemas/v1/resource-limits.yaml",
        "resources/schemas/v1/finding-registry.yaml",
    ] {
        assert!(
            paths.contains(&required),
            "missing validator input: {required}"
        );
    }
    for entry in entries {
        let path = entry["logical_path"].as_str().unwrap();
        assert!(
            !path.starts_with('/')
                && !path.contains("..")
                && !path.contains("README")
                && !path.contains("bench")
        );
        let bytes = std::fs::read(root.join(path)).unwrap();
        assert_eq!(
            entry["raw_sha256"],
            format!("sha256:{:x}", Sha256::digest(&bytes)),
            "{path}"
        );
        if let Some(relative) = path.strip_prefix("resources/schemas/v1/") {
            let source = root
                .join("../../schemas/v1")
                .join(relative);
            assert_eq!(
                bytes,
                std::fs::read(source).unwrap(),
                "embedded normative resource drift: {relative}"
            );
        }
    }
    assert_eq!(fingerprint(), jcs_sha256(&manifest).unwrap());
}

#[test]
fn report_fingerprint_logical_identity_is_relocatable_and_every_entry_is_bound() {
    let original = manifest();
    let entries = original["entries"].as_array().expect("manifest entries");
    assert!(!entries.is_empty());
    let mut relocated = original.clone();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Rebuild only from logical paths and the bytes a relocated tree would contain.
    relocated["entries"] = Value::Array(entries.iter().map(|entry| {
        let logical = entry["logical_path"].as_str().unwrap();
        let bytes = std::fs::read(root.join(logical)).unwrap();
        json!({"logical_path": logical, "raw_sha256": format!("sha256:{:x}", Sha256::digest(bytes))})
    }).collect());
    assert_eq!(jcs_sha256(&relocated).unwrap(), fingerprint());
    for position in 0..entries.len() {
        let mut mutated = original.clone();
        mutated["entries"][position]["raw_sha256"] = json!(format!("sha256:{}", "0".repeat(64)));
        assert_ne!(jcs_sha256(&mutated).unwrap(), fingerprint());
    }
}
