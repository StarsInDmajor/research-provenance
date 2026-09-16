mod common;

use common::TempProject;
use rp_core::{ProjectLimits, validate_project, validate_project_with_limits};

fn limited(project: &TempProject, cap: usize) -> rp_core::ValidationReport {
    validate_project_with_limits(
        project.path(),
        ProjectLimits {
            findings: cap,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn tiny_exact_boundary_permutation_and_validation_isolation() {
    let mut outputs = Vec::new();
    for reverse in [false, true] {
        let project = TempProject::copy_fixture("overview-v1");
        for i in if reverse {
            vec![3, 2, 1, 0]
        } else {
            vec![0, 1, 2, 3]
        } {
            std::fs::write(
                project.research(&format!("records/questions/bad-{i}.yaml")),
                b"[invalid",
            )
            .unwrap();
        }
        let exact = limited(&project, 4);
        assert!(exact.is_complete() && exact.had_error());
        assert_eq!(exact.findings.len(), 4);
        assert!(!exact.is_valid());
        let overflow = limited(&project, 3);
        assert!(!overflow.is_complete() && overflow.had_error());
        assert_eq!(overflow.findings.len(), 3);
        outputs.push(overflow.findings);
        for cap in [0, 1] {
            let tiny = limited(&project, cap);
            assert!(tiny.had_error() && !tiny.is_complete() && !tiny.is_valid());
            assert_eq!(tiny.findings.len(), 1);
            assert_eq!(tiny.findings[0].error_code, "RP_W_FINDINGS_TRUNCATED");
            assert!(tiny.index.is_none());
        }
    }
    assert_eq!(outputs[0], outputs[1]);
    let clean = TempProject::copy_fixture("overview-v1");
    assert!(limited(&clean, 1).is_valid());
}

#[test]
fn canonical_content_access_and_claim_share_allowance() {
    let project = TempProject::copy_fixture("claim-chain-minimum-v1");
    let path = project.research(
        "records/questions/core-structural-question--qst_01J00000000000000000000010.yaml",
    );
    common::mutate_object(&path, |v| {
        v["narrative"] = serde_json::json!({"path":".research/notes/missing.md", "role":"detail", "sha256":format!("sha256:{}", "0".repeat(64))});
        v["access"]["compartments"] = serde_json::json!(["z", "a"]);
    });
    std::fs::rename(
        &path,
        path.with_file_name("wrong--qst_01J00000000000000000000099.yaml"),
    )
    .unwrap();
    let full = limited(&project, 1000);
    assert!(full.stage3_ran, "{:?}", full.findings);
    for family in [
        "object_identity",
        "narrative_integrity",
        "access_compartment_normalization",
        "source_closure_integrity",
    ] {
        assert!(
            full.findings.iter().any(|f| f.finding_family == family),
            "{family}: {:?}",
            full.findings
        );
    }
    let bounded = limited(&project, 2);
    assert_eq!(bounded.findings.len(), 2);
    assert!(!bounded.is_complete() && bounded.had_error());
    assert!(bounded.index.is_none());
}

#[test]
fn many_schema_instances_share_allowance_and_stop_before_stage3() {
    let project = TempProject::copy_fixture("overview-v1");
    for i in 0..20 {
        std::fs::write(
            project.research(&format!("records/questions/bad-{i:04}.yaml")),
            br#"{"schema":"rp/node-revision/v1", "kind":"Question"}"#,
        )
        .unwrap();
    }
    let result = limited(&project, 3);
    assert_eq!(result.findings.len(), 3);
    assert!(!result.is_complete() && result.had_error());
    assert!(!result.stage3_ran && result.index.is_none());
}

#[test]
fn malformed_files_share_one_default_findings_cap() {
    let project = TempProject::copy_fixture("overview-v1");
    for i in 0..1002 {
        std::fs::write(
            project.research(&format!("records/questions/bad-{i:04}.yaml")),
            b"[invalid",
        )
        .unwrap();
    }
    let report = validate_project(project.path()).unwrap();
    assert_eq!(report.findings.len(), 1000);
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|f| f.error_code == "RP_W_FINDINGS_TRUNCATED")
            .count(),
        1
    );
    assert!(!report.stage3_ran);
    assert!(report.index.is_none());
}
