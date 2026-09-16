mod common;

use common::{
    TempProject, fixture_root, materialize_overlay, mutate_object, parse_yaml_file, suite_file,
};
use rp_core::validate_project;
use serde_json::Value;

#[test]
fn all_four_positive_snapshots_match_exact_source_closure_oracles() {
    check_single_oracle(
        "claim-chain-minimum-v1",
        "expected-claim-chain.yaml",
        "/snapshot_id",
        "/profile",
        "/source_closure",
    );
    check_single_oracle(
        "claim-chain-confirmatory-v1",
        "expected-claim-chain.yaml",
        "/snapshot/id",
        "/profile",
        "/snapshot/source_closure",
    );

    let fixture = "claim-chain-evidential-v1";
    let report = validate_project(&fixture_root(fixture)).expect("validate evidential fixture");
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    let index = report.index.unwrap();
    let oracle = parse_yaml_file(&suite_file(fixture, "expected-claim-chains.yaml"));
    for expected in oracle["snapshots"].as_array().unwrap() {
        let id = expected["id"].as_str().unwrap();
        let actual = index.claim_chain_report(id).expect("claim report");
        assert!(actual.valid, "{id}");
        assert_eq!(
            actual.profile.as_ref(),
            oracle["profile"].as_str().unwrap(),
            "{id}"
        );
        check_source_closure(actual, &expected["source_closure"]);
    }
}

#[test]
fn all_fourteen_claim_profile_overlays_emit_their_frozen_code() {
    let overlays = [
        ("claim-chain-minimum-v1", "overlay-claim-chain-cycle"),
        ("claim-chain-minimum-v1", "overlay-claim-chain-logical-id"),
        ("claim-chain-minimum-v1", "overlay-disconnected-node"),
        ("claim-chain-minimum-v1", "overlay-invalidated-relation"),
        ("claim-chain-minimum-v1", "overlay-missing-endpoint"),
        (
            "claim-chain-evidential-v1",
            "overlay-invalid-input-to-endpoint",
        ),
        (
            "claim-chain-evidential-v1",
            "overlay-missing-evaluative-path",
        ),
        (
            "claim-chain-evidential-v1",
            "overlay-multi-input-missing-synthesis",
        ),
        (
            "claim-chain-evidential-v1",
            "overlay-unsupported-evaluative-relation",
        ),
        (
            "claim-chain-confirmatory-v1",
            "overlay-invalid-confirmatory-root",
        ),
        (
            "claim-chain-confirmatory-v1",
            "overlay-missing-confirmatory-backbone",
        ),
        ("claim-chain-confirmatory-v1", "overlay-provenance-gap"),
        ("claim-chain-confirmatory-v1", "overlay-result-without-test"),
        (
            "claim-chain-confirmatory-v1",
            "overlay-temporal-predeclaration-inverted",
        ),
    ];

    for (fixture, overlay) in overlays {
        let (project, expected_code) = materialize_overlay(fixture, overlay);
        let report = validate_project(project.path()).expect("validate claim overlay");
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.error_code == expected_code),
            "{fixture}/{overlay}: expected {expected_code}, got {:?}",
            report.findings
        );
    }
}

#[test]
fn source_closure_excludes_snapshot_and_validation_report() {
    let report = validate_project(&fixture_root("claim-chain-minimum-v1")).unwrap();
    let index = report.index.unwrap();
    let id = "cch_01J00000000000000000000051";
    let chain = index.claim_chain_report(id).unwrap();
    assert!(
        !chain
            .source_entries
            .iter()
            .any(|entry| entry.id.as_ref() == id)
    );
    assert!(
        chain
            .source_entries
            .windows(2)
            .all(|pair| (&pair[0].object_type, &pair[0].id) < (&pair[1].object_type, &pair[1].id))
    );
}

const CONFIRMATORY_CHAIN: &str = "cch_01J00000000000000000000053";
const CONFIRMATORY_CHAIN_FILE: &str =
    "claim-chains/confirmatory-mixed-outcome-chain--cch_01J00000000000000000000053.yaml";
const STRESS_TESTED_BY: &str = "rel_01J00000000000000000000064";
const STRESS_TESTED_BY_FILE: &str =
    "relations/prediction-tested-by-stress--rel_01J00000000000000000000064.yaml";
const RESULT_MAPPING_CODE: &str = "RP_E_CONFIRMATORY_RESULT_TEST_REQUIRED";

