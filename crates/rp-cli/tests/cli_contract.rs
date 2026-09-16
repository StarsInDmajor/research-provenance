use std::{path::PathBuf, process::Command};

use rp_core::{CommandResult, Finding, ProjectPath, SchemaBundle, Severity, Status};
use serde_json::json;

fn rp() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rp"))
}

fn assert_cli_schema(result: &serde_json::Value) {
    SchemaBundle::new()
        .unwrap()
        .validate(result, ProjectPath::new("cli-result.json").unwrap())
        .unwrap_or_else(|findings| panic!("CLI schema mismatch: {findings:?}"));
}

fn fixture_root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
        .join("valid")
}

#[test]
fn help_is_available_without_a_harness() {
    let output = rp().arg("--help").output().expect("run rp --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert!(stdout.contains("Research Provenance Workbench"));
    assert!(stdout.contains("validate"));
}

#[test]
fn json_usage_error_is_one_result_object_with_exit_two() {
    let output = rp()
        .args(["validate", "--json"])
        .output()
        .expect("run rp usage error");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
    assert_eq!(stdout.lines().count(), 1);

    let result: serde_json::Value = serde_json::from_str(&stdout).expect("result JSON");
    assert_eq!(result["schema"], "rp/cli-result/v1");
    assert_eq!(result["command"], "validate");
    assert_eq!(result["status"], "usage-error");
    assert_eq!(result["exit_code"], 2);
    assert_eq!(result["findings"][0]["error_code"], "RP_E_CLI_USAGE");
}

#[test]
fn status_has_the_frozen_exit_mapping() {
    assert_eq!(Status::Ok.exit_code(), 0);
    assert_eq!(Status::Invalid.exit_code(), 1);
    assert_eq!(Status::Denied.exit_code(), 1);
    assert_eq!(Status::NotFound.exit_code(), 1);
    assert_eq!(Status::Conflict.exit_code(), 1);
    assert_eq!(Status::UsageError.exit_code(), 2);
    assert_eq!(Status::IoError.exit_code(), 3);
    assert_eq!(Status::InternalError.exit_code(), 4);
    assert_eq!(Status::Interrupted.exit_code(), 130);
}

#[test]
fn result_sorts_findings_by_the_frozen_order() {
    let findings = vec![
        Finding::new(
            "RP_I_LAST",
            "example",
            Severity::Info,
            "info",
            Some(ProjectPath::new("z.yaml").unwrap()),
            "/z",
        ),
        Finding::new(
            "RP_E_B",
            "example",
            Severity::Error,
            "second",
            Some(ProjectPath::new("b.yaml").unwrap()),
            "/b",
        ),
        Finding::new(
            "RP_E_A",
            "example",
            Severity::Error,
            "first",
            Some(ProjectPath::new("a.yaml").unwrap()),
            "/a",
        ),
    ];

    let result = CommandResult::new("validate", Status::Invalid, None, findings, Some(json!({})));
    let codes: Vec<_> = result
        .findings
        .iter()
        .map(|finding| finding.error_code)
        .collect();
    assert_eq!(codes, ["RP_E_A", "RP_E_B", "RP_I_LAST"]);
}

#[test]
fn validate_runs_parser_schema_and_project_semantics() {
    let output = rp()
        .args(["validate", "--project"])
        .arg(fixture_root("overview-v1"))
        .arg("--json")
        .output()
        .expect("run rp validate");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let result: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("validate result JSON");
    assert_eq!(result["status"], "ok");
    assert_eq!(result["exit_code"], 0);
    assert_eq!(result["data"]["schema"], "rp/cli-data/validate/v1");
    assert_eq!(result["data"]["valid"], true);
    assert_eq!(result["data"]["canonical_object_count"], 48);
    assert_eq!(result["data"]["stage3_ran"], true);
}

#[test]
fn validate_reports_project_open_failures_with_exit_three() {
    let output = rp()
        .args([
            "validate",
            "--project",
            "/definitely/not/a/project",
            "--json",
        ])
        .output()
        .expect("run rp validate on missing root");

    assert_eq!(output.status.code(), Some(3));
    let result: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("validate error JSON");
    assert_eq!(result["status"], "io-error");
    assert_eq!(result["exit_code"], 3);
    assert_eq!(result["findings"][0]["error_code"], "RP_E_IO_READ_FAILED");
}

#[test]
fn chain_validate_returns_the_frozen_data_contract() {
    let output = rp()
        .args(["chain", "validate", "--project"])
        .arg(fixture_root("claim-chain-minimum-v1"))
        .args([
            "cch_01J00000000000000000000051",
            "--profile",
            "minimum-v1",
            "--json",
        ])
        .output()
        .expect("run rp chain validate");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["command"], "chain validate");
    assert_eq!(result["data"]["schema"], "rp/cli-data/chain-validate/v1");
    assert_eq!(result["data"]["profile"], "minimum-v1");
    assert_eq!(result["data"]["valid"], true);
    assert_cli_schema(&result);
}

