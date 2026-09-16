//! C2 acquisition examples are synthetic, never evidence of subject binding.
mod common;

use common::{TempProject, mutate_object, parse_yaml_file};
use rp_core::validate_project;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;

const ART: &str = "art_01J00000000000000000000001";
const CHAIN: &str = "claim-chains/minimum-structural-chain--cch_01J00000000000000000000051.yaml";

fn manifest(p: &TempProject) -> std::path::PathBuf {
    p.research(&format!("artifacts/report--{ART}.yaml"))
}

fn synthetic_body(p: &TempProject) -> serde_json::Value {
    let snapshot = parse_yaml_file(&p.research(CHAIN));
    let digest = format!("sha256:{}", "0".repeat(64));
    let entry = |kind: &str, id: &serde_json::Value| json!({"object_type":kind, "id":id, "canonical_digest":digest});
    json!({
        "schema":"rp/claim-chain-validation-report/v1", "scope":"claim-chain-subject-pre-binding/v1",
        "subject":{"id": snapshot["id"], "canonical_digest":rp_core::jcs_sha256(&snapshot).unwrap(), "validation_policy":snapshot["validation_policy"]},
        "selected_nodes": snapshot["node_revisions"].as_array().unwrap().iter().map(|id| entry("rp/node-revision/v1", id)).collect::<Vec<_>>(),
        "selected_relations":snapshot["relation_revisions"].as_array().unwrap().iter().map(|id| entry("rp/scientific-relation-revision/v1", id)).collect::<Vec<_>>(),
        "source_closure":{"algorithm":"rp/source-closure/v1", "entries":[], "sha256":digest, "unresolved_ids":[]},
        "validation_dependencies":[entry("rp/claim-chain-snapshot/v1", &snapshot["id"])], "validation_context":[],
        "validator":{"id":"rp-core", "ruleset":"claim-chain-subject-pre-binding/v1", "fingerprint":digest},
        "outcome":{"passed":true,"findings":[]}
    })
}

fn install(p: &TempProject, bytes: &[u8]) {
    fs::write(p.path().join("report.json"), bytes).unwrap();
    mutate_object(&manifest(p), |v| {
        v["size_bytes"] = json!(bytes.len());
        v["sha256"] = json!(format!("sha256:{:x}", Sha256::digest(bytes)));
    });
}

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
fn preallocated_id_synthetic_candidate_is_explicitly_unbound() {
    let p = attach(b"{}");
    let body = synthetic_body(&p);
    install(&p, &serde_json::to_vec(&body).unwrap());
    let report = validate_project(p.path()).unwrap();
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    assert_eq!(
        report.findings[0].error_code,
        "RP_E_CLAIM_CHAIN_VALIDATION_REPORT_MISMATCH"
    );
    let index = report.index.unwrap();
    assert!(index.artifact_verified(ART)); // only byte verification
    assert_eq!(index.unbound_report_candidate(ART).unwrap().body(), &body);
    // Fingerprint/source closure above are fabricated: acquisition is NOT binding.
    assert_eq!(
        index
            .report_binding("cch_01J00000000000000000000051", ART)
            .unwrap()
            .comparison,
        rp_core::ReportComparison::Mismatch
    );
    assert!(
        !index
            .claim_chain_report("cch_01J00000000000000000000051")
            .unwrap()
            .valid
    );
}

#[test]
fn missing_https_unreadable_size_hash_and_depth_never_produce_candidates() {
    for case in [
        "missing",
        "https",
        "unreadable",
        "size",
        "hash",
        "depth",
        "symlink",
        "manifest",
        "directory",
    ] {
        let p = attach(b"{}");
        let body = synthetic_body(&p);
        install(&p, &serde_json::to_vec(&body).unwrap());
        let expected = match case {
            "manifest" => {
                fs::remove_file(manifest(&p)).unwrap();
                "RP_E_REFERENCE_NOT_FOUND"
            }
            "directory" => {
                fs::remove_file(p.path().join("report.json")).unwrap();
                fs::create_dir(p.path().join("report.json")).unwrap();
                "RP_E_PATH_NON_REGULAR_FILE"
            }
            "missing" => {
                fs::remove_file(p.path().join("report.json")).unwrap();
                "RP_E_ARTIFACT_NOT_FOUND"
            }
            "https" => {
                mutate_object(&manifest(&p), |v| {
                    v["uri"] = json!("https://example.invalid/report.json")
                });
                "RP_E_REPORT_BODY_UNAVAILABLE"
            }
            "unreadable" => {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(
                    p.path().join("report.json"),
                    fs::Permissions::from_mode(0o0),
                )
                .unwrap();
                "RP_E_ARTIFACT_NOT_FOUND"
            }
            "size" => {
                mutate_object(&manifest(&p), |v| v["size_bytes"] = json!(1));
                "RP_E_ARTIFACT_SIZE_MISMATCH"
            }
            "hash" => {
                mutate_object(&manifest(&p), |v| {
                    v["sha256"] = json!(format!("sha256:{}", "1".repeat(64)))
                });
                "RP_E_ARTIFACT_DIGEST_MISMATCH"
            }
            "depth" => {
                install(
                    &p,
                    format!("{{\"a\":{}0{}}}", "[".repeat(64), "]".repeat(64)).as_bytes(),
                );
                "RP_E_RESOURCE_NESTING_DEPTH_EXCEEDED"
            }
            "symlink" => {
                fs::rename(p.path().join("report.json"), p.path().join("original.json")).unwrap();
                std::os::unix::fs::symlink("original.json", p.path().join("report.json")).unwrap();
                "RP_E_PATH_SYMLINK_FORBIDDEN"
            }
            _ => unreachable!(),
        };
        let report = validate_project(p.path()).unwrap();
        assert!(
            report.findings.iter().any(|f| f.error_code == expected),
            "{case}: {:?}",
            report.findings
        );
        let index = report.index.unwrap();
        assert!(index.unbound_report_candidate(ART).is_none(), "{case}");
        assert_eq!(
            index
                .report_binding("cch_01J00000000000000000000051", ART)
                .unwrap()
                .comparison,
            rp_core::ReportComparison::CandidateUnavailable,
            "{case}"
        );
        assert!(
            !index
                .claim_chain_report("cch_01J00000000000000000000051")
                .unwrap()
                .valid
        );
        assert_eq!(index.artifact_verified(ART), case == "depth");
    }
}
