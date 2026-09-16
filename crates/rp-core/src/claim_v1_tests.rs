//! Bounded v1 repairs exercised through real temporary projects, never frozen overlays.
use super::*;
use crate::validate_project;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

const CONF: &str = "claim-chain-confirmatory-v1";
const EVID: &str = "claim-chain-evidential-v1";
const RESULT: (&str, &str, &str) = (
    "RP_E_CONFIRMATORY_RESULT_REQUIRED",
    "confirmatory_required_result",
    "/node_revisions",
);
const EVALUATION: (&str, &str, &str) = (
    "RP_E_CONFIRMATORY_EVALUATION_REQUIRED",
    "confirmatory_scientific_evaluation",
    "/relation_revisions",
);
const SYNTHESIS: (&str, &str, &str) = (
    "RP_E_EVIDENTIAL_SYNTHESIS_REQUIRED",
    "evidential_synthesis_requirement",
    "/relation_revisions",
);

struct Project {
    root: PathBuf,
    chain: String,
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
impl Project {
    fn new(suite: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "rp-claim-v1-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fn copy(from: &Path, to: &Path) {
            fs::create_dir_all(to).unwrap();
            for entry in fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                let dest = to.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy(&entry.path(), &dest);
                } else {
                    fs::copy(entry.path(), dest).unwrap();
                }
            }
        }
        copy(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures")
                .join(suite)
                .join("valid"),
            &root,
        );
        let chain = id("cch", if suite == CONF { 53 } else { 52 });
        Self { root, chain }
    }
    fn index(&self) -> ProjectIndex {
        let r = validate_project(&self.root).unwrap();
        r.index.unwrap_or_else(|| panic!("{:?}", r.findings))
    }
    fn change(&self, id: &str, f: impl FnOnce(&mut Value)) {
        let index = self.index();
        let object = index.get(id).unwrap();
        let mut value = object.value.clone();
        f(&mut value);
        fs::write(
            self.root.join(object.source_file.as_str()),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
    fn clone_object(&self, old: &str, new: &str, f: impl FnOnce(&mut Value)) {
        let index = self.index();
        let object = index.get(old).unwrap();
        let logical = format!("claim-v1-{}", new.to_lowercase().replace('_', "-"));
        let path = Path::new(object.source_file.as_str())
            .parent()
            .unwrap()
            .join(format!("{logical}--{new}.yaml"));
        let mut value = object.value.clone();
        value["id"] = new.into();
        value["logical_id"] = logical.into();
        f(&mut value);
        let path = if value["kind"] == "Observation" {
            PathBuf::from(".research/records/observations").join(path.file_name().unwrap())
        } else {
            path
        };
        fs::create_dir_all(self.root.join(path.parent().unwrap())).unwrap();
        fs::write(
            self.root.join(path),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
    fn select(&self, field: &str, id: &str) {
        self.change(&self.chain, |c| {
            c[field].as_array_mut().unwrap().push(id.into())
        });
    }
    fn edge(&self, n: usize, from: &str, ty: &str, to: &str) {
        let new = id("rel", n);
        self.clone_object(&id("rel", 41), &new, |v| {
            v["from_revision"] = from.into();
            v["to_revision"] = to.into();
            v["type"] = ty.into();
        });
        self.select("relation_revisions", &new);
    }
    fn remove_evaluations(&self) {
        let index = self.index();
        self.change(&self.chain, |c| {
            c["relation_revisions"].as_array_mut().unwrap().retain(|r| {
                !matches!(
                    index.get(r.as_str().unwrap()).unwrap().value["type"].as_str(),
                    Some("supports" | "weakens" | "contradicts" | "consistent-with")
                )
            })
        });
    }
    fn assert_findings(&self, expected: &[(&str, &str, &str)]) -> ProjectIndex {
        // Independent source.* traversal and JCS oracle, not the production helper.
        let index = self.index();
        let chain = index.get(&self.chain).unwrap();
        let mut pending: Vec<String> = ["node_revisions", "relation_revisions"]
            .into_iter()
            .flat_map(|f| {
                chain.value[f]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
            })
            .collect();
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let o = index.get(&id).unwrap();
            entries.push(SourceClosureEntry {
                object_type: o.value["schema"].as_str().unwrap().into(),
                id: id.into(),
                canonical_digest: jcs_sha256(&o.value).unwrap().into(),
            });
            for field in ["revisions", "relations", "artifacts", "external_references"] {
                if let Some(ids) = o
                    .value
                    .pointer(&format!("/source/{field}"))
                    .and_then(Value::as_array)
                {
                    pending.extend(ids.iter().map(|id| id.as_str().unwrap().to_string()));
                }
            }
        }
        entries.sort_by(|a, b| (&a.object_type, &a.id).cmp(&(&b.object_type, &b.id)));
        let digest = jcs_sha256(&entries).unwrap();
        self.change(&self.chain, |c| {
            c["source_closure_sha256"] = digest.clone().into()
        });
        let report = validate_project(&self.root).unwrap();
        let index = report.index.unwrap();
        let actual = index.claim_chain_report(&self.chain).unwrap();
        assert_eq!(actual.source_entries, entries);
        assert_eq!(actual.source_sha256.as_ref(), digest);
        let mut tuples: Vec<_> = report
            .findings
            .iter()
            .map(|f| {
                assert_eq!(f.severity, Severity::Error);
                assert_eq!(
                    f.source_file.as_ref(),
                    Some(&index.get(&self.chain).unwrap().source_file)
                );
                (f.error_code, f.finding_family, f.json_pointer.as_str())
            })
            .collect();
        let mut expected = expected.to_vec();
        tuples.sort();
        expected.sort();
        assert_eq!(tuples, expected, "{:?}", report.findings);
        assert_eq!(actual.valid, expected.is_empty());
        index
    }
}
fn id(prefix: &str, n: usize) -> String {
    format!("{prefix}_01J{:023}", n)
}

#[test]
fn v1_flat_third_input_cannot_bypass_existing_synthesis_and_fixup_passes() {
    let p = Project::new(EVID);
    let d = id("meas", 90);
    p.clone_object(&id("meas", 24), &d, |_| {});
    p.select("node_revisions", &d);
    p.edge(90, &d, "supports", &id("con", 29));
    p.assert_findings(&[SYNTHESIS]);
    p.edge(91, &d, "input-to", &id("syn", 28));
    p.assert_findings(&[]);
}

#[test]
fn v1_confirmatory_connected_generated_path_does_not_replace_scientific_evaluation() {
    let p = Project::new(CONF);
    p.remove_evaluations();
    p.assert_findings(&[EVALUATION]);
}

#[test]
fn v1_confirmatory_paper_claim_and_backbone_do_not_replace_required_result() {
    let p = Project::new(CONF);
    let clm = id("clm", 90);
    // A real PaperClaim record already exists in the evidential fixture's shared data.
    let other = Project::new("overview-v1");
    let index = other.index();
    let template = index
        .objects()
        .find(|o| matches!(&o.object_type, ObjectType::Node(k) if k.as_ref() == "PaperClaim"))
        .unwrap();
    let mut value = template.value.clone();
    value["id"] = clm.clone().into();
    value["logical_id"] = "claim-v1-literature".into();
    // Keep the exact source reference ID, also present in this fixture.
    let path = p.root.join(format!(
        ".research/records/paper-claims/claim-v1-literature--{clm}.yaml"
    ));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    p.change(&p.chain, |c| {
        c["node_revisions"] = json!([
            id("qst", 10),
            id("hyp", 11),
            id("pred", 20),
            id("tst", 22),
            clm,
            id("con", 29)
        ]);
        c["relation_revisions"] = json!([id("rel", 41), id("rel", 47), id("rel", 48)]);
    });
    p.edge(90, &clm, "provides-prior", &id("tst", 22));
    p.edge(91, &clm, "supports", &id("con", 29));
    p.assert_findings(&[RESULT]);

    // With genuine mapped results present, a PaperClaim evaluation still cannot
    // replace the narrower confirmatory scientific-outcome source kinds.
    let original = Project::new(CONF).index();
    p.change(&p.chain, |c| {
        for field in ["node_revisions", "relation_revisions"] {
            c[field] = original.get(&p.chain).unwrap().value[field].clone();
        }
    });
    p.remove_evaluations();
    p.select("node_revisions", &clm);
    p.select("relation_revisions", &id("rel", 91));
    p.edge(92, &clm, "input-to", &id("syn", 28));
    p.assert_findings(&[EVALUATION]);
}

#[test]
fn v1_confirmatory_all_evaluation_types_sources_and_destinations_are_accepted() {
    for (source, destination, ty) in [
        (id("meas", 25), id("hyp", 11), "supports"),
        (id("meas", 25), id("pred", 20), "weakens"),
        (id("syn", 28), id("con", 29), "contradicts"),
        (id("syn", 28), id("pred", 20), "consistent-with"),
    ] {
        let p = Project::new(CONF);
        p.remove_evaluations();
        p.edge(90, &source, ty, &destination);
        p.assert_findings(&[]);
    }
}

#[test]
fn v1_confirmatory_inactive_unselected_duplicate_and_wrong_target_evaluations() {
    for mode in ["inactive", "unselected", "duplicate", "separate-target"] {
        let p = Project::new(CONF);
        p.remove_evaluations();
        let target = if mode == "separate-target" {
            let c = id("con", 90);
            p.clone_object(&id("con", 29), &c, |_| {});
            p.select("node_revisions", &c);
            p.edge(92, &c, "derived-from", &id("con", 29));
            c
        } else {
            id("hyp", 11)
        };
        p.edge(90, &id("syn", 28), "supports", &target);
        match mode {
            "inactive" => {
                p.change(&id("rel", 90), |r| {
                    r["relation_state"] = "invalidated".into()
                });
                p.assert_findings(&[(
                    "RP_E_CLAIM_CHAIN_RELATION_NOT_ACTIVE",
                    "relation_state",
                    "/relation_revisions",
                )]);
            }
            "unselected" => {
                p.change(&p.chain, |c| {
                    c["relation_revisions"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|r| r != &id("rel", 90))
                });
                p.assert_findings(&[EVALUATION]);
            }
            "duplicate" => {
                p.edge(91, &id("syn", 28), "supports", &target);
                p.assert_findings(&[]);
            }
            _ => {
                p.assert_findings(&[EVALUATION]);
            }
        }
    }
}

#[test]
fn v1_flat_duplicate_source_and_inactive_or_unselected_fixup_do_not_hide_bypass() {
    for mode in ["duplicate", "inactive", "unselected"] {
        let p = Project::new(EVID);
        let d = id("meas", 90);
        p.clone_object(&id("meas", 24), &d, |_| {});
        p.select("node_revisions", &d);
        p.edge(90, &d, "supports", &id("con", 29));
        if mode == "duplicate" {
            p.edge(91, &id("meas", 24), "input-to", &id("syn", 28));
            p.assert_findings(&[SYNTHESIS]);
        } else {
            p.edge(91, &d, "input-to", &id("syn", 28));
            if mode == "inactive" {
                p.change(&id("rel", 91), |r| {
                    r["relation_state"] = "invalidated".into()
                });
                p.assert_findings(&[(
                    "RP_E_CLAIM_CHAIN_RELATION_NOT_ACTIVE",
                    "relation_state",
                    "/relation_revisions",
                )]);
            } else {
                p.change(&p.chain, |c| {
                    c["relation_revisions"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|r| r != &id("rel", 91))
                });
                p.assert_findings(&[SYNTHESIS]);
            }
        }
    }
}

#[test]
fn v1_nested_measurement_and_multiple_contexts_keep_existing_acceptance() {
    for mode in [
        "nested",
        "nested-observation",
        "separate-output",
        "separate-evaluation",
        "shared-input",
    ] {
        let p = Project::new(EVID);
        let d = id(
            if mode == "nested-observation" {
                "obs"
            } else {
                "meas"
            },
            90,
        );
        let s = id("syn", 90);
        p.clone_object(&id("meas",24), &d, |v| {
            if mode == "nested-observation" {
                v["kind"] = "Observation".into(); v.as_object_mut().unwrap().remove("measurement");
                v["observation"] = json!({"context":"Synthetic intermediate", "method":{"description":"Repeat protocol"}, "reproducibility_criteria":["Repeat selection"]});
            }
        });
        p.select("node_revisions", &d);
        p.edge(90, &d, "supports", &id("con", 29));
        if mode.starts_with("nested") || mode == "shared-input" {
            p.clone_object(&id("syn", 28), &s, |_| {});
            p.select("node_revisions", &s);
            p.edge(91, &id("meas", 24), "input-to", &s);
            let output = if mode.starts_with("nested") {
                d.clone()
            } else {
                id("con", 29)
            };
            p.edge(92, &s, "generated", &output);
            if mode.starts_with("nested") {
                p.edge(93, &d, "input-to", &id("syn", 28));
            }
        } else if mode == "separate-output" {
            let c = id("con", 90);
            p.clone_object(&id("con", 29), &c, |_| {});
            p.select("node_revisions", &c);
            p.edge(91, &id("syn", 28), "generated", &c);
        } else {
            p.edge(91, &d, "contradicts", &id("hyp", 14));
        }
        p.assert_findings(&[]);
    }
}

#[test]
fn v1_confirmatory_dataset_with_backbone_has_no_result_and_no_absence_cascade() {
    let p = Project::new(CONF);
    p.change(&p.chain, |c| {
        c["node_revisions"] = json!([
            id("qst", 10),
            id("hyp", 11),
            id("pred", 20),
            id("tst", 22),
            id("dset", 8),
            id("con", 29)
        ]);
        c["relation_revisions"] = json!([id("rel", 41), id("rel", 47), id("rel", 48)]);
    });
    p.edge(90, &id("dset", 8), "provides-prior", &id("tst", 22));
    p.edge(91, &id("dset", 8), "derived-from", &id("con", 29));
    p.assert_findings(&[RESULT]);
}

#[test]
fn v1_missing_evaluation_retains_independent_result_mapping_error() {
    let p = Project::new(CONF);
    p.remove_evaluations();
    p.change(&p.chain, |c| {
        c["relation_revisions"]
            .as_array_mut()
            .unwrap()
            .retain(|r| r != &id("rel", 50))
    });
    p.assert_findings(&[
        EVALUATION,
        (
            "RP_E_CONFIRMATORY_RESULT_TEST_REQUIRED",
            "confirmatory_result_mapping",
            "/relation_revisions",
        ),
    ]);
}

#[test]
fn v1_honest_failed_expected_body_binds_but_forged_pass_does_not() {
    use crate::report_subject::{SubjectBudget, expected_subject_report};
    let p = Project::new(CONF);
    p.remove_evaluations();
    p.assert_findings(&[EVALUATION]);
    let report_id = id("art", 999);
    p.change(&p.chain, |c| {
        c["validation_report"] = report_id.clone().into()
    });
    let index = p.index();
    let expected = expected_subject_report(&index, &p.chain, &mut SubjectBudget::default())
        .unwrap()
        .body;
    assert_eq!(
        expected["outcome"],
        json!({"passed":false, "findings":[{
            "owner":{"kind":"object","id":p.chain}, "error_code":EVALUATION.0,
            "finding_family":EVALUATION.1, "severity":"error", "json_pointer":EVALUATION.2
        }]})
    );
    let install = |body: &Value| {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(body).unwrap();
        fs::write(p.root.join("report.json"), &bytes).unwrap();
        fs::create_dir_all(p.root.join(".research/artifacts")).unwrap();
        fs::write(p.root.join(format!(".research/artifacts/report--{report_id}.yaml")), serde_json::to_vec(&json!({
            "schema":"rp/artifact-manifest/v1", "id":report_id, "title":"Report", "uri":"file:report.json",
            "media_type":"application/json", "size_bytes":bytes.len(), "sha256":format!("sha256:{:x}",Sha256::digest(&bytes)),
            "created_at":"2026-02-05T00:00:00Z", "access":{"level":"internal","compartments":[]}
        })).unwrap()).unwrap();
    };
    install(&expected);
    let index = p.assert_findings(&[EVALUATION]);
    let binding = index.report_binding(&p.chain, &report_id).unwrap();
    assert_eq!(
        binding.comparison,
        crate::ReportComparison::MatchedSubjectFailed
    );
    assert_eq!(binding.labels, crate::ReportLabels::Accepted);
    let mut forged = expected;
    forged["outcome"] = json!({"passed":true,"findings":[]});
    install(&forged);
    let index = p.assert_findings(&[
        EVALUATION,
        (
            "RP_E_CLAIM_CHAIN_VALIDATION_REPORT_MISMATCH",
            "claim_chain_validation_report",
            "/validation_report",
        ),
    ]);
    assert_eq!(
        index
            .report_binding(&p.chain, &report_id)
            .unwrap()
            .comparison,
        crate::ReportComparison::Mismatch
    );
}

#[test]
fn v1_unselected_evaluation_source_keeps_primary_endpoint_error() {
    let p = Project::new(CONF);
    p.remove_evaluations();
    let s = id("syn", 90);
    p.clone_object(&id("syn", 28), &s, |_| {});
    p.edge(90, &s, "supports", &id("hyp", 11));
    p.assert_findings(&[(
        "RP_E_CLAIM_CHAIN_ENDPOINT_NOT_SELECTED",
        "endpoint_containment",
        "/node_revisions",
    )]);
}

#[test]
fn v1_flat_helper_and_profile_obey_shared_findings_cancellation() {
    let p = Project::new(CONF);
    p.remove_evaluations();
    let index = p.assert_findings(&[EVALUATION]);
    let mut findings = Findings::new(1);
    for _ in 0..2 {
        findings.push(|| {
            chain_finding(
                index.get(&p.chain).unwrap(),
                EVALUATION.0,
                EVALUATION.1,
                "probe",
                EVALUATION.2,
            )
        });
    }
    assert!(findings.cancelled());
    let nodes = string_array(&index.get(&p.chain).unwrap().value, "/node_revisions");
    let relations = string_array(&index.get(&p.chain).unwrap().value, "/relation_revisions");
    assert!(!flat_synthesis_bypass(
        &index,
        &nodes,
        &relations,
        &id("con", 29),
        &findings
    ));
    let mut child = findings.fork();
    validate_confirmatory(
        &index,
        index.get(&p.chain).unwrap(),
        &nodes,
        &relations,
        &BTreeSet::new(),
        &mut child,
    );
    assert!(child.is_empty());
    assert!(child.cancelled());
}

#[test]
fn v1_backbone_kind_presence_is_explicit_even_without_relation_compatibility_pass() {
    let p = Project::new(CONF);
    p.remove_evaluations();
    p.edge(90, &id("syn", 28), "supports", &id("con", 29));
    let original = p.assert_findings(&[]);
    for missing in [id("hyp", 11), id("pred", 20), id("tst", 22)] {
        // Defensive helper contract: normally the project relation-kind validator
        // also diagnoses this. No invalid kind sequence may establish a backbone.
        let mut index = original.clone();
        let mut object = index.get(&missing).unwrap().clone();
        object.object_type = ObjectType::Node("Method".into());
        index.insert(object);
        if missing.starts_with("tst") {
            let mut other = index.get(&id("tst", 23)).unwrap().clone();
            other.object_type = ObjectType::Node("Method".into());
            index.insert(other);
        }
        let chain = index.get(&p.chain).unwrap();
        let nodes = string_array(&chain.value, "/node_revisions");
        let relations = string_array(&chain.value, "/relation_revisions");
        let verified = original
            .objects()
            .filter(|o| original.artifact_verified(&o.id))
            .map(|o| o.id.as_ref().into())
            .collect();
        let mut findings = Findings::default();
        validate_confirmatory(&index, chain, &nodes, &relations, &verified, &mut findings);
        let mut actual: Vec<_> = findings
            .iter()
            .map(|f| {
                assert_eq!(f.source_file.as_ref(), Some(&chain.source_file));
                (f.error_code, f.finding_family, f.json_pointer.as_str())
            })
            .collect();
        let mut expected = vec![(
            "RP_E_CONFIRMATORY_BACKBONE_MISSING",
            "confirmatory_backbone",
            "/relation_revisions",
        )];
        if missing.starts_with("tst") {
            expected.extend(
                [(
                    "RP_E_CONFIRMATORY_RESULT_TEST_REQUIRED",
                    "confirmatory_result_mapping",
                    "/relation_revisions",
                ); 4],
            );
        }
        actual.sort();
        expected.sort();
        assert_eq!(actual, expected);
    }
}
