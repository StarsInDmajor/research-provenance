use super::*;
use crate::{ExecutionBudget, ExecutionStop, ProjectPath};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

// One thread, no policy/lineage/access: show has six pre-build checkpoints
// (entry, freshness entry/exit, two scans, pre-build). Thread show has three
// (entry, one binding scan, pre-build). Expire only at the next/post-build call.
// These exact call sequences deliberately detect a missing final boundary.
fn index_expiring_after(pre_build_checks: usize, stop: ExecutionStop) -> ProjectIndex {
    let calls = Arc::new(AtomicUsize::new(0));
    let token = Arc::new(AtomicBool::new(false));
    let execution = ExecutionBudget::with_clock(Duration::from_secs(1), token.clone(), move || {
        if calls.fetch_add(1, Ordering::Relaxed) >= pre_build_checks {
            match stop {
                ExecutionStop::Interrupted => token.store(true, Ordering::Release),
                ExecutionStop::Deadline => return Duration::from_secs(1),
            }
        }
        Duration::ZERO
    });
    let mut index = ProjectIndex::default();
    index.execution = execution;
    index.insert(ObjectRecord {
        ordinal: 0,
        id: "thread".into(),
        logical_id: None,
        object_type: ObjectType::Thread,
        source_file: ProjectPath::new("thread.yaml").unwrap(),
        value: json!({"id":"thread", "title":"Thread", "nested":{"value":[1,2,3]}}),
    });
    index
}

#[test]
fn empty_index_fallback_preserves_nondefault_limits() {
    let mut index = index_expiring_after(0, ExecutionStop::Interrupted);
    index.validation_limits.graph_edges = 17;
    index.validation_limits.traversal_depth = 23;
    index.validation_limits.findings = 5;
    let empty = index.empty_with_context();
    assert!(empty.is_empty());
    assert_eq!(empty.validation_limits.graph_edges, 17);
    assert_eq!(empty.validation_limits.traversal_depth, 23);
    assert_eq!(empty.validation_limits.findings, 5);
    assert_eq!(
        empty.execution.checkpoint(),
        Err(ExecutionStop::Interrupted)
    );
}

#[test]
fn show_checks_stop_after_final_dto_build() {
    for stop in [ExecutionStop::Deadline, ExecutionStop::Interrupted] {
        let index = index_expiring_after(6, stop);
        assert_eq!(
            index.show("thread", "2026-01-01T00:00:00Z"),
            Err(NavigationError::Stopped(stop))
        );
    }
}

#[test]
fn thread_show_checks_stop_after_final_dto_build() {
    for stop in [ExecutionStop::Deadline, ExecutionStop::Interrupted] {
        let index = index_expiring_after(3, stop);
        assert_eq!(
            index.thread_show("thread", "2026-01-01T00:00:00Z"),
            Err(NavigationError::Stopped(stop))
        );
    }
}

#[test]
fn snapshot_checks_stop_before_allocation() {
    for stop in [ExecutionStop::Deadline, ExecutionStop::Interrupted] {
        let index = index_expiring_after(0, stop);
        assert_eq!(
            index.snapshot("2026-01-01T00:00:00Z"),
            Err(NavigationError::Stopped(stop))
        );
    }
}

