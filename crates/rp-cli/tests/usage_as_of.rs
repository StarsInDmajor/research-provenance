use std::{path::PathBuf, process::Command};

use rp_core::{ProjectPath, SchemaBundle};
use serde_json::Value;

const INVALID_TIMES: &[&str] = &[
    "not-a-time",
    "2027-01-01",
    "2027-01-01T00:00:00",
    "2027-02-30T00:00:00Z",
];
const VALID_TIMES: &[&str] = &[
    "2027-01-01T00:00:00Z",
    "2027-01-01T02:00:00+02:00",
    "2026-12-31T18:30:00-05:30",
    "2027-01-01T00:00:00.123456789Z",
];
const TIME_MESSAGE: &str = "--as-of must be an RFC 3339 timestamp";
const QUERY_ERRORS: &[(&[&str], &str)] = &[
    (&["--limit", "0"], "--limit must be between 1 and 100000"),
    (
        &["--limit", "100001"],
        "--limit must be between 1 and 100000",
    ),
    (
        &["--freshness", "invalid"],
        "--freshness must be unknown, fresh, review-due, or stale",
    ),
    (
        &["--limit", "0", "--freshness", "invalid"],
        "--limit must be between 1 and 100000",
    ),
];
const COMMANDS: &[(&str, &[&str])] = &[
    ("overview", &["overview"]),
    ("show", &["show", "hyp_01J00000000000000000000011"]),
    ("query", &["query"]),
    (
        "thread show",
        &["thread", "show", "thd_01J00000000000000000000031"],
    ),
    (
        "export check",
        &[
            "export",
            "check",
            "hyp_01J00000000000000000000011",
            "--level-ceiling",
            "exclusive",
        ],
    ),
];

fn run(args: &[&str], as_of: Option<&str>, project_exists: bool) -> (i32, Value) {
    let mut root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/overview-v1/valid");
    if !project_exists {
        root = root.join("missing-time-envelope-project");
        assert!(!root.exists());
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_rp"));
    command.args(args).args(["--json", "--project"]).arg(root);
    if let Some(as_of) = as_of {
        command.args(["--as-of", as_of]);
    }
    let output = command.output().unwrap();
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    assert_eq!(output.stdout.iter().filter(|b| **b == b'\n').count(), 1);
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

fn assert_schema(result: &Value) {
    SchemaBundle::new()
        .unwrap()
        .validate(result, ProjectPath::new("usage-result.json").unwrap())
        .unwrap_or_else(|findings| panic!("{findings:?}\n{result}"));
}

fn assert_usage(output: (i32, Value), command: &str, as_of: Option<&str>, message: &str) {
    let (code, result) = output;
    assert_eq!(code, 2, "{result}");
    assert_eq!(result["schema"], "rp/cli-result/v1");
    assert_eq!(result["command"], command);
    assert_eq!(result["status"], "usage-error");
    assert_eq!(result["exit_code"], 2);
    assert!(result["data"].is_null());
    assert_eq!(result["findings"].as_array().unwrap().len(), 1);
    assert_eq!(result["findings"][0]["error_code"], "RP_E_CLI_USAGE");
    assert_eq!(result["findings"][0]["message"], message);
    assert_eq!(result["as_of"], serde_json::json!(as_of), "{result}");
    assert_schema(&result);
}

fn rejects_invalid_time(command_index: usize) {
    let (name, args) = COMMANDS[command_index];
    for project_exists in [true, false] {
        for invalid in INVALID_TIMES {
            assert_usage(
                run(args, Some(invalid), project_exists),
                name,
                None,
                TIME_MESSAGE,
            );
        }
    }
}

#[test]
fn overview_invalid_time_is_null_before_project_loading() {
    rejects_invalid_time(0);
}

#[test]
fn show_invalid_time_is_null_before_project_loading() {
    rejects_invalid_time(1);
}

#[test]
fn query_invalid_time_is_null_before_project_loading() {
    rejects_invalid_time(2);
}

#[test]
fn thread_show_invalid_time_is_null_before_project_loading() {
    rejects_invalid_time(3);
}

#[test]
fn export_invalid_time_remains_null_before_project_loading() {
    rejects_invalid_time(4);
}

#[test]
fn query_error_precedence_survives_invalid_time() {
    for project_exists in [true, false] {
        for (options, message) in QUERY_ERRORS {
            let args = [&["query"][..], options].concat();
            for invalid in INVALID_TIMES {
                assert_usage(
                    run(&args, Some(invalid), project_exists),
                    "query",
                    None,
                    message,
                );
            }
        }
    }
}

#[test]
fn query_usage_without_supplied_time_does_not_resolve_now() {
    for project_exists in [true, false] {
        for (options, message) in QUERY_ERRORS {
            let args = [&["query"][..], options].concat();
            assert_usage(run(&args, None, project_exists), "query", None, message);
        }
    }
}

#[test]
fn valid_explicit_times_match_success_and_schema_and_survive_other_usage_errors() {
    for valid in VALID_TIMES {
        // Check the public commands' actual parser and the schema's format/pattern,
        // rather than assuming every Jiff Timestamp spelling is RFC 3339.
        for (name, args) in COMMANDS {
            let (code, result) = run(args, Some(valid), true);
            assert_eq!(code, 0, "{name}: {result}");
            assert_eq!(result["as_of"], *valid);
            assert_schema(&result);
        }
        for project_exists in [true, false] {
            for (options, message) in QUERY_ERRORS {
                let args = [&["query"][..], options].concat();
                assert_usage(
                    run(&args, Some(valid), project_exists),
                    "query",
                    Some(valid),
                    message,
                );
            }
        }
    }
}
