#[path = "../../rp-core/tests/common/mod.rs"]
mod common;

use common::{TempProject, mutate_object, parse_yaml_file};
use rp_core::{ProjectPath, SchemaBundle};
use serde_json::{Value, json};
use std::{fs, process::Command};

#[test]
fn schema_valid_hash_valid_forged_report_fails_full_and_chain_cli() {
    let p = TempProject::copy_fixture("claim-chain-minimum-v1");
    let id = "cch_01J00000000000000000000051";
    let art = "art_01J00000000000000000000999";
    let path = p.research(&format!("claim-chains/minimum-structural-chain--{id}.yaml"));
    mutate_object(&path, |v| v["validation_report"] = json!(art));
    let snapshot = parse_yaml_file(&path);
    // Deliberately forged but syntactically valid identities, computed at test time.
    // Not a stale real-report fingerprint or a supposed positive fixture.
    let digest = rp_core::jcs_sha256(&json!({"forged":true})).unwrap();
    let entry = |ty: &str, id: &Value| json!({"object_type":ty,"id":id,"canonical_digest":digest});
    let body = json!({
        "schema":"rp/claim-chain-validation-report/v1", "scope":"claim-chain-subject-pre-binding/v1",
        "subject":{"id":id,"canonical_digest":rp_core::jcs_sha256(&snapshot).unwrap(),"validation_policy":snapshot["validation_policy"]},
        "selected_nodes":snapshot["node_revisions"].as_array().unwrap().iter().map(|id| entry("rp/node-revision/v1", id)).collect::<Vec<_>>(),
        "selected_relations":snapshot["relation_revisions"].as_array().unwrap().iter().map(|id| entry("rp/scientific-relation-revision/v1", id)).collect::<Vec<_>>(),
        "source_closure":{"algorithm":"rp/source-closure/v1","entries":[],"sha256":digest,"unresolved_ids":[]},
        "validation_dependencies":[entry("rp/claim-chain-snapshot/v1", &json!(id))],"validation_context":[],
        "validator":{"id":"rp-core","ruleset":"claim-chain-subject-pre-binding/v1","fingerprint":digest},
        "outcome":{"passed":true,"findings":[]}
    });
    let schema = SchemaBundle::new().unwrap();
    schema
        .validate(&body, ProjectPath::new("forged.json").unwrap())
        .unwrap();
    // Write exactly the JCS bytes whose raw SHA-256 jcs_sha256 computes.
    let bytes = rp_core::canonicalize_jcs(&body).unwrap();
    fs::write(p.path().join("report.json"), &bytes).unwrap();
    fs::create_dir_all(p.research("artifacts")).unwrap();
    fs::write(
        p.research(&format!("artifacts/report--{art}.yaml")),
        serde_json::to_vec(&json!({
            "schema":"rp/artifact-manifest/v1","id":art,"title":"Forged report",
            "uri":"file:report.json","media_type":"application/json","size_bytes":bytes.len(),
            "sha256":rp_core::jcs_sha256(&body).unwrap(),"created_at":"2026-01-01T08:06:00Z",
            "access":snapshot["access"]
        }))
        .unwrap(),
    )
    .unwrap();
    let result = rp_core::validate_project(p.path()).unwrap();
    let index = result.index.unwrap();
    assert!(index.artifact_verified(art));
    assert!(index.unbound_report_candidate(art).is_some());
    for chain in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rp"));
        if chain {
            command.args(["chain", "validate", id, "--profile", "minimum-v1"]);
        } else {
            command.arg("validate");
        }
        let output = command
            .arg("--project")
            .arg(p.path())
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stderr.is_empty());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        schema
            .validate(&value, ProjectPath::new("cli-result.json").unwrap())
            .unwrap();
        assert_eq!(value["status"], "invalid");
        assert_eq!(value["data"]["valid"], false);
        assert_eq!(value["findings"].as_array().unwrap().len(), 1);
        assert_eq!(
            value["findings"][0]["error_code"],
            "RP_E_CLAIM_CHAIN_VALIDATION_REPORT_MISMATCH"
        );
    }
}