#[test]
fn confirmatory_each_result_test_requires_a_selected_prediction_link() {
    let project = TempProject::copy_fixture("claim-chain-confirmatory-v1");
    remove_stress_prediction_link(&project);
    assert_mapping_findings(&project, 1, &[]);
}

#[test]
fn confirmatory_hypothesis_test_link_does_not_replace_prediction() {
    let project = TempProject::copy_fixture("claim-chain-confirmatory-v1");
    mutate_object(&project.research(STRESS_TESTED_BY_FILE), |relation| {
        relation["from_revision"] = "hyp_01J00000000000000000000011".into();
    });
    assert_mapping_findings(&project, 1, &[]);
}

#[test]
fn confirmatory_invalidated_prediction_link_does_not_map_result() {
    let project = TempProject::copy_fixture("claim-chain-confirmatory-v1");
    mutate_object(&project.research(STRESS_TESTED_BY_FILE), |relation| {
        relation["relation_state"] = "invalidated".into();
    });
    assert_mapping_findings(&project, 1, &["RP_E_CLAIM_CHAIN_RELATION_NOT_ACTIVE"]);
}

#[test]
fn confirmatory_unselected_prediction_cannot_supply_result_mapping() {
    let project = TempProject::copy_fixture("claim-chain-confirmatory-v1");
    mutate_object(&project.research(CONFIRMATORY_CHAIN_FILE), |chain| {
        chain["node_revisions"]
            .as_array_mut()
            .unwrap()
            .retain(|id| id.as_str() != Some("pred_01J00000000000000000000020"));
    });
    assert_mapping_findings(&project, 4, &["RP_E_CLAIM_CHAIN_ENDPOINT_NOT_SELECTED"; 6]);
}

#[test]
fn confirmatory_unselected_test_cannot_supply_result_mapping() {
    let project = TempProject::copy_fixture("claim-chain-confirmatory-v1");
    mutate_object(&project.research(CONFIRMATORY_CHAIN_FILE), |chain| {
        chain["node_revisions"]
            .as_array_mut()
            .unwrap()
            .retain(|id| id.as_str() != Some("tst_01J00000000000000000000023"));
    });
    assert_mapping_findings(&project, 1, &["RP_E_CLAIM_CHAIN_ENDPOINT_NOT_SELECTED"; 3]);
}

#[test]
fn confirmatory_invalidated_result_of_does_not_supply_result_mapping() {
    let project = TempProject::copy_fixture("claim-chain-confirmatory-v1");
    mutate_object(
        &project.research("relations/stress-result-of-test--rel_01J00000000000000000000051.yaml"),
        |relation| relation["relation_state"] = "invalidated".into(),
    );
    assert_mapping_findings(&project, 1, &["RP_E_CLAIM_CHAIN_RELATION_NOT_ACTIVE"]);
}

#[test]
fn confirmatory_every_mapped_test_needs_prediction_even_with_another_valid_test() {
    let project = TempProject::copy_fixture("claim-chain-confirmatory-v1");
    remove_stress_prediction_link(&project);
    let extra_id = "rel_01J00000000000000000000065";
    let extra_path = project
        .research("relations/candidate-result-of-stress--rel_01J00000000000000000000065.yaml");
    std::fs::copy(
        project.research(
            "relations/candidate-result-of-standard--rel_01J00000000000000000000050.yaml",
        ),
        &extra_path,
    )
    .unwrap();
    mutate_object(&extra_path, |relation| {
        relation["id"] = extra_id.into();
        relation["logical_id"] = "candidate-result-of-stress".into();
        relation["to_revision"] = "tst_01J00000000000000000000023".into();
    });
    mutate_object(&project.research(CONFIRMATORY_CHAIN_FILE), |chain| {
        chain["relation_revisions"]
            .as_array_mut()
            .unwrap()
            .push(extra_id.into());
    });
    assert_mapping_findings(&project, 2, &[]);
}

