use std::{collections::BTreeMap, fs, path::PathBuf};

use rp_core::{ProjectPath, SchemaBundle, parse_restricted_yaml};
use serde_json::json;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../")
}

#[test]
fn embedded_bundle_is_byte_identical_to_the_frozen_plan_snapshot() {
    let source = repository_root().join("schemas/v1");
    let expected: BTreeMap<_, _> = fs::read_dir(source)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().into_string().unwrap(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    let embedded: BTreeMap<_, _> = SchemaBundle::resources()
        .iter()
        .map(|resource| (resource.name.to_string(), resource.bytes.to_vec()))
        .collect();

    assert_eq!(embedded, expected);
}

#[test]
fn built_in_relative_refs_resolve_offline_and_formats_are_asserted() {
    let bundle = SchemaBundle::new().unwrap();
    let valid = json!({
        "schema": "rp/artifact-manifest/v1",
        "id": "art_01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "title": "result",
        "uri": "https://example.invalid/result.json",
        "media_type": "application/json",
        "size_bytes": 1,
        "sha256": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "created_at": "2026-09-04T12:00:00Z",
        "access": {"level": "public", "compartments": []}
    });
    assert!(
        bundle
            .validate(
                &valid,
                ProjectPath::new(".research/artifacts/result.yaml").unwrap()
            )
            .is_ok()
    );

    let mut invalid = valid;
    invalid["created_at"] = json!("2026-13-40T25:61:61Z");
    let findings = bundle
        .validate(
            &invalid,
            ProjectPath::new(".research/artifacts/result.yaml").unwrap(),
        )
        .unwrap_err();
    assert!(findings.iter().any(|finding| {
        finding.error_code == "RP_E_SCHEMA_FORMAT" && finding.json_pointer == "/created_at"
    }));
}

#[test]
fn remote_or_unknown_schema_refs_are_rejected_without_retrieval() {
    let bundle = SchemaBundle::new().unwrap();
    let remote = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$ref": "https://attacker.invalid/schema.json"
    });
    let escaping = json!({"$ref": "../outside.schema.json"});

    assert!(bundle.check_offline_references(&remote).is_err());
    assert!(bundle.check_offline_references(&escaping).is_err());
}

#[test]
fn node_kind_is_pre_discriminated_to_report_a_leaf_finding() {
    let path = repository_root().join(
        "fixtures/overview-v1/valid/.research/records/observations/protocol-repeatability-control--obs_01J00000000000000000000019.yaml",
    );
    let bytes = fs::read(path).unwrap();
    let source = ProjectPath::new(
        ".research/records/observations/protocol-repeatability-control--obs_01J00000000000000000000019.yaml",
    )
    .unwrap();
    let mut value = parse_restricted_yaml(&bytes, source.clone(), &Default::default())
        .unwrap()
        .value;
    value.as_object_mut().unwrap().remove("observation");

    let findings = SchemaBundle::new()
        .unwrap()
        .validate(&value, source)
        .unwrap_err();

    assert!(findings.iter().any(|finding| {
        finding.error_code == "RP_E_SCHEMA_REQUIRED_PROPERTY"
            && finding.message.contains("observation")
    }));
    assert!(
        !findings
            .iter()
            .any(|finding| finding.error_code == "RP_E_SCHEMA_DISCRIMINATOR")
    );
}

#[test]
fn frozen_contextual_schema_overrides_use_their_specific_codes() {
    let bundle = SchemaBundle::new().unwrap();

    let assessment_path = repository_root().join(
        "fixtures/overview-v1/valid/.research/assessments/proposed-broad-hypothesis--asm_01J00000000000000000000021.yaml",
    );
    let assessment_source = ProjectPath::new(
        ".research/assessments/proposed-broad-hypothesis--asm_01J00000000000000000000021.yaml",
    )
    .unwrap();
    let mut assessment = parse_restricted_yaml(
        &fs::read(assessment_path).unwrap(),
        assessment_source.clone(),
        &Default::default(),
    )
    .unwrap()
    .value;
    assessment["target"]["type"] = json!("relation_revision");
    let findings = bundle.validate(&assessment, assessment_source).unwrap_err();
    assert!(
        findings
            .iter()
            .any(|finding| finding.error_code == "RP_E_SCHEMA_TARGET_ID_TYPE")
    );

    let chain_path = repository_root().join(
        "fixtures/claim-chain-minimum-v1/valid/.research/claim-chains/minimum-structural-chain--cch_01J00000000000000000000051.yaml",
    );
    let chain_source = ProjectPath::new(
        ".research/claim-chains/minimum-structural-chain--cch_01J00000000000000000000051.yaml",
    )
    .unwrap();
    let mut chain = parse_restricted_yaml(
        &fs::read(chain_path).unwrap(),
        chain_source.clone(),
        &Default::default(),
    )
    .unwrap()
    .value;
    chain["node_revisions"][0] = json!("mutable-logical-id");
    let findings = bundle.validate(&chain, chain_source).unwrap_err();
    assert!(
        findings
            .iter()
            .any(|finding| { finding.error_code == "RP_E_CLAIM_CHAIN_REQUIRES_EXACT_REVISION" })
    );

    let artifact = json!({
        "schema": "rp/artifact-manifest/v1",
        "id": "art_01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "title": "result",
        "uri": "https://example.invalid/result.json",
        "media_type": "application/json",
        "size_bytes": 1,
        "sha256": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "created_at": "2026-09-04T12:00:00Z",
        "access": {"level": "public", "compartments": []},
        "declassification_attestation": {}
    });
    let findings = bundle
        .validate(
            &artifact,
            ProjectPath::new(".research/artifacts/result.yaml").unwrap(),
        )
        .unwrap_err();
    assert!(
        findings
            .iter()
            .any(|finding| finding.error_code == "RP_E_DECLASSIFICATION_UNSUPPORTED")
    );
}

#[test]
fn schema_errors_map_to_kind_specific_stable_codes() {
    let bundle = SchemaBundle::new().unwrap();
    let value = json!({
        "schema": "rp/artifact-manifest/v1",
        "id": "art_01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "title": "result",
        "uri": "https://example.invalid/result.json",
        "media_type": "application/json",
        "size_bytes": 1,
        "sha256": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "created_at": "2026-09-04T12:00:00Z",
        "access": {"level": "secret", "compartments": []},
        "unexpected": true
    });

    let findings = bundle
        .validate(
            &value,
            ProjectPath::new(".research/artifacts/result.yaml").unwrap(),
        )
        .unwrap_err();
    let codes: Vec<_> = findings.iter().map(|finding| finding.error_code).collect();

    assert!(codes.contains(&"RP_E_ACCESS_LEVEL_UNKNOWN"));
    assert!(codes.contains(&"RP_E_CLOSED_SCHEMA_UNKNOWN_PROPERTY"));
}
