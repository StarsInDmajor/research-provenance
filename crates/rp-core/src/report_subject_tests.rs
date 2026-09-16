//! Private C3 expected-body and runtime integration tests.
use crate::{
    ObjectRecord, ObjectType, ProjectIndex, ProjectPath, ReferenceEdge, ReferenceExpectation,
};
use serde_json::json;

use crate::report_subject::{SubjectBudget, SubjectGraph, SubjectUnavailable};

fn record(id: &str) -> ObjectRecord {
    ObjectRecord {
        ordinal: 99,
        id: id.into(),
        logical_id: None,
        object_type: ObjectType::Node("Question".into()),
        source_file: ProjectPath::new(format!("{id}.yaml")).unwrap(),
        value: json!({"access": {"level": "internal", "compartments": []}}),
    }
}

fn graph() -> ProjectIndex {
    let mut index = ProjectIndex::default();
    for id in [
        "a-unrelated",
        "chain",
        "nested",
        "node",
        "project",
        "report",
        "nested-report",
    ] {
        let mut object = record(id);
        match id {
            "chain" | "nested" => {
                object.object_type = ObjectType::ClaimChain;
                object.value = json!({"validation_report": if id == "chain" {"report"} else {"nested-report"},
                    "access": {"level": "internal", "compartments": []}});
            }
            "project" => {
                object.object_type = ObjectType::Project;
            }
            "report" | "nested-report" => {
                object.object_type = ObjectType::Artifact;
            }
            _ => {}
        }
        index.insert(object);
    }
    index.assign_ordinals();
    index.set_references(vec![
        edge(
            "chain",
            "node",
            ReferenceExpectation::Node,
            "/node_revisions/0",
        ),
        edge(
            "chain",
            "report",
            ReferenceExpectation::Artifact,
            "/validation_report",
        ),
        edge(
            "node",
            "nested",
            ReferenceExpectation::ClaimChain,
            "/extensions/fixture/link",
        ),
        edge(
            "nested",
            "nested-report",
            ReferenceExpectation::Artifact,
            "/validation_report",
        ),
        edge(
            "node",
            "missing",
            ReferenceExpectation::Node,
            "/source/revisions/0",
        ),
        edge(
            "node",
            "missing",
            ReferenceExpectation::Artifact,
            "/source/artifacts/0",
        ),
    ]);
    index.mark_subject_graph_ready();
    index
}

fn edge(from: &str, to: &str, expected: ReferenceExpectation, pointer: &str) -> ReferenceEdge {
    ReferenceEdge {
        source_id: from.into(),
        target_id: to.into(),
        expected,
        json_pointer: pointer.into(),
        target_ordinal: None,
    }
}

#[test]
fn report_subject_cuts_all_role_edges_retains_typed_missing_and_own_ordinals() {
    let full = graph();
    let subject = SubjectGraph::build(&full, "chain", &mut SubjectBudget::default()).unwrap();
    assert_eq!(
        subject.index.ids().collect::<Vec<_>>(),
        ["chain", "nested", "node", "project"]
    );
    assert_eq!(subject.missing.len(), 2);
    assert!(
        subject
            .missing
            .contains(&(ReferenceExpectation::Node, "missing".into()))
    );
    assert!(
        subject
            .missing
            .contains(&(ReferenceExpectation::Artifact, "missing".into()))
    );
    assert!(!subject.own_report_reached);
    for (ordinal, object) in subject.index.objects().enumerate() {
        assert_eq!(object.ordinal, ordinal);
        for reference in subject.index.references_from(&object.id) {
            assert_ne!(reference.json_pointer, "/validation_report");
            assert_eq!(
                reference.target_ordinal.map(|n| n.get() as usize - 1),
                subject.index.get(&reference.target_id).map(|o| o.ordinal)
            );
        }
    }
    let (_, findings) = subject
        .prerequisites(crate::ProjectLimits::default(), Default::default())
        .unwrap();
    assert_eq!(
        findings
            .iter()
            .filter(|f| f.finding_family == "reference_integrity")
            .count(),
        2
    );
}

#[test]
fn report_subject_never_silently_excludes_direct_or_transitive_self_sources() {
    for (from, pointer) in [
        ("chain", "/source/artifacts/0"),
        ("node", "/extensions/fixture/source"),
        ("nested", "/source/artifacts/0"),
        ("project", "/extensions/fixture/source"),
    ] {
        let mut full = graph();
        let mut edges: Vec<_> = full
            .objects()
            .flat_map(|o| full.references_from(&o.id).cloned())
            .collect();
        edges.push(edge(
            from,
            "report",
            ReferenceExpectation::Artifact,
            pointer,
        ));
        full.set_references(edges);
        let subject = SubjectGraph::build(&full, "chain", &mut SubjectBudget::default()).unwrap();
        assert!(subject.own_report_reached, "{from}:{pointer}");
        assert!(
            subject.index.get("report").is_some(),
            "scientific dependency must remain"
        );
    }
}

#[test]
fn report_subject_is_unavailable_for_unprepared_identity_or_exhausted_batch() {
    let mut full = graph();
    full.invalidate_subject_graph();
    assert!(matches!(
        SubjectGraph::build(&full, "chain", &mut SubjectBudget::default()),
        Err(SubjectUnavailable::IndexIncomplete)
    ));
    full.mark_subject_graph_ready();
    let mut budget = SubjectBudget {
        remaining_work: 0,
        ..Default::default()
    };
    assert!(matches!(
        SubjectGraph::build(&full, "chain", &mut budget),
        Err(SubjectUnavailable::Budget)
    ));
    assert!(matches!(
        SubjectGraph::build(&full, "chain", &mut budget),
        Err(SubjectUnavailable::Budget)
    ));
}

#[test]
fn report_subject_recomputes_its_cycle_even_when_global_first_cycle_is_unrelated() {
    let mut full = graph();
    let mut edges: Vec<_> = full
        .objects()
        .flat_map(|o| full.references_from(&o.id).cloned())
        .collect();
    for id in ["a-unrelated", "node"] {
        let mut object = full.get(id).unwrap().clone();
        object.value["revision"] = json!({"parents": [{"id": id}]});
        full.insert(object);
        edges.push(edge(
            id,
            id,
            ReferenceExpectation::Node,
            "/revision/parents/0/id",
        ));
    }
    full.set_references(edges);
    let mut global = crate::findings::Findings::default();
    crate::project::validate_semantics(&full, crate::ProjectLimits::default(), &mut global);
    let cycles: Vec<_> = global
        .iter()
        .filter(|f| f.error_code == "RP_E_REVISION_DAG_CYCLE")
        .collect();
    assert_eq!(cycles.len(), 1);
    assert_eq!(
        cycles[0].source_file.as_ref().unwrap().as_str(),
        "a-unrelated.yaml"
    );
    let subject = SubjectGraph::build(&full, "chain", &mut SubjectBudget::default()).unwrap();
    let (_, findings) = subject
        .prerequisites(crate::ProjectLimits::default(), Default::default())
        .unwrap();
    let cycles: Vec<_> = findings
        .iter()
        .filter(|f| f.error_code == "RP_E_REVISION_DAG_CYCLE")
        .collect();
    assert_eq!(cycles.len(), 1);
    assert_eq!(
        cycles[0].source_file.as_ref().unwrap().as_str(),
        "node.yaml"
    );
}