#[test]
fn confirmatory_additional_hypothesis_link_is_not_a_prediction_predeclaration() {
    let project = TempProject::copy_fixture("claim-chain-confirmatory-v1");
    let extra_id = "rel_01J00000000000000000000065";
    let extra_path = project
        .research("relations/hypothesis-tested-by-stress--rel_01J00000000000000000000065.yaml");
    std::fs::copy(project.research(STRESS_TESTED_BY_FILE), &extra_path).unwrap();
    mutate_object(&extra_path, |relation| {
        relation["id"] = extra_id.into();
        relation["logical_id"] = "hypothesis-tested-by-stress".into();
        relation["from_revision"] = "hyp_01J00000000000000000000011".into();
    });
    // Generic relation compatibility permits Hypothesis -> Test. Its timestamp
    // must not be mistaken for an applicable Prediction's predeclaration time.
    mutate_object(
        &project.research(
            "records/hypotheses/hypothesis-confirmatory-scope--hyp_01J00000000000000000000011.yaml",
        ),
        |hypothesis| hypothesis["created_at"] = "2026-03-01T00:00:00Z".into(),
    );
    mutate_object(&project.research(CONFIRMATORY_CHAIN_FILE), |chain| {
        chain["relation_revisions"]
            .as_array_mut()
            .unwrap()
            .push(extra_id.into());
    });
    assert_mapping_findings(&project, 0, &[]);
}

fn remove_stress_prediction_link(project: &TempProject) {
    mutate_object(&project.research(CONFIRMATORY_CHAIN_FILE), |chain| {
        chain["relation_revisions"]
            .as_array_mut()
            .unwrap()
            .retain(|id| id.as_str() != Some(STRESS_TESTED_BY));
    });
}

fn assert_mapping_findings(project: &TempProject, count: usize, other_codes: &[&str]) {
    // Keep closure maintenance separate from the semantic regression: otherwise
    // a stale digest alone could make an incorrectly accepted chain look invalid.
    let report = validate_project(project.path()).unwrap();
    let digest = report
        .index
        .unwrap()
        .claim_chain_report(CONFIRMATORY_CHAIN)
        .unwrap()
        .source_sha256
        .to_string();
    mutate_object(&project.research(CONFIRMATORY_CHAIN_FILE), |chain| {
        chain["source_closure_sha256"] = digest.into();
    });
    let report = validate_project(project.path()).unwrap();
    let mappings: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| finding.error_code == RESULT_MAPPING_CODE)
        .collect();
    assert_eq!(mappings.len(), count, "{:?}", report.findings);
    for finding in mappings {
        assert_eq!(finding.finding_family, "confirmatory_result_mapping");
        assert_eq!(finding.json_pointer, "/relation_revisions");
    }
    assert_eq!(
        report.findings.len(),
        count + other_codes.len(),
        "{:?}",
        report.findings
    );
    for code in other_codes {
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.error_code == *code)
        );
    }
    assert_eq!(
        report
            .index
            .unwrap()
            .claim_chain_report(CONFIRMATORY_CHAIN)
            .unwrap()
            .valid,
        count == 0 && other_codes.is_empty(),
    );
}

fn check_single_oracle(
    fixture: &str,
    oracle_name: &str,
    id_pointer: &str,
    profile_pointer: &str,
    source_pointer: &str,
) {
    let report = validate_project(&fixture_root(fixture)).expect("validate claim fixture");
    assert!(
        report.findings.is_empty(),
        "{fixture}: {:?}",
        report.findings
    );
    let index = report.index.unwrap();
    let oracle = parse_yaml_file(&suite_file(fixture, oracle_name));
    let id = oracle.pointer(id_pointer).and_then(Value::as_str).unwrap();
    let actual = index.claim_chain_report(id).expect("claim report");
    assert!(actual.valid);
    assert_eq!(
        actual.profile.as_ref(),
        oracle
            .pointer(profile_pointer)
            .and_then(Value::as_str)
            .unwrap()
    );
    check_source_closure(actual, oracle.pointer(source_pointer).unwrap());
}

fn check_source_closure(actual: &rp_core::ClaimChainReport, expected: &Value) {
    assert_eq!(
        actual.source_sha256.as_ref(),
        expected["sha256"].as_str().unwrap()
    );
    let expected_entries = expected["entries"].as_array().unwrap();
    assert_eq!(actual.source_entries.len(), expected_entries.len());
    for (actual, expected) in actual.source_entries.iter().zip(expected_entries) {
        assert_eq!(
            actual.object_type.as_ref(),
            expected["object_type"].as_str().unwrap()
        );
        assert_eq!(actual.id.as_ref(), expected["id"].as_str().unwrap());
        assert_eq!(
            actual.canonical_digest.as_ref(),
            expected["canonical_digest"].as_str().unwrap()
        );
    }
}
