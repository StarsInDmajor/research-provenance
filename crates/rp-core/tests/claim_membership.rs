mod common;

use common::{TempProject, mutate_object};
use rp_core::{
    ObjectType, ProjectPath, SchemaBundle, Severity, ValidationReport, validate_project,
};
use serde_json::{Value, json};
use std::path::PathBuf;

const PROFILES: [&str; 3] = ["minimum-v1", "evidential-v1", "confirmatory-v1"];
const ROOT: &str = "qst_01J00000000000000000000090";
const TARGET: &str = "con_01J00000000000000000000091";
const MISSING: &str = "qst_01J00000000000000000000099";
const MEMBERSHIP: &str = "RP_E_CLAIM_CHAIN_ENDPOINT_NOT_SELECTED";

type Expected<'a> = (&'a str, &'a str, &'a str);

struct Case {
    project: TempProject,
    chain_id: String,
    chain_path: PathBuf,
    source: String,
    root_path: PathBuf,
}

impl Case {
    fn new(profile: &str) -> Self {
        let project = TempProject::copy_fixture(&format!("claim-chain-{profile}"));
        let report = validate_project(project.path()).unwrap();
        assert!(report.findings.is_empty(), "{:?}", report.findings);
        let index = report.index.unwrap();
        let chain = index
            .objects()
            .find(|object| {
                object.object_type == ObjectType::ClaimChain
                    && (profile != "evidential-v1" || object.id.ends_with("52"))
            })
            .unwrap();
        let source = chain.source_file.as_str().to_string();
        let chain_path = project.path().join(&source);
        let mut root_path = PathBuf::new();
        for (field, id) in [
            ("root_node_revisions", ROOT),
            ("target_node_revision", TARGET),
        ] {
            let old_id = if field == "root_node_revisions" {
                chain.value[field][0].as_str().unwrap()
            } else {
                chain.value[field].as_str().unwrap()
            };
            let old = index.get(old_id).unwrap();
            let path = project
                .path()
                .join(old.source_file.as_str())
                .parent()
                .unwrap()
                .join(format!("membership-control--{id}.yaml"));
            std::fs::copy(project.path().join(old.source_file.as_str()), &path).unwrap();
            mutate_object(&path, |value| {
                value["id"] = id.into();
                value["logical_id"] =
                    format!("membership-control-{}", id.to_lowercase().replace('_', "-")).into();
            });
            if id == ROOT {
                root_path = path;
            }
        }
        Self {
            project,
            chain_id: chain.id.to_string(),
            chain_path,
            source,
            root_path,
        }
    }

    fn endpoints(&self, root: Option<&str>, target: Option<&str>) {
        mutate_object(&self.chain_path, |chain| {
            if let Some(root) = root {
                chain["root_node_revisions"] = json!([root]);
            }
            if let Some(target) = target {
                chain["target_node_revision"] = target.into();
            }
        });
    }

    fn refresh_digest(&self) {
        let report = validate_project(self.project.path()).unwrap();
        assert!(report.stage3_ran, "{:?}", report.findings);
        let digest = report
            .index
            .unwrap()
            .claim_chain_report(&self.chain_id)
            .unwrap()
            .source_sha256
            .to_string();
        mutate_object(&self.chain_path, |chain| {
            chain["source_closure_sha256"] = digest.into()
        });
    }

