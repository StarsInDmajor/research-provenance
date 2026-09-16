mod common;

use std::{
    fs,
    os::unix::fs::{FileExt, symlink},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use common::{TempProject, fixture_root, mutate_object};
use rp_core::{jcs_sha256, validate_project};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[test]
fn file_artifacts_are_verified_without_network_access() {
    for fixture in [
        "access-closure-v1",
        "claim-chain-evidential-v1",
        "claim-chain-confirmatory-v1",
    ] {
        let report = validate_project(&fixture_root(fixture)).expect("validate artifact fixture");
        assert!(
            report.findings.is_empty(),
            "{fixture}: {:?}",
            report.findings
        );
        let index = report.index.unwrap();
        for object in index.objects() {
            if object.value.get("schema").and_then(Value::as_str) == Some("rp/artifact-manifest/v1")
                && object
                    .value
                    .get("uri")
                    .and_then(Value::as_str)
                    .is_some_and(|uri| uri.starts_with("file:"))
            {
                assert!(index.artifact_verified(&object.id), "{}", object.id);
            }
        }
    }
}

#[test]
fn artifact_missing_size_digest_symlink_and_encoded_escape_fail_closed() {
    let digest = TempProject::copy_fixture("claim-chain-evidential-v1");
    let data = digest.path().join("data/evidential-summary.csv");
    let mut bytes = fs::read(&data).unwrap();
    bytes[0] ^= 1;
    fs::write(&data, bytes).unwrap();
    assert_code(&digest, "RP_E_ARTIFACT_DIGEST_MISMATCH");

    let size = TempProject::copy_fixture("claim-chain-evidential-v1");
    mutate_object(
        &size.research("artifacts/evidential-summary--art_01J00000000000000000000005.yaml"),
        |object| object["size_bytes"] = json!(999),
    );
    assert_code(&size, "RP_E_ARTIFACT_SIZE_MISMATCH");

    let missing = TempProject::copy_fixture("claim-chain-evidential-v1");
    fs::remove_file(missing.path().join("data/evidential-summary.csv")).unwrap();
    assert_code(&missing, "RP_E_ARTIFACT_NOT_FOUND");

    let linked = TempProject::copy_fixture("claim-chain-evidential-v1");
    let local = linked.path().join("data/evidential-summary.csv");
    let outside = linked.path().join("outside.csv");
    fs::rename(&local, &outside).unwrap();
    symlink(&outside, &local).unwrap();
    assert_code(&linked, "RP_E_PATH_SYMLINK_FORBIDDEN");

    let escaped = TempProject::copy_fixture("claim-chain-evidential-v1");
    mutate_object(
        &escaped.research("artifacts/evidential-summary--art_01J00000000000000000000005.yaml"),
        |object| object["uri"] = json!("file:%2e%2e/outside.csv"),
    );
    assert_code(&escaped, "RP_E_PATH_PROJECT_ESCAPE");
}

#[test]
fn artifact_changes_during_streaming_verification_are_rejected() {
    let project = TempProject::copy_fixture("claim-chain-evidential-v1");
    let data_path = project.path().join("data/evidential-summary.csv");
    let bytes = vec![b'a'; 32 * 1024 * 1024];
    fs::write(&data_path, &bytes).unwrap();
    let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    mutate_object(
        &project.research("artifacts/evidential-summary--art_01J00000000000000000000005.yaml"),
        |object| {
            object["size_bytes"] = json!(bytes.len());
            object["sha256"] = json!(digest);
        },
    );

    let running = Arc::new(AtomicBool::new(true));
    let writer_running = Arc::clone(&running);
    let writer_path = data_path.clone();
    let writer = std::thread::spawn(move || {
        let file = fs::OpenOptions::new()
            .write(true)
            .open(writer_path)
            .unwrap();
        let mut byte = b'b';
        while writer_running.load(Ordering::Relaxed) {
            file.write_all_at(&[byte], 0).unwrap();
            byte = if byte == b'b' { b'c' } else { b'b' };
            std::thread::yield_now();
        }
    });
    let report = validate_project(project.path()).unwrap();
    running.store(false, Ordering::Relaxed);
    writer.join().unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|finding| { finding.error_code == "RP_E_ARTIFACT_CHANGED_DURING_VERIFICATION" }),
        "{:?}",
        report.findings
    );
}

