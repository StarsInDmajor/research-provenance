//! Read-only per-(snapshot, artifact) runtime binding. No IO, writer or export gate.
use crate::findings::Findings;
use std::collections::BTreeMap;

use crate::report_subject::{SubjectBudget, SubjectUnavailable, expected_subject_report};
use crate::{ObjectType, ProjectIndex, report_acquisition::report_finding};

const MAX_PAIRS: usize = 256;

/// Completeness failures are not fabricated failed subject bodies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReportSubjectUnavailable {
    ExecutionStopped(crate::ExecutionStop),
    IndexIncomplete,
    Budget,
    ContentIncomplete,
    Attribution,
    WireContract,
}

/// Equality is orthogonal to subject validity, acquisition and safe labels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReportComparison {
    MatchedSubjectPassed,
    MatchedSubjectFailed,
    Mismatch,
    CandidateUnavailable,
    SourceSeparation,
    SubjectUnavailable(ReportSubjectUnavailable),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReportLabels {
    NotEvaluated,
    Accepted,
    Rejected,
}

/// A read-only observation for one admitted pair, not scientific truth or export
/// permission. MatchedSubjectFailed is honest equality but never chain success.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReportBindingObservation {
    pub comparison: ReportComparison,
    pub labels: ReportLabels,
}
impl ReportBindingObservation {
    pub(crate) fn permits_chain_valid(self) -> bool {
        self.comparison == ReportComparison::MatchedSubjectPassed
            && self.labels == ReportLabels::Accepted
    }
}

pub(crate) type Bindings = BTreeMap<Box<str>, (Box<str>, ReportBindingObservation)>;

pub(crate) fn bind_reports(index: &ProjectIndex, findings: &mut Findings) -> Bindings {
    bind_with_budget(index, findings, &mut SubjectBudget::default(), MAX_PAIRS)
}

