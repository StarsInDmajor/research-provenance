mod common;

use std::{collections::BTreeSet, fs};

use common::{TempProject, fixture_root, materialize_overlay, parse_yaml_file, suite_file};
use rp_core::{AccessLabel, AccessLevel, ExportRequest, validate_project};
use serde_json::Value;

#[test]
fn all_fifty_access_oracle_rows_match_complete_transitive_closure() {
    let report = validate_project(&fixture_root("access-closure-v1")).expect("validate fixture");
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    let index = report.index.unwrap();
    let oracle = parse_yaml_file(&suite_file(
        "access-closure-v1",
        "expected-access-closure.yaml",
    ));
    let entries = oracle["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 50);

    for expected in entries {
        let id = expected["id"].as_str().unwrap();
        let actual = index.access_report(id).expect("access report exists");
        assert_eq!(
            actual.required_floor,
            access_label(&expected["required_floor"]),
            "{id}"
        );
        assert_eq!(actual.declared, access_label(&expected["declared"]), "{id}");
        assert_eq!(
            actual.effective,
            access_label(&expected["effective"]),
            "{id}"
        );
        assert_eq!(actual.valid, expected["valid"].as_bool().unwrap(), "{id}");
        assert_eq!(
            actual.dependency_count,
            expected["dependency_count"].as_u64().unwrap() as usize,
            "{id}"
        );
    }
}

#[test]
fn all_fifteen_access_overlays_emit_their_frozen_primary_code() {
    let overlays = [
        "overlay-assessment-supersession-leak",
        "overlay-assessment-target-leak",
        "overlay-binding-target-leak",
        "overlay-claim-selection-leak",
        "overlay-compartment-drop",
        "overlay-declassification-in-mvp",
        "overlay-exclusive-downgrade",
        "overlay-invalid-access-downgrade",
        "overlay-relation-endpoint-leak",
        "overlay-relation-parent-leak",
        "overlay-revision-parent-leak",
        "overlay-thread-parent-leak",
        "overlay-thread-root-leak",
        "overlay-unknown-access-level",
        "overlay-unsorted-compartments",
    ];

    for overlay in overlays {
        let (project, expected_code) = materialize_overlay("access-closure-v1", overlay);
        let report = validate_project(project.path()).expect("validate access overlay");
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.error_code == expected_code),
            "{overlay}: expected {expected_code}, got {:?}",
            report.findings
        );
    }
}