#[test]
fn extension_payloads_require_allowed_local_offline_schemas() {
    let missing = TempProject::copy_fixture("overview-v1");
    fs::remove_file(missing.research("schemas/fixture/synthetic/v1.schema.json")).unwrap();
    assert_code(&missing, "RP_E_EXTENSION_SCHEMA_NOT_FOUND");

    let payload = TempProject::copy_fixture("overview-v1");
    mutate_object(&payload.research("project.yaml"), |object| {
        object["extensions"]["fixture.synthetic/v1"]["domain_neutral"] = json!(false);
    });
    assert_code(&payload, "RP_E_EXTENSION_PAYLOAD_SCHEMA_MISMATCH");

    let local_ref = TempProject::copy_fixture("overview-v1");
    let schema_path = local_ref.research("schemas/fixture/synthetic/v1.schema.json");
    let mut schema: Value = serde_json::from_slice(&fs::read(&schema_path).unwrap()).unwrap();
    schema["properties"]["domain_neutral"] =
        json!({"$ref": "defs.schema.json#/$defs/domainNeutral"});
    fs::write(&schema_path, serde_json::to_vec_pretty(&schema).unwrap()).unwrap();
    fs::write(
        local_ref.research("schemas/fixture/synthetic/defs.schema.json"),
        serde_json::to_vec_pretty(&json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$defs": {"domainNeutral": {"const": true}}
        }))
        .unwrap(),
    )
    .unwrap();
    let report = validate_project(local_ref.path()).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);

    let remote = TempProject::copy_fixture("overview-v1");
    let schema_path = remote.research("schemas/fixture/synthetic/v1.schema.json");
    let mut schema: Value = serde_json::from_slice(&fs::read(&schema_path).unwrap()).unwrap();
    schema["allOf"] = json!([{ "$ref": "https://example.invalid/remote.schema.json" }]);
    fs::write(&schema_path, serde_json::to_vec_pretty(&schema).unwrap()).unwrap();
    assert_code(&remote, "RP_E_EXTENSION_REF_FORBIDDEN");

    let disallowed = TempProject::copy_fixture("overview-v1");
    mutate_object(&disallowed.research("project.yaml"), |object| {
        object["schema_policy"]["allowed_extensions"] = json!(["legacy-import/v1"]);
    });
    assert_code(&disallowed, "RP_E_EXTENSION_NAMESPACE_NOT_ALLOWED");
}

#[test]
fn extension_semantic_ids_require_declarations_and_join_access_closure() {
    let declared = TempProject::copy_fixture("overview-v1");
    let schema_path = declared.research("schemas/fixture/synthetic/v1.schema.json");
    let mut schema: Value = serde_json::from_slice(&fs::read(&schema_path).unwrap()).unwrap();
    schema["properties"]["target_id"] = json!({"type": "string"});
    schema["required"]
        .as_array_mut()
        .unwrap()
        .push(json!("target_id"));
    schema["x-rp-semantic-references"] = json!([{
        "pointer_template": "/target_id",
        "target_schema": "rp/node-revision/v1"
    }]);
    fs::write(&schema_path, serde_json::to_vec_pretty(&schema).unwrap()).unwrap();
    mutate_object(&declared.research("project.yaml"), |object| {
        object["extensions"]["fixture.synthetic/v1"]["target_id"] =
            json!("qst_01J00000000000000000000010");
    });
    let report = validate_project(declared.path()).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    let index = report.index.unwrap();
    let project_id = "proj_01J00000000000000000000001";
    assert!(
        index
            .references_from(project_id)
            .any(|reference| { reference.target_id.as_ref() == "qst_01J00000000000000000000010" })
    );
    assert!(index.access_report(project_id).unwrap().dependency_count >= 1);

    let undeclared = TempProject::copy_fixture("overview-v1");
    let schema_path = undeclared.research("schemas/fixture/synthetic/v1.schema.json");
    let mut schema: Value = serde_json::from_slice(&fs::read(&schema_path).unwrap()).unwrap();
    schema["properties"]["target_id"] = json!({"type": "string"});
    schema["required"]
        .as_array_mut()
        .unwrap()
        .push(json!("target_id"));
    fs::write(&schema_path, serde_json::to_vec_pretty(&schema).unwrap()).unwrap();
    mutate_object(&undeclared.research("project.yaml"), |object| {
        object["extensions"]["fixture.synthetic/v1"]["target_id"] =
            json!("qst_01J00000000000000000000010");
    });
    assert_code(&undeclared, "RP_E_EXTENSION_REFERENCE_DECLARATION_INVALID");
}