pub(crate) fn bind_with_budget(
    index: &ProjectIndex,
    findings: &mut Findings,
    budget: &mut SubjectBudget,
    pair_limit: usize,
) -> Bindings {
    budget.findings = findings.fork();
    let mut pairs = Vec::new();
    // Bounded reference admission before any per-pair graph/access/profile pass.
    // Count pairs, not distinct artifacts; a shared artifact cannot bypass this cap.
    for snapshot in index
        .objects()
        .filter(|o| o.object_type == ObjectType::ClaimChain)
    {
        if findings.cancelled() {
            return BTreeMap::new();
        }
        let Some(artifact) = snapshot
            .value
            .get("validation_report")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        if pairs.len() == pair_limit.min(MAX_PAIRS) {
            findings.push(|| {
                report_finding(
                    snapshot,
                    "RP_E_RESOURCE_REPORT_PAIRS_EXCEEDED",
                    "resource_limit",
                    "report pair limit exceeded; binding batch not evaluated",
                )
            });
            return BTreeMap::new();
        }
        pairs.push((snapshot, artifact));
    }
    let mut bindings = BTreeMap::new();
    for (snapshot, artifact_id) in pairs {
        if findings.cancelled() {
            break;
        }
        let mut observation = ReportBindingObservation {
            comparison: ReportComparison::CandidateUnavailable,
            labels: ReportLabels::NotEvaluated,
        };
        // Evaluate separation even for missing/bad candidates. The subject builder
        // proves non-role reachability before content checks and never recursively
        // evaluates the report's own findings. Drop each expected body at loop end.
        match expected_subject_report(index, &snapshot.id, budget) {
            Err(reason) => {
                let (comparison, code, family) = unavailable(reason);
                observation.comparison = comparison;
                findings.push(|| {
                    report_finding(
                        snapshot,
                        code,
                        family,
                        "report subject binding evaluation unavailable",
                    )
                });
            }
            Ok(expected) => {
                if findings.cancelled() {
                    return BTreeMap::new();
                }
                if let Some(manifest) = index
                    .get(artifact_id)
                    .filter(|o| o.object_type == ObjectType::Artifact)
                {
                    let label = &snapshot.value["access"];
                    let compartments = label["compartments"]
                        .as_array()
                        .expect("schema-valid label");
                    let manifest_compartments = manifest.value["access"]["compartments"]
                        .as_array()
                        .expect("schema-valid label");
                    let label_work = compartments
                        .iter()
                        .chain(manifest_compartments)
                        .fold(0_usize, |n, v| {
                            n.saturating_add(v.as_str().map_or(0, str::len).saturating_add(1))
                        })
                        .saturating_mul(4 + usize::BITS as usize);
                    if budget.work(label_work).is_err() {
                        let (comparison, code, family) = unavailable(SubjectUnavailable::Budget);
                        observation.comparison = comparison;
                        findings.push(|| {
                            report_finding(
                                snapshot,
                                code,
                                family,
                                "report label comparison budget exceeded",
                            )
                        });
                        bindings.insert(
                            snapshot.id.as_ref().into(),
                            (artifact_id.into(), observation),
                        );
                        continue;
                    }
                    let sorted = compartments
                        .windows(2)
                        .all(|w| w[0].as_str() < w[1].as_str());
                    let covers = crate::AccessLevel::parse(label["level"].as_str().unwrap())
                        .is_some_and(|level| level >= expected.disclosure_floor.level)
                        && sorted
                        && expected
                            .disclosure_floor
                            .compartments
                            .iter()
                            .all(|required| {
                                compartments
                                    .binary_search_by(|v| {
                                        v.as_str().unwrap().cmp(required.as_str())
                                    })
                                    .is_ok()
                            });
                    observation.labels = if label == &manifest.value["access"] && sorted && covers {
                        ReportLabels::Accepted
                    } else {
                        findings.push(|| report_finding(
                            snapshot,
                            "RP_E_REPORT_LABEL_REJECTED",
                            "claim_chain_validation_report",
                            "snapshot/report labels must be equal and cover every subject dependency and consumed context",
                        ));
                        ReportLabels::Rejected
                    };
                }
                if let Some(candidate) = index.unbound_report_candidate(artifact_id) {
                    observation.comparison = if candidate.body() == &expected.body {
                        if expected.body["outcome"]["passed"] == true {
                            ReportComparison::MatchedSubjectPassed
                        } else {
                            ReportComparison::MatchedSubjectFailed
                        }
                    } else {
                        findings.push(|| report_finding(
                            snapshot,
                            "RP_E_CLAIM_CHAIN_VALIDATION_REPORT_MISMATCH",
                            "claim_chain_validation_report",
                            "report body differs from the complete independently evaluated subject",
                        ));
                        ReportComparison::Mismatch
                    };
                }
                // No new generic mismatch for acquisition failures: retain C2/ref
                // findings and the explicit CandidateUnavailable observation.
            }
        }
        if findings.cancelled() {
            return BTreeMap::new();
        }
        bindings.insert(
            snapshot.id.as_ref().into(),
            (artifact_id.into(), observation),
        );
    }
    bindings
}

fn unavailable(reason: SubjectUnavailable) -> (ReportComparison, &'static str, &'static str) {
    use ReportSubjectUnavailable as U;
    let (reason, code) = match reason {
        SubjectUnavailable::Stopped(stop) => {
            return (
                ReportComparison::SubjectUnavailable(U::ExecutionStopped(stop)),
                stop.finding().error_code,
                stop.finding().finding_family,
            );
        }
        SubjectUnavailable::OwnReport => {
            return (
                ReportComparison::SourceSeparation,
                "RP_E_REPORT_SOURCE_SEPARATION",
                "claim_chain_validation_report",
            );
        }
        SubjectUnavailable::Budget => {
            return (
                ReportComparison::SubjectUnavailable(U::Budget),
                "RP_E_RESOURCE_REPORT_SUBJECT_EXCEEDED",
                "resource_limit",
            );
        }
        SubjectUnavailable::IndexIncomplete => {
            (U::IndexIncomplete, "RP_E_REPORT_SUBJECT_INDEX_INCOMPLETE")
        }
        SubjectUnavailable::ContentIncomplete => (
            U::ContentIncomplete,
            "RP_E_REPORT_SUBJECT_CONTENT_INCOMPLETE",
        ),
        SubjectUnavailable::Attribution => (U::Attribution, "RP_E_REPORT_SUBJECT_ATTRIBUTION"),
        SubjectUnavailable::WireContract => (U::WireContract, "RP_E_REPORT_SUBJECT_WIRE_CONTRACT"),
    };
    (
        ReportComparison::SubjectUnavailable(reason),
        code,
        "claim_chain_validation_report",
    )
}
