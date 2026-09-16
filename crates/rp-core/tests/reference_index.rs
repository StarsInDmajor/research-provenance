mod common;

use std::{cmp::Ordering, fs};

use common::{TempProject, fixture_root, mutate_object};
use rp_core::{ObjectType, ReferenceExpectation, validate_project};

#[test]
fn reference_equality_and_order_ignore_project_local_target_ordinals() {
    let project = TempProject::copy_fixture("overview-v1");
    let before = validate_project(project.path()).unwrap().index.unwrap();
    let added = project.research("references/added--ref_01J00000000000000000000000.yaml");
    fs::copy(
        project.research("references/measurement-protocol-v3--ref_01J00000000000000000000003.yaml"),
        &added,
    )
    .unwrap();
    mutate_object(&added, |value| {
        value["id"] = "ref_01J00000000000000000000000".into();
    });
    let report = validate_project(project.path()).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    let after = report.index.unwrap();
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(
        before.reference_fingerprint(),
        after.reference_fingerprint()
    );

    for object in before.objects() {
        let old: Vec<_> = before.references_from(&object.id).collect();
        let new: Vec<_> = after.references_from(&object.id).collect();
        assert_eq!(old.len(), new.len());
        for (left, right) in old.into_iter().zip(new) {
            assert_eq!(
                left, right,
                "edge identity must not depend on added objects"
            );
            assert_eq!(left.cmp(right), Ordering::Equal);
            assert_eq!(left.partial_cmp(right), Some(Ordering::Equal));
        }
    }
}

#[test]
fn public_reference_iteration_retains_semantic_field_order() {
    let index = validate_project(&fixture_root("access-closure-v1"))
        .unwrap()
        .index
        .unwrap();
    let mut expected_fingerprint = Vec::new();
    for object in index.objects() {
        let actual: Vec<_> = index.references_from(&object.id).collect();
        let mut expected = actual.clone();
        expected.sort_by_key(|edge| {
            (
                &edge.source_id,
                &edge.target_id,
                &edge.expected,
                &edge.json_pointer,
            )
        });
        assert_eq!(actual, expected, "{}", object.id);
        for edge in expected {
            let label = match &edge.expected {
                ReferenceExpectation::Project => "project".to_string(),
                ReferenceExpectation::Node => "node revision".to_string(),
                ReferenceExpectation::NodeKind(kind) => format!("node:{kind}"),
                ReferenceExpectation::Relation => "relation revision".to_string(),
                ReferenceExpectation::Assessment => "assessment".to_string(),
                ReferenceExpectation::Thread => "research thread".to_string(),
                ReferenceExpectation::ThreadBinding => "thread binding".to_string(),
                ReferenceExpectation::ClaimChain => "claim-chain snapshot".to_string(),
                ReferenceExpectation::Artifact => "artifact manifest".to_string(),
                ReferenceExpectation::ExternalReference => "external reference".to_string(),
                ReferenceExpectation::ResearchRun => "research run".to_string(),
            };
            expected_fingerprint.push((
                edge.source_id.to_string(),
                edge.target_id.to_string(),
                label,
                edge.json_pointer.clone(),
            ));
        }
    }
    assert_eq!(index.reference_fingerprint(), expected_fingerprint);
}

#[test]
fn unordered_and_forward_references_resolve_in_two_pass_index() {
    let report = validate_project(&fixture_root("overview-v1")).expect("validate overview");
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    let index = report.index.unwrap();

    let hypothesis = index
        .get("hyp_01J00000000000000000000011")
        .expect("hypothesis exists");
    assert_eq!(
        hypothesis.object_type,
        ObjectType::Node("Hypothesis".into())
    );
    assert!(
        index
            .references_from(hypothesis.id.as_ref())
            .any(|reference| {
                reference.target_id.as_ref() == "qst_01J00000000000000000000010"
                    && reference.json_pointer == "/source/revisions/0"
            })
    );

    assert!(
        index
            .references_from("run_01J00000000000000000000002")
            .any(|reference| {
                reference.target_id.as_ref() == "proj_01J00000000000000000000001"
                    && reference.expected == ReferenceExpectation::Project
            })
    );
}

#[test]
fn all_builtin_reference_classes_are_collected_without_claim_or_access_evaluation() {
    let report =
        validate_project(&fixture_root("access-closure-v1")).expect("validate access fixture");
    let index = report.index.unwrap();

    let relation_refs: Vec<_> = index
        .references_from("rel_01J00000000000000000000042")
        .map(|reference| (&reference.expected, reference.json_pointer.as_str()))
        .collect();
    assert!(relation_refs.iter().any(|(expected, pointer)| **expected
        == ReferenceExpectation::Node
        && *pointer == "/from_revision"));
    assert!(relation_refs.iter().any(|(expected, pointer)| **expected
        == ReferenceExpectation::Node
        && *pointer == "/to_revision"));

    let chain_refs: Vec<_> = index
        .references_from("cch_01J00000000000000000000080")
        .collect();
    assert!(chain_refs.iter().any(|reference| {
        reference.expected == ReferenceExpectation::Relation
            && reference.json_pointer.starts_with("/relation_revisions/")
    }));
    assert!(chain_refs.iter().any(|reference| {
        reference.expected == ReferenceExpectation::Node
            && reference.json_pointer.starts_with("/node_revisions/")
    }));
}

#[test]
fn canonical_layout_rejects_an_object_of_the_wrong_type() {
    let project = TempProject::copy_fixture("freshness-v1");
    fs::create_dir(project.research("relations")).expect("create relation directory");
    fs::rename(
        project.research("references/protocol-current--ref_01J00000000000000000000103.yaml"),
        project.research("relations/protocol-current--ref_01J00000000000000000000103.yaml"),
    )
    .expect("move object to wrong canonical directory");

    let report = validate_project(project.path()).expect("validate wrong layout");
    assert!(report.stage3_ran);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.error_code == "RP_E_REFERENCE_TYPE_MISMATCH")
    );
}

#[test]
fn index_identity_and_adjacency_are_independent_of_file_scan_permutation() {
    let project = TempProject::copy_fixture("overview-v1");
    let before = validate_project(project.path())
        .expect("first validation")
        .index
        .expect("first index");

    fs::rename(
        project.research(
            "relations/question-motivates-hypothesis-v1--rel_01J00000000000000000000041.yaml",
        ),
        project.research("relations/zzz-permuted--rel_01J00000000000000000000041.yaml"),
    )
    .expect("permute scan order");
    let after = validate_project(project.path())
        .expect("second validation")
        .index
        .expect("second index");

    assert_eq!(
        before.ids().collect::<Vec<_>>(),
        after.ids().collect::<Vec<_>>()
    );
    assert_eq!(
        before.reference_fingerprint(),
        after.reference_fingerprint()
    );
}
