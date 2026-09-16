mod common;

use std::fs;

use common::{TempProject, mutate_object, parse_yaml_file};
use rp_core::{FreshnessStatus, ProjectIndex, QueryOptions, validate_project};
use serde_json::{Value, json};

const AS_OF: &str = "2027-01-01T00:00:00Z";
const QUESTION: &str = "qst_01J00000000000000000000114";
const QUESTION_PATH: &str =
    "records/questions/stale-source-version--qst_01J00000000000000000000114.yaml";
const PINNED: &str = "ref_01J00000000000000000000103";
const PINNED_PATH: &str = "references/protocol-current--ref_01J00000000000000000000103.yaml";
const CANDIDATE: &str = "ref_01J00000000000000000000104";
const CANDIDATE_PATH: &str = "references/protocol-newer--ref_01J00000000000000000000104.yaml";
const ARTIFACT: &str = "art_01J00000000000000000000105";
const ARTIFACT_PATH: &str = "artifacts/content--art_01J00000000000000000000105.yaml";

fn index(project: &TempProject) -> ProjectIndex {
    let report = validate_project(project.path()).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    report.index.unwrap()
}

fn assert_status(index: &ProjectIndex, expected: FreshnessStatus) {
    assert_eq!(index.freshness(QUESTION, AS_OF).unwrap().status, expected);
    assert_eq!(
        index.show(QUESTION, AS_OF).unwrap().derived["freshness"],
        expected.as_str()
    );
    for status in [
        FreshnessStatus::Fresh,
        FreshnessStatus::ReviewDue,
        FreshnessStatus::Stale,
        FreshnessStatus::Unknown,
    ] {
        let query = index
            .query(QueryOptions {
                kind: Some("Question".into()),
                thread_id: None,
                freshness: Some(status),
                as_of: AS_OF.into(),
                limit: 100,
            })
            .unwrap();
        assert_eq!(
            query.rows.iter().any(|row| row["id"] == QUESTION),
            status == expected,
            "query filter {status:?}"
        );
    }
}

fn artifact_project(uri: &str) -> TempProject {
    let project = TempProject::copy_fixture("freshness-v1");
    fs::create_dir_all(project.research("artifacts")).unwrap();
    fs::write(
        project.research(ARTIFACT_PATH),
        serde_json::to_vec(&json!({
            "schema": "rp/artifact-manifest/v1", "id": ARTIFACT,
            "title": "Empty content digest control", "uri": uri,
            "media_type": "application/octet-stream", "size_bytes": 0,
            "sha256": "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "created_at": "2026-01-01T00:00:00Z",
            "access": {"level": "public", "compartments": []}
        }))
        .unwrap(),
    )
    .unwrap();
    mutate_object(&project.research(QUESTION_PATH), |object| {
        object["tags"] = json!(["freshness-artifact-watch"]);
        object["source"]["artifacts"] = json!([ARTIFACT]);
    });
    project
}

#[test]
fn https_identifier_only_artifact_is_unavailable_not_a_digest_mismatch() {
    let project = artifact_project("https://example.invalid/content");
    let index = index(&project);
    assert!(!index.artifact_verified(ARTIFACT));
    assert_status(&index, FreshnessStatus::Unknown);
}

#[test]
fn verified_local_artifact_satisfies_the_required_digest_check() {
    let project = artifact_project("file:content.bin");
    fs::write(project.path().join("content.bin"), []).unwrap();
    let index = index(&project);
    assert!(index.artifact_verified(ARTIFACT));
    assert_status(&index, FreshnessStatus::Fresh);
}

#[test]
fn missing_required_check_outranks_review_due_but_not_stale() {
    for unavailable in [
        "external-source-version",
        "artifact-digest",
        "review-deadline",
    ] {
        for stale in [false, true] {
            let project = TempProject::copy_fixture("freshness-v1");
            mutate_object(&project.research(QUESTION_PATH), |object| {
                object["temporal"]["review_due_at"] = json!(AS_OF);
                if unavailable == "review-deadline" {
                    object["temporal"]
                        .as_object_mut()
                        .unwrap()
                        .remove("review_due_at");
                }
                if stale {
                    object["temporal"]["effective_until"] = json!(AS_OF);
                }
                object["source"] = json!({});
            });
            mutate_object(&project.research("policies/freshness-v1.yaml"), |policy| {
                policy["rules"][1]["required_checks"] = json!([unavailable]);
            });
            assert_status(
                &index(&project),
                if stale {
                    FreshnessStatus::Stale
                } else {
                    FreshnessStatus::Unknown
                },
            );
        }
    }
}

#[test]
fn source_versions_require_same_reference_type() {
    let project = TempProject::copy_fixture("freshness-v1");
    mutate_object(&project.research(CANDIDATE_PATH), |object| {
        object["type"] = json!("doi");
    });
    assert_status(&index(&project), FreshnessStatus::Fresh);
}

