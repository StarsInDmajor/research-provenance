use rp_core::*;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/overview-v1/valid")
}
#[test]
fn cancelled_validation_is_incomplete_no_index_and_not_findings_truncation() {
    let token = Arc::new(AtomicBool::new(true));
    let budget = ExecutionBudget::new(Duration::from_secs(60), token);
    let report =
        validate_project_with_budget(&fixture(), ProjectLimits::default(), &budget).unwrap();
    assert!(!report.is_complete());
    assert!(!report.is_valid());
    assert!(report.index.is_none());
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].error_code, "RP_E_INTERRUPTED");
}
#[test]
fn init_precancelled_creates_nothing() {
    let root = std::env::temp_dir().join(format!("rp-budget-init-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let budget = ExecutionBudget::new(Duration::from_secs(60), Arc::new(AtomicBool::new(true)));
    let result = initialize_project_with_budget(&root, &budget);
    let entries = std::fs::read_dir(&root).unwrap().count();
    std::fs::remove_dir_all(&root).unwrap();
    assert!(result.is_err());
    assert_eq!(entries, 0);
}
#[test]
fn deadline_is_not_a_truncation_warning() {
    let budget = ExecutionBudget::with_clock(
        Duration::from_secs(1),
        Arc::new(AtomicBool::new(false)),
        || Duration::from_secs(1),
    );
    let report =
        validate_project_with_budget(&fixture(), ProjectLimits::default(), &budget).unwrap();
    assert!(!report.is_complete());
    assert!(report.had_error());
    assert!(report.index.is_none());
    assert_eq!(
        report.findings[0].error_code,
        "RP_E_RESOURCE_DEADLINE_EXCEEDED"
    );
}
#[test]
fn query_midrow_and_diff_stops_are_typed_without_partial_success() {
    let armed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicU64::new(0));
    let budget =
        ExecutionBudget::with_clock(Duration::from_secs(1), Arc::new(AtomicBool::new(false)), {
            let armed = armed.clone();
            let calls = calls.clone();
            move || {
                if armed.load(Ordering::Relaxed) && calls.fetch_add(1, Ordering::Relaxed) > 3 {
                    Duration::from_secs(1)
                } else {
                    Duration::ZERO
                }
            }
        });
    let index = validate_project_with_budget(&fixture(), ProjectLimits::default(), &budget)
        .unwrap()
        .index
        .unwrap();
    armed.store(true, Ordering::Relaxed);
    assert_eq!(
        index.query(QueryOptions {
            kind: None,
            thread_id: None,
            freshness: None,
            as_of: "2026-01-01T00:00:00Z".into(),
            limit: 100
        }),
        Err(NavigationError::Stopped(ExecutionStop::Deadline))
    );
    assert_eq!(
        index.diff("missing-a", "missing-b"),
        Err(NavigationError::Stopped(ExecutionStop::Deadline))
    );
    assert!(matches!(
        index.thread_list_checked(10),
        Err(NavigationError::Stopped(ExecutionStop::Deadline))
    ));
    assert_eq!(
        index.access_explanation_checked("missing"),
        Err(ExecutionStop::Deadline)
    );
}

#[test]
fn builtin_edge_overflow_preserves_execution_in_empty_fallback_index() {
    let token = Arc::new(AtomicBool::new(false));
    let budget = ExecutionBudget::new(Duration::from_secs(60), token.clone());
    let report = validate_project_with_budget(
        &fixture(),
        ProjectLimits {
            graph_edges: 0,
            ..ProjectLimits::default()
        },
        &budget,
    )
    .unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.error_code == "RP_E_RESOURCE_GRAPH_EDGES_EXCEEDED")
    );
    let index = report.index.expect("retain existing empty-index contract");
    assert!(index.is_empty());
    token.store(true, Ordering::Release);
    assert_eq!(
        index.query(QueryOptions {
            kind: None,
            thread_id: None,
            freshness: None,
            as_of: "2026-01-01T00:00:00Z".into(),
            limit: 10
        }),
        Err(NavigationError::Stopped(ExecutionStop::Interrupted))
    );
}

#[test]
fn freshness_stop_precedes_missing_object_and_invalid_time() {
    for stop in [ExecutionStop::Deadline, ExecutionStop::Interrupted] {
        let token = Arc::new(AtomicBool::new(false));
        let expired = Arc::new(AtomicBool::new(false));
        let budget = ExecutionBudget::with_clock(Duration::from_secs(1), token.clone(), {
            let expired = expired.clone();
            move || {
                if expired.load(Ordering::Acquire) {
                    Duration::from_secs(1)
                } else {
                    Duration::ZERO
                }
            }
        });
        let index = validate_project_with_budget(&fixture(), ProjectLimits::default(), &budget)
            .unwrap()
            .index
            .unwrap();
        match stop {
            ExecutionStop::Deadline => expired.store(true, Ordering::Release),
            ExecutionStop::Interrupted => token.store(true, Ordering::Release),
        }
        assert_eq!(budget.checkpoint(), Err(stop));
        for (id, time) in [
            ("missing", "2026-01-01T00:00:00Z"),
            ("qst_01J00000000000000000000010", "invalid-time"),
            ("missing", "invalid-time"),
        ] {
            assert_eq!(
                index.freshness(id, time),
                Err(NavigationError::Stopped(stop)),
                "{stop:?}: {id}, {time}"
            );
        }
    }
}

#[test]
fn postvalidation_query_uses_original_budget() {
    let now = Arc::new(AtomicU64::new(0));
    let budget =
        ExecutionBudget::with_clock(Duration::from_secs(1), Arc::new(AtomicBool::new(false)), {
            let now = now.clone();
            move || Duration::from_millis(now.load(Ordering::Relaxed))
        });
    let report =
        validate_project_with_budget(&fixture(), ProjectLimits::default(), &budget).unwrap();
    assert!(report.is_valid(), "{:?}", report.findings);
    now.store(1000, Ordering::Relaxed);
    let result = report.index.unwrap().query(QueryOptions {
        kind: None,
        thread_id: None,
        freshness: None,
        as_of: "2026-01-01T00:00:00Z".into(),
        limit: 100,
    });
    assert!(
        result.is_err(),
        "postvalidation query must not reset execution"
    );
}