#[test]
fn snapshot_rejects_oversized_records_nodes_relations_and_bytes() {
    // Alpha-1 caps (512 records / 100 nodes / 300 relations) were raised in
    // beta-1 so full-project graphs fit. Keep the guard tests, now at the
    // raised ceilings; also confirm an 86-node graph passes.
    let mut index_nodes = ProjectIndex::default();
    index_nodes.insert(ObjectRecord {
        ordinal: 0,
        id: "proj".into(),
        logical_id: None,
        object_type: ObjectType::Project,
        source_file: ProjectPath::new(".research/project.yaml").unwrap(),
        value: json!({"id": "proj", "schema": "rp/project/v1"}),
    });
    for i in 1..=86 {
        index_nodes.insert(ObjectRecord {
            ordinal: i,
            id: format!("node_{i:026}").into(),
            logical_id: Some(format!("log_{i}").into()),
            object_type: ObjectType::Node("Hypothesis".into()),
            source_file: ProjectPath::new(".research/records/h.yaml").unwrap(),
            value: json!({"id": format!("node_{i:026}"), "title": "Node", "kind": "Hypothesis"}),
        });
    }
    assert!(index_nodes.snapshot("2026-01-01T00:00:00Z").is_ok());

    // 20_001 semantic nodes exceeds the 20_000 node ceiling.
    let mut index_nodes_big = ProjectIndex::default();
    index_nodes_big.insert(ObjectRecord {
        ordinal: 0,
        id: "proj".into(),
        logical_id: None,
        object_type: ObjectType::Project,
        source_file: ProjectPath::new(".research/project.yaml").unwrap(),
        value: json!({"id": "proj", "schema": "rp/project/v1"}),
    });
    for i in 1..=20_001 {
        index_nodes_big.insert(ObjectRecord {
            ordinal: i,
            id: format!("node_{i:026}").into(),
            logical_id: Some(format!("log_{i}").into()),
            object_type: ObjectType::Node("Hypothesis".into()),
            source_file: ProjectPath::new(".research/records/h.yaml").unwrap(),
            value: json!({"id": format!("node_{i:026}"), "title": "Node", "kind": "Hypothesis"}),
        });
    }
    assert_eq!(
        index_nodes_big.snapshot("2026-01-01T00:00:00Z"),
        Err(NavigationError::OversizedGraph(
            OversizedReason::SemanticNodes {
                count: 20_001,
                limit: 20_000,
            }
        ))
    );

    // 50_001 relations exceeds the 50_000 relation ceiling.
    let mut index_rels = ProjectIndex::default();
    index_rels.insert(ObjectRecord {
        ordinal: 0,
        id: "proj".into(),
        logical_id: None,
        object_type: ObjectType::Project,
        source_file: ProjectPath::new(".research/project.yaml").unwrap(),
        value: json!({"id": "proj", "schema": "rp/project/v1"}),
    });
    for i in 1..=50_001 {
        index_rels.insert(ObjectRecord {
            ordinal: i,
            id: format!("rel_{i:026}").into(),
            logical_id: Some(format!("log_rel_{i}").into()),
            object_type: ObjectType::Relation,
            source_file: ProjectPath::new(".research/relations/r.yaml").unwrap(),
            value: json!({"id": format!("rel_{i:026}"), "type": "supports"}),
        });
    }
    assert_eq!(
        index_rels.snapshot("2026-01-01T00:00:00Z"),
        Err(NavigationError::OversizedGraph(
            OversizedReason::ScientificRelations {
                count: 50_001,
                limit: 50_000,
            }
        ))
    );

    // 100_001 canonical records (plus the Project descriptor) exceeds the
    // 100_000 record ceiling.
    let mut index_records = ProjectIndex::default();
    index_records.insert(ObjectRecord {
        ordinal: 0,
        id: "proj".into(),
        logical_id: None,
        object_type: ObjectType::Project,
        source_file: ProjectPath::new(".research/project.yaml").unwrap(),
        value: json!({"id": "proj", "schema": "rp/project/v1"}),
    });
    for i in 1..=100_000 {
        index_records.insert(ObjectRecord {
            ordinal: i,
            id: format!("rec_{i:026}").into(),
            logical_id: None,
            object_type: ObjectType::Artifact,
            source_file: ProjectPath::new(".research/artifacts/a.yaml").unwrap(),
            value: json!({"id": format!("rec_{i:026}")}),
        });
    }
    // 100_000 records exactly fits; adding one more crosses the ceiling.
    index_records.insert(ObjectRecord {
        ordinal: 100_001,
        id: "rec_over".into(),
        logical_id: None,
        object_type: ObjectType::Artifact,
        source_file: ProjectPath::new(".research/artifacts/a.yaml").unwrap(),
        value: json!({"id": "rec_over"}),
    });
    assert_eq!(
        index_records.snapshot("2026-01-01T00:00:00Z"),
        Err(NavigationError::OversizedGraph(
            OversizedReason::CanonicalRecords {
                count: 100_002,
                limit: 100_000,
            }
        ))
    );

    // CanonicalBytes exceeded (> 2,000,000 bytes)
    let mut index_bytes = ProjectIndex::default();
    index_bytes.insert(ObjectRecord {
        ordinal: 0,
        id: "proj".into(),
        logical_id: None,
        object_type: ObjectType::Project,
        source_file: ProjectPath::new(".research/project.yaml").unwrap(),
        value: json!({"id": "proj", "schema": "rp/project/v1"}),
    });
    let big_string = "x".repeat(2_000_100);
    index_bytes.insert(ObjectRecord {
        ordinal: 1,
        id: "node_big".into(),
        logical_id: Some("big".into()),
        object_type: ObjectType::Node("Observation".into()),
        source_file: ProjectPath::new(".research/records/big.yaml").unwrap(),
        value: json!({"id": "node_big", "big": big_string}),
    });
    assert!(matches!(
        index_bytes.snapshot("2026-01-01T00:00:00Z"),
        Err(NavigationError::OversizedGraph(
            OversizedReason::CanonicalBytes { .. }
        ))
    ));
}
