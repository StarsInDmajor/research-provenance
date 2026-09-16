mod common;

use common::{TempProject, fixture_root, replace};
use rp_core::validate_project;

fn has_code(project: &TempProject, code: &str) -> bool {
    validate_project(project.path())
        .expect("validate project")
        .findings
        .iter()
        .any(|finding| finding.error_code == code)
}

#[test]
fn overview_derives_multi_parent_heads_and_invalidated_relation_head() {
    let report = validate_project(&fixture_root("overview-v1")).expect("validate overview");
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    let index = report.index.unwrap();

    assert_eq!(
        index.heads_for("hypothesis-drift"),
        ["hyp_01J00000000000000000000014"]
    );
    assert_eq!(
        index.heads_for("interpretation-stress-cause"),
        [
            "int_01J00000000000000000000015",
            "int_01J00000000000000000000016",
        ]
    );
    assert_eq!(
        index.heads_for("relation-candidate-support"),
        ["rel_01J00000000000000000000044"]
    );
}

#[test]
fn node_relation_binding_and_thread_cycles_have_distinct_stable_codes() {
    let node = TempProject::copy_fixture("overview-v1");
    replace(
        &node.research(
            "records/hypotheses/hypothesis-drift-v2-scope--hyp_01J00000000000000000000012.yaml",
        ),
        "- id: hyp_01J00000000000000000000011",
        "- id: hyp_01J00000000000000000000014",
    );
    assert!(has_code(&node, "RP_E_REVISION_DAG_CYCLE"));

    let relation = TempProject::copy_fixture("overview-v1");
    replace(
        &relation.research("relations/candidate-support-v1--rel_01J00000000000000000000043.yaml"),
        "  parents: []",
        "  parents:\n  - id: rel_01J00000000000000000000044\n    change_type: error_correction",
    );
    assert!(has_code(&relation, "RP_E_RELATION_DAG_CYCLE"));

    let binding = TempProject::copy_fixture("overview-v1");
    replace(
        &binding.research("thread-bindings/primary-question--tbd_01J00000000000000000000061.yaml"),
        "  parents: []",
        "  parents:\n  - id: tbd_01J00000000000000000000062\n    change_type: clarification",
    );
    replace(
        &binding
            .research("thread-bindings/primary-hypothesis-v1--tbd_01J00000000000000000000062.yaml"),
        "  parents: []",
        "  parents:\n  - id: tbd_01J00000000000000000000061\n    change_type: clarification",
    );
    assert!(has_code(&binding, "RP_E_BINDING_DAG_CYCLE"));

    let thread = TempProject::copy_fixture("overview-v1");
    replace(
        &thread.research("threads/primary-efficacy--thd_01J00000000000000000000031.yaml"),
        "parent_thread_id: null",
        "parent_thread_id: thd_01J00000000000000000000033",
    );
    assert!(has_code(&thread, "RP_E_THREAD_DAG_CYCLE"));
}

#[test]
fn parent_lineage_requires_matching_identity_kind_type_and_binding_context() {
    let node = TempProject::copy_fixture("overview-v1");
    replace(
        &node.research(
            "records/hypotheses/hypothesis-drift-v2-scope--hyp_01J00000000000000000000012.yaml",
        ),
        "- id: hyp_01J00000000000000000000011",
        "- id: meas_01J00000000000000000000025",
    );
    assert!(has_code(&node, "RP_E_NODE_LINEAGE_LOGICAL_ID_MISMATCH"));
    assert!(has_code(&node, "RP_E_NODE_LINEAGE_KIND_MISMATCH"));

    let relation = TempProject::copy_fixture("overview-v1");
    replace(
        &relation.research(
            "relations/candidate-support-v2-invalidated--rel_01J00000000000000000000044.yaml",
        ),
        "- id: rel_01J00000000000000000000043",
        "- id: rel_01J00000000000000000000045",
    );
    assert!(has_code(
        &relation,
        "RP_E_RELATION_LINEAGE_LOGICAL_ID_MISMATCH"
    ));
    assert!(has_code(&relation, "RP_E_RELATION_LINEAGE_TYPE_MISMATCH"));

    let binding = TempProject::copy_fixture("overview-v1");
    replace(
        &binding
            .research("thread-bindings/primary-hypothesis-v3--tbd_01J00000000000000000000063.yaml"),
        "  parents: []",
        "  parents:\n  - id: tbd_01J00000000000000000000071\n    change_type: clarification",
    );
    assert!(has_code(
        &binding,
        "RP_E_BINDING_LINEAGE_LOGICAL_ID_MISMATCH"
    ));
    assert!(has_code(&binding, "RP_E_BINDING_LINEAGE_CONTEXT_MISMATCH"));
}

#[test]
fn relations_require_existing_exact_endpoints_and_kind_compatibility() {
    let dangling = TempProject::copy_fixture("overview-v1");
    replace(
        &dangling.research("relations/candidate-support-v1--rel_01J00000000000000000000043.yaml"),
        "to_revision: hyp_01J00000000000000000000011",
        "to_revision: hyp_01J00000000000000000000099",
    );
    assert!(has_code(&dangling, "RP_E_DANGLING_REVISION_REFERENCE"));

    let incompatible = TempProject::copy_fixture("claim-chain-evidential-v1");
    replace(
        &incompatible.research(
            "relations/candidate-input-overlay-control--rel_01J00000000000000000000052.yaml",
        ),
        "to_revision: syn_01J00000000000000000000028",
        "to_revision: hyp_01J00000000000000000000014",
    );
    assert!(has_code(&incompatible, "RP_E_RELATION_KIND_INCOMPATIBLE"));
}

#[test]
fn assessment_supersession_is_cycle_free_and_confined_to_one_lane() {
    let mismatch = TempProject::copy_fixture("overview-v1");
    replace(
        &mismatch.research(
            "assessments/supported-by-standard-measurement--asm_01J00000000000000000000022.yaml",
        ),
        "supersedes_assessments: []",
        "supersedes_assessments:\n- asm_01J00000000000000000000021",
    );
    replace(
        &mismatch
            .research("assessments/proposed-broad-hypothesis--asm_01J00000000000000000000021.yaml"),
        "assessment_scope: overall-claim",
        "assessment_scope: initial-proposal",
    );
    assert!(has_code(
        &mismatch,
        "RP_E_ASSESSMENT_SUPERSESSION_LANE_MISMATCH"
    ));

    let cycle = TempProject::copy_fixture("overview-v1");
    replace(
        &cycle.research(
            "assessments/supported-by-standard-measurement--asm_01J00000000000000000000022.yaml",
        ),
        "supersedes_assessments: []",
        "supersedes_assessments:\n- asm_01J00000000000000000000021",
    );
    replace(
        &cycle
            .research("assessments/proposed-broad-hypothesis--asm_01J00000000000000000000021.yaml"),
        "supersedes_assessments: []",
        "supersedes_assessments:\n- asm_01J00000000000000000000022",
    );
    assert!(has_code(&cycle, "RP_E_ASSESSMENT_SUPERSESSION_CYCLE"));
}

#[test]
fn orphan_thread_bindings_use_the_contextual_integrity_code() {
    let project = TempProject::copy_fixture("overview-v1");
    replace(
        &project
            .research("thread-bindings/primary-hypothesis-v3--tbd_01J00000000000000000000063.yaml"),
        "thread_id: thd_01J00000000000000000000031",
        "thread_id: thd_01J00000000000000000000099",
    );
    assert!(has_code(&project, "RP_E_ORPHAN_THREAD_BINDING"));
}
