mod common;

use std::fs;

use common::{TempProject, mutate_object};
use rp_core::{ProjectLimits, ValidationReport, validate_project, validate_project_with_limits};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const ART: &str = "art_01J00000000000000000000005";
const NEXT_ART: &str = "art_01J00000000000000000000006";
const MANIFEST: &str = "artifacts/evidential-summary--art_01J00000000000000000000005.yaml";

fn artifact_project(bytes: &[u8], declared: usize, next: Option<&[u8]>) -> TempProject {
    let project = TempProject::copy_fixture("claim-chain-evidential-v1");
    // These tests concern verification, not the fixture's frozen closure digest.
    fs::remove_dir_all(project.research("claim-chains")).unwrap();
    fs::write(project.path().join("data/evidential-summary.csv"), bytes).unwrap();
    mutate_object(&project.research(MANIFEST), |object| {
        object["size_bytes"] = json!(declared);
        object["sha256"] = json!(format!("sha256:{:x}", Sha256::digest(bytes)));
    });
    if let Some(bytes) = next {
        let manifest = project.research(&format!("artifacts/next--{NEXT_ART}.yaml"));
        fs::copy(project.research(MANIFEST), &manifest).unwrap();
        fs::write(project.path().join("data/next.csv"), bytes).unwrap();
        mutate_object(&manifest, |object| {
            object["id"] = json!(NEXT_ART);
            object["uri"] = json!("file:data/next.csv");
            object["size_bytes"] = json!(bytes.len());
            object["sha256"] = json!(format!("sha256:{:x}", Sha256::digest(bytes)));
        });
    }
    project
}

fn check(project: &TempProject, item: usize, aggregate: usize) -> ValidationReport {
    let report = validate_project_with_limits(
        project.path(),
        ProjectLimits {
            local_artifact_bytes_per_item: item,
            local_artifact_aggregate_bytes: aggregate,
            ..ProjectLimits::default()
        },
    )
    .unwrap();
    assert!(report.stage3_ran, "{:?}", report.findings);
    report
}

fn codes(report: &ValidationReport) -> Vec<&str> {
    report
        .findings
        .iter()
        .map(|finding| finding.error_code)
        .collect()
}

#[test]
fn underdeclared_artifact_cannot_bypass_aggregate_and_unavailable_digest_is_not_mismatch() {
    let project = artifact_project(b"12345678", 1, None);
    let report = check(&project, 8, 7);
    assert_eq!(
        codes(&report),
        ["RP_E_RESOURCE_ARTIFACT_AGGREGATE_EXCEEDED"]
    );
    assert!(!report.index.unwrap().artifact_verified(ART));
}

#[test]
fn actual_bytes_from_failed_size_verification_still_consume_aggregate() {
    let project = artifact_project(b"12345678", 1, Some(b"x"));
    let report = check(&project, 8, 8);
    assert!(codes(&report).contains(&"RP_E_ARTIFACT_SIZE_MISMATCH"));
    assert!(codes(&report).contains(&"RP_E_RESOURCE_ARTIFACT_AGGREGATE_EXCEEDED"));
    assert!(!codes(&report).contains(&"RP_E_ARTIFACT_DIGEST_MISMATCH"));
    let index = report.index.unwrap();
    assert!(!index.artifact_verified(ART));
    assert!(!index.artifact_verified(NEXT_ART));
}

#[test]
fn artifact_item_limit_is_distinct_and_metadata_rejection_does_not_spend_bytes() {
    let project = artifact_project(b"12345678", 1, Some(b"x"));
    let report = check(&project, 7, 1);
    assert_eq!(codes(&report), ["RP_E_RESOURCE_ARTIFACT_SIZE_EXCEEDED"]);
    assert!(report.index.unwrap().artifact_verified(NEXT_ART));
    // Oversize declared metadata is rejected before attempting to open the URI.
    fs::remove_file(project.path().join("data/evidential-summary.csv")).unwrap();
    mutate_object(&project.research(MANIFEST), |object| {
        object["size_bytes"] = json!(9)
    });
    assert_eq!(
        codes(&check(&project, 8, 1)),
        ["RP_E_RESOURCE_ARTIFACT_SIZE_EXCEEDED"]
    );
}