#[test]
fn source_versions_require_differing_version_identifiers() {
    let project = TempProject::copy_fixture("freshness-v1");
    let pinned = parse_yaml_file(&project.research(PINNED_PATH));
    mutate_object(&project.research(CANDIDATE_PATH), |object| {
        object["source_version"]["identifier"] = pinned["source_version"]["identifier"].clone();
    });
    assert_status(&index(&project), FreshnessStatus::Fresh);
}

#[test]
fn source_version_uri_release_order_and_as_of_boundaries_are_exact() {
    for (pointer, value, expected) in [
        (
            "/canonical_uri",
            "https://example.invalid/unrelated",
            FreshnessStatus::Fresh,
        ),
        (
            "/source_version/released_at",
            "2025-12-01T00:00:00Z",
            FreshnessStatus::Fresh,
        ),
        (
            "/source_version/released_at",
            "2027-01-01T00:00:01Z",
            FreshnessStatus::Fresh,
        ),
        ("/source_version/released_at", AS_OF, FreshnessStatus::Stale),
    ] {
        let project = TempProject::copy_fixture("freshness-v1");
        mutate_object(&project.research(CANDIDATE_PATH), |object| {
            *object.pointer_mut(pointer).unwrap() = json!(value);
        });
        assert_status(&index(&project), expected);
    }
    let project = TempProject::copy_fixture("freshness-v1");
    let pinned = parse_yaml_file(&project.research(PINNED_PATH));
    mutate_object(&project.research(CANDIDATE_PATH), |object| {
        object["source_version"]["released_at"] = pinned["source_version"]["released_at"].clone();
    });
    assert_status(&index(&project), FreshnessStatus::Fresh);
}

#[test]
fn missing_matching_candidate_release_is_unavailable_not_fresh() {
    for absent in [false, true] {
        let project = TempProject::copy_fixture("freshness-v1");
        remove_release(&project, CANDIDATE_PATH, absent);
        assert_status(&index(&project), FreshnessStatus::Unknown);
    }
}

#[test]
fn incomplete_pinned_reference_is_not_silently_dropped_when_another_is_complete() {
    for absent in [false, true] {
        let project = TempProject::copy_fixture("freshness-v1");
        remove_release(&project, PINNED_PATH, absent);
        mutate_object(&project.research(QUESTION_PATH), |object| {
            object["source"]["external_references"] = json!([PINNED, CANDIDATE]);
        });
        assert_status(&index(&project), FreshnessStatus::Unknown);
    }
}

fn remove_release(project: &TempProject, path: &str, absent: bool) {
    mutate_object(&project.research(path), |object| {
        if absent {
            object["source_version"]
                .as_object_mut()
                .unwrap()
                .remove("released_at");
        } else {
            object["source_version"]["released_at"] = Value::Null;
        }
    });
}

#[test]
fn known_newer_source_stays_stale_with_another_required_check_unavailable() {
    let project = TempProject::copy_fixture("freshness-v1");
    mutate_object(&project.research(QUESTION_PATH), |object| {
        object["tags"] = json!(["freshness-version-watch", "freshness-artifact-watch"]);
        object["temporal"]
            .as_object_mut()
            .unwrap()
            .remove("review_due_at");
    });
    assert_status(&index(&project), FreshnessStatus::Stale);

    // An incomplete pinned source cannot hide a known newer version of another source.
    let mut incomplete = parse_yaml_file(&project.research(PINNED_PATH));
    incomplete["id"] = json!("ref_01J00000000000000000000106");
    incomplete["canonical_uri"] = json!("https://example.invalid/other-source");
    incomplete["source_version"]["released_at"] = Value::Null;
    fs::write(
        project.research("references/incomplete--ref_01J00000000000000000000106.yaml"),
        serde_json::to_vec(&incomplete).unwrap(),
    )
    .unwrap();
    mutate_object(&project.research(QUESTION_PATH), |object| {
        object["source"]["external_references"] = json!([incomplete["id"], PINNED]);
    });
    assert_status(&index(&project), FreshnessStatus::Stale);
}

#[test]
fn source_change_without_invalidates_policy_does_not_make_object_stale() {
    let project = TempProject::copy_fixture("freshness-v1");
    mutate_object(&project.research("policies/freshness-v1.yaml"), |policy| {
        policy["rules"][1]["invalidates_on_change"] = json!(false);
    });
    assert_status(&index(&project), FreshnessStatus::Fresh);
}

#[test]
fn as_of_uses_current_knowledge_not_retrieval_time_travel() {
    let project = TempProject::copy_fixture("freshness-v1");
    mutate_object(&project.research(CANDIDATE_PATH), |object| {
        object["retrieved_at"] = json!("2028-01-01T00:00:00Z");
    });
    mutate_object(&project.research(QUESTION_PATH), |object| {
        object["created_at"] = json!("2028-01-01T00:00:00Z");
    });
    assert_status(&index(&project), FreshnessStatus::Stale);
}