const EXTENSION_SOURCE: &str = "qst_01J00000000000000000000095";
const EXTENSION_SOURCE_FILE: &str =
    "records/questions/extension-public-source--qst_01J00000000000000000000095.yaml";
const EXTENSION_POINTER: &str = "/extensions/fixture.synthetic~1v1/target_id";

fn extension_reference_project(value: Value, declarations: Value) -> TempProject {
    let project = TempProject::copy_fixture("access-closure-v1");
    let schema_path = project.research("schemas/fixture/synthetic/v1.schema.json");
    let mut schema: Value = serde_json::from_slice(&fs::read(&schema_path).unwrap()).unwrap();
    // Intentionally permissive: semantic declarations must enforce exact references
    // even when the extension's own payload schema does not constrain the field.
    schema["properties"]["target_id"] = json!({});
    schema["x-rp-semantic-references"] = declarations;
    fs::write(schema_path, serde_json::to_vec_pretty(&schema).unwrap()).unwrap();
    // Give the public source its own identity so existing fixture dependents do
    // not acquire additional access violations unrelated to this regression.
    fs::copy(
        project.research(
            "records/questions/question-public-control--qst_01J00000000000000000000015.yaml",
        ),
        project.research(EXTENSION_SOURCE_FILE),
    )
    .unwrap();
    mutate_object(&project.research(EXTENSION_SOURCE_FILE), |object| {
        object["id"] = json!(EXTENSION_SOURCE);
        object["logical_id"] = json!("extension-public-source");
        object["extensions"] = json!({"fixture.synthetic/v1": {
            "domain_neutral": true,
            "scientific_claim": false,
            "network_resolution_required": false,
            "target_id": value
        }});
    });
    project
}

fn node_reference_declaration() -> Value {
    json!([{
        "pointer_template": "/target_id",
        "target_schema": "rp/node-revision/v1"
    }])
}

fn rename_exact_id(project: &TempProject, old: &str, new: &str) {
    fn visit(path: &std::path::Path, old: &str, new: &str) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, old, new);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("yaml") {
                let text = fs::read_to_string(&path).unwrap();
                if text.contains(old) {
                    fs::write(&path, text.replace(old, new)).unwrap();
                }
                let name = path.file_name().unwrap().to_str().unwrap();
                if name.contains(old) {
                    fs::rename(&path, path.with_file_name(name.replace(old, new))).unwrap();
                }
            }
        }
    }
    visit(&project.research(""), old, new);
    // ID mutations alter canonical digests; keep the unrelated snapshots valid.
    let index = validate_project(project.path()).unwrap().index.unwrap();
    for object in index.objects() {
        if let Some(chain) = index.claim_chain_report(&object.id) {
            mutate_object(
                &project.path().join(object.source_file.as_str()),
                |object| {
                    object["source_closure_sha256"] = json!(chain.source_sha256);
                },
            );
        }
    }
}