struct TempProject(std::path::PathBuf);
impl TempProject {
    fn fixture() -> Self {
        Self::fixture_suite("claim-chain-minimum-v1")
    }
    fn fixture_suite(suite: &str) -> Self {
        fn copy(from: &std::path::Path, to: &std::path::Path) {
            std::fs::create_dir_all(to).unwrap();
            for entry in std::fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    copy(&entry.path(), &to.join(entry.file_name()));
                } else {
                    std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
                }
            }
        }
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "rp-c3-subject-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(suite)
            .join("valid");
        copy(&source, &root);
        Self(root)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn research(&self, path: &str) -> std::path::PathBuf {
        self.0.join(".research").join(path)
    }
}
impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn report_subject_actual_loader_tracks_ambiguous_ids_without_changing_cli_findings() {
    let p = TempProject::fixture();
    let result = crate::validate_project(p.path()).unwrap();
    assert!(result.findings.is_empty(), "{:?}", result.findings);
    let index = result.index.unwrap();
    let chain = index
        .objects()
        .find(|o| o.object_type == ObjectType::ClaimChain)
        .unwrap();
    assert!(SubjectGraph::build(&index, &chain.id, &mut SubjectBudget::default()).is_ok());
    let node = index
        .objects()
        .find(|o| matches!(o.object_type, ObjectType::Node(_)))
        .unwrap();
    std::fs::write(
        p.research(&format!("records/questions/duplicate--{}.yaml", node.id)),
        serde_json::to_vec(&node.value).unwrap(),
    )
    .unwrap();
    let result = crate::validate_project(p.path()).unwrap();
    assert!(
        result
            .findings
            .iter()
            .any(|f| f.error_code == "RP_E_OBJECT_DUPLICATE_ID")
    );
    assert!(matches!(
        SubjectGraph::build(
            &result.index.unwrap(),
            &chain.id,
            &mut SubjectBudget::default()
        ),
        Err(SubjectUnavailable::IndexIncomplete)
    ));
}

#[test]
fn report_subject_pipeline_captures_filename_at_generation_with_exact_owner() {
    let p = TempProject::fixture();
    let report = crate::validate_project(p.path()).unwrap();
    let index = report.index.unwrap();
    let chain = index
        .objects()
        .find(|o| o.object_type == ObjectType::ClaimChain)
        .unwrap();
    let mut value = chain.value.clone();
    value["validation_report"] = json!("art_01J00000000000000000000999");
    std::fs::write(
        p.path().join(chain.source_file.as_str()),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let node = index
        .objects()
        .find(|o| matches!(o.object_type, ObjectType::Node(_)))
        .unwrap();
    let source = p.path().join(node.source_file.as_str());
    std::fs::rename(&source, source.with_file_name("wrong-name.yaml")).unwrap();
    let report = crate::validate_project(p.path()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.error_code == "RP_E_OBJECT_FILENAME_ID_MISMATCH")
    );
    let index = report.index.unwrap();
    assert!(index.subject_graph_ready());
    let observations = index
        .content_observations
        .as_ref()
        .expect("content pass attaches private store");
    assert!(observations.pipeline_provenance_complete);
    let owned = observations
        .records
        .iter()
        .find(|r| {
            r.owner == crate::content_observations::Owner::Object(node.id.clone())
                && r.findings
                    .iter()
                    .any(|f| f.error_code == "RP_E_OBJECT_FILENAME_ID_MISMATCH")
        })
        .expect("canonical diagnostics retain generating object owner before flattening");
    assert_eq!(owned.findings[0].json_pointer.as_ref(), "/id");
}

#[test]
fn report_subject_access_uses_cut_graph_and_byte_depth_limits_are_unavailable() {
    let mut full = graph();
    for id in ["node", "nested-report"] {
        let mut object = full.get(id).unwrap().clone();
        object.value["access"]["level"] = json!(if id == "node" {
            "restricted"
        } else {
            "exclusive"
        });
        full.insert(object);
    }
    let subject = SubjectGraph::build(&full, "chain", &mut SubjectBudget::default()).unwrap();
    let (access, findings) = subject
        .prerequisites(crate::ProjectLimits::default(), Default::default())
        .unwrap();
    let label = access
        .report(subject.index.get("chain").unwrap().ordinal)
        .unwrap();
    assert_eq!(label.required_floor.level, crate::AccessLevel::Restricted);
    assert!(
        findings
            .iter()
            .any(|f| f.error_code == "RP_E_ACCESS_CLAIM_SELECTION")
    );
    let mut budget = SubjectBudget {
        remaining_bytes: 1,
        ..Default::default()
    };
    assert!(matches!(
        SubjectGraph::build(&full, "chain", &mut budget),
        Err(SubjectUnavailable::Budget)
    ));
    assert_eq!(budget.remaining_bytes, 0);
}

#[test]
fn report_subject_single_snapshot_helper_does_not_evaluate_other_profiles() {
    let p = TempProject::fixture();
    let result = crate::validate_project(p.path()).unwrap();
    let mut index = result.index.unwrap();
    let chain = index
        .objects()
        .find(|o| o.object_type == ObjectType::ClaimChain)
        .unwrap()
        .clone();
    let original = index.claim_chain_report(&chain.id).unwrap().clone();
    let mut unrelated = chain.clone();
    unrelated.id = "unrelated".into();
    unrelated.value["validation_policy"]["profile"] = json!("confirmatory-v1");
    index.insert(unrelated);
    let (report, findings) = crate::claim::validate_one_claim_chain(
        &index,
        &chain,
        &Default::default(),
        Default::default(),
    );
    assert_eq!(report, original);
    assert!(findings.is_empty());
    let (_, global) =
        crate::claim::validate_claim_chains(&index, &Default::default(), Default::default());
    assert!(!global.is_empty());
}

#[test]
fn report_subject_bounds_edge_bytes_before_cloning_them() {
    let mut full = graph();
    let pointer = format!("/extensions/{}", "x".repeat(4 * 1024 * 1024));
    let mut edges: Vec<_> = full
        .objects()
        .flat_map(|o| full.references_from(&o.id).cloned())
        .collect();
    edges.push(edge(
        "node",
        "missing",
        ReferenceExpectation::Node,
        &pointer,
    ));
    full.set_references(edges);
    assert!(matches!(
        SubjectGraph::build(&full, "chain", &mut SubjectBudget::default()),
        Err(SubjectUnavailable::Budget)
    ));
}

#[test]
fn report_subject_budget_rejects_node_edge_and_traversal_exhaustion() {
    let mut full = graph();
    let mut edges: Vec<_> = full
        .objects()
        .flat_map(|o| full.references_from(&o.id).cloned())
        .collect();
    for position in 0..130 {
        let id = format!("extra-{position}");
        full.insert(record(&id));
        edges.push(edge(
            "node",
            &id,
            ReferenceExpectation::Node,
            &format!("/source/revisions/{position}"),
        ));
    }
    full.assign_ordinals();
    full.set_references(edges);
    assert!(matches!(
        SubjectGraph::build(&full, "chain", &mut SubjectBudget::default()),
        Err(SubjectUnavailable::Budget)
    ));

    let mut full = graph();
    let mut edges: Vec<_> = full
        .objects()
        .flat_map(|o| full.references_from(&o.id).cloned())
        .collect();
    for position in 0..1025 {
        edges.push(edge(
            "node",
            "missing",
            ReferenceExpectation::Node,
            &format!("/source/revisions/{position}"),
        ));
    }
    full.set_references(edges);
    assert!(matches!(
        SubjectGraph::build(&full, "chain", &mut SubjectBudget::default()),
        Err(SubjectUnavailable::Budget)
    ));

    let mut full = graph();
    let mut edges: Vec<_> = full
        .objects()
        .flat_map(|o| full.references_from(&o.id).cloned())
        .collect();
    for (child, parent) in [("node", "parent"), ("parent", "grandparent")] {
        let mut object = if child == "node" {
            full.get(child).unwrap().clone()
        } else {
            record(child)
        };
        object.value["revision"] = json!({"parents": [{"id": parent}]});
        full.insert(object);
        edges.push(edge(
            child,
            parent,
            ReferenceExpectation::Node,
            "/revision/parents/0/id",
        ));
    }
    full.insert(record("grandparent"));
    full.assign_ordinals();
    full.set_references(edges);
    let subject = SubjectGraph::build(&full, "chain", &mut SubjectBudget::default()).unwrap();
    assert!(matches!(
        subject.prerequisites(
            crate::ProjectLimits {
                traversal_depth: 0,
                ..Default::default()
            },
            Default::default()
        ),
        Err(SubjectUnavailable::Budget)
    ));
}