#[test]
fn artifact_exact_boundaries_zero_bytes_and_failed_digest_accounting() {
    for (bytes, item, aggregate) in [(b"".as_slice(), 0, 0), (b"12345678".as_slice(), 8, 8)] {
        let project = artifact_project(bytes, bytes.len(), Some(b""));
        let report = check(&project, item, aggregate);
        assert!(report.findings.is_empty(), "{:?}", report.findings);
        let index = report.index.unwrap();
        assert!(index.artifact_verified(ART));
        assert!(index.artifact_verified(NEXT_ART));
    }
    let project = artifact_project(b"12345678", 8, Some(b"x"));
    mutate_object(&project.research(MANIFEST), |object| {
        object["sha256"] = json!(format!("sha256:{}", "0".repeat(64)))
    });
    let report = check(&project, 8, 8);
    assert!(codes(&report).contains(&"RP_E_ARTIFACT_DIGEST_MISMATCH"));
    assert!(codes(&report).contains(&"RP_E_RESOURCE_ARTIFACT_AGGREGATE_EXCEEDED"));
    assert!(!report.index.unwrap().artifact_verified(NEXT_ART));
}

fn extension_project(count: usize, malformed_tail: bool) -> (TempProject, usize) {
    let project = TempProject::copy_fixture("overview-v1");
    let baseline = validate_project(project.path()).unwrap();
    assert!(baseline.findings.is_empty());
    let builtin_count = baseline.index.unwrap().reference_fingerprint().len();
    let schema_path = project.research("schemas/fixture/synthetic/v1.schema.json");
    let mut schema: Value = serde_json::from_slice(&fs::read(&schema_path).unwrap()).unwrap();
    schema["properties"]["targets"] = json!({"type": "array"});
    schema["x-rp-semantic-references"] = json!([{
        "pointer_template": "/targets/*", "target_schema": "rp/node-revision/v1"
    }]);
    fs::write(schema_path, serde_json::to_vec(&schema).unwrap()).unwrap();
    mutate_object(&project.research("project.yaml"), |object| {
        let mut targets = vec![json!("qst_01J00000000000000000000010"); count];
        if malformed_tail {
            targets.push(json!(false));
        }
        object["extensions"]["fixture.synthetic/v1"]["targets"] = json!(targets);
    });
    (project, builtin_count)
}

#[test]
fn extension_exhaustion_stops_visiting_and_never_publishes_truncated_graph() {
    for remaining in [0, 1] {
        let (project, builtin_count) = extension_project(remaining + 1, true);
        let report = validate_project_with_limits(
            project.path(),
            ProjectLimits {
                graph_edges: builtin_count + remaining,
                ..ProjectLimits::default()
            },
        )
        .unwrap();
        assert_eq!(codes(&report), ["RP_E_RESOURCE_GRAPH_EDGES_EXCEEDED"]);
        let index = report.index.unwrap();
        assert!(index.is_empty());
        assert!(
            index
                .access_report("proj_01J00000000000000000000001")
                .is_none()
        );
    }
}

#[test]
fn extension_edge_overflow_preserves_execution_in_empty_fallback_index() {
    use rp_core::{
        ExecutionBudget, ExecutionStop, NavigationError, QueryOptions, validate_project_with_budget,
    };
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    let (project, builtin_count) = extension_project(1, false);
    let token = Arc::new(AtomicBool::new(false));
    let budget = ExecutionBudget::new(Duration::from_secs(60), token.clone());
    let report = validate_project_with_budget(
        project.path(),
        ProjectLimits {
            graph_edges: builtin_count,
            ..ProjectLimits::default()
        },
        &budget,
    )
    .unwrap();
    assert_eq!(codes(&report), ["RP_E_RESOURCE_GRAPH_EDGES_EXCEEDED"]);
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
fn extension_and_builtin_edges_share_exact_budget_without_false_exhaustion() {
    for remaining in [0, 1, 2] {
        let (project, builtin_count) = extension_project(remaining, false);
        let report = validate_project_with_limits(
            project.path(),
            ProjectLimits {
                graph_edges: builtin_count + remaining,
                ..ProjectLimits::default()
            },
        )
        .unwrap();
        assert!(report.findings.is_empty(), "{:?}", report.findings);
        let index = report.index.unwrap();
        assert_eq!(
            index.reference_fingerprint().len(),
            builtin_count + remaining
        );
        assert!(
            index
                .access_report("proj_01J00000000000000000000001")
                .is_some()
        );
    }
}