#[test]
fn extension_non01_exact_target_resolves_and_raises_public_source_access() {
    let old = "qst_01J00000000000000000000013";
    let target = "qst_ZZJ00000000000000000000013";
    let project = extension_reference_project(json!(target), node_reference_declaration());
    // Preserve the restricted fixture object's references and filename identity.
    rename_exact_id(&project, old, target);
    let report = validate_project(project.path()).unwrap();
    let index = report.index.unwrap();
    assert!(
        index.get(target).is_some(),
        "canonical non-01 target must load"
    );
    let reference = index
        .references_from(EXTENSION_SOURCE)
        .find(|reference| reference.json_pointer == EXTENSION_POINTER)
        .expect("declared non-01 exact ID must enter the reference index");
    assert_eq!(reference.target_id.as_ref(), target);
    assert_eq!(
        report.findings.len(),
        1,
        "only the deliberate public-to-restricted access violation is expected: {:?}",
        report.findings
    );
    assert_eq!(report.findings[0].error_code, "RP_E_ACCESS_LEVEL_DOWNGRADE");
    assert_eq!(
        report.findings[0].source_file.as_ref().unwrap().as_str(),
        format!(".research/{EXTENSION_SOURCE_FILE}")
    );
    assert_eq!(
        index
            .access_report(EXTENSION_SOURCE)
            .unwrap()
            .effective
            .level,
        rp_core::AccessLevel::Restricted
    );

    mutate_object(&project.research(EXTENSION_SOURCE_FILE), |object| {
        object["access"] = json!({"level": "restricted", "compartments": ["partner-beta"]});
    });
    let report = validate_project(project.path()).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
}

#[test]
fn extension_undeclared_non01_canonical_ids_are_rejected_for_all_frozen_prefixes() {
    let common_schema: Value = serde_json::from_slice(
        rp_core::SchemaBundle::resources()
            .iter()
            .find(|resource| resource.name == "common.schema.json")
            .unwrap()
            .bytes,
    )
    .unwrap();
    let id_schemas = common_schema
        .pointer("/$defs/ids/$defs")
        .unwrap()
        .as_object()
        .unwrap();
    for (name, schema) in id_schemas {
        let pattern = schema["pattern"].as_str().unwrap();
        let prefix_pattern = pattern
            .strip_prefix('^')
            .unwrap()
            .split_once('_')
            .unwrap()
            .0;
        let prefixes = prefix_pattern
            .strip_prefix("(?:")
            .and_then(|pattern| pattern.strip_suffix(')'))
            .unwrap_or(prefix_pattern);
        for prefix in prefixes.split('|') {
            let id = format!("{prefix}_ZZJ00000000000000000000013");
            let project = extension_reference_project(json!(id), json!([]));
            let report = validate_project(project.path()).unwrap();
            assert!(
                report.findings.iter().any(|finding| {
                    finding.error_code == "RP_E_EXTENSION_REFERENCE_DECLARATION_INVALID"
                        && finding.json_pointer == EXTENSION_POINTER
                }),
                "{name}/{id}: {:?}",
                report.findings
            );
        }
    }
}

#[test]
fn extension_declared_malformed_strings_are_rejected_not_skipped() {
    for value in [
        "not-an-exact-id",
        "question-restricted-branch",
        "qst_01J0000000000000000000001I",
        "qst_01J0000000000000000000001",
        "custom_01J00000000000000000000013",
        "qst_zzj00000000000000000000013",
        "",
    ] {
        let project = extension_reference_project(json!(value), node_reference_declaration());
        let report = validate_project(project.path()).unwrap();
        assert!(
            report.findings.iter().any(|finding| {
                finding.error_code == "RP_E_EXTENSION_REFERENCE_DECLARATION_INVALID"
                    && finding.json_pointer == EXTENSION_POINTER
            }),
            "{value:?}: {:?}",
            report.findings
        );
        assert!(
            !report
                .index
                .unwrap()
                .references_from(EXTENSION_SOURCE)
                .any(|reference| reference.json_pointer == EXTENSION_POINTER)
        );
    }
}