#[test]
fn report_scoped_reindex_clears_stale_missing_target_ordinals() {
    let mut full = ProjectIndex::default();
    full.insert(record("a"));
    full.insert(record("b"));
    full.assign_ordinals();
    full.set_references(vec![ReferenceEdge {
        source_id: "a".into(),
        target_id: "b".into(),
        target_ordinal: None,
        expected: ReferenceExpectation::Node,
        json_pointer: "/source/revisions/0".into(),
    }]);
    let edges = full.references_from("a").cloned().collect();
    let mut scoped = ProjectIndex::default();
    scoped.insert(full.get("a").unwrap().clone());
    scoped.assign_ordinals();
    scoped.set_references(edges);
    assert!(
        scoped
            .references_from("a")
            .next()
            .unwrap()
            .target_ordinal
            .is_none(),
        "a missing target must not retain an ordinal from the parent index"
    );
    let (_, findings) = crate::access::compute_access(&scoped, Default::default());
    assert!(findings.is_empty());
}

#[test]
fn review_canonical_same_length_change_between_read_and_stat_blocks_binding() {
    use std::{
        fs::{File, FileTimes, OpenOptions},
        io::Write,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    for nanoseconds_only in [false, true] {
        let (p, index, id) = expected_fixture();
        install_report(
            &p,
            &body(&index, &id).unwrap(),
            index.get(&id).unwrap().value["access"].clone(),
        );
        assert!(
            crate::validate_project(p.path())
                .unwrap()
                .findings
                .is_empty()
        );
        let node = index.get("qst_01J00000000000000000000010").unwrap();
        let path = p.path().join(node.source_file.as_str());
        // Fix the initial mtime so second-only and nanosecond-only changes are
        // deterministic; no sleeps, scheduling assumptions or background writers.
        let initial_time = std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        File::open(&path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(initial_time))
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        let mut changed = String::from_utf8(original.clone()).unwrap();
        assert!(changed.contains("Minimal"));
        changed = changed.replacen("Minimal", "Altered", 1);
        assert_eq!(changed.len(), original.len());
        let called = Arc::new(AtomicBool::new(false));
        let flag = called.clone();
        let result = crate::project::canonical_read_test::with_hook(
            node.source_file.clone(),
            move |read_fd| {
                let before = rustix::fs::fstat(read_fd).unwrap();
                let mut writer = OpenOptions::new().write(true).open(&path).unwrap();
                writer.write_all(changed.as_bytes()).unwrap();
                writer
                    .set_times(FileTimes::new().set_modified(
                        initial_time
                            + if nanoseconds_only {
                                Duration::from_nanos(1)
                            } else {
                                Duration::from_secs(1)
                            },
                    ))
                    .unwrap();
                let after = rustix::fs::fstat(read_fd).unwrap();
                assert_eq!(
                    (before.st_dev, before.st_ino, before.st_size),
                    (after.st_dev, after.st_ino, after.st_size)
                );
                if nanoseconds_only {
                    assert_eq!(before.st_mtime, after.st_mtime);
                    assert_ne!(before.st_mtime_nsec, after.st_mtime_nsec);
                } else {
                    assert_ne!(before.st_mtime, after.st_mtime);
                }
                flag.store(true, Ordering::SeqCst);
            },
            || crate::validate_project(p.path()).unwrap(),
        );
        assert!(called.load(Ordering::SeqCst));
        assert!(
            result
                .findings
                .iter()
                .any(|f| f.error_code == "RP_E_IO_READ_FAILED"
                    && f.source_file.as_ref() == Some(&node.source_file)),
            "{:?}",
            result.findings
        );
        assert!(!result.stage3_ran);
        assert!(
            result.index.is_none(),
            "no canonical completeness or binding on unstable input"
        );
    }
}

#[test]
fn review_canonical_hook_is_cleared_on_unwind_before_target_read() {
    let (p, _index, _id) = expected_fixture();
    let target = ProjectPath::new(".research/project.yaml").unwrap();
    let result = std::panic::catch_unwind(|| {
        crate::project::canonical_read_test::with_hook(
            target,
            |_| panic!("stale hook"),
            || panic!("intentional pre-read panic"),
        );
    });
    assert!(result.is_err());
    assert!(crate::validate_project(p.path()).unwrap().stage3_ran);
}

#[test]
fn review_limits_scoped_subject_reuses_caller_depth_and_exact_boundary() {
    use crate::{ReportComparison as C, ReportSubjectUnavailable as U};
    let (p, index, id) = expected_fixture();
    let child = index.get("qst_01J00000000000000000000010").unwrap();
    let parent_id = "qst_01J00000000000000000000990";
    let grandparent_id = "qst_01J00000000000000000000991";
    for (ancestor, parent) in [(parent_id, Some(grandparent_id)), (grandparent_id, None)] {
        let mut value = child.value.clone();
        value["id"] = json!(ancestor);
        value["revision"]["parents"] = parent.map_or(json!([]), |id| json!([{"id":id}]));
        std::fs::write(
            p.research(&format!("records/questions/ancestor--{ancestor}.yaml")),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
    save(&p, child, |v| {
        v["revision"]["parents"] = json!([{"id":parent_id}])
    });
    // Freeze the modified subject's actual scientific closure before generating.
    let index = reload(&p);
    let digest = index.claim_chain_report(&id).unwrap().source_sha256.clone();
    save(&p, index.get(&id).unwrap(), |v| {
        v["source_closure_sha256"] = json!(digest)
    });
    let index = reload(&p);
    let expected = body(&index, &id).unwrap();
    assert_eq!(expected["outcome"]["passed"], true, "{expected}");
    install_report(
        &p,
        &expected,
        index.get(&id).unwrap().value["access"].clone(),
    );
    assert!(
        crate::validate_project(p.path())
            .unwrap()
            .findings
            .is_empty()
    );
    // Child -> parent -> grandparent: exactly two edges are sufficient.
    for depth in [0, 1, 2] {
        let result = crate::validate_project_with_limits(
            p.path(),
            crate::ProjectLimits {
                traversal_depth: depth,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(result.stage3_ran);
        let index = result.index.unwrap();
        let observation = index.report_binding(&id, REPORT_ID).unwrap();
        if depth < 2 {
            assert_eq!(
                observation.comparison,
                C::SubjectUnavailable(U::Budget),
                "depth {depth}"
            );
            assert!(!index.claim_chain_report(&id).unwrap().valid);
            assert_eq!(
                body(&index, &id),
                Err(SubjectUnavailable::Budget),
                "private helper must retain caller limits too"
            );
            assert!(
                result
                    .findings
                    .iter()
                    .any(|f| f.error_code == "RP_E_RESOURCE_REPORT_SUBJECT_EXCEEDED")
            );
            assert!(
                result
                    .findings
                    .iter()
                    .any(|f| f.error_code == "RP_E_RESOURCE_TRAVERSAL_DEPTH_EXCEEDED")
            );
        } else {
            assert!(result.findings.is_empty(), "{:?}", result.findings);
            assert_eq!(observation.comparison, C::MatchedSubjectPassed);
            assert!(index.claim_chain_report(&id).unwrap().valid);
            assert_eq!(body(&index, &id).unwrap(), expected);
        }
    }
}

const REPORT_ID: &str = "art_01J00000000000000000000999";

fn install_report(p: &TempProject, body: &serde_json::Value, access: serde_json::Value) {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(body).unwrap();
    std::fs::write(p.path().join("report.json"), &bytes).unwrap();
    std::fs::create_dir_all(p.research("artifacts")).unwrap();
    std::fs::write(
        p.research(&format!("artifacts/report--{REPORT_ID}.yaml")),
        serde_json::to_vec(&json!({
            "schema":"rp/artifact-manifest/v1", "id":REPORT_ID, "title":"Runtime report",
            "uri":"file:report.json", "media_type":"application/json", "size_bytes":bytes.len(),
            "sha256":format!("sha256:{:x}", Sha256::digest(&bytes)),
            "created_at":"2026-01-01T08:06:00Z", "access":access
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn findings_cap_nested_subject_is_unavailable_not_a_truncated_pass() {
    let (p, index, id) = expected_fixture();
    save(&p, index.get(&id).unwrap(), |v| {
        v["validation_policy"]["profile"] = json!("confirmatory-v1")
    });
    let index = reload(&p);
    let mut budget = SubjectBudget {
        findings: crate::findings::Findings::new(1),
        ..Default::default()
    };
    assert!(matches!(
        crate::report_subject::expected_subject_report(&index, &id, &mut budget),
        Err(SubjectUnavailable::Budget)
    ));
    assert!(budget.findings.cancelled());
    assert!(matches!(
        crate::report_subject::expected_subject_report(&index, &id, &mut budget),
        Err(SubjectUnavailable::Budget)
    ));
    // An independent run still evaluates the complete honest failed body.
    assert_eq!(body(&index, &id).unwrap()["outcome"]["passed"], false);
    let report = crate::validate_project_with_limits(
        p.path(),
        crate::ProjectLimits {
            findings: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!report.is_complete() && !report.is_valid());
    assert!(report.index.is_none());
}

#[test]
fn runtime_binding_roundtrip_and_schema_valid_forgery() {
    let (p, index, id) = expected_fixture();
    let expected = body(&index, &id).unwrap();
    let access = index.get(&id).unwrap().value["access"].clone();
    install_report(&p, &expected, access.clone());
    let report = crate::validate_project(p.path()).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    assert!(report.index.unwrap().claim_chain_report(&id).unwrap().valid);
    let mut forged = expected;
    forged["subject"]["canonical_digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    crate::SchemaBundle::new()
        .unwrap()
        .validate(&forged, ProjectPath::new("forged.json").unwrap())
        .unwrap();
    install_report(&p, &forged, access);
    let report = crate::validate_project(p.path()).unwrap();
    let index = report.index.unwrap();
    assert!(index.artifact_verified(REPORT_ID));
    assert!(index.unbound_report_candidate(REPORT_ID).is_some());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.error_code == "RP_E_CLAIM_CHAIN_VALIDATION_REPORT_MISMATCH"),
        "{:?}",
        report.findings
    );
    assert!(!index.claim_chain_report(&id).unwrap().valid);
}

#[test]
fn runtime_binding_honest_failed_subject_and_forged_pass() {
    let (p, index, id) = expected_fixture();
    save(&p, index.get(&id).unwrap(), |v| {
        v["validation_policy"]["profile"] = json!("confirmatory-v1")
    });
    let index = reload(&p);
    let expected = body(&index, &id).unwrap();
    assert_eq!(expected["outcome"]["passed"], false);
    let access = index.get(&id).unwrap().value["access"].clone();
    install_report(&p, &expected, access.clone());
    let report = crate::validate_project(p.path()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.error_code == "RP_E_CONFIRMATORY_BACKBONE_MISSING")
    );
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.error_code == "RP_E_CLAIM_CHAIN_VALIDATION_REPORT_MISMATCH")
    );
    let index = report.index.unwrap();
    assert!(!index.claim_chain_report(&id).unwrap().valid);
    assert_eq!(
        index.report_binding(&id, REPORT_ID).unwrap().comparison,
        crate::ReportComparison::MatchedSubjectFailed
    );
    assert_eq!(
        index.report_binding(&id, REPORT_ID).unwrap().labels,
        crate::ReportLabels::Accepted
    );
    let mut forged = expected;
    forged["outcome"] = json!({"passed":true,"findings":[]});
    install_report(&p, &forged, access);
    let report = crate::validate_project(p.path()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.error_code == "RP_E_CLAIM_CHAIN_VALIDATION_REPORT_MISMATCH"),
        "{:?}",
        report.findings
    );
}

#[test]
fn runtime_binding_pair_specific_and_unrelated_errors_remain_outer() {
    use crate::{ReportComparison as C, ReportLabels as L};
    let (p, index, id) = expected_fixture();
    let expected = body(&index, &id).unwrap();
    let chain = index.get(&id).unwrap();
    install_report(&p, &expected, chain.value["access"].clone());
    let other_id = "cch_01J00000000000000000000997";
    let mut other = chain.value.clone();
    other["id"] = json!(other_id);
    std::fs::write(
        p.research(&format!("claim-chains/other--{other_id}.yaml")),
        serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();
    let unrelated = index
        .objects()
        .find(|o| {
            matches!(o.object_type, ObjectType::Node(_))
                && !chain.value["node_revisions"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(o.id.as_ref()))
        })
        .unwrap();
    save(&p, unrelated, |v| {
        v["revision"]["parents"] = json!([{"id":unrelated.id.as_ref()}])
    });
    let report = crate::validate_project(p.path()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.error_code == "RP_E_REVISION_DAG_CYCLE")
    );
    let index = report.index.unwrap();
    assert_eq!(body(&index, &id).unwrap(), expected);
    assert_eq!(
        index.report_binding(&id, REPORT_ID).unwrap().comparison,
        C::MatchedSubjectPassed
    );
    assert_eq!(
        index.report_binding(&id, REPORT_ID).unwrap().labels,
        L::Accepted
    );
    assert_eq!(
        index
            .report_binding(other_id, REPORT_ID)
            .unwrap()
            .comparison,
        C::Mismatch
    );
    assert!(
        index
            .report_binding(&id, "art_01J00000000000000000000998")
            .is_none()
    );
    assert!(index.claim_chain_report(&id).unwrap().valid);
    assert!(!index.claim_chain_report(other_id).unwrap().valid);
}

#[test]
fn runtime_binding_equal_labels_cover_all_v_and_context_not_candidate_access() {
    use crate::{ReportComparison as C, ReportLabels as L};
    for case in [
        "equal",
        "low",
        "high",
        "compartment",
        "equal-high",
        "project-floor",
        "effective-floor",
    ] {
        let (p, index, id) = expected_fixture();
        let chain = index.get(&id).unwrap();
        match case {
            "equal-high" => save(&p, chain, |v| {
                v["access"] = json!({"level":"restricted","compartments":["analysis"]})
            }),
            "project-floor" => {
                let project = index
                    .objects()
                    .find(|o| o.object_type == ObjectType::Project)
                    .unwrap();
                save(&p, project, |v| {
                    v["access_defaults"] =
                        json!({"level":"restricted", "compartments":["project-secret"]});
                    v["validation_policy"] = json!(".research/policies/context.yaml");
                });
                std::fs::create_dir_all(p.research("policies")).unwrap();
                std::fs::write(p.research("policies/context.yaml"), "ignored: true\n").unwrap();
            }
            "effective-floor" => {
                let node = index
                    .get(chain.value["node_revisions"][0].as_str().unwrap())
                    .unwrap();
                save(&p, node, |v| {
                    v["access"] = json!({"level":"restricted","compartments":["secret"]})
                });
            }
            _ => {}
        }
        let index = reload(&p);
        let expected = body(&index, &id).unwrap();
        let mut access = index.get(&id).unwrap().value["access"].clone();
        match case {
            "low" => access["level"] = json!("public"),
            "high" => access["level"] = json!("restricted"),
            "compartment" => access["compartments"] = json!(["extra"]),
            _ => {}
        }
        install_report(&p, &expected, access);
        let report = crate::validate_project(p.path()).unwrap();
        if case == "effective-floor" {
            assert!(
                report
                    .findings
                    .iter()
                    .any(|f| f.error_code == "RP_E_ACCESS_CLAIM_SELECTION")
            );
        }
        let index = report.index.unwrap();
        let obs = index.report_binding(&id, REPORT_ID).unwrap();
        assert_eq!(
            obs.comparison,
            if expected["outcome"]["passed"] == true {
                C::MatchedSubjectPassed
            } else {
                C::MatchedSubjectFailed
            },
            "{case}"
        );
        let accepted = matches!(case, "equal" | "equal-high");
        assert_eq!(
            obs.labels,
            if accepted { L::Accepted } else { L::Rejected },
            "{case}"
        );
        assert_eq!(
            report
                .findings
                .iter()
                .any(|f| f.error_code == "RP_E_REPORT_LABEL_REJECTED"),
            !accepted,
            "{case}"
        );
        assert_eq!(
            index.claim_chain_report(&id).unwrap().valid,
            accepted,
            "{case}"
        );
    }
}

#[test]
fn runtime_binding_source_separation_before_missing_or_bad_candidate() {
    use crate::ReportComparison;
    for case in ["direct", "transitive", "missing"] {
        let (p, index, id) = expected_fixture();
        if case != "missing" {
            install_report(
                &p,
                &json!({}),
                index.get(&id).unwrap().value["access"].clone(),
            );
        }
        let node_id = index.get(&id).unwrap().value["node_revisions"][0]
            .as_str()
            .unwrap();
        let node = index.get(node_id).unwrap();
        if case == "transitive" {
            let unselected = index
                .objects()
                .find(|o| {
                    matches!(o.object_type, ObjectType::Node(_))
                        && o.id.as_ref() == "qst_01J00000000000000000000020"
                })
                .unwrap();
            save(&p, node, |v| {
                v["source"]["revisions"] = json!([unselected.id.as_ref()])
            });
            save(&p, unselected, |v| {
                v["source"]["artifacts"] = json!([REPORT_ID])
            });
        } else {
            save(&p, node, |v| v["source"]["artifacts"] = json!([REPORT_ID]));
        }
        let report = crate::validate_project(p.path()).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.error_code == "RP_E_REPORT_SOURCE_SEPARATION"),
            "{case}: {:?}",
            report.findings
        );
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.error_code == "RP_E_CLAIM_CHAIN_VALIDATION_REPORT_MISMATCH")
        );
        assert!(report.findings.iter().any(|f| f.error_code
            == if case == "missing" {
                "RP_E_REFERENCE_NOT_FOUND"
            } else {
                "RP_E_REPORT_SCHEMA_INVALID"
            }));
        let index = report.index.unwrap();
        assert_eq!(
            index.report_binding(&id, REPORT_ID).unwrap().comparison,
            ReportComparison::SourceSeparation
        );
        assert!(!index.claim_chain_report(&id).unwrap().valid);
    }
}

#[test]
fn runtime_binding_subject_mutations_and_honest_failure_rebinding() {
    use crate::ReportComparison as C;
    for mutation in ["dependency", "policy", "schema", "narrative", "artifact"] {
        let p = TempProject::fixture_suite("claim-chain-confirmatory-v1");
        let index = reload(&p);
        let chain = index
            .objects()
            .find(|o| o.object_type == ObjectType::ClaimChain)
            .unwrap();
        let id = chain.id.to_string();
        save(&p, chain, |v| v["validation_report"] = json!(REPORT_ID));
        let index = reload(&p);
        let access = index.get(&id).unwrap().value["access"].clone();
        let expected = body(&index, &id).unwrap();
        install_report(&p, &expected, access.clone());
        let report = crate::validate_project(p.path()).unwrap();
        assert!(report.findings.is_empty(), "{:?}", report.findings);
        let node = index
            .get(
                index.get(&id).unwrap().value["node_revisions"][0]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
        match mutation {
            "dependency" => save(&p, node, |v| v["title"] = json!("Edited dependency")),
            "policy" => {
                let project = index
                    .objects()
                    .find(|o| o.object_type == ObjectType::Project)
                    .unwrap();
                save(&p, project, |v| {
                    v["validation_policy"] = json!(".research/policies/new.yaml")
                });
                std::fs::create_dir_all(p.research("policies")).unwrap();
                std::fs::write(p.research("policies/new.yaml"), "ignored: true\n").unwrap();
            }
            "schema" => {
                std::fs::write(
                    p.research("schemas/fixture/synthetic/v1.schema.json"),
                    br#"{"type":"object","minProperties":1}"#,
                )
                .unwrap();
            }
            "narrative" => {
                std::fs::create_dir_all(p.research("notes")).unwrap();
                std::fs::write(p.research("notes/note.md"), "actual").unwrap();
                save(
                    &p,
                    node,
                    |v| v["narrative"] = json!({"path":".research/notes/note.md", "role":"detail", "sha256":format!("sha256:{}", "0".repeat(64))}),
                );
            }
            _ => {
                let artifact = index
                    .objects()
                    .find(|o| o.object_type == ObjectType::Artifact)
                    .unwrap();
                std::fs::write(
                    p.path().join(
                        artifact.value["uri"]
                            .as_str()
                            .unwrap()
                            .strip_prefix("file:")
                            .unwrap(),
                    ),
                    "modified",
                )
                .unwrap();
            }
        }
        let report = crate::validate_project(p.path()).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.error_code == "RP_E_CLAIM_CHAIN_VALIDATION_REPORT_MISMATCH"),
            "{mutation}: {:?}",
            report.findings
        );
        let index = report.index.unwrap();
        assert_eq!(
            index.report_binding(&id, REPORT_ID).unwrap().comparison,
            C::Mismatch
        );
        let changed = body(&index, &id).unwrap();
        assert_ne!(expected, changed);
        install_report(&p, &changed, access);
        let index = reload(&p);
        assert_eq!(
            index.report_binding(&id, REPORT_ID).unwrap().comparison,
            if changed["outcome"]["passed"] == true {
                C::MatchedSubjectPassed
            } else {
                C::MatchedSubjectFailed
            },
            "{mutation}"
        );
    }
}

#[test]
fn runtime_binding_multiple_pairs_share_one_nonrefunded_subject_budget() {
    use crate::{ReportComparison as C, ReportSubjectUnavailable as U};
    let (p, index, id) = expected_fixture();
    install_report(
        &p,
        &body(&index, &id).unwrap(),
        index.get(&id).unwrap().value["access"].clone(),
    );
    let index = reload(&p);
    let mut single_budget = SubjectBudget::default();
    let initial_bytes = single_budget.remaining_bytes;
    let single = crate::report_binding::bind_with_budget(
        &index,
        &mut Default::default(),
        &mut single_budget,
        256,
    );
    assert_eq!(single[id.as_str()].1.comparison, C::MatchedSubjectPassed);
    let other_id = "cch_01J00000000000000000000997";
    let mut other = index.get(&id).unwrap().value.clone();
    other["id"] = json!(other_id);
    std::fs::write(
        p.research(&format!("claim-chains/other--{other_id}.yaml")),
        serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();
    let index = reload(&p);
    let mut budget = SubjectBudget {
        // The added snapshot also adds a canonical observation/map reservation.
        // Leave bounded headroom for that, but nowhere near a second subject.
        remaining_bytes: initial_bytes - single_budget.remaining_bytes + 4096,
        ..Default::default()
    };
    let mut findings = crate::findings::Findings::default();
    let pairs = crate::report_binding::bind_with_budget(&index, &mut findings, &mut budget, 256);
    assert_eq!(pairs[id.as_str()].1.comparison, C::MatchedSubjectPassed);
    assert_eq!(
        pairs[other_id].1.comparison,
        C::SubjectUnavailable(U::Budget)
    );
    assert_eq!(budget.remaining_bytes, 0);
    assert!(
        findings
            .iter()
            .any(|f| f.error_code == "RP_E_RESOURCE_REPORT_SUBJECT_EXCEEDED")
    );
}

#[test]
fn runtime_binding_shared_artifact_pair_count_is_bounded_before_evaluation() {
    let (_p, mut index, id) = expected_fixture();
    let original = index.get(&id).unwrap().clone();
    for n in 0..256 {
        let mut chain = original.clone();
        chain.id = format!("cch_{n:026}").into();
        chain.value["id"] = json!(chain.id.as_ref());
        index.insert(chain);
    }
    let mut findings = crate::findings::Findings::default();
    let mut budget = SubjectBudget::default();
    let work = budget.remaining_work;
    let bindings = crate::report_binding::bind_with_budget(&index, &mut findings, &mut budget, 256);
    assert!(bindings.is_empty());
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings.as_slice()[0].error_code,
        "RP_E_RESOURCE_REPORT_PAIRS_EXCEEDED"
    );
    assert_eq!(
        budget.remaining_work, work,
        "no per-pair subject work before admission"
    );
    index.set_report_bindings(bindings);
    assert!(!index.claim_chain_report(&id).unwrap().valid);
}

#[test]
fn runtime_binding_observation_exhaustion_has_resource_identity() {
    let (p, _index, _id) = expected_fixture();
    crate::content_observations::test_limits(crate::content_observations::ObservationLimits {
        records: 0,
        ..Default::default()
    });
    let report = crate::validate_project(p.path()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.error_code == "RP_E_RESOURCE_REPORT_SUBJECT_EXCEEDED"
                && f.finding_family == "resource_limit"),
        "{:?}",
        report.findings
    );
}

