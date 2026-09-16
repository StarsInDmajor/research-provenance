//! Same-read private observations, not expected report bodies or binding verdicts.
use super::*;
use crate::content_observations::{Completeness, Owner, Phase};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

#[test]
fn findings_cap_preserves_source_observation_and_marks_coverage_incomplete() {
    let mut findings = ContentFindings::with_budget(true, crate::findings::Findings::new(1));
    findings.begin(
        Owner::Object("qst_owner".into()),
        Phase::Narrative,
        None,
        None,
        &[],
    );
    findings.state(Completeness::Verified);
    findings.push(|| Finding::new("test", "test", Severity::Error, "retained", None, "/first"));
    findings.push(|| panic!("overflow finding must not allocate"));
    let store = findings.store.as_ref().unwrap();
    assert!(!store.coverage_complete);
    assert_eq!(store.records[0].findings.len(), 1);
    assert_eq!(store.records[0].findings[0].json_pointer.as_ref(), "/first");
    assert_eq!(store.records[0].owner, Owner::Object("qst_owner".into()));
    let (output, error, complete) = findings.output.finish();
    assert!(error && !complete);
    assert_eq!(output[0].error_code, "RP_W_FINDINGS_TRUNCATED");
}

struct Fixture {
    root: PathBuf,
    index: ProjectIndex,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "rp-c3-content-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join(".research/schemas/demo/a")).unwrap();
        let mut f = Self {
            root,
            index: ProjectIndex::default(),
        };
        f.object(
            "project",
            ObjectType::Project,
            json!({"schema_policy":{"allowed_extensions":["demo.a/v1", "unused/v1"]}}),
        );
        // A missing report activates tracking without any extra content read.
        f.object(
            "chain",
            ObjectType::ClaimChain,
            json!({"validation_report":"report"}),
        );
        f
    }
    fn object(&mut self, id: &str, object_type: ObjectType, value: Value) {
        self.index.insert(crate::ObjectRecord {
            ordinal: 0,
            id: id.into(),
            logical_id: None,
            object_type,
            // Deliberately shared filename: attribution cannot be inferred from this.
            source_file: ProjectPath::new(".research/shared.yaml").unwrap(),
            value,
        });
    }
    fn file(&self, path: &str, bytes: &[u8]) {
        fs::write(self.root.join(path), bytes).unwrap();
    }
    fn artifact(&mut self, id: &str, path: &str, declared: &[u8]) {
        self.object(id, ObjectType::Artifact, json!({"uri":format!("file:{path}"),"size_bytes":declared.len(),"sha256":raw_sha256(declared)}));
    }
    fn run(&self, limits: ProjectLimits) -> ContentValidation {
        validate_content(
            &self.root,
            &self.index,
            limits,
            &SchemaBundle::new().unwrap(),
            100,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn observations_actual_not_declared_mismatch_is_complete_unread_and_resource_are_not() {
    let mut f = Fixture::new();
    f.file("data", b"actual bytes");
    f.artifact("a", "data", b"declared");
    f.artifact("b", "missing", b"missing");
    let result = f.run(ProjectLimits::default());
    let store = result.observations.unwrap();
    let a = store
        .records
        .iter()
        .find(|r| r.owner == Owner::Object("a".into()))
        .unwrap();
    assert_eq!(
        a.raw_sha256.as_deref(),
        Some(raw_sha256(b"actual bytes").as_str())
    );
    assert_eq!(a.completeness, Completeness::Mismatch);
    assert_eq!(
        a.findings.iter().map(|f| f.error_code).collect::<Vec<_>>(),
        [
            "RP_E_ARTIFACT_SIZE_MISMATCH",
            "RP_E_ARTIFACT_DIGEST_MISMATCH"
        ]
    );
    let b = store
        .records
        .iter()
        .find(|r| r.owner == Owner::Object("b".into()))
        .unwrap();
    assert_eq!(b.completeness, Completeness::Unavailable);
    assert!(b.raw_sha256.is_none());
    let result = f.run(ProjectLimits {
        local_artifact_aggregate_bytes: 1,
        ..Default::default()
    });
    let store = result.observations.unwrap();
    assert!(
        !store.coverage_complete,
        "break must not make absent visits verified"
    );
    assert!(
        store
            .records
            .iter()
            .any(|r| r.completeness == Completeness::ResourceLimit)
    );
}

#[test]
fn observations_policy_schema_transitive_bytes_and_shared_consumers_are_explicit() {
    let mut f = Fixture::new();
    f.object("project", ObjectType::Project, json!({"validation_policy":"policy.yaml", "schema_policy":{"allowed_extensions":["demo.a/v1", "unused/v1"]}}));
    f.file(
        "policy.yaml",
        b"schema: rp/freshness-policy/v1\nversion: false\n",
    );
    let schema = br#"{"$ref":"defs.schema.json"}"#;
    let defs = br#"{"type":"boolean","const":true}"#;
    f.file(".research/schemas/demo/a/v1.schema.json", schema);
    f.file(".research/schemas/demo/a/defs.schema.json", defs);
    for id in ["a", "b"] {
        f.object(
            id,
            ObjectType::Node("Question".into()),
            json!({"extensions":{"demo.a/v1":false}}),
        );
    }
    let result = f.run(ProjectLimits::default());
    let store = result.observations.unwrap();
    for (path, bytes) in [
        (
            "policy.yaml",
            b"schema: rp/freshness-policy/v1\nversion: false\n".as_slice(),
        ),
        (".research/schemas/demo/a/v1.schema.json", schema.as_slice()),
        (".research/schemas/demo/a/defs.schema.json", defs.as_slice()),
    ] {
        let r = store
            .records
            .iter()
            .find(|r| r.owner == Owner::Context(path.into()))
            .unwrap();
        assert_eq!(r.raw_sha256.as_deref(), Some(raw_sha256(bytes).as_str()));
        if r.phase == Phase::ExtensionSchema {
            assert_eq!(
                r.consumers.iter().map(AsRef::as_ref).collect::<Vec<&str>>(),
                ["a", "b"]
            );
            assert_eq!(r.namespace.as_deref(), Some("demo.a/v1"));
        } else {
            assert_eq!(r.completeness, Completeness::Mismatch);
            assert!(!r.findings.is_empty());
        }
    }
    assert!(
        !store
            .records
            .iter()
            .any(|r| r.namespace.as_deref() == Some("unused/v1")),
        "unconsumed approved schemas must not be read"
    );
    for id in ["a", "b"] {
        let r = store
            .records
            .iter()
            .find(|r| r.owner == Owner::Object(id.into()) && r.phase == Phase::ExtensionPayload)
            .unwrap();
        assert_eq!(
            r.findings[0].error_code,
            "RP_E_EXTENSION_PAYLOAD_SCHEMA_MISMATCH"
        );
    }
}

#[test]
fn observations_six_report_absent_suites_keep_exact_content_transcript_and_allocate_no_store() {
    for suite in [
        "overview-v1",
        "freshness-v1",
        "access-closure-v1",
        "claim-chain-minimum-v1",
        "claim-chain-evidential-v1",
        "claim-chain-confirmatory-v1",
    ] {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(suite)
            .join("valid");
        let report = crate::validate_project(&root).unwrap();
        let mut index = report.index.unwrap();
        assert!(index.content_observations.is_none());
        let bundle = SchemaBundle::new().unwrap();
        let absent = validate_content(&root, &index, ProjectLimits::default(), &bundle, 10000);
        index.insert(crate::ObjectRecord {
            ordinal: 0,
            id: "tracking-chain".into(),
            logical_id: None,
            object_type: ObjectType::ClaimChain,
            source_file: ProjectPath::new(".research/tracking.yaml").unwrap(),
            value: json!({"validation_report":"missing-report"}),
        });
        let enabled = validate_content(&root, &index, ProjectLimits::default(), &bundle, 10000);
        assert_eq!(
            absent.findings.as_slice(),
            enabled.findings.as_slice(),
            "exact content order/set: {suite}"
        );
        assert!(absent.observations.is_none());
        assert!(enabled.observations.unwrap().coverage_complete);
    }
}

#[test]
fn observations_schema_hashes_bind_pre_substitution_compiled_bytes_and_mutations() {
    let mut f = Fixture::new();
    f.object(
        "a",
        ObjectType::Node("Question".into()),
        json!({"extensions":{"demo.a/v1":true}}),
    );
    let path = ".research/schemas/demo/a/v1.schema.json";
    let old = br#"{"const":true}"#;
    f.file(path, old);
    let root = f.root.clone();
    read_audit::on_after_verified(move || {
        fs::write(root.join(path), br#"{"const":false}"#).unwrap();
    });
    read_audit::reset();
    let first = f.run(ProjectLimits::default());
    assert!(first.findings.is_empty());
    assert_eq!(read_audit::events(), [(path.into(), true)]);
    let first_store = first.observations.unwrap();
    let context = first_store
        .records
        .iter()
        .find(|r| r.owner == Owner::Context(path.into()))
        .unwrap();
    assert_eq!(
        context.raw_sha256.as_deref(),
        Some(raw_sha256(old).as_str())
    );
    let second = f.run(ProjectLimits::default());
    assert_eq!(
        second.findings.as_slice()[0].error_code,
        "RP_E_EXTENSION_PAYLOAD_SCHEMA_MISMATCH"
    );
    let second_store = second.observations.unwrap();
    let context2 = second_store
        .records
        .iter()
        .find(|r| r.owner == Owner::Context(path.into()))
        .unwrap();
    assert_ne!(context.raw_sha256, context2.raw_sha256);
}

#[test]
fn observations_narrative_and_schema_use_metered_descriptor_without_hash_reopen() {
    let mut f = Fixture::new();
    f.file("note", b"original");
    f.object(
        "node",
        ObjectType::Node("Question".into()),
        json!({"narrative":{"path":"note","sha256":raw_sha256(b"original")}}),
    );
    read_audit::reset();
    let store = f.run(ProjectLimits::default()).observations.unwrap();
    assert_eq!(
        read_audit::events(),
        [("note".into(), true)],
        "narrative must use metered secure descriptor capture"
    );
    let note = store
        .records
        .iter()
        .find(|r| r.phase == Phase::Narrative)
        .unwrap();
    assert_eq!(
        note.raw_sha256.as_deref(),
        Some(raw_sha256(b"original").as_str())
    );
}

#[test]
fn observations_post_read_substitution_and_conflicting_same_path_are_never_last_writer_wins() {
    let mut f = Fixture::new();
    f.file("data", b"old");
    f.artifact("a", "data", b"old");
    let root = f.root.clone();
    read_audit::on_after_verified(move || {
        fs::rename(root.join("data"), root.join("original")).unwrap();
        fs::write(root.join("data"), b"new").unwrap();
    });
    read_audit::reset();
    let store = f.run(ProjectLimits::default()).observations.unwrap();
    let a = store
        .records
        .iter()
        .find(|r| r.owner == Owner::Object("a".into()))
        .unwrap();
    assert_eq!(a.raw_sha256.as_deref(), Some(raw_sha256(b"old").as_str()));
    assert_eq!(read_audit::events(), [("data".into(), false)]);
    // Two actual secure reads in one run with a deterministic pathname substitution.
    f.file("data", b"old");
    f.artifact("b", "data", b"new");
    let root = f.root.clone();
    read_audit::on_after_verified(move || {
        fs::write(root.join("data"), b"new").unwrap();
    });
    let store = f.run(ProjectLimits::default()).observations.unwrap();
    for id in ["a", "b"] {
        let r = store
            .records
            .iter()
            .find(|r| r.owner == Owner::Object(id.into()))
            .unwrap();
        assert_eq!(r.completeness, Completeness::Ambiguous);
        assert!(r.raw_sha256.is_none());
    }
}

#[test]
fn observations_policy_mutation_and_normalized_same_path_conflict_bind_actual_reads() {
    use crate::content_observations::{ObservationLimits, ObservationStore};
    let mut f = Fixture::new();
    f.object(
        "project",
        ObjectType::Project,
        json!({"validation_policy":"policy.yaml"}),
    );
    f.file("policy.yaml", b"ignored: true\n");
    let first = f.run(ProjectLimits::default()).observations.unwrap();
    f.file("policy.yaml", b"ignored: false\n");
    let second = f.run(ProjectLimits::default()).observations.unwrap();
    assert_ne!(first.records[0].raw_sha256, second.records[0].raw_sha256);
    let mut store = ObservationStore::new(ObservationLimits::default());
    for (path, hash) in [
        ("data//file", raw_sha256(b"old")),
        ("data/file", raw_sha256(b"new")),
    ] {
        let key = store.begin(
            Owner::Context(path.into()),
            Phase::ExtensionSchema,
            Some(path),
            None,
            &[],
        );
        store.state(key, Completeness::Verified);
        store.hash(key, &hash);
    }
    assert!(
        store
            .records
            .iter()
            .all(|r| r.completeness == Completeness::Ambiguous && r.raw_sha256.is_none())
    );
    assert!(
        store
            .records
            .iter()
            .all(|r| r.owner == Owner::Context("data/file".into()))
    );
}

#[test]
fn observations_unstable_read_and_low_store_limits_are_explicitly_incomplete() {
    use crate::content_observations::{ObservationLimits, ObservationStore};
    let mut f = Fixture::new();
    f.file("data", b"old");
    f.artifact("a", "data", b"old");
    let root = f.root.clone();
    read_audit::on_after_hash(move || {
        fs::write(root.join("data"), b"changed size").unwrap();
    });
    let store = f.run(ProjectLimits::default()).observations.unwrap();
    let r = store
        .records
        .iter()
        .find(|r| r.owner == Owner::Object("a".into()))
        .unwrap();
    assert_eq!(r.completeness, Completeness::Unavailable);
    assert!(r.raw_sha256.is_none());
    for limits in [
        ObservationLimits {
            records: 0,
            ..Default::default()
        },
        ObservationLimits {
            bytes: 0,
            ..Default::default()
        },
        ObservationLimits {
            owners: 0,
            ..Default::default()
        },
        ObservationLimits {
            findings: 0,
            ..Default::default()
        },
    ] {
        let mut store = ObservationStore::new(limits);
        let key = store.begin(
            Owner::Object("a".into()),
            Phase::Artifact,
            Some("data"),
            None,
            &[],
        );
        store.state(key, Completeness::Verified);
        store.hash(key, &raw_sha256(b"old"));
        store.finding(
            key,
            &Finding::new(
                "TEST",
                "artifact_integrity",
                Severity::Error,
                "not retained",
                None,
                "/uri",
            ),
        );
        assert!(
            !store.coverage_complete,
            "reservation must fail closed: {limits:?}"
        );
    }
}

#[test]
fn observations_failed_schema_visit_cannot_make_skipped_consumers_verified() {
    let mut f = Fixture::new();
    for id in ["a", "b"] {
        f.object(
            id,
            ObjectType::Node("Question".into()),
            json!({"extensions":{"demo.a/v1":false}}),
        );
    }
    for bytes in [br#"{"$ref":"missing.schema.json"}"#.as_slice(), b"not json"] {
        f.file(".research/schemas/demo/a/v1.schema.json", bytes);
        let result = f.run(ProjectLimits::default());
        assert!(!result.findings.is_empty());
        let store = result.observations.unwrap();
        assert!(
            !store.coverage_complete,
            "payloads skipped after failed schema visit cannot imply success"
        );
        assert!(
            store
                .records
                .iter()
                .any(|r| matches!(r.owner, Owner::Context(_)) && !r.findings.is_empty())
        );
    }
}

#[test]
fn observations_low_tracking_budget_preserves_output_but_not_empty_success() {
    let mut f = Fixture::new();
    f.file("data", b"actual");
    f.artifact("a", "data", b"declared");
    let expected = f.run(ProjectLimits::default());
    crate::content_observations::test_limits(crate::content_observations::ObservationLimits {
        records: 0,
        ..Default::default()
    });
    let actual = f.run(ProjectLimits::default());
    assert_eq!(expected.findings.as_slice(), actual.findings.as_slice());
    let store = actual.observations.unwrap();
    assert!(store.records.is_empty());
    assert!(!store.coverage_complete);
    assert!(!store.pipeline_provenance_complete);
}

#[test]
fn observations_report_technical_errors_are_outer_and_https_is_identifier_only() {
    let mut f = Fixture::new();
    f.file("report.json", b"{}");
    f.artifact("report", "report.json", b"{}");
    f.object(
        "remote",
        ObjectType::Artifact,
        json!({"uri":"https://example.invalid/data"}),
    );
    let store = f.run(ProjectLimits::default()).observations.unwrap();
    let report = store
        .records
        .iter()
        .find(|r| r.owner == Owner::Object("report".into()))
        .unwrap();
    assert_eq!(report.phase, Phase::OuterReport);
    assert_eq!(report.findings[0].error_code, "RP_E_REPORT_SCHEMA_INVALID");
    assert!(
        store
            .records
            .iter()
            .filter(|r| r.phase != Phase::OuterReport)
            .all(|r| r.findings.is_empty())
    );
    let remote = store
        .records
        .iter()
        .find(|r| r.owner == Owner::Object("remote".into()))
        .unwrap();
    assert_eq!(remote.completeness, Completeness::IdentifierOnly);
    assert!(remote.raw_sha256.is_none());
}
