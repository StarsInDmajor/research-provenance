mod common;

use std::collections::BTreeMap;

use common::{fixture_root, parse_yaml_file, suite_file};
use rp_core::{FreshnessStatus, QueryOptions, validate_project};
use serde_json::Value;

#[test]
fn freshness_fixture_matches_every_positive_and_boundary_oracle() {
    let report = validate_project(&fixture_root("freshness-v1")).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    let index = report.index.unwrap();
    let oracle = parse_yaml_file(&suite_file("freshness-v1", "expected-freshness.yaml"));

    for case in oracle["positive_cases"].as_array().unwrap() {
        let status = index
            .freshness(
                case["object_id"].as_str().unwrap(),
                oracle["as_of"].as_str().unwrap(),
            )
            .unwrap();
        assert_eq!(
            status.status,
            FreshnessStatus::parse(case["expected_status"].as_str().unwrap()).unwrap(),
            "{}",
            case["id"].as_str().unwrap()
        );
    }
    for case in oracle["negative_cases"].as_array().unwrap() {
        let status = index
            .freshness(
                case["object_id"].as_str().unwrap(),
                case["as_of"].as_str().unwrap(),
            )
            .unwrap();
        assert_eq!(
            status.status,
            FreshnessStatus::parse(case["expected_status"].as_str().unwrap()).unwrap(),
            "{}",
            case["id"].as_str().unwrap()
        );
        assert_ne!(
            status.status,
            FreshnessStatus::parse(case["forbidden_status"].as_str().unwrap()).unwrap()
        );
    }
}

#[test]
fn overview_fixture_matches_counts_heads_current_work_and_unknown_freshness() {
    let report = validate_project(&fixture_root("overview-v1")).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    let index = report.index.unwrap();
    let oracle = parse_yaml_file(&suite_file("overview-v1", "expected-overview.yaml"));
    let overview = index.overview("2026-09-01T00:00:00Z").unwrap();

    let expected_counts: BTreeMap<String, usize> = oracle["object_counts"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.as_u64().unwrap() as usize))
        .collect();
    assert_eq!(overview.object_counts, expected_counts);
    assert_eq!(
        overview.unresolved_forks,
        vec!["interpretation-stress-cause"]
    );
    assert_eq!(overview.blockers, vec!["blk_01J00000000000000000000017"]);
    assert_eq!(
        overview.next_actions,
        vec!["nxt_01J00000000000000000000018"]
    );
    assert_eq!(overview.freshness_counts.unknown, 41);
    assert_eq!(overview.freshness_counts.fresh, 0);
}

#[test]
fn show_history_diff_query_and_thread_views_are_deterministic() {
    let report = validate_project(&fixture_root("overview-v1")).unwrap();
    let index = report.index.unwrap();

    let shown = index
        .show("hyp_01J00000000000000000000011", "2026-09-01T00:00:00Z")
        .unwrap();
    assert_eq!(shown.object["logical_id"], "hypothesis-drift");
    assert_eq!(shown.derived["is_head"], false);
    assert_eq!(shown.derived["freshness"], "unknown");
    assert_eq!(shown.derived["assessment_ids"].as_array().unwrap().len(), 3);

    let history = index.history("hypothesis-drift").unwrap();
    let revisions: Vec<_> = history
        .revisions
        .iter()
        .map(|value| value["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        revisions,
        [
            "hyp_01J00000000000000000000011",
            "hyp_01J00000000000000000000012",
            "hyp_01J00000000000000000000013",
            "hyp_01J00000000000000000000014",
        ]
    );
    assert_eq!(history.heads, ["hyp_01J00000000000000000000014"]);

    let diff = index
        .diff(
            "hyp_01J00000000000000000000012",
            "hyp_01J00000000000000000000013",
        )
        .unwrap();
    assert!(
        diff.changes
            .windows(2)
            .all(|pair| pair[0].path < pair[1].path)
    );
    assert!(
        diff.changes
            .iter()
            .any(|change| change.path == "/statement")
    );

    let query = index
        .query(QueryOptions {
            kind: Some("Measurement".to_string()),
            thread_id: Some("thd_01J00000000000000000000031".to_string()),
            freshness: Some(FreshnessStatus::Unknown),
            as_of: "2026-09-01T00:00:00Z".to_string(),
            limit: 10,
        })
        .unwrap();
    assert_eq!(query.returned_count, 2);
    assert!(!query.truncated);
    assert!(
        query
            .rows
            .windows(2)
            .all(|pair| pair[0]["id"].as_str() < pair[1]["id"].as_str())
    );

    let threads = index.thread_list(10);
    assert_eq!(threads.returned_count, 3);
    let thread = index
        .thread_show("thd_01J00000000000000000000031", "2026-09-01T00:00:00Z")
        .unwrap();
    assert_eq!(thread.bindings.len(), 5);
    let stress_roles: Vec<_> = thread
        .bindings
        .iter()
        .filter(|binding| {
            binding.pointer("/target/id").and_then(Value::as_str)
                == Some("meas_01J00000000000000000000026")
        })
        .map(|binding| binding["role"].as_str().unwrap())
        .collect();
    assert_eq!(stress_roles, ["sensitivity"]);
}

#[test]
fn query_limit_is_bounded_and_reports_truncation() {
    let index = validate_project(&fixture_root("overview-v1"))
        .unwrap()
        .index
        .unwrap();
    let query = index
        .query(QueryOptions {
            kind: None,
            thread_id: None,
            freshness: None,
            as_of: "2026-09-01T00:00:00Z".to_string(),
            limit: 3,
        })
        .unwrap();
    assert_eq!(query.returned_count, 3);
    assert!(query.truncated);
}