#[test]
fn runtime_binding_unavailable_and_shared_batch_budget_are_not_mismatch() {
    use crate::{ReportComparison as C, ReportSubjectUnavailable as U};
    let (p, index, id) = expected_fixture();
    install_report(
        &p,
        &body(&index, &id).unwrap(),
        index.get(&id).unwrap().value["access"].clone(),
    );
    let mut index = reload(&p);
    let mut findings = crate::findings::Findings::default();
    let mut budget = SubjectBudget {
        remaining_work: 0,
        ..Default::default()
    };
    let bindings = crate::report_binding::bind_with_budget(&index, &mut findings, &mut budget, 256);
    assert_eq!(
        bindings[id.as_str()].1.comparison,
        C::SubjectUnavailable(U::Budget)
    );
    assert_eq!(
        findings.as_slice()[0].error_code,
        "RP_E_RESOURCE_REPORT_SUBJECT_EXCEEDED"
    );
    assert_eq!(findings.as_slice()[0].finding_family, "resource_limit");
    findings = crate::findings::Findings::default();
    let bindings = crate::report_binding::bind_with_budget(
        &index,
        &mut findings,
        &mut SubjectBudget::default(),
        0,
    );
    assert!(bindings.is_empty());
    assert_eq!(
        findings.as_slice()[0].error_code,
        "RP_E_RESOURCE_REPORT_PAIRS_EXCEEDED"
    );
    index.set_report_bindings(bindings);
    assert!(!index.claim_chain_report(&id).unwrap().valid);
    let node = index
        .get(
            index.get(&id).unwrap().value["node_revisions"][0]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    save(
        &p,
        node,
        |v| v["narrative"] = json!({"path":".research/notes/missing.md", "role":"detail", "sha256":format!("sha256:{}", "0".repeat(64))}),
    );
    let report = crate::validate_project(p.path()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.error_code == "RP_E_REPORT_SUBJECT_CONTENT_INCOMPLETE")
    );
    let index = report.index.unwrap();
    assert_eq!(
        index.report_binding(&id, REPORT_ID).unwrap().comparison,
        C::SubjectUnavailable(U::ContentIncomplete)
    );
    assert!(!index.claim_chain_report(&id).unwrap().valid);
}

