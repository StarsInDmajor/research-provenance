use std::{path::PathBuf, process::Command};

use rp_core::{ProjectPath, SchemaBundle};
use serde_json::Value;

fn export(as_of: Option<&str>, project_exists: bool) -> (i32, Value) {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/freshness-v1/valid")
        .join(if project_exists {
            "."
        } else {
            "missing-project"
        });
    let mut command = Command::new(env!("CARGO_BIN_EXE_rp"));
    command
        .args([
            "export",
            "check",
            "qst_01J00000000000000000000114",
            "--level-ceiling",
            "internal",
            "--json",
            "--project",
        ])
        .arg(project);
    if let Some(as_of) = as_of {
        command.args(["--as-of", as_of]);
    }
    let output = command.output().unwrap();
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

fn assert_schema(value: &Value) {
    SchemaBundle::new()
        .unwrap()
        .validate(value, ProjectPath::new("export-result.json").unwrap())
        .unwrap_or_else(|findings| panic!("{findings:?}"));
}

#[test]
fn export_rejects_invalid_as_of_before_project_loading() {
    for project_exists in [true, false] {
        for invalid in ["not-a-time", "2027-01-01", "2027-01-01T00:00:00"] {
            let (code, result) = export(Some(invalid), project_exists);
            assert_eq!(code, 2, "{result}");
            assert_eq!(result["status"], "usage-error");
            assert_eq!(result["command"], "export check");
            assert_eq!(result["findings"][0]["error_code"], "RP_E_CLI_USAGE");
            assert!(result["data"].is_null());
            assert_schema(&result);
        }
    }
}

#[test]
fn export_echoes_explicit_as_of_and_resolves_interactive_default() {
    let before = jiff::Timestamp::now();
    let (code, result) = export(None, true);
    let after = jiff::Timestamp::now();
    assert_eq!(code, 0);
    let resolved = result["as_of"]
        .as_str()
        .expect("resolved default")
        .parse::<jiff::Timestamp>()
        .unwrap();
    assert!(before <= resolved && resolved <= after);
    assert_schema(&result);

    let explicit = "2027-01-01T02:00:00+02:00";
    let (code, explicit_result) = export(Some(explicit), true);
    assert_eq!(code, 0);
    assert_eq!(explicit_result["as_of"], explicit);
    assert_eq!(
        explicit_result["data"], result["data"],
        "access eligibility is not historical"
    );
    assert_schema(&explicit_result);
}