    fn assert_findings(&self, expected: &[Expected<'_>]) -> ValidationReport {
        let report = validate_project(self.project.path()).unwrap();
        assert!(report.stage3_ran, "{:?}", report.findings);
        // Exact complete diagnostic identities, including multiplicity, family,
        // snapshot source path and pointer; prose is not a frozen oracle.
        let actual: Vec<_> = report
            .findings
            .iter()
            .map(|finding| {
                assert_eq!(finding.severity, Severity::Error);
                assert_eq!(finding.schema, "rp/finding/v1");
                assert_eq!(
                    finding.source_file.as_ref().unwrap().as_str(),
                    self.source,
                    "{:?}",
                    report.findings
                );
                (
                    finding.error_code,
                    finding.finding_family,
                    finding.json_pointer.as_str(),
                )
            })
            .collect();
        let mut expected = expected.to_vec();
        expected.sort_by_key(|(code, _, pointer)| (*pointer, *code));
        assert_eq!(actual, expected, "{:?}", report.findings);
        if expected.is_empty() || expected.iter().any(|(code, _, _)| *code == MEMBERSHIP) {
            assert_eq!(
                report
                    .index
                    .as_ref()
                    .unwrap()
                    .claim_chain_report(&self.chain_id)
                    .unwrap()
                    .valid,
                expected.is_empty()
            );
        }
        report
    }
}

#[test]
fn resolved_unselected_root_target_and_both_are_exact_errors_in_every_profile() {
    for profile in PROFILES {
        for (root, target) in [(true, false), (false, true), (true, true)] {
            let case = Case::new(profile);
            case.endpoints(root.then_some(ROOT), target.then_some(TARGET));
            case.refresh_digest();
            let mut expected = Vec::new();
            if root {
                expected.push((MEMBERSHIP, "endpoint_containment", "/root_node_revisions/0"));
            }
            if target {
                expected.push((MEMBERSHIP, "endpoint_containment", "/target_node_revision"));
            }
            case.assert_findings(&expected);
        }
    }
}

#[test]
fn every_unselected_root_uses_its_exact_array_index() {
    for profile in PROFILES {
        let case = Case::new(profile);
        mutate_object(&case.chain_path, |chain| {
            chain["root_node_revisions"]
                .as_array_mut()
                .unwrap()
                .extend([json!(ROOT), json!(TARGET)]);
            chain["target_node_revision"] = TARGET.into();
        });
        case.refresh_digest();
        case.assert_findings(&[
            (MEMBERSHIP, "endpoint_containment", "/root_node_revisions/1"),
            (MEMBERSHIP, "endpoint_containment", "/root_node_revisions/2"),
            (MEMBERSHIP, "endpoint_containment", "/target_node_revision"),
        ]);
    }
}

#[test]
fn unresolved_endpoints_keep_only_reference_errors_in_every_profile() {
    for profile in PROFILES {
        for (root, target) in [(true, false), (false, true), (true, true)] {
            let case = Case::new(profile);
            case.endpoints(root.then_some(MISSING), target.then_some(MISSING));
            case.refresh_digest();
            let mut expected = Vec::new();
            if root {
                expected.push((
                    "RP_E_DANGLING_REVISION_REFERENCE",
                    "reference_integrity",
                    "/root_node_revisions/0",
                ));
            }
            if target {
                expected.push((
                    "RP_E_DANGLING_REVISION_REFERENCE",
                    "reference_integrity",
                    "/target_node_revision",
                ));
            }
            case.assert_findings(&expected);
        }
    }
}

#[test]
fn unresolved_root_does_not_hide_a_resolved_unselected_target() {
    for profile in PROFILES {
        let case = Case::new(profile);
        case.endpoints(Some(MISSING), Some(TARGET));
        case.refresh_digest();
        case.assert_findings(&[
            (
                "RP_E_DANGLING_REVISION_REFERENCE",
                "reference_integrity",
                "/root_node_revisions/0",
            ),
            (MEMBERSHIP, "endpoint_containment", "/target_node_revision"),
        ]);
    }
}

#[test]
fn wrong_type_endpoint_ids_remain_stage_two_errors_without_membership_cascades() {
    // Non-Node IDs cannot pass the frozen node-ID pattern. Low-level resolved
    // wrong-object-type controls live beside claim.rs to exercise Stage 3 too.
    for profile in PROFILES {
        for (root, target) in [(true, false), (false, true), (true, true)] {
            let case = Case::new(profile);
            let wrong = "ref_01J00000000000000000000003";
            case.endpoints(root.then_some(wrong), target.then_some(wrong));
            let report = validate_project(case.project.path()).unwrap();
            assert!(!report.stage3_ran);
            assert!(report.index.is_none());
            let pointers: Vec<_> = report
                .findings
                .iter()
                .map(|finding| {
                    assert_eq!(finding.error_code, "RP_E_SCHEMA_PATTERN");
                    assert_eq!(finding.finding_family, "schema_pattern");
                    assert_eq!(finding.severity, Severity::Error);
                    assert_eq!(finding.source_file.as_ref().unwrap().as_str(), case.source);
                    finding.json_pointer.as_str()
                })
                .collect();
            let mut expected = Vec::new();
            if root {
                expected.push("/root_node_revisions/0");
            }
            if target {
                expected.push("/target_node_revision");
            }
            assert_eq!(pointers, expected);
        }
    }
}

#[test]
fn valid_multi_root_minimum_chain_stays_valid() {
    let case = Case::new("minimum-v1");
    mutate_object(&case.chain_path, |chain| {
        chain["root_node_revisions"]
            .as_array_mut()
            .unwrap()
            .push(json!("hyp_01J00000000000000000000011"));
    });
    case.refresh_digest();
    case.assert_findings(&[]);
}

#[test]
fn membership_failure_preserves_independent_source_digest_and_access_errors() {
    for profile in PROFILES {
        let case = Case::new(profile);
        case.endpoints(Some(ROOT), Some(TARGET));
        mutate_object(&case.root_path, |root| {
            root["access"]["level"] = "exclusive".into()
        });
        case.refresh_digest();
        mutate_object(&case.chain_path, |chain| {
            chain["source_closure_sha256"] = format!("sha256:{}", "0".repeat(64)).into();
        });
        case.assert_findings(&[
            (
                "RP_E_ACCESS_CLAIM_SELECTION",
                "access_claim_chain_selection",
                "/access",
            ),
            (MEMBERSHIP, "endpoint_containment", "/root_node_revisions/0"),
            (
                "RP_E_SOURCE_CLOSURE_DIGEST_MISMATCH",
                "source_closure_integrity",
                "/source_closure_sha256",
            ),
            (MEMBERSHIP, "endpoint_containment", "/target_node_revision"),
        ]);
    }
}

#[test]
fn membership_failure_preserves_independent_minimum_relation_checks() {
    let case = Case::new("minimum-v1");
    case.endpoints(Some(ROOT), Some(TARGET));
    mutate_object(
        &case.project.research(
            "relations/question-motivates-hypothesis--rel_01J00000000000000000000041.yaml",
        ),
        |relation| {
            relation["relation_state"] = "invalidated".into();
            relation["from_revision"] = ROOT.into();
        },
    );
    case.refresh_digest();
    case.assert_findings(&[
        (MEMBERSHIP, "endpoint_containment", "/node_revisions"),
        (
            "RP_E_CLAIM_CHAIN_RELATION_NOT_ACTIVE",
            "relation_state",
            "/relation_revisions",
        ),
        (MEMBERSHIP, "endpoint_containment", "/root_node_revisions/0"),
        (MEMBERSHIP, "endpoint_containment", "/target_node_revision"),
    ]);
}

#[test]
fn membership_failure_preserves_independent_connectivity_and_cycle_checks() {
    for cycle in [false, true] {
        let case = Case::new("minimum-v1");
        case.endpoints(Some(ROOT), Some(TARGET));
        mutate_object(&case.chain_path, |chain| {
            if cycle {
                chain["relation_revisions"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("rel_01J00000000000000000000044"));
            } else {
                chain["node_revisions"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("qst_01J00000000000000000000020"));
            }
        });
        case.refresh_digest();
        let independent = if cycle {
            (
                "RP_E_CLAIM_CHAIN_DIRECTED_CYCLE",
                "claim_chain_cycle",
                "/relation_revisions",
            )
        } else {
            (
                "RP_E_CLAIM_CHAIN_MULTIPLE_COMPONENTS",
                "graph_connectivity",
                "/node_revisions",
            )
        };
        case.assert_findings(&[
            independent,
            (MEMBERSHIP, "endpoint_containment", "/root_node_revisions/0"),
            (MEMBERSHIP, "endpoint_containment", "/target_node_revision"),
        ]);
    }
}

#[test]
fn membership_findings_fit_the_frozen_cli_envelope() {
    let case = Case::new("minimum-v1");
    case.endpoints(Some(ROOT), Some(TARGET));
    case.refresh_digest();
    let report = case.assert_findings(&[
        (MEMBERSHIP, "endpoint_containment", "/root_node_revisions/0"),
        (MEMBERSHIP, "endpoint_containment", "/target_node_revision"),
    ]);
    let result = rp_core::CommandResult::new(
        "validate",
        rp_core::Status::Invalid,
        None,
        report.findings,
        Some(json!({
            "schema": "rp/cli-data/validate/v1", "valid": false,
            "canonical_object_count": report.canonical_object_count, "stage3_ran": true
        })),
    );
    let value: Value = serde_json::to_value(result).unwrap();
    assert_eq!(value["exit_code"], 1);
    SchemaBundle::new()
        .unwrap()
        .validate(&value, ProjectPath::new("cli-result.json").unwrap())
        .unwrap();
}