#[test]
fn execution_stopped_subject_has_no_body_or_binding_success() {
    use std::{
        sync::{Arc, atomic::AtomicBool},
        time::Duration,
    };
    let (_root, mut index, id) = expected_fixture();
    let execution =
        crate::ExecutionBudget::new(Duration::from_secs(60), Arc::new(AtomicBool::new(true)));
    index.execution = execution.clone();
    let mut findings = crate::findings::Findings::with_execution(1000, execution);
    let mut budget = SubjectBudget {
        findings: findings.fork(),
        ..SubjectBudget::default()
    };
    assert!(matches!(
        crate::report_subject::expected_subject_report(&index, &id, &mut budget),
        Err(SubjectUnavailable::Stopped(
            crate::ExecutionStop::Interrupted
        ))
    ));
    assert!(crate::report_binding::bind_reports(&index, &mut findings).is_empty());
    let (display, error, complete) = findings.finish();
    assert!(error && !complete);
    assert_eq!(display[0].error_code, "RP_E_INTERRUPTED");
}

fn expected_fixture() -> (TempProject, ProjectIndex, String) {
    let p = TempProject::fixture();
    let index = crate::validate_project(p.path()).unwrap().index.unwrap();
    let chain = index
        .objects()
        .find(|o| o.object_type == ObjectType::ClaimChain)
        .unwrap();
    let id = chain.id.to_string();
    let mut value = chain.value.clone();
    value["validation_report"] = json!("art_01J00000000000000000000999");
    std::fs::write(
        p.path().join(chain.source_file.as_str()),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let index = crate::validate_project(p.path()).unwrap().index.unwrap();
    (p, index, id)
}

#[test]
fn expected_body_real_preallocated_subject_is_schema_valid_and_nonbinding() {
    let (_p, index, id) = expected_fixture();
    let body =
        crate::report_subject::expected_subject_report(&index, &id, &mut SubjectBudget::default())
            .unwrap()
            .body;
    crate::SchemaBundle::new()
        .unwrap()
        .validate(&body, ProjectPath::new("expected.json").unwrap())
        .unwrap();
    assert_eq!(body["outcome"], json!({"passed":true,"findings":[]}));
    assert_eq!(
        body["subject"]["canonical_digest"],
        json!(index.canonical_digest(&id).unwrap())
    );
    assert_eq!(
        body["subject"]["validation_policy"],
        index.get(&id).unwrap().value["validation_policy"]
    );
    assert_eq!(
        body["source_closure"]["sha256"],
        json!(index.claim_chain_report(&id).unwrap().source_sha256)
    );
    assert_eq!(
        body["validator"]["fingerprint"],
        json!(crate::report_fingerprint::fingerprint())
    );
    assert!(
        body["validation_dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["id"] != "art_01J00000000000000000000999")
    );
    assert!(!index.artifact_verified("art_01J00000000000000000000999"));
}

fn body(index: &ProjectIndex, id: &str) -> Result<serde_json::Value, SubjectUnavailable> {
    crate::report_subject::expected_subject_report(index, id, &mut SubjectBudget::default())
        .map(|r| r.body)
}
fn save(p: &TempProject, object: &ObjectRecord, mutate: impl FnOnce(&mut serde_json::Value)) {
    let mut value = object.value.clone();
    mutate(&mut value);
    std::fs::write(
        p.path().join(object.source_file.as_str()),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
}
fn reload(p: &TempProject) -> ProjectIndex {
    let report = crate::validate_project(p.path()).unwrap();
    report
        .index
        .unwrap_or_else(|| panic!("stage 1/2: {:?}", report.findings))
}
fn error_owned(body: &serde_json::Value, code: &str, id: &str) {
    assert_eq!(body["outcome"]["passed"], false, "{body:#}");
    assert!(
        body["outcome"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["error_code"] == code && f["owner"]["id"] == id),
        "missing {code} owned by {id}: {body:#}"
    );
}

#[test]
fn expected_body_failed_profile_access_layout_reference_and_narrative_are_honest() {
    for mutation in ["profile", "access", "layout", "reference", "narrative"] {
        let (p, index, id) = expected_fixture();
        let chain = index.get(&id).unwrap();
        let node_id = chain.value["node_revisions"][0].as_str().unwrap();
        let node = index.get(node_id).unwrap();
        let (code, owner) = match mutation {
            "profile" => {
                save(&p, chain, |v| {
                    v["validation_policy"]["profile"] = json!("confirmatory-v1")
                });
                ("RP_E_CONFIRMATORY_BACKBONE_MISSING", id.as_str())
            }
            "access" => {
                save(&p, node, |v| v["access"]["level"] = json!("restricted"));
                ("RP_E_ACCESS_CLAIM_SELECTION", id.as_str())
            }
            "layout" => {
                std::fs::rename(
                    p.path().join(node.source_file.as_str()),
                    p.research("relations/wrong.yaml"),
                )
                .unwrap();
                ("RP_E_OBJECT_FILENAME_ID_MISMATCH", node_id)
            }
            "reference" => {
                save(&p, node, |v| {
                    v["source"]["revisions"] = json!(["qst_01J00000000000000000000998"])
                });
                ("RP_E_DANGLING_REVISION_REFERENCE", node_id)
            }
            _ => {
                std::fs::create_dir_all(p.research("notes")).unwrap();
                std::fs::write(p.research("notes/note.md"), "actual").unwrap();
                save(
                    &p,
                    node,
                    |v| v["narrative"] = json!({"path":".research/notes/note.md", "role":"detail", "sha256":format!("sha256:{}", "0".repeat(64))}),
                );
                ("RP_E_NARRATIVE_DIGEST_MISMATCH", node_id)
            }
        };
        let result = body(&reload(&p), &id).unwrap();
        error_owned(&result, code, owner);
        if mutation == "reference" {
            assert!(
                result["validation_dependencies"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e["id"] == "qst_01J00000000000000000000998"
                        && e["object_type"] == "rp/node-revision/v1"
                        && e["canonical_digest"].is_null())
            );
            assert_eq!(
                result["source_closure"]["unresolved_ids"],
                json!(["qst_01J00000000000000000000998"])
            );
        }
    }
}

#[test]
fn expected_body_unread_draft_duplicate_and_coverage_are_unavailable() {
    for mutation in ["unread", "draft", "duplicate", "coverage", "low-store"] {
        let (p, mut index, id) = expected_fixture();
        let node_id = index.get(&id).unwrap().value["node_revisions"][0]
            .as_str()
            .unwrap();
        let node = index.get(node_id).unwrap();
        match mutation {
            "unread" => save(
                &p,
                node,
                |v| v["narrative"] = json!({"path":".research/notes/missing.md", "role":"detail", "sha256":format!("sha256:{}", "0".repeat(64))}),
            ),
            "draft" => save(&p, node, |v| v["record_state"] = json!("draft")),
            "duplicate" => {
                std::fs::write(
                    p.research("records/questions/duplicate.yaml"),
                    serde_json::to_vec(&node.value).unwrap(),
                )
                .unwrap();
            }
            "coverage" => {
                index
                    .content_observations
                    .as_mut()
                    .unwrap()
                    .coverage_complete = false;
            }
            _ => crate::content_observations::test_limits(
                crate::content_observations::ObservationLimits {
                    records: 0,
                    ..Default::default()
                },
            ),
        }
        if mutation != "coverage" {
            index = reload(&p);
        }
        assert!(body(&index, &id).is_err(), "{mutation}");
    }
    let (_p, index, id) = expected_fixture();
    let mut budget = SubjectBudget {
        remaining_work: 1,
        ..Default::default()
    };
    assert!(matches!(
        crate::report_subject::expected_subject_report(&index, &id, &mut budget),
        Err(SubjectUnavailable::Budget)
    ));
    assert_eq!(budget.remaining_work, 0);
}

#[test]
fn expected_body_two_snapshots_and_unrelated_errors_do_not_pollute() {
    let (p, index, id) = expected_fixture();
    let chain = index.get(&id).unwrap();
    let other_id = "cch_01J00000000000000000000997";
    let mut other = chain.value.clone();
    other["id"] = json!(other_id);
    other["validation_policy"]["profile"] = json!("confirmatory-v1");
    std::fs::write(
        p.research(&format!("claim-chains/other--{other_id}.yaml")),
        serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();
    let unrelated = index
        .objects()
        .find(|o| {
            matches!(o.object_type, ObjectType::Node(_))
                && !chain.value["node_revisions"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(o.id.as_ref()))
        })
        .unwrap();
    save(&p, unrelated, |v| {
        v["narrative"] = json!({"path":".research/notes/unread.md", "role":"detail", "sha256":format!("sha256:{}", "0".repeat(64))});
        v["revision"]["parents"] = json!([{"id":unrelated.id.as_ref()}]);
    });
    let index = reload(&p);
    assert_eq!(body(&index, &id).unwrap()["outcome"]["passed"], true);
    error_owned(
        &body(&index, other_id).unwrap(),
        "RP_E_CONFIRMATORY_BACKBONE_MISSING",
        other_id,
    );
    let node = index
        .get(chain.value["node_revisions"][0].as_str().unwrap())
        .unwrap();
    save(&p, node, |v| {
        v["revision"]["parents"] = json!([{"id":node.id.as_ref()}])
    });
    error_owned(
        &body(&reload(&p), &id).unwrap(),
        "RP_E_REVISION_DAG_CYCLE",
        &node.id,
    );
}

#[test]
fn expected_body_context_hashes_bind_observed_bytes_without_reopening() {
    let (p, index, id) = expected_fixture();
    let node = index
        .get(
            index.get(&id).unwrap().value["node_revisions"][0]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    save(&p, node, |v| {
        v["extensions"] = json!({"fixture.synthetic/v1": {}})
    });
    let path = ".research/schemas/fixture/synthetic/v1.schema.json";
    let old = br#"{"type":"object"}"#;
    std::fs::write(p.path().join(path), old).unwrap();
    let index = reload(&p);
    let first = body(&index, &id).unwrap();
    assert_eq!(first["validation_context"].as_array().unwrap().len(), 1);
    std::fs::write(
        p.path().join(path),
        br#"{"type":"object","minProperties":1}"#,
    )
    .unwrap();
    assert_eq!(
        body(&index, &id).unwrap(),
        first,
        "no re-open during expected body"
    );
    let second = body(&reload(&p), &id).unwrap();
    assert_ne!(first["validation_context"], second["validation_context"]);
    error_owned(&second, "RP_E_EXTENSION_PAYLOAD_SCHEMA_MISMATCH", &node.id);
}

#[test]
fn expected_body_confirmatory_fixture_is_complete_including_artifact_mismatch() {
    let p = TempProject::fixture_suite("claim-chain-confirmatory-v1");
    let index = reload(&p);
    let chain = index
        .objects()
        .find(|o| o.object_type == ObjectType::ClaimChain)
        .unwrap();
    save(&p, chain, |v| {
        v["validation_report"] = json!("art_01J00000000000000000000999")
    });
    let id = chain.id.to_string();
    let index = reload(&p);
    assert_eq!(body(&index, &id).unwrap()["outcome"]["passed"], true);
    let artifact = index
        .objects()
        .find(|o| o.object_type == ObjectType::Artifact)
        .unwrap();
    std::fs::write(
        p.path().join(
            artifact.value["uri"]
                .as_str()
                .unwrap()
                .strip_prefix("file:")
                .unwrap(),
        ),
        "mismatch",
    )
    .unwrap();
    let failed = body(&reload(&p), &id).unwrap();
    error_owned(&failed, "RP_E_ARTIFACT_DIGEST_MISMATCH", &artifact.id);
}

#[test]
fn expected_body_own_report_nonrole_and_samepath_ambiguity_are_unavailable() {
    use crate::content_observations::{Completeness, Owner, Phase};
    let (p, mut index, id) = expected_fixture();
    let node_id = index.get(&id).unwrap().value["node_revisions"][0]
        .as_str()
        .unwrap()
        .to_string();
    let store = index.content_observations.as_mut().unwrap();
    let k = store.begin(
        Owner::Object(node_id.as_str().into()),
        Phase::Narrative,
        Some(".research/notes/x.md"),
        None,
        &[],
    );
    store.state(k, Completeness::Verified);
    store.hash(k, &format!("sha256:{}", "0".repeat(64)));
    let k = store.begin(
        Owner::Object(node_id.as_str().into()),
        Phase::Narrative,
        Some(".research/notes/x.md"),
        None,
        &[],
    );
    store.state(k, Completeness::Verified);
    store.hash(k, &format!("sha256:{}", "1".repeat(64)));
    assert_eq!(
        body(&index, &id),
        Err(SubjectUnavailable::ContentIncomplete)
    );
    let index = reload(&p);
    save(&p, index.get(&node_id).unwrap(), |v| {
        v["source"]["artifacts"] = json!(["art_01J00000000000000000000999"])
    });
    assert_eq!(body(&reload(&p), &id), Err(SubjectUnavailable::OwnReport));
}

#[test]
fn expected_body_unrelated_artifact_report_errors_and_https_science_are_scoped() {
    let p = TempProject::fixture_suite("claim-chain-confirmatory-v1");
    let index = reload(&p);
    let chain = index
        .objects()
        .find(|o| o.object_type == ObjectType::ClaimChain)
        .unwrap();
    let id = chain.id.to_string();
    save(&p, chain, |v| {
        v["validation_report"] = json!("art_01J00000000000000000000999")
    });
    let artifact = index
        .objects()
        .find(|o| o.object_type == ObjectType::Artifact)
        .unwrap();
    for (new_id, uri) in [
        ("art_01J00000000000000000000998", "file:data/missing"),
        (
            "art_01J00000000000000000000999",
            "https://example.invalid/report",
        ),
    ] {
        let mut value = artifact.value.clone();
        value["id"] = json!(new_id);
        value["uri"] = json!(uri);
        std::fs::write(
            p.research(&format!("artifacts/unrelated--{new_id}.yaml")),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
    let index = reload(&p);
    assert_eq!(body(&index, &id).unwrap()["outcome"]["passed"], true);
    let artifact = index.get(&artifact.id).unwrap();
    save(&p, artifact, |v| {
        v["uri"] = json!("https://example.invalid/science")
    });
    let result = body(&reload(&p), &id).unwrap();
    assert_eq!(result["outcome"]["passed"], false);
    assert!(
        result["outcome"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["error_code"] == "RP_E_CONFIRMATORY_ARTIFACT_REQUIRED")
    );
    assert!(
        result["outcome"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["error_code"] != "RP_E_REPORT_BODY_UNAVAILABLE")
    );
}

#[test]
fn expected_body_context_consumers_transitive_policy_and_compact_sets_are_exact() {
    let (p, index, id) = expected_fixture();
    let project = index
        .objects()
        .find(|o| o.object_type == ObjectType::Project)
        .unwrap();
    save(&p, project, |v| {
        v["validation_policy"] = json!(".research/policies/policy.yaml")
    });
    std::fs::create_dir_all(p.research("policies")).unwrap();
    std::fs::write(p.research("policies/policy.yaml"), "ignored: true\n").unwrap();
    let schema = p.research("schemas/fixture/synthetic/v1.schema.json");
    std::fs::write(&schema, br#"{"$ref":"defs.schema.json"}"#).unwrap();
    std::fs::write(
        schema.with_file_name("defs.schema.json"),
        br#"{"type":"object"}"#,
    )
    .unwrap();
    let mut index = reload(&p);
    let result = body(&index, &id).unwrap();
    let paths: Vec<_> = result["validation_context"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        [
            ".research/policies/policy.yaml",
            ".research/schemas/fixture/synthetic/defs.schema.json",
            ".research/schemas/fixture/synthetic/v1.schema.json"
        ]
    );
    let store = index.content_observations.as_mut().unwrap();
    let key = store.begin(
        crate::content_observations::Owner::Context("unused".into()),
        crate::content_observations::Phase::ExtensionSchema,
        Some("unused"),
        Some("other/v1"),
        &["qst_01J00000000000000000000997".into()],
    );
    store.state(key, crate::content_observations::Completeness::Unavailable);
    assert_eq!(
        body(&index, &id).unwrap(),
        result,
        "unconsumed namespace context is excluded"
    );
    let chain = index.get(&id).unwrap().clone();
    save(&p, &chain, |v| {
        v["validation_policy"]["profile"] = json!("confirmatory-v1")
    });
    let index = reload(&p);
    let result = body(&index, &id).unwrap();
    for field in [
        "selected_nodes",
        "selected_relations",
        "validation_dependencies",
    ] {
        let keys: Vec<_> = result[field]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    e["object_type"].as_str().unwrap(),
                    e["id"].as_str().unwrap(),
                )
            })
            .collect();
        assert!(keys.windows(2).all(|w| w[0] < w[1]));
    }
    let fs = result["outcome"]["findings"].as_array().unwrap();
    let key = |f: &serde_json::Value| {
        (
            f["severity"].as_str().unwrap().to_owned(),
            f["owner"]["kind"].as_str().unwrap().to_owned(),
            f["owner"]["id"].as_str().unwrap_or_default().to_owned(),
            f["json_pointer"].as_str().unwrap().to_owned(),
            f["error_code"].as_str().unwrap().to_owned(),
            f["finding_family"].as_str().unwrap().to_owned(),
        )
    };
    assert!(fs.windows(2).all(|w| key(&w[0]) < key(&w[1])));
}