#[test]
fn access_explain_returns_effective_floor_and_ordered_steps() {
    let id = "asm_01J00000000000000000000071";
    let output = rp()
        .args(["access", "explain", "--project"])
        .arg(fixture_root("access-closure-v1"))
        .args([id, "--json"])
        .output()
        .expect("run rp access explain");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["command"], "access explain");
    assert_eq!(result["data"]["schema"], "rp/cli-data/access-explain/v1");
    assert_eq!(result["data"]["target_id"], id);
    assert_eq!(
        result["data"]["required_dependency_floor"]["level"],
        "restricted"
    );
    assert_eq!(result["data"]["dependency_count"], 3);
    assert_eq!(
        result["data"]["ordered_dependency_explanation"][0]["edge"],
        "source.external_references"
    );
    assert_cli_schema(&result);
}

#[test]
fn export_check_uses_effective_access_and_denied_exit_mapping() {
    let id = "con_01J00000000000000000000031";
    let output = rp()
        .args([
            "export",
            "check",
            "--level-ceiling",
            "restricted",
            "--compartment",
            "analysis-alpha",
            "--compartment",
            "control-room",
            "--compartment",
            "partner-beta",
            "--project",
        ])
        .arg(fixture_root("access-closure-v1"))
        .args([id, "--json"])
        .output()
        .expect("run rp export check");
    assert_eq!(output.status.code(), Some(1));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["command"], "export check");
    assert_eq!(result["status"], "denied");
    assert_eq!(result["data"]["schema"], "rp/cli-data/export-check/v1");
    assert_eq!(result["data"]["eligible"], false);
    assert_eq!(result["data"]["effective_access"]["level"], "exclusive");
    assert_cli_schema(&result);
}

#[test]
fn snapshot_envelope_conforms_to_cli_result_schema() {
    let output = rp()
        .args(["snapshot", "--project"])
        .arg(fixture_root("overview-v1"))
        .arg("--json")
        .output()
        .expect("run rp snapshot");
    assert!(output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["command"], "snapshot");
    assert_eq!(result["status"], "ok");
    assert_eq!(result["data"]["schema"], "rp/cli-data/snapshot/v1");
    assert_cli_schema(&result);
}

#[test]
fn snapshot_negative_envelope_with_validate_data_fails_schema() {
    let invalid_envelope = json!({
        "schema": "rp/cli-result/v1",
        "command": "snapshot",
        "status": "ok",
        "exit_code": 0,
        "as_of": "2026-09-12T00:00:00Z",
        "findings": [],
        "data": {
            "schema": "rp/cli-data/validate/v1",
            "valid": true,
            "canonical_object_count": 10,
            "stage3_ran": true
        }
    });
    let bundle = SchemaBundle::new().unwrap();
    let res = bundle.validate(
        &invalid_envelope,
        ProjectPath::new("cli-result.json").unwrap(),
    );
    assert!(
        res.is_err(),
        "snapshot with validate data must fail schema validation"
    );
}

#[test]
fn snapshot_failed_status_requires_null_data() {
    let invalid_envelope = json!({
        "schema": "rp/cli-result/v1",
        "command": "snapshot",
        "status": "invalid",
        "exit_code": 1,
        "as_of": "2026-09-12T00:00:00Z",
        "findings": [],
        "data": {
            "schema": "rp/cli-data/snapshot/v1",
            "project": {},
            "threads": [],
            "objects": {},
            "heads": {},
            "relation_heads": [],
            "assessments": {},
            "thread_bindings": {},
            "freshness": {},
            "canonical_count": 0
        }
    });
    let bundle = SchemaBundle::new().unwrap();
    let res = bundle.validate(
        &invalid_envelope,
        ProjectPath::new("cli-result.json").unwrap(),
    );
    assert!(
        res.is_err(),
        "snapshot with non-ok status must have null data"
    );
}

#[test]
fn cli_contract_declares_snapshot_command() {
    let contract_resource = SchemaBundle::resources()
        .iter()
        .find(|r| r.name == "cli-contract.yaml")
        .expect("cli-contract.yaml must be present in schema bundle");
    let contract_text = std::str::from_utf8(contract_resource.bytes).unwrap();
    assert!(contract_text.contains("rp snapshot:"));
    assert!(contract_text.contains("canonical_records: 512"));
    assert!(contract_text.contains("canonical_bytes: 2000000"));
    assert!(contract_text.contains("semantic_nodes: 100"));
    assert!(contract_text.contains("scientific_relations: 300"));
}

#[test]
fn project_paths_cannot_contain_host_paths_or_traversal() {
    assert!(ProjectPath::new(".research/project.yaml").is_ok());
    assert!(ProjectPath::new("/home/alice/private/project.yaml").is_err());
    assert!(ProjectPath::new("../private/project.yaml").is_err());
    assert!(ProjectPath::new(r"C:\\private\\project.yaml").is_err());
    assert!(ProjectPath::new("C:/private/project.yaml").is_err());
}
