use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use rp_core::{ProjectPath, SchemaBundle};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn rp() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rp"))
}

fn fixture_root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
        .join("valid")
}

fn run_json(args: &[&str], fixture: &str) -> (i32, serde_json::Value) {
    let output = rp()
        .args(args)
        .arg("--project")
        .arg(fixture_root(fixture))
        .arg("--json")
        .output()
        .expect("run rp command");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    SchemaBundle::new()
        .unwrap()
        .validate(&value, ProjectPath::new("result.json").unwrap())
        .unwrap_or_else(|findings| panic!("invalid CLI result: {findings:?}\n{value}"));
    (output.status.code().unwrap(), value)
}

#[test]
fn init_creates_a_valid_private_project_and_refuses_overwrite() {
    let root = std::env::temp_dir().join(format!(
        "rp-cli-init-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let output = rp()
        .args(["init", "--project"])
        .arg(&root)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    SchemaBundle::new()
        .unwrap()
        .validate(&result, ProjectPath::new("result.json").unwrap())
        .unwrap_or_else(|findings| panic!("invalid init result: {findings:?}"));
    assert_eq!(result["data"]["schema"], "rp/cli-data/init/v1");
    assert!(
        result["data"]["created_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".research/project.yaml")
    );
    let conflict = rp()
        .args(["init", "--project"])
        .arg(&root)
        .arg("--json")
        .output()
        .unwrap();
    fs::remove_dir_all(root).unwrap();
    assert_eq!(conflict.status.code(), Some(1));
    let result: serde_json::Value = serde_json::from_slice(&conflict.stdout).unwrap();
    assert_eq!(result["status"], "conflict");
}

#[test]
fn overview_matches_oracle_counts_and_echoes_resolved_as_of() {
    let (code, result) = run_json(
        &["overview", "--as-of", "2026-09-01T00:00:00Z"],
        "overview-v1",
    );
    assert_eq!(code, 0);
    assert_eq!(result["as_of"], "2026-09-01T00:00:00Z");
    assert_eq!(result["data"]["schema"], "rp/cli-data/overview/v1");
    assert_eq!(result["data"]["object_counts"]["node_revision"], 14);
    assert_eq!(
        result["data"]["unresolved_forks"],
        serde_json::json!(["interpretation-stress-cause"])
    );
}

#[test]
fn show_history_diff_and_query_return_bounded_deterministic_dtos() {
    let (code, show) = run_json(
        &[
            "show",
            "hyp_01J00000000000000000000011",
            "--as-of",
            "2026-09-01T00:00:00Z",
        ],
        "overview-v1",
    );
    assert_eq!(code, 0);
    assert_eq!(show["data"]["schema"], "rp/cli-data/show/v1");
    assert_eq!(show["data"]["derived"]["freshness"], "unknown");

    let (code, history) = run_json(&["history", "hypothesis-drift"], "overview-v1");
    assert_eq!(code, 0);
    assert_eq!(history["data"]["schema"], "rp/cli-data/history/v1");
    assert_eq!(history["data"]["revisions"].as_array().unwrap().len(), 4);

    let (code, diff) = run_json(
        &[
            "diff",
            "hyp_01J00000000000000000000012",
            "hyp_01J00000000000000000000013",
        ],
        "overview-v1",
    );
    assert_eq!(code, 0);
    assert_eq!(diff["data"]["schema"], "rp/cli-data/diff/v1");
    assert!(!diff["data"]["changes"].as_array().unwrap().is_empty());

    let (code, query) = run_json(
        &[
            "query",
            "--kind",
            "Measurement",
            "--thread",
            "thd_01J00000000000000000000031",
            "--freshness",
            "unknown",
            "--as-of",
            "2026-09-01T00:00:00Z",
            "--limit",
            "1",
        ],
        "overview-v1",
    );
    assert_eq!(code, 0);
    assert_eq!(query["data"]["returned_count"], 1);
    assert_eq!(query["data"]["truncated"], true);
}

#[test]
fn thread_list_and_show_return_contextual_bindings() {
    let (code, list) = run_json(&["thread", "list"], "overview-v1");
    assert_eq!(code, 0);
    assert_eq!(list["data"]["schema"], "rp/cli-data/thread-list/v1");
    assert_eq!(list["data"]["returned_count"], 3);

    let (code, show) = run_json(
        &[
            "thread",
            "show",
            "thd_01J00000000000000000000031",
            "--as-of",
            "2026-09-01T00:00:00Z",
        ],
        "overview-v1",
    );
    assert_eq!(code, 0);
    assert_eq!(show["data"]["schema"], "rp/cli-data/thread-show/v1");
    assert_eq!(show["data"]["bindings"].as_array().unwrap().len(), 5);
}

#[test]
fn not_found_usage_bounds_and_invalid_project_are_rejected() {
    let (code, missing) = run_json(
        &[
            "show",
            "hyp_01J00000000000000000000999",
            "--as-of",
            "2026-09-01T00:00:00Z",
        ],
        "overview-v1",
    );
    assert_eq!(code, 1);
    assert_eq!(missing["status"], "not-found");

    let output = rp()
        .args([
            "query",
            "--project",
            fixture_root("overview-v1").to_str().unwrap(),
            "--limit",
            "100001",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));

    let temp = std::env::temp_dir().join(format!(
        "rp-invalid-query-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(temp.join(".research")).unwrap();
    fs::write(temp.join(".research/project.yaml"), b"schema: [\n").unwrap();
    let output = rp()
        .args(["overview", "--project"])
        .arg(&temp)
        .args(["--as-of", "2026-09-01T00:00:00Z", "--json"])
        .output()
        .unwrap();
    fs::remove_dir_all(temp).unwrap();
    assert_eq!(output.status.code(), Some(1));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "invalid");
    assert!(result["data"].is_null());
}

#[test]
fn snapshot_exports_validated_canonical_records_and_graph_metadata() {
    let (code, value) = run_json(&["snapshot"], "overview-v1");
    assert_eq!(code, 0);
    assert_eq!(value["data"]["schema"], "rp/cli-data/snapshot/v1");
    assert!(value["data"]["canonical_count"].as_u64().unwrap() > 0);
    assert!(value["data"]["project"]["id"].is_string());
    assert!(!value["data"]["threads"].as_array().unwrap().is_empty());
    assert!(value["data"]["objects"].is_object());
    assert!(value["data"]["heads"].is_object());
}

#[test]
fn snapshot_rejects_invalid_as_of() {
    let output = rp()
        .args(["snapshot", "--project"])
        .arg(fixture_root("overview-v1"))
        .args(["--as-of", "invalid-time", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "usage-error");
}