#[test]
fn all_eight_export_oracle_cases_match_effective_access() {
    let report = validate_project(&fixture_root("access-closure-v1")).expect("validate fixture");
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    let index = report.index.unwrap();
    let oracle = parse_yaml_file(&suite_file(
        "access-closure-v1",
        "expected-access-closure.yaml",
    ));

    for query in oracle["export_queries"].as_array().unwrap() {
        let request = ExportRequest {
            level_ceiling: AccessLevel::parse(query["request"]["level_ceiling"].as_str().unwrap())
                .unwrap(),
            allowed_compartments: query["request"]["allowed_compartments"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect::<BTreeSet<_>>(),
        };
        let decision = index
            .export_check(query["target_id"].as_str().unwrap(), request)
            .expect("export target exists");
        assert_eq!(
            decision.eligible,
            query["eligible"].as_bool().unwrap(),
            "{}",
            query["name"].as_str().unwrap()
        );
    }
}

#[test]
fn access_reports_do_not_own_dependency_step_vectors() {
    let source = include_str!("../src/access.rs");
    assert!(
        !source.contains("ordered_dependency_explanation: Vec<DependencyStep>"),
        "AccessReport must retain only closure summaries; explanations are generated on demand"
    );
}

#[test]
fn access_explanation_is_stable_and_follows_policy_edge_order() {
    let first = validate_project(&fixture_root("access-closure-v1"))
        .unwrap()
        .index
        .unwrap();
    let second = validate_project(&fixture_root("access-closure-v1"))
        .unwrap()
        .index
        .unwrap();
    let id = "asm_01J00000000000000000000071";
    let first_steps = first.access_explanation(id).unwrap();
    let second_steps = second.access_explanation(id).unwrap();
    assert_eq!(first_steps, second_steps);
    assert_eq!(
        first_steps.len(),
        first.access_report(id).unwrap().dependency_count
    );
    assert_eq!(first_steps[0].from_id.as_ref(), id);
    assert_eq!(first_steps[0].edge, "source.external_references");
    assert_eq!(first_steps[1].edge, "assessment.target");
    assert_eq!(first_steps[2].edge, "assessment.supersedes_assessments");
}

#[test]
fn compact_compartment_boundaries_handle_cycles_self_edges_and_dangling_targets() {
    const LEFT: &str = "qst_01J00000000000000000000010";
    const RIGHT: &str = "qst_01J00000000000000000000013";
    const MISSING: &str = "qst_01J00000000000000000000999";
    let fixture = validate_project(&fixture_root("access-closure-v1"))
        .unwrap()
        .index
        .unwrap();
    for size in [63, 64, 65] {
        let project = TempProject::copy_fixture("access-closure-v1");
        // Retain the descriptor and extension schemas, but no unrelated access vocabulary.
        for object in fixture.objects() {
            if !matches!(object.object_type, rp_core::ObjectType::Project) {
                fs::remove_file(project.path().join(object.source_file.as_str())).unwrap();
            }
        }
        let all: BTreeSet<_> = (0..size).map(|n| format!("compartment-{n:02}")).collect();
        let left_compartments = BTreeSet::from(["compartment-00".to_string()]);
        let right_compartments: BTreeSet<_> = all.difference(&left_compartments).cloned().collect();
        for (id, target, level, compartments) in [
            (LEFT, RIGHT, "restricted", &left_compartments),
            (RIGHT, LEFT, "internal", &right_compartments),
        ] {
            let mut value = fixture.get(LEFT).unwrap().value.clone();
            value["id"] = id.into();
            value["logical_id"] =
                format!("boundary-{}", if id == LEFT { "left" } else { "right" }).into();
            value["source"]["external_references"] = serde_json::json!([]);
            value["source"]["revisions"] = serde_json::json!([id, target, MISSING]);
            value["access"] = serde_json::json!({"level": level, "compartments": compartments});
            fs::write(
                project.research(&format!("records/questions/boundary--{id}.yaml")),
                serde_json::to_vec_pretty(&value).unwrap(),
            )
            .unwrap();
        }
        let report = validate_project(project.path()).unwrap();
        assert!(report.stage3_ran, "size {size}: {:?}", report.findings);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| { finding.error_code == "RP_E_DANGLING_REVISION_REFERENCE" })
        );
        let index = report.index.unwrap();
        for (id, target, floor_level, floor_compartments) in [
            (LEFT, RIGHT, AccessLevel::Internal, &right_compartments),
            (RIGHT, LEFT, AccessLevel::Restricted, &left_compartments),
        ] {
            let actual = index.access_report(id).unwrap();
            assert_eq!(actual.dependency_count, 1, "size {size}: {id}");
            assert_eq!(actual.required_floor.level, floor_level);
            assert_eq!(&actual.required_floor.compartments, floor_compartments);
            assert_eq!(actual.effective.level, AccessLevel::Restricted);
            assert_eq!(actual.effective.compartments, all);
            assert!(!actual.valid);
            assert_eq!(
                index.access_explanation(id).unwrap(),
                vec![rp_core::DependencyStep {
                    from_id: id.into(),
                    edge: "source.revisions".to_string(),
                    to_id: target.into(),
                }]
            );
            for (allowed_compartments, eligible) in [
                (all.clone(), true),
                (right_compartments.clone(), false),
                (left_compartments.clone(), false),
            ] {
                let decision = index
                    .export_check(
                        id,
                        ExportRequest {
                            level_ceiling: AccessLevel::Restricted,
                            allowed_compartments,
                        },
                    )
                    .unwrap();
                assert_eq!(decision.eligible, eligible, "size {size}: {id}");
            }
        }
    }
}

fn access_label(value: &Value) -> AccessLabel {
    AccessLabel {
        level: AccessLevel::parse(value["level"].as_str().unwrap()).unwrap(),
        compartments: value["compartments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap().to_string())
            .collect(),
    }
}
