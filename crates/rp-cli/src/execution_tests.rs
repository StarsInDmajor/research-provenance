use super::*;
use rp_core::{ExecutionBudget, ExecutionStop};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/overview-v1/valid")
}
fn result() -> CommandResult {
    CommandResult::new(
        "query",
        Status::Ok,
        None,
        vec![],
        Some(json!({"payload":"a".repeat(10000)})),
    )
}
#[test]
fn cancelled_command_dispatch_precedes_io() {
    let budget = ExecutionBudget::new(Duration::from_secs(60), Arc::new(AtomicBool::new(true)));
    let cli = Cli {
        json: true,
        command: RpCommand::Validate(ProjectOnly {
            project: PathBuf::from("/nonexistent-budget-test"),
        }),
    };
    let result = command_result_with_budget(&cli, &budget);
    assert_eq!(result.exit_code, 130);
    assert!(result.data.is_none());
    assert_eq!(result.findings[0].error_code, "RP_E_INTERRUPTED");
}
#[test]
fn command_and_render_share_state_terminal_schema_and_newline_cap() {
    let budget = ExecutionBudget::with_clock(
        Duration::from_secs(1),
        Arc::new(AtomicBool::new(false)),
        || Duration::from_secs(1),
    );
    let (bytes, code) = encode_result(result(), &budget, 33554432);
    assert_eq!(code, 1);
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value["data"].is_null());
    assert_eq!(
        value["findings"][0]["error_code"],
        "RP_E_RESOURCE_DEADLINE_EXCEEDED"
    );
    rp_core::SchemaBundle::new()
        .unwrap()
        .validate(&value, rp_core::ProjectPath::new("output.json").unwrap())
        .unwrap();
    let (bytes, _) = encode_result(result(), &ExecutionBudget::default(), 1000);
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value["findings"][0]["error_code"],
        "RP_E_RESOURCE_OUTPUT_SIZE_EXCEEDED"
    );
    assert!(bytes.ends_with(b"\n"));
    let small = CommandResult::new("validate", Status::Ok, None, vec![], None);
    let size = serde_json::to_vec(&small).unwrap().len();
    let (bytes, _) = encode_result(small, &ExecutionBudget::default(), size);
    assert!(serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["data"].is_null());
    assert!(
        String::from_utf8(bytes)
            .unwrap()
            .contains("RP_E_RESOURCE_OUTPUT_SIZE_EXCEEDED")
    );
}
#[test]
fn rendering_checks_during_serialization_without_partial_envelope() {
    let ticks = Arc::new(AtomicUsize::new(0));
    let budget =
        ExecutionBudget::with_clock(Duration::from_secs(1), Arc::new(AtomicBool::new(false)), {
            let ticks = ticks.clone();
            move || {
                if ticks.fetch_add(1, Ordering::Relaxed) < 5 {
                    Duration::ZERO
                } else {
                    Duration::from_secs(1)
                }
            }
        });
    let (bytes, code) = encode_result(result(), &budget, 33554432);
    assert_eq!(code, 1);
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(value["data"].is_null());
    assert_eq!(budget.checkpoint(), Err(ExecutionStop::Deadline));
}
#[test]
fn interrupted_render_is_schema_conformant_small_and_does_not_refund() {
    let budget = ExecutionBudget::new(Duration::from_secs(60), Arc::new(AtomicBool::new(true)));
    let (bytes, code) = encode_result(result(), &budget, 1);
    assert_eq!(code, 130);
    assert!(bytes.len() < 1024);
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["status"], "interrupted");
    assert!(value["data"].is_null());
    rp_core::SchemaBundle::new()
        .unwrap()
        .validate(&value, rp_core::ProjectPath::new("output.json").unwrap())
        .unwrap();
    assert_eq!(budget.checkpoint(), Err(ExecutionStop::Interrupted));
}

#[test]
fn below_budget_dispatch_keeps_valid_output() {
    let cli = Cli {
        json: true,
        command: RpCommand::Validate(ProjectOnly { project: fixture() }),
    };
    let result = command_result_with_budget(&cli, &ExecutionBudget::default());
    assert_eq!(result.exit_code, 0);
}
