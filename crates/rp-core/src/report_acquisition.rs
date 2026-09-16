//! C2 observations only. A schema-valid body is never a subject/binding verdict.
use crate::content_observations::ContentFindings;
use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::report_json::parse_report_json_with_budget;
use crate::{
    Finding, ObjectRecord, ObjectType, ProjectIndex, ReportJsonError, ReportJsonLimits,
    SchemaBundle, Severity,
};

/// Local manifest-verified, strict/schema-valid report body; NOT bound to any snapshot.
/// Synthetic or forged outcomes can inhabit this type. No raw bytes are retained.
#[derive(Clone, Debug)]
pub struct UnboundReportCandidate {
    pub(crate) body: Value,
}

impl UnboundReportCandidate {
    #[must_use]
    pub fn body(&self) -> &Value {
        &self.body
    }
}

pub(crate) struct ReportLimits {
    pub json: ReportJsonLimits,
    pub captured_bytes: usize,
    pub nodes: usize,
    pub references: usize,
}

impl Default for ReportLimits {
    fn default() -> Self {
        Self {
            json: ReportJsonLimits::default(),
            captured_bytes: 16 * 1024 * 1024,
            nodes: 524_288,
            references: 256,
        }
    }
}

pub(crate) struct Acquisition<'a> {
    ids: BTreeSet<&'a str>,
    pub overflow: bool,
    pub limits: ReportLimits,
    pub remaining_capture: usize,
    pub remaining_nodes: usize,
    pub candidates: BTreeMap<Box<str>, UnboundReportCandidate>,
}

impl<'a> Acquisition<'a> {
    pub fn new(
        index: &'a ProjectIndex,
        mut limits: ReportLimits,
        findings: &mut ContentFindings,
    ) -> Self {
        let ceiling = ReportLimits::default();
        limits.captured_bytes = limits.captured_bytes.min(ceiling.captured_bytes);
        limits.nodes = limits.nodes.min(ceiling.nodes);
        limits.references = limits.references.min(ceiling.references);
        limits.json.max_file_bytes = limits.json.max_file_bytes.min(ceiling.json.max_file_bytes);
        let mut ids = BTreeSet::new();
        let mut overflow = false;
        for snapshot in index
            .objects()
            .filter(|o| o.object_type == ObjectType::ClaimChain)
        {
            if let Some(id) = snapshot
                .value
                .get("validation_report")
                .and_then(Value::as_str)
            {
                if ids.contains(id) {
                    continue;
                }
                if ids.len() == limits.references {
                    findings.push(|| report_finding(
                        snapshot,
                        "RP_E_RESOURCE_REPORT_COUNT_EXCEEDED",
                        "resource_limit",
                        "distinct report reference limit exceeded; artifact acquisition aborted",
                    ));
                    overflow = true;
                    break;
                }
                ids.insert(id);
            }
        }
        Self {
            ids,
            overflow,
            remaining_capture: limits.captured_bytes,
            remaining_nodes: limits.nodes,
            limits,
            candidates: BTreeMap::new(),
        }
    }

    pub fn is_report(&self, id: &str) -> bool {
        self.ids.contains(id)
    }

    // Called only after descriptor stability and manifest size/digest verification.
    // Deliberately accepts bytes, not a URI or callback capable of reopening it.
    pub fn admit(
        &mut self,
        object: &ObjectRecord,
        bytes: &[u8],
        bundle: &SchemaBundle,
        findings: &mut ContentFindings,
    ) {
        if findings.cancelled() {
            return;
        }
        let body = match parse_report_json_with_budget(
            bytes,
            &self.limits.json,
            &mut self.remaining_nodes,
            &findings.output.execution,
        ) {
            Ok(body) => body,
            Err(error) => {
                let (code, family) = parser_mapping(error);
                findings.push(|| report_finding(object, code, family, &error.to_string()));
                return;
            }
        };
        if body.get("schema").and_then(Value::as_str) != Some("rp/claim-chain-validation-report/v1")
            || !bundle.is_valid_with_budget(&body, &findings.output.execution)
        {
            findings.push(|| {
                report_finding(
                    object,
                    "RP_E_REPORT_SCHEMA_INVALID",
                    "claim_chain_validation_report",
                    "report body does not satisfy the derived report wire schema",
                )
            });
            return;
        }
        if findings.cancelled() {
            findings.incomplete();
            return;
        }
        self.candidates
            .insert(object.id.as_ref().into(), UnboundReportCandidate { body });
    }
}

pub(crate) fn report_finding(
    object: &ObjectRecord,
    code: &'static str,
    family: &'static str,
    message: &str,
) -> Finding {
    Finding::new(
        code,
        family,
        Severity::Error,
        message,
        Some(object.source_file.clone()),
        if object.object_type == ObjectType::ClaimChain {
            "/validation_report"
        } else {
            "/uri"
        },
    )
}

fn parser_mapping(error: ReportJsonError) -> (&'static str, &'static str) {
    use ReportJsonError::*;
    match error {
        Stopped(crate::ExecutionStop::Interrupted) => ("RP_E_INTERRUPTED", "interrupted"),
        Stopped(crate::ExecutionStop::Deadline) => {
            ("RP_E_RESOURCE_DEADLINE_EXCEEDED", "resource_limit")
        }
        DocumentTooLarge => ("RP_E_RESOURCE_REPORT_SIZE_EXCEEDED", "resource_limit"),
        DepthExceeded => ("RP_E_RESOURCE_NESTING_DEPTH_EXCEEDED", "resource_limit"),
        ScalarTooLarge => ("RP_E_RESOURCE_SCALAR_LENGTH_EXCEEDED", "resource_limit"),
        TooManyMappingKeys => ("RP_E_RESOURCE_MAPPING_KEYS_EXCEEDED", "resource_limit"),
        TooManySequenceItems => ("RP_E_RESOURCE_SEQUENCE_ITEMS_EXCEEDED", "resource_limit"),
        TooManyNodes => ("RP_E_RESOURCE_REPORT_NODES_EXCEEDED", "resource_limit"),
        BomForbidden | InvalidUtf8 | InvalidJson | RootNotMapping | DuplicateKey => {
            ("RP_E_REPORT_JSON_INVALID", "claim_chain_validation_report")
        }
    }
}
