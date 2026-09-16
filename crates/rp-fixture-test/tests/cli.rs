use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use rp_core::{ProjectPath, SchemaBundle};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rp-fixture-test"))
}

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
}

#[test]
fn cli_preserves_negative_validator_exit_and_emits_one_schema_conformant_envelope() {
    let root = fixtures_root();
    let output = binary()
        .arg("--baseline")
        .arg(root.join("overview-v1/valid"))
        .arg("--overlay")
        .arg(root.join("overview-v1/mutations/overlay-dangling-reference.yaml"))
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let result: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    SchemaBundle::new()
        .unwrap()
        .validate(&result, ProjectPath::new("result.json").unwrap())
        .unwrap_or_else(|findings| panic!("invalid fixture-test result: {findings:?}"));
    assert_eq!(result["schema"], "rp/cli-result/v1");
    assert_eq!(result["command"], "rp-fixture-test");
    assert_eq!(result["data"]["schema"], "rp/cli-data/fixture-test/v1");
    assert_eq!(result["data"]["expectation_matched"], true);
    assert_eq!(result["data"]["observed_exit_code"], 1);
}

#[test]
fn expectation_mismatch_is_internal_error_exit_four() {
    let root = fixtures_root();
    let source =
        fs::read_to_string(root.join("overview-v1/mutations/overlay-dangling-reference.yaml"))
            .unwrap();
    let descriptor = source.replace(
        "RP_E_DANGLING_REVISION_REFERENCE",
        "RP_E_REFERENCE_NOT_FOUND",
    );
    let path = std::env::temp_dir().join(format!(
        "rp-fixture-cli-{}-{}.yaml",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, descriptor).unwrap();
    let output = binary()
        .arg("--baseline")
        .arg(root.join("overview-v1/valid"))
        .arg("--overlay")
        .arg(&path)
        .arg("--json")
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(output.status.code(), Some(4));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "internal-error");
    assert_eq!(result["data"]["expectation_matched"], false);
}
