#[path = "../../rp-core/tests/common/mod.rs"]
mod common;
use common::TempProject;
use rp_core::{ProjectPath, SchemaBundle};
use serde_json::Value;
use std::process::Command;

#[test]
fn exhausted_validation_stays_invalid_in_cli_envelope_and_cannot_append_over_cap() {
    let project = TempProject::copy_fixture("claim-chain-minimum-v1");
    let path = project.research(
        "records/questions/core-structural-question--qst_01J00000000000000000000010.yaml",
    );
    common::mutate_object(&path, |v| {
        v["extensions"] = serde_json::json!({"unknown/test/v1": {"x":1}});
    });
    // Canonical filename diagnostics exhaust Stage 3 before requested-ID errors.
    let object = common::parse_yaml_file(&path);
    for i in 0..1000 {
        let mut value = object.clone();
        value["id"] = serde_json::json!(format!("qst_{i:026}"));
        value["logical_id"] = serde_json::json!(format!("test-{i}"));
        value.as_object_mut().unwrap().remove("extensions");
        std::fs::write(
            project.research(&format!("records/questions/bad-{i:04}.yaml")),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
    common::mutate_object(&path, |v| {
        v.as_object_mut().unwrap().remove("extensions");
    });
    // Stage 3 can finish with exactly 1000 filename findings; command ID lookup adds one.
    let report = rp_core::validate_project(project.path()).unwrap();
    assert!(report.is_complete(), "{:?}", report.findings.last());
    assert_eq!(report.findings.len(), 1000);
    let schema = SchemaBundle::new().unwrap();
    for args in [
        vec!["chain", "validate", "missing", "--profile", "minimum-v1"],
        vec!["access", "explain", "missing"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rp"))
            .args(args)
            .arg("--project")
            .arg(project.path())
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stderr.is_empty());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        schema
            .validate(&value, ProjectPath::new("result.json").unwrap())
            .unwrap();
        assert_eq!(value["status"], "not-found");
        let findings = value["findings"].as_array().unwrap();
        assert_eq!(findings.len(), 1000);
        assert_eq!(
            findings
                .iter()
                .filter(|f| f["error_code"] == "RP_W_FINDINGS_TRUNCATED")
                .count(),
            1
        );
        assert_ne!(value["data"]["valid"], true);
    }
}
