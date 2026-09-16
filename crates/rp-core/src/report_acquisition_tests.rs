use super::*;
use crate::report_json::parse_report_json_metered;
use serde_json::json;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: std::path::PathBuf,
    index: ProjectIndex,
    body: Vec<u8>,
}
impl Fixture {
    fn new(count: usize) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rp-c2-unit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let d = format!("sha256:{}", "0".repeat(64));
        let body = serde_json::to_vec(&json!({
            "schema":"rp/claim-chain-validation-report/v1", "scope":"claim-chain-subject-pre-binding/v1",
            "subject":{"id":"cch_01J00000000000000000000001", "canonical_digest":d, "validation_policy":{"profile":"minimum-v1","version":1}},
            "selected_nodes":[],"selected_relations":[],
            "source_closure":{"algorithm":"rp/source-closure/v1","entries":[],"sha256":d,"unresolved_ids":[]},
            "validation_dependencies":[],"validation_context":[],
            "validator":{"id":"rp-core","ruleset":"claim-chain-subject-pre-binding/v1","fingerprint":d},
            "outcome":{"passed":true,"findings":[]}
        })).unwrap();
        let mut fixture = Self {
            root,
            index: ProjectIndex::default(),
            body,
        };
        for n in 0..count {
            let id = format!("art_{n:026}");
            fixture.artifact(&id, &fixture.body.clone());
            fixture.snapshot(n, &id);
        }
        fixture
    }
    fn record(&mut self, id: &str, kind: ObjectType, value: Value) {
        self.index.insert(crate::ObjectRecord {
            ordinal: 0,
            id: id.into(),
            logical_id: None,
            object_type: kind,
            source_file: ProjectPath::new(format!(".research/{id}.yaml")).unwrap(),
            value,
        });
    }
    fn artifact(&mut self, id: &str, bytes: &[u8]) {
        fs::write(self.root.join(id), bytes).unwrap();
        self.record(
            id,
            ObjectType::Artifact,
            json!({"uri":format!("file:{id}"),"size_bytes":bytes.len(),"sha256":raw_sha256(bytes)}),
        );
    }
    fn snapshot(&mut self, n: usize, id: &str) {
        self.record(
            &format!("cch_{n:026}"),
            ObjectType::ClaimChain,
            json!({"validation_report":id}),
        );
    }
    fn run(
        &self,
        limits: ReportLimits,
        aggregate: usize,
    ) -> (Acquisition<'_>, Vec<Finding>, BTreeSet<Box<str>>) {
        let mut findings = ContentFindings::new(false);
        let mut verified = BTreeSet::new();
        let mut reports = Acquisition::new(&self.index, limits, &mut findings);
        validate_artifacts(
            &self.root,
            &self.index,
            ProjectLimits {
                local_artifact_aggregate_bytes: aggregate,
                ..ProjectLimits::default()
            },
            &mut findings,
            &mut verified,
            &mut reports,
            &SchemaBundle::new().unwrap(),
        );
        (reports, findings.output.finish().0, verified)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn code(findings: &[Finding], code: &str) -> bool {
    findings.iter().any(|f| f.error_code == code)
}
fn node_count(bytes: &[u8]) -> usize {
    let mut remaining = 1000;
    parse_report_json_metered(bytes, &crate::ReportJsonLimits::default(), &mut remaining).unwrap();
    1000 - remaining
}

#[test]
fn distinct_report_count_overflow_aborts_all_artifact_reads() {
    let mut fixture = Fixture::new(2);
    fixture.artifact("art_z_science", b"ordinary scientific bytes");
    read_audit::reset();
    let (reports, findings, verified) = fixture.run(
        ReportLimits {
            references: 1,
            ..ReportLimits::default()
        },
        usize::MAX,
    );
    assert!(code(&findings, "RP_E_RESOURCE_REPORT_COUNT_EXCEEDED"));
    assert!(reports.overflow);
    assert!(
        read_audit::events().is_empty(),
        "overflow must abort science and report reads"
    );
    assert!(reports.candidates.is_empty());
    assert!(verified.is_empty());
    assert_eq!(
        reports.remaining_capture,
        ReportLimits::default().captured_bytes
    );
    assert_eq!(reports.remaining_nodes, ReportLimits::default().nodes);
}

#[test]
fn report_document_capture_count_and_nodes_exact_and_one_over() {
    let fixture = Fixture::new(2);
    let size = fixture.body.len();
    let nodes = node_count(&fixture.body);
    for which in ["document", "capture", "count", "nodes"] {
        for over in [false, true] {
            let mut limits = ReportLimits::default();
            limits.json.max_file_bytes = size;
            limits.captured_bytes = 2 * size;
            limits.nodes = 2 * nodes;
            limits.references = 2;
            let expected = match which {
                "document" => {
                    limits.json.max_file_bytes -= usize::from(over);
                    "RP_E_RESOURCE_REPORT_SIZE_EXCEEDED"
                }
                "capture" => {
                    limits.captured_bytes -= usize::from(over);
                    "RP_E_RESOURCE_REPORT_CAPTURE_EXCEEDED"
                }
                "count" => {
                    limits.references -= usize::from(over);
                    "RP_E_RESOURCE_REPORT_COUNT_EXCEEDED"
                }
                "nodes" => {
                    limits.nodes -= usize::from(over);
                    "RP_E_RESOURCE_REPORT_NODES_EXCEEDED"
                }
                _ => unreachable!(),
            };
            let (reports, findings, _) = fixture.run(limits, 2 * size);
            if !over {
                assert!(findings.is_empty(), "{:?}", findings);
                assert_eq!(reports.candidates.len(), 2);
                assert_eq!((reports.remaining_capture, reports.remaining_nodes), (0, 0));
            } else {
                assert!(code(&findings, expected), "{which}: {:?}", findings);
                assert_eq!(
                    reports.candidates.len(),
                    if matches!(which, "count" | "document") {
                        0
                    } else {
                        1
                    }
                );
                if which == "nodes" {
                    assert_eq!(reports.remaining_nodes, 0);
                }
                if matches!(which, "count" | "document") {
                    assert_eq!(reports.remaining_capture, 2 * size);
                }
            }
        }
    }
}

#[test]
fn report_reuse_is_one_secure_read_and_ordinary_artifacts_never_capture() {
    let mut fixture = Fixture::new(1);
    fixture.snapshot(1, "art_00000000000000000000000000");
    fixture.artifact("art_z_science", b"stream-only");
    read_audit::reset();
    let (reports, findings, verified) =
        fixture.run(ReportLimits::default(), fixture.body.len() + 11);
    assert!(findings.is_empty(), "{:?}", findings);
    assert_eq!(reports.candidates.len(), 1);
    assert_eq!(verified.len(), 2);
    assert_eq!(
        read_audit::events(),
        vec![
            ("art_z_science".into(), false),
            ("art_00000000000000000000000000".into(), true)
        ]
    );
}

#[test]
fn science_first_wins_competing_actual_byte_allowance() {
    let mut fixture = Fixture::new(1);
    fixture.artifact("art_z_science", b"stream-only");
    let (reports, findings, verified) = fixture.run(ReportLimits::default(), fixture.body.len());
    assert!(code(&findings, "RP_E_RESOURCE_ARTIFACT_AGGREGATE_EXCEEDED"));
    assert!(verified.contains("art_z_science"));
    assert!(reports.candidates.is_empty());
    assert_eq!(
        reports.remaining_capture,
        ReportLimits::default().captured_bytes
    );
}

#[test]
fn syntax_schema_and_manifest_failure_never_refund_capture_or_parser_work() {
    for bad in [b"{\"a\":0,\"a\":1}".as_slice(), b"{}", b"{\"a\":"] {
        let mut fixture = Fixture::new(2);
        fixture.artifact("art_00000000000000000000000000", bad);
        let (reports, findings, _) = fixture.run(
            ReportLimits {
                captured_bytes: fixture.body.len() + bad.len() - 1,
                ..ReportLimits::default()
            },
            usize::MAX,
        );
        assert!(code(&findings, "RP_E_RESOURCE_REPORT_CAPTURE_EXCEEDED"));
        assert_eq!(reports.remaining_capture, fixture.body.len() - 1);
        assert!(reports.remaining_nodes < ReportLimits::default().nodes);
        assert!(reports.candidates.is_empty());
    }
    let mut fixture = Fixture::new(2);
    // Keep manifest unchanged while tampering the same-length bytes.
    let mut bad = fixture.body.clone();
    bad[0] = b'[';
    fs::write(fixture.root.join("art_00000000000000000000000000"), bad).unwrap();
    let (reports, findings, _) = fixture.run(
        ReportLimits {
            captured_bytes: fixture.body.len() * 2 - 1,
            ..ReportLimits::default()
        },
        usize::MAX,
    );
    assert!(code(&findings, "RP_E_ARTIFACT_DIGEST_MISMATCH"));
    assert!(code(&findings, "RP_E_RESOURCE_REPORT_CAPTURE_EXCEEDED"));
    assert_eq!(reports.remaining_nodes, ReportLimits::default().nodes);
    // Actual-byte artifact allowance also remains spent after verification failure.
    fixture.snapshot(2, "art_00000000000000000000000000");
    let (_, findings, _) = fixture.run(ReportLimits::default(), fixture.body.len() * 2 - 1);
    assert!(code(&findings, "RP_E_RESOURCE_ARTIFACT_AGGREGATE_EXCEEDED"));
}

#[test]
fn aggregate_parser_failures_preserve_nodes_and_stop_later_reads_at_zero() {
    let mut fixture = Fixture::new(2);
    fixture.artifact("art_00000000000000000000000000", b"{\"a\":0,\"a\":1}");
    read_audit::reset();
    let (reports, findings, _) = fixture.run(
        ReportLimits {
            nodes: 2,
            ..ReportLimits::default()
        },
        usize::MAX,
    );
    assert_eq!(reports.remaining_nodes, 0);
    assert!(code(&findings, "RP_E_REPORT_JSON_INVALID"));
    assert!(code(&findings, "RP_E_RESOURCE_REPORT_NODES_EXCEEDED"));
    assert_eq!(read_audit::events().len(), 1);
}

#[test]
fn post_hash_substitution_cannot_change_candidate_and_never_reopens_uri() {
    let fixture = Fixture::new(1);
    let id = "art_00000000000000000000000000";
    let object = fixture.index.get(id).unwrap();
    let mut remaining = fixture.body.len();
    let mut captured = fixture.body.len();
    let mut capture = ReportCapture {
        bytes: Vec::new(),
        max_bytes: fixture.body.len(),
        remaining: &mut captured,
    };
    read_audit::reset();
    let (size, digest) = secure_hash(
        &fixture.root,
        id,
        fixture.body.len(),
        &mut remaining,
        Some(&mut capture),
        &crate::ExecutionBudget::default(),
    )
    .unwrap();
    assert_eq!(size, fixture.body.len());
    assert_eq!(Some(digest.as_str()), object.value["sha256"].as_str());
    // The real secure descriptor+manifest checks precede substitution. No callback
    // pretends that a later pathname observation authenticates an earlier read.
    fs::rename(fixture.root.join(id), fixture.root.join("original")).unwrap();
    fs::write(fixture.root.join(id), b"forged replacement").unwrap();
    let mut findings = ContentFindings::new(false);
    let mut reports = Acquisition::new(&fixture.index, ReportLimits::default(), &mut findings);
    reports.admit(
        object,
        &capture.bytes,
        &SchemaBundle::new().unwrap(),
        &mut findings,
    );
    assert!(findings.output.is_empty());
    assert_eq!(
        reports.candidates[id].body(),
        &serde_json::from_slice::<Value>(&fixture.body).unwrap()
    );
    assert_eq!(read_audit::events(), vec![(id.into(), true)]);
}

#[test]
fn stat_observed_report_limits_reject_before_capture_allocation_or_io() {
    for (max, remaining, expected) in [
        (3, 9, SecureReadError::ReportTooLarge),
        (9, 3, SecureReadError::CaptureTooLarge),
    ] {
        let mut reader = resource_tests::CountingReader::new(b"1234");
        let mut aggregate = 9;
        let mut budget = remaining;
        let mut capture = ReportCapture {
            bytes: Vec::new(),
            max_bytes: max,
            remaining: &mut budget,
        };
        assert_eq!(
            metered_hash_capture(
                &mut reader,
                4,
                9,
                &mut aggregate,
                Some(&mut capture),
                &crate::ExecutionBudget::default()
            ),
            Err(expected)
        );
        assert_eq!(capture.bytes.capacity(), 0);
        assert!(reader.requests.is_empty());
        assert_eq!(aggregate, 9);
        assert_eq!(budget, remaining);
    }
}

#[test]
fn partial_io_failure_debits_both_actual_byte_meters_without_refund() {
    let mut reader = resource_tests::CountingReader::new(b"1234");
    reader.error_at = Some(3);
    let mut remaining = 4;
    let mut budget = 4;
    let mut capture = ReportCapture {
        bytes: Vec::new(),
        max_bytes: 4,
        remaining: &mut budget,
    };
    assert_eq!(
        metered_hash_capture(
            &mut reader,
            4,
            4,
            &mut remaining,
            Some(&mut capture),
            &crate::ExecutionBudget::default()
        ),
        Err(SecureReadError::Missing)
    );
    assert_eq!(capture.bytes, b"123");
    assert_eq!((remaining, budget), (1, 1));
}

#[test]
fn missing_and_https_references_count_before_any_report_read() {
    let mut fixture = Fixture::new(0);
    fixture.snapshot(0, "art_missing");
    fixture.artifact("art_remote", b"{}");
    fixture.record(
        "art_remote",
        ObjectType::Artifact,
        json!({"uri":"https://example.invalid/report"}),
    );
    fixture.snapshot(1, "art_remote");
    fixture.snapshot(2, "art_remote");
    read_audit::reset();
    let (reports, findings, _) = fixture.run(
        ReportLimits {
            references: 1,
            ..ReportLimits::default()
        },
        usize::MAX,
    );
    assert!(code(&findings, "RP_E_RESOURCE_REPORT_COUNT_EXCEEDED"));
    assert!(reports.candidates.is_empty());
    assert!(read_audit::events().is_empty());
    let (reports, findings, _) = fixture.run(
        ReportLimits {
            references: 2,
            ..ReportLimits::default()
        },
        usize::MAX,
    );
    assert!(!reports.overflow);
    assert!(code(&findings, "RP_E_REPORT_BODY_UNAVAILABLE"));
    assert!(reports.candidates.is_empty());
    assert!(read_audit::events().is_empty());
}

#[test]
fn lying_small_manifest_cannot_bypass_stat_capture_limits() {
    let mut fixture = Fixture::new(1);
    let id = "art_00000000000000000000000000";
    fixture.record(
        id,
        ObjectType::Artifact,
        json!({"uri":format!("file:{id}"),"size_bytes":1,"sha256":raw_sha256(&fixture.body)}),
    );
    for document in [true, false] {
        let mut limits = ReportLimits::default();
        if document {
            limits.json.max_file_bytes = fixture.body.len() - 1;
        } else {
            limits.captured_bytes = fixture.body.len() - 1;
        }
        let (reports, findings, verified) = fixture.run(limits, usize::MAX);
        assert!(code(
            &findings,
            if document {
                "RP_E_RESOURCE_REPORT_SIZE_EXCEEDED"
            } else {
                "RP_E_RESOURCE_REPORT_CAPTURE_EXCEEDED"
            }
        ));
        assert!(reports.candidates.is_empty());
        assert!(verified.is_empty());
        assert_eq!(
            reports.remaining_capture,
            if document {
                ReportLimits::default().captured_bytes
            } else {
                fixture.body.len() - 1
            }
        );
    }
}

#[test]
fn descriptor_change_after_hash_rejects_capture_and_retains_actual_charges() {
    let fixture = Fixture::new(2);
    let path = fixture.root.join("art_00000000000000000000000000");
    // This hook only mutates the file; the real production fstat comparison
    // remains the authority. It cannot synthesize a trusted read observation.
    read_audit::on_after_hash(move || {
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(b" ")
            .unwrap();
    });
    let (reports, findings, verified) =
        fixture.run(ReportLimits::default(), fixture.body.len() * 2 - 1);
    assert!(code(&findings, "RP_E_ARTIFACT_CHANGED_DURING_VERIFICATION"));
    assert!(code(&findings, "RP_E_RESOURCE_ARTIFACT_AGGREGATE_EXCEEDED"));
    assert_eq!(
        reports.remaining_capture,
        ReportLimits::default().captured_bytes - fixture.body.len()
    );
    assert_eq!(reports.remaining_nodes, ReportLimits::default().nodes);
    assert!(reports.candidates.is_empty());
    assert!(verified.is_empty());
}

#[test]
fn shared_failed_report_is_not_retried_or_recharged_and_report_order_is_by_id() {
    let mut fixture = Fixture::new(2);
    let first = "art_00000000000000000000000000";
    let second = "art_00000000000000000000000001";
    fixture.artifact(first, b"{}");
    fixture.snapshot(2, first);
    read_audit::reset();
    let (reports, findings, verified) = fixture.run(ReportLimits::default(), usize::MAX);
    assert_eq!(
        read_audit::events(),
        vec![(first.into(), true), (second.into(), true)]
    );
    assert_eq!(
        findings
            .iter()
            .filter(|f| f.error_code == "RP_E_REPORT_SCHEMA_INVALID")
            .count(),
        1
    );
    assert_eq!(reports.candidates.len(), 1);
    assert!(reports.candidates.contains_key(second));
    assert_eq!(
        reports.remaining_capture,
        ReportLimits::default().captured_bytes - 2 - fixture.body.len()
    );
    assert_eq!(
        reports.remaining_nodes,
        ReportLimits::default().nodes - 1 - node_count(&fixture.body)
    );
    assert_eq!(verified.len(), 2); // byte verification, not syntax/schema/binding
}
