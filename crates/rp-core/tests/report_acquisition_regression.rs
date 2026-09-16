//! C2 acquisition examples are synthetic, never evidence of subject binding.
mod common;

use common::{TempProject, mutate_object};
use rp_core::{Severity, validate_project};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;

const ART: &str = "art_01J00000000000000000000001";
const CHAIN: &str = "claim-chains/minimum-structural-chain--cch_01J00000000000000000000051.yaml";

fn attach(bytes: &[u8]) -> TempProject {
    let p = TempProject::copy_fixture("claim-chain-minimum-v1");
    mutate_object(&p.research(CHAIN), |v| v["validation_report"] = json!(ART));
    fs::create_dir_all(p.research("artifacts")).unwrap();
    fs::write(p.path().join("report.json"), bytes).unwrap();
    fs::write(p.research(&format!("artifacts/report--{ART}.yaml")), serde_json::to_vec(&json!({
        "schema": "rp/artifact-manifest/v1", "id": ART, "title": "Synthetic report",
        "uri": "file:report.json", "media_type": "application/json", "size_bytes": bytes.len(),
        "sha256": format!("sha256:{:x}", Sha256::digest(bytes)),
        "created_at": "2026-01-01T08:06:00Z", "access": {"level": "internal", "compartments": []}
    })).unwrap()).unwrap();
    p
}

#[test]
fn strict_report_failures_are_outer_findings_not_generic_binding_mismatch() {
    for (bytes, code) in [
        (b"{\"a\":1,\"a\":2}".as_slice(), "RP_E_REPORT_JSON_INVALID"),
        (b"{\"a\":\"\xff\"}".as_slice(), "RP_E_REPORT_JSON_INVALID"),
        (b"{}".as_slice(), "RP_E_REPORT_SCHEMA_INVALID"),
        (
            b"{\"schema\":\"rp/finding/v1\"}".as_slice(),
            "RP_E_REPORT_SCHEMA_INVALID",
        ),
    ] {
        let p = attach(bytes);
        let result = validate_project(p.path()).unwrap();
        assert!(result.stage3_ran, "{:?}", result.findings);
        assert!(
            result
                .findings
                .iter()
                .any(|f| f.error_code == code && f.severity == Severity::Error),
            "expected {code}: {:?}",
            result.findings
        );
    }
}