#[test]
fn extension_declared_nonstring_values_are_rejected() {
    for value in [json!(null), json!(false), json!(123), json!([]), json!({})] {
        let project = extension_reference_project(value.clone(), node_reference_declaration());
        let report = validate_project(project.path()).unwrap();
        assert!(
            report.findings.iter().any(|finding| {
                finding.error_code == "RP_E_EXTENSION_REFERENCE_DECLARATION_INVALID"
                    && finding.json_pointer == EXTENSION_POINTER
            }),
            "{value}: {:?}",
            report.findings
        );
    }
}

#[test]
fn extension_declared_incompatible_existing_target_is_rejected() {
    let old = "art_01J00000000000000000000005";
    let target = "art_ZZJ00000000000000000000005";
    let project = extension_reference_project(json!(target), node_reference_declaration());
    rename_exact_id(&project, old, target);
    let report = validate_project(project.path()).unwrap();
    assert!(
        report.findings.iter().any(|finding| {
            finding.error_code == "RP_E_REFERENCE_TYPE_MISMATCH"
                && finding.json_pointer == EXTENSION_POINTER
        }),
        "{:?}",
        report.findings
    );
}

#[test]
fn extension_ordinary_undeclared_strings_and_logical_ids_are_not_exact_references() {
    for value in [
        "ordinary text",
        "question-restricted-branch",
        "custom_01J00000000000000000000013",
        "qst_01J0000000000000000000001I",
        "qst_01j00000000000000000000013",
    ] {
        let project = extension_reference_project(json!(value), json!([]));
        let report = validate_project(project.path()).unwrap();
        assert!(report.findings.is_empty(), "{value}: {:?}", report.findings);
        assert!(
            !report
                .index
                .unwrap()
                .references_from(EXTENSION_SOURCE)
                .any(|reference| reference.json_pointer == EXTENSION_POINTER)
        );
    }
}

#[test]
fn policy_and_narrative_paths_are_contained_and_digest_checked() {
    let policy = TempProject::copy_fixture("freshness-v1");
    fs::remove_file(policy.research("policies/freshness-v1.yaml")).unwrap();
    assert_code(&policy, "RP_E_POLICY_NOT_FOUND");

    let narrative = TempProject::copy_fixture("freshness-v1");
    fs::create_dir(narrative.research("notes")).unwrap();
    let note = b"Synthetic narrative.\n";
    fs::write(narrative.research("notes/fresh.md"), note).unwrap();
    let digest = format!("sha256:{:x}", Sha256::digest(note));
    mutate_object(
        &narrative
            .research("records/questions/fresh-review-window--qst_01J00000000000000000000110.yaml"),
        |object| {
            object["narrative"] = json!({
                "path": ".research/notes/fresh.md",
                "sha256": digest,
                "role": "detail"
            });
        },
    );
    let report = validate_project(narrative.path()).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    fs::write(
        narrative.research("notes/fresh.md"),
        b"Tampered narrative.\n",
    )
    .unwrap();
    assert_code(&narrative, "RP_E_NARRATIVE_DIGEST_MISMATCH");
}

#[test]
fn phase_one_rejects_event_backed_provenance() {
    let project = TempProject::copy_fixture("freshness-v1");
    mutate_object(
        &project
            .research("records/questions/fresh-review-window--qst_01J00000000000000000000110.yaml"),
        |object| object["source"]["events"] = json!(["ev_01J00000000000000000000999"]),
    );
    assert_code(&project, "RP_E_EVENT_PROVENANCE_UNSUPPORTED");
}

#[test]
fn canonical_object_digests_remain_full_field_jcs() {
    let report = validate_project(&fixture_root("claim-chain-minimum-v1")).unwrap();
    let index = report.index.unwrap();
    let object = index.get("qst_01J00000000000000000000010").unwrap();
    assert_eq!(
        index.canonical_digest(&object.id).unwrap(),
        jcs_sha256(&object.value).unwrap()
    );
}

fn assert_code(project: &TempProject, code: &str) {
    let report = validate_project(project.path()).expect("validate content mutation");
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.error_code == code),
        "expected {code}, got {:?}",
        report.findings
    );
}
