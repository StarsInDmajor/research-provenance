use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::OnceLock,
};

use jsonschema::{Draft, Registry};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    Finding, ObjectType, ProjectIndex, ProjectLimits, ProjectPath, ReferenceEdge,
    ReferenceExpectation, SchemaBundle, Severity, YamlLimits,
};

use crate::report_acquisition::{
    Acquisition, ReportLimits, UnboundReportCandidate, report_finding,
};

use crate::content_observations::{Completeness, ContentFindings, ObservationStore, Owner, Phase};

pub(crate) struct ContentValidation {
    pub observations: Option<ObservationStore>,
    pub findings: crate::findings::Findings,
    pub verified_artifacts: BTreeSet<Box<str>>,
    pub unbound_reports: BTreeMap<Box<str>, UnboundReportCandidate>,
    pub extension_references: Vec<ReferenceEdge>,
    pub extension_references_exhausted: bool,
    pub freshness_policy: Option<Value>,
}

#[cfg(test)]
pub(crate) fn validate_content(
    root: &Path,
    index: &ProjectIndex,
    limits: ProjectLimits,
    schema_bundle: &SchemaBundle,
    remaining_edges: usize,
) -> ContentValidation {
    let tracking = index.objects().any(|o| {
        o.object_type == ObjectType::ClaimChain
            && o.value
                .get("validation_report")
                .is_some_and(|v| !v.is_null())
    });
    validate_content_with_observations(
        root,
        index,
        limits,
        schema_bundle,
        remaining_edges,
        ContentFindings::new(tracking),
    )
}

pub(crate) fn validate_content_with_observations(
    root: &Path,
    index: &ProjectIndex,
    limits: ProjectLimits,
    schema_bundle: &SchemaBundle,
    remaining_edges: usize,
    mut findings: ContentFindings,
) -> ContentValidation {
    let mut verified_artifacts = BTreeSet::new();
    let freshness_policy = validate_policies(root, index, limits, schema_bundle, &mut findings);
    validate_narratives(root, index, limits, &mut findings);
    findings.begin(Owner::Outer, Phase::OuterReport, None, None, &[]);
    let mut reports = Acquisition::new(index, ReportLimits::default(), &mut findings);
    validate_artifacts(
        root,
        index,
        limits,
        &mut findings,
        &mut verified_artifacts,
        &mut reports,
        schema_bundle,
    );
    let (extension_references, extension_references_exhausted) =
        validate_extensions(root, index, limits, &mut findings, remaining_edges);
    validate_event_provenance(index, &mut findings);
    if findings.cancelled() {
        findings.incomplete();
    }
    ContentValidation {
        observations: findings.store,
        findings: findings.output,
        verified_artifacts,
        unbound_reports: reports.candidates,
        extension_references,
        extension_references_exhausted,
        freshness_policy,
    }
}

fn validate_policies(
    root: &Path,
    index: &ProjectIndex,
    limits: ProjectLimits,
    schema_bundle: &SchemaBundle,
    findings: &mut ContentFindings,
) -> Option<Value> {
    let mut freshness_policy = None;
    let Some(project) = index
        .object_values()
        .find(|object| matches!(object.object_type, ObjectType::Project))
    else {
        return freshness_policy;
    };
    for field in ["redaction_policy", "validation_policy"] {
        if findings.cancelled() {
            break;
        }
        let Some(path) = project.value.get(field).and_then(Value::as_str) else {
            continue;
        };
        findings.context(path, Phase::Policy, None, std::slice::from_ref(&project.id));
        match secure_read(
            root,
            path,
            limits.yaml_bytes_per_object,
            &findings.output.execution,
        ) {
            Ok(bytes) => {
                findings.state(Completeness::Verified);
                findings.hash(&raw_sha256(&bytes));
                let source_path = match ProjectPath::new(path.to_string()) {
                    Ok(path) => path,
                    Err(_) => continue,
                };
                let parsed = match crate::yaml::parse_restricted_yaml_with_budget(
                    &bytes,
                    source_path.clone(),
                    &YamlLimits::default(),
                    &findings.output.execution,
                ) {
                    Ok(parsed) => parsed.value,
                    Err(policy_finding) => {
                        findings.push(|| policy_finding);
                        continue;
                    }
                };
                if parsed.get("schema").and_then(Value::as_str) == Some("rp/freshness-policy/v1") {
                    let mut policy_findings = findings.output.fork();
                    if schema_bundle.validate_into(&parsed, source_path, &mut policy_findings) {
                        freshness_policy = Some(parsed);
                    }
                    findings.absorb(policy_findings);
                }
            }
            Err(error) => {
                findings.state(read_completeness(error));
                let (code, family, message) = match error {
                    SecureReadError::Stopped(_) => {
                        findings.incomplete();
                        break;
                    }
                    SecureReadError::UnsafePath => (
                        "RP_E_PATH_PROJECT_ESCAPE",
                        "path_containment",
                        "policy path escapes or encodes traversal outside the Project",
                    ),
                    SecureReadError::Symlink => (
                        "RP_E_PATH_SYMLINK_FORBIDDEN",
                        "path_containment",
                        "policy paths may not traverse symlinks",
                    ),
                    SecureReadError::NonRegular => (
                        "RP_E_PATH_NON_REGULAR_FILE",
                        "path_containment",
                        "policy path is not a regular file",
                    ),
                    SecureReadError::TooLarge
                    | SecureReadError::AggregateTooLarge
                    | SecureReadError::ReportTooLarge
                    | SecureReadError::CaptureTooLarge => (
                        "RP_E_RESOURCE_FILE_SIZE_EXCEEDED",
                        "resource_limit",
                        "policy file exceeds the bounded byte limit",
                    ),
                    SecureReadError::Changed | SecureReadError::Missing => (
                        "RP_E_POLICY_NOT_FOUND",
                        "policy_integrity",
                        "project policy was not found as a stable contained file",
                    ),
                };
                findings.push(|| {
                    Finding::new(
                        code,
                        family,
                        Severity::Error,
                        message,
                        Some(project.source_file.clone()),
                        format!("/{field}"),
                    )
                });
            }
        }
    }
    freshness_policy
}

fn validate_narratives(
    root: &Path,
    index: &ProjectIndex,
    limits: ProjectLimits,
    findings: &mut ContentFindings,
) {
    for object in index.object_values() {
        if findings.cancelled() {
            break;
        }
        let Some(narrative) = object.value.get("narrative") else {
            continue;
        };
        let Some(path) = narrative.get("path").and_then(Value::as_str) else {
            continue;
        };
        findings.begin(
            Owner::Object(object.id.clone()),
            Phase::Narrative,
            Some(path),
            None,
            &[],
        );
        match secure_read(
            root,
            path,
            limits.yaml_bytes_per_object,
            &findings.output.execution,
        ) {
            Ok(bytes) => {
                findings.state(Completeness::Verified);
                findings.hash(&raw_sha256(&bytes));
                let expected = narrative.get("sha256").and_then(Value::as_str);
                if expected != Some(raw_sha256(&bytes).as_str()) {
                    findings.push(|| {
                        Finding::new(
                            "RP_E_NARRATIVE_DIGEST_MISMATCH",
                            "narrative_integrity",
                            Severity::Error,
                            "narrative digest does not match the contained file bytes",
                            Some(object.source_file.clone()),
                            "/narrative/sha256",
                        )
                    });
                }
            }
            Err(error) => {
                findings.state(read_completeness(error));
                findings.push(|| {
                    path_or_content_finding(
                        error,
                        "RP_E_NARRATIVE_NOT_FOUND",
                        "narrative_integrity",
                        object,
                        "/narrative/path",
                    )
                });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_artifacts(
    root: &Path,
    index: &ProjectIndex,
    limits: ProjectLimits,
    findings: &mut ContentFindings,
    verified: &mut BTreeSet<Box<str>>,
    reports: &mut Acquisition<'_>,
    schema_bundle: &SchemaBundle,
) {
    // The bounded report-ID set is incomplete on overflow. Abort the entire
    // artifact phase rather than classify artifacts with repeated index scans.
    // Independent non-artifact validation continues in validate_content.
    if reports.overflow {
        findings.incomplete();
        return;
    }
    let mut remaining = limits.local_artifact_aggregate_bytes;
    // BTreeMap iteration is stable ID order within each class; never allocate
    // a second artifact list or read a shared report once per snapshot.
    for report_pass in [false, true] {
        for object in index.object_values() {
            if findings.cancelled() {
                break;
            }
            if object.object_type != ObjectType::Artifact
                || reports.is_report(&object.id) != report_pass
            {
                continue;
            }
            let Some(uri) = object.value.get("uri").and_then(Value::as_str) else {
                continue;
            };
            findings.begin(
                Owner::Object(object.id.clone()),
                if report_pass {
                    Phase::OuterReport
                } else {
                    Phase::Artifact
                },
                uri.strip_prefix("file:"),
                None,
                &[],
            );
            if uri.starts_with("https://") {
                findings.state(if report_pass {
                    Completeness::Unavailable
                } else {
                    Completeness::IdentifierOnly
                });
                if report_pass {
                    findings.push(|| {
                        report_finding(
                            object,
                            "RP_E_REPORT_BODY_UNAVAILABLE",
                            "claim_chain_validation_report",
                            "HTTPS report is identifier-only; local report body unavailable",
                        )
                    });
                }
                continue;
            }
            let Some(path) = uri.strip_prefix("file:") else {
                continue;
            };
            let declared_size = object
                .value
                .get("size_bytes")
                .and_then(Value::as_u64)
                .and_then(|size| usize::try_from(size).ok())
                .unwrap_or(usize::MAX);
            if declared_size > limits.local_artifact_bytes_per_item {
                findings.push(|| {
                    Finding::new(
                        "RP_E_RESOURCE_ARTIFACT_SIZE_EXCEEDED",
                        "resource_limit",
                        Severity::Error,
                        "artifact byte limit exceeded",
                        Some(object.source_file.clone()),
                        "/size_bytes",
                    )
                });
                continue;
            }
            if declared_size > remaining {
                findings.incomplete();
                findings.push(|| {
                    Finding::new(
                        "RP_E_RESOURCE_ARTIFACT_AGGREGATE_EXCEEDED",
                        "resource_limit",
                        Severity::Error,
                        "aggregate artifact byte limit exceeded",
                        Some(object.source_file.clone()),
                        "/size_bytes",
                    )
                });
                break;
            }
            if report_pass {
                let rejected = if declared_size > reports.limits.json.max_file_bytes {
                    Some((
                        "RP_E_RESOURCE_REPORT_SIZE_EXCEEDED",
                        "report document byte limit exceeded",
                    ))
                } else if declared_size > reports.remaining_capture {
                    Some((
                        "RP_E_RESOURCE_REPORT_CAPTURE_EXCEEDED",
                        "aggregate report capture byte limit exceeded",
                    ))
                } else if reports.remaining_nodes == 0 {
                    Some((
                        "RP_E_RESOURCE_REPORT_NODES_EXCEEDED",
                        "aggregate report node limit exhausted",
                    ))
                } else {
                    None
                };
                if let Some((code, message)) = rejected {
                    findings.push(|| report_finding(object, code, "resource_limit", message));
                    continue;
                }
            }
            let mut capture = ReportCapture {
                bytes: Vec::new(),
                max_bytes: reports.limits.json.max_file_bytes,
                remaining: &mut reports.remaining_capture,
            };
            match secure_hash(
                root,
                path,
                limits.local_artifact_bytes_per_item,
                &mut remaining,
                report_pass.then_some(&mut capture),
                &findings.output.execution,
            ) {
                Ok((observed_size, observed_digest)) => {
                    findings.state(Completeness::Verified);
                    findings.hash(&observed_digest);
                    let mut valid = true;
                    if observed_size != declared_size {
                        valid = false;
                        findings.push(|| {
                            Finding::new(
                                "RP_E_ARTIFACT_SIZE_MISMATCH",
                                "artifact_integrity",
                                Severity::Error,
                                "artifact size does not match size_bytes",
                                Some(object.source_file.clone()),
                                "/size_bytes",
                            )
                        });
                    }
                    let expected = object.value.get("sha256").and_then(Value::as_str);
                    if expected != Some(observed_digest.as_str()) {
                        valid = false;
                        findings.push(|| {
                            Finding::new(
                                "RP_E_ARTIFACT_DIGEST_MISMATCH",
                                "artifact_integrity",
                                Severity::Error,
                                "artifact SHA-256 does not match the manifest",
                                Some(object.source_file.clone()),
                                "/sha256",
                            )
                        });
                    }
                    if valid {
                        verified.insert(object.id.as_ref().into());
                        if report_pass {
                            let bytes = capture.bytes;
                            reports.admit(object, &bytes, schema_bundle, findings);
                        }
                    }
                }
                Err(error) => {
                    findings.state(read_completeness(error));
                    findings.push(|| {
                        path_or_content_finding(
                            error,
                            "RP_E_ARTIFACT_NOT_FOUND",
                            "artifact_integrity",
                            object,
                            "/uri",
                        )
                    });
                }
            }
        }
    }
}

fn validate_extensions(
    root: &Path,
    index: &ProjectIndex,
    limits: ProjectLimits,
    findings: &mut ContentFindings,
    remaining_edges: usize,
) -> (Vec<ReferenceEdge>, bool) {
    let mut extension_references = Vec::new();
    let Some(project) = index
        .object_values()
        .find(|object| matches!(object.object_type, ObjectType::Project))
    else {
        return (extension_references, false);
    };
    let allowed: BTreeSet<_> = project
        .value
        .pointer("/schema_policy/allowed_extensions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut payloads: BTreeMap<&str, Vec<(&crate::ObjectRecord, &Value)>> = BTreeMap::new();
    for object in index.object_values() {
        if findings.cancelled() {
            break;
        }
        if let Some(values) = object.value.get("extensions").and_then(Value::as_object) {
            for (namespace, payload) in values {
                if findings.cancelled() {
                    break;
                }
                payloads
                    .entry(namespace)
                    .or_default()
                    .push((object, payload));
            }
        }
        if let ObjectType::Node(kind) = &object.object_type
            && let Some((namespace, _)) = kind.split_once(':')
            && object
                .value
                .pointer(&format!("/extensions/{}", escape_pointer(namespace)))
                .is_none()
        {
            findings.begin(
                Owner::Object(object.id.clone()),
                Phase::ExtensionPayload,
                None,
                Some(namespace),
                &[],
            );
            findings.state(Completeness::Verified);
            findings.push(|| {
                Finding::new(
                    "RP_E_EXTENSION_KIND_NAMESPACE_MISMATCH",
                    "extension_namespace",
                    Severity::Error,
                    "extension kind requires a payload in the exact same namespace",
                    Some(object.source_file.clone()),
                    "/kind",
                )
            });
        }
    }

    if payloads.len() > limits.extension_schemas {
        findings.begin(
            Owner::Object(project.id.clone()),
            Phase::ExtensionSchema,
            None,
            None,
            &[],
        );
        findings.incomplete();
        findings.push(|| {
            Finding::new(
                "RP_E_RESOURCE_EXTENSION_SCHEMAS_EXCEEDED",
                "resource_limit",
                Severity::Error,
                "extension schema count limit exceeded",
                None,
                "",
            )
        });
        return (extension_references, false);
    }

    let mut aggregate = 0_usize;
    let mut schema_files = 0_usize;
    for (namespace, instances) in payloads {
        if findings.cancelled() {
            break;
        }
        if !allowed.contains(namespace) {
            for (object, _) in instances {
                if findings.cancelled() {
                    break;
                }
                findings.begin(
                    Owner::Object(object.id.clone()),
                    Phase::ExtensionPayload,
                    None,
                    Some(namespace),
                    &[],
                );
                findings.state(Completeness::Verified);
                findings.push(|| {
                    Finding::new(
                        "RP_E_EXTENSION_NAMESPACE_NOT_ALLOWED",
                        "extension_namespace",
                        Severity::Error,
                        format!("extension namespace {namespace} is not allowed by the Project"),
                        Some(object.source_file.clone()),
                        format!("/extensions/{}", escape_pointer(namespace)),
                    )
                });
            }
            continue;
        }
        let Some(path) = extension_schema_path(namespace) else {
            findings.incomplete();
            continue;
        };
        // Borrow consumer identities already in the canonical index, and reserve before retention.
        let consumers = if let Some(store) = &mut findings.store {
            store.consumers(instances.iter().map(|(object, _)| &object.id))
        } else {
            Vec::new()
        };
        findings.context(&path, Phase::ExtensionSchema, Some(namespace), &consumers);
        let root_observation = findings.active;
        let bytes = match secure_read(
            root,
            &path,
            limits.extension_schema_aggregate_bytes,
            &findings.output.execution,
        ) {
            Ok(bytes) => {
                findings.state(Completeness::Verified);
                findings.hash(&raw_sha256(&bytes));
                bytes
            }
            Err(error) => {
                findings.state(read_completeness(error));
                findings.incomplete();
                findings.push(|| {
                    Finding::new(
                        "RP_E_EXTENSION_SCHEMA_NOT_FOUND",
                        "extension_schema",
                        Severity::Error,
                        format!("local extension schema {path} was not found"),
                        Some(project.source_file.clone()),
                        "/schema_policy/allowed_extensions",
                    )
                });
                continue;
            }
        };
        aggregate = aggregate.saturating_add(bytes.len());
        schema_files += 1;
        if aggregate > limits.extension_schema_aggregate_bytes {
            findings.incomplete();
            findings.push(|| {
                Finding::new(
                    "RP_E_RESOURCE_EXTENSION_SCHEMA_BYTES_EXCEEDED",
                    "resource_limit",
                    Severity::Error,
                    "extension schema aggregate byte limit exceeded",
                    None,
                    "",
                )
            });
            return (extension_references, false);
        }
        let schema: Value = match serde_json::from_slice(&bytes) {
            Ok(schema) => schema,
            Err(_) => {
                findings.incomplete();
                findings.push(|| extension_invalid(project, "extension schema is not valid JSON"));
                continue;
            }
        };
        let (schema, registry) = match prepare_extension_schema(
            root,
            &path,
            schema,
            limits,
            &mut aggregate,
            &mut schema_files,
            findings,
            namespace,
            &consumers,
        ) {
            Ok(prepared) => prepared,
            Err(ExtensionSchemaError::Forbidden) => {
                findings.incomplete();
                findings.push(|| Finding::new(
                    "RP_E_EXTENSION_REF_FORBIDDEN",
                    "extension_schema_ref",
                    Severity::Error,
                    "extension schemas may reference only contained local schemas or internal fragments",
                    Some(project.source_file.clone()),
                    "/schema_policy/allowed_extensions",
                ));
                continue;
            }
            Err(
                error @ (ExtensionSchemaError::SchemaCount
                | ExtensionSchemaError::Bytes
                | ExtensionSchemaError::RefDepth),
            ) => {
                findings.incomplete();
                let code = match error {
                    ExtensionSchemaError::SchemaCount => "RP_E_RESOURCE_EXTENSION_SCHEMAS_EXCEEDED",
                    ExtensionSchemaError::Bytes => "RP_E_RESOURCE_EXTENSION_SCHEMA_BYTES_EXCEEDED",
                    ExtensionSchemaError::RefDepth => "RP_E_RESOURCE_SCHEMA_REF_DEPTH_EXCEEDED",
                    _ => unreachable!(),
                };
                findings.push(|| {
                    Finding::new(
                        code,
                        "resource_limit",
                        Severity::Error,
                        "extension schema registry exceeded a bounded resource limit",
                        Some(project.source_file.clone()),
                        "/schema_policy/allowed_extensions",
                    )
                });
                continue;
            }
            Err(ExtensionSchemaError::Invalid) => {
                findings.incomplete();
                findings.push(|| {
                    extension_invalid(
                        project,
                        "extension schema reference could not be resolved locally",
                    )
                });
                continue;
            }
        };
        findings.active = root_observation;
        if findings.cancelled() {
            break;
        }
        let validator = match jsonschema::options()
            .with_draft(Draft::Draft202012)
            .with_registry(&registry)
            .offline()
            .should_validate_formats(true)
            .build(&schema)
        {
            Ok(validator) => validator,
            Err(_) => {
                findings.incomplete();
                findings.push(|| extension_invalid(project, "extension schema failed to compile"));
                continue;
            }
        };
        if findings.cancelled() {
            break;
        }
        let declarations = match semantic_declarations(&schema) {
            Ok(declarations) => declarations,
            Err(message) => {
                findings.incomplete();
                findings.push(|| {
                    Finding::new(
                        "RP_E_EXTENSION_REFERENCE_DECLARATION_INVALID",
                        "extension_semantic_reference",
                        Severity::Error,
                        message,
                        Some(project.source_file.clone()),
                        "/schema_policy/allowed_extensions",
                    )
                });
                continue;
            }
        };
        for (object, payload) in instances {
            if findings.cancelled() {
                break;
            }
            findings.begin(
                Owner::Object(object.id.clone()),
                Phase::ExtensionPayload,
                None,
                Some(namespace),
                &[],
            );
            findings.state(Completeness::Verified);
            if findings.cancelled() {
                break;
            }
            let valid = validator.is_valid(payload);
            if findings.cancelled() {
                break;
            }
            if !valid {
                findings.push(|| {
                    Finding::new(
                        "RP_E_EXTENSION_PAYLOAD_SCHEMA_MISMATCH",
                        "extension_payload",
                        Severity::Error,
                        format!("extension payload does not satisfy {namespace}"),
                        Some(object.source_file.clone()),
                        format!("/extensions/{}", escape_pointer(namespace)),
                    )
                });
            }
            if let Err(reason) = collect_extension_references(
                object,
                namespace,
                payload,
                &declarations,
                &mut extension_references,
                findings,
                remaining_edges,
            ) {
                let code = match reason {
                    PayloadStop::EdgeLimit => "RP_E_RESOURCE_GRAPH_EDGES_EXCEEDED",
                    PayloadStop::DepthLimit => "RP_E_RESOURCE_NESTING_DEPTH_EXCEEDED",
                    PayloadStop::FindingsExhausted => {
                        findings.incomplete();
                        return (extension_references, true);
                    }
                };
                findings.incomplete();
                findings.push(|| {
                    Finding::new(
                        code,
                        "resource_limit",
                        Severity::Error,
                        "extension reference collection exceeded its bounded resource limit",
                        None,
                        "",
                    )
                });
                return (extension_references, true);
            }
        }
    }
    (extension_references, false)
}

#[derive(Clone)]
struct SemanticDeclaration {
    pointer_segments: Vec<String>,
    expected: ReferenceExpectation,
}

fn semantic_declarations(schema: &Value) -> Result<Vec<SemanticDeclaration>, &'static str> {
    let Some(raw) = schema.get("x-rp-semantic-references") else {
        return Ok(Vec::new());
    };
    let entries = raw
        .as_array()
        .ok_or("x-rp-semantic-references must be an array")?;
    let mut declarations = Vec::new();
    for entry in entries {
        let pointer = entry
            .get("pointer_template")
            .and_then(Value::as_str)
            .ok_or("semantic reference declaration lacks pointer_template")?;
        if !pointer.starts_with('/') || pointer.ends_with('/') {
            return Err("semantic reference pointer_template is not an RFC 6901 pointer");
        }
        let mut segments = Vec::new();
        for segment in pointer.split('/').skip(1) {
            if segment.contains('*') && segment != "*" {
                return Err("semantic reference wildcard must occupy a complete array segment");
            }
            if segment.contains('~') {
                let mut chars = segment.chars();
                while let Some(character) = chars.next() {
                    if character == '~' && !matches!(chars.next(), Some('0' | '1')) {
                        return Err("semantic reference pointer_template has invalid escaping");
                    }
                }
            }
            segments.push(segment.replace("~1", "/").replace("~0", "~"));
        }
        let target_schema = entry
            .get("target_schema")
            .and_then(Value::as_str)
            .ok_or("semantic reference declaration lacks target_schema")?;
        let expected = expectation_for_schema(target_schema)
            .ok_or("semantic reference target_schema is not an exact supported rp/*/v1 schema")?;
        declarations.push(SemanticDeclaration {
            pointer_segments: segments,
            expected,
        });
    }
    Ok(declarations)
}

fn expectation_for_schema(schema: &str) -> Option<ReferenceExpectation> {
    match schema {
        "rp/project/v1" => Some(ReferenceExpectation::Project),
        "rp/node-revision/v1" => Some(ReferenceExpectation::Node),
        "rp/scientific-relation-revision/v1" => Some(ReferenceExpectation::Relation),
        "rp/assessment/v1" => Some(ReferenceExpectation::Assessment),
        "rp/research-thread/v1" => Some(ReferenceExpectation::Thread),
        "rp/thread-binding/v1" => Some(ReferenceExpectation::ThreadBinding),
        "rp/claim-chain-snapshot/v1" => Some(ReferenceExpectation::ClaimChain),
        "rp/artifact-manifest/v1" => Some(ReferenceExpectation::Artifact),
        "rp/external-reference/v1" => Some(ReferenceExpectation::ExternalReference),
        "rp/research-run/v1" => Some(ReferenceExpectation::ResearchRun),
        _ => None,
    }
}

fn collect_extension_references(
    object: &crate::ObjectRecord,
    namespace: &str,
    payload: &Value,
    declarations: &[SemanticDeclaration],
    references: &mut Vec<ReferenceEdge>,
    findings: &mut ContentFindings,
    edge_allowance: usize,
) -> Result<(), PayloadStop> {
    visit_payload_values(
        payload,
        &mut Vec::new(),
        YamlLimits::default().max_depth,
        &mut |segments, value| {
            if findings.cancelled() {
                return Err(PayloadStop::FindingsExhausted);
            }
            let mut matches = declarations
                .iter()
                .filter(|declaration| template_matches(&declaration.pointer_segments, segments));
            let declaration = matches.next();
            let ambiguous = matches.next().is_some();
            let exact_id = value.as_str().filter(|value| looks_like_exact_id(value));
            // Declared fields are semantic references even when their value is malformed.
            // Only undeclared, non-ID values may be treated as ordinary payload data.
            if declaration.is_none() && exact_id.is_none() {
                return Ok(());
            }
            if declaration.is_none() || ambiguous || exact_id.is_none() {
                findings.push(|| Finding::new(
                "RP_E_EXTENSION_REFERENCE_DECLARATION_INVALID",
                "extension_semantic_reference",
                Severity::Error,
                "extension semantic references require a canonical exact-ID string and exactly one declaration",
                Some(object.source_file.clone()),
                format!(
                    "/extensions/{}/{}",
                    escape_pointer(namespace),
                    segments
                        .iter()
                        .map(|segment| escape_pointer(segment))
                        .collect::<Vec<_>>()
                        .join("/")
                ),
            ));
                return if findings.cancelled() {
                    Err(PayloadStop::FindingsExhausted)
                } else {
                    Ok(())
                };
            }
            // Do not allocate the overflowing edge or visit any later payload value.
            if references.len() >= edge_allowance {
                return Err(PayloadStop::EdgeLimit);
            }
            references.push(ReferenceEdge {
                source_id: object.id.clone(),
                target_id: exact_id.expect("validated exact-ID string").into(),
                target_ordinal: None,
                expected: declaration.expect("validated declaration").expected.clone(),
                json_pointer: format!(
                    "/extensions/{}/{}",
                    escape_pointer(namespace),
                    segments
                        .iter()
                        .map(|segment| escape_pointer(segment))
                        .collect::<Vec<_>>()
                        .join("/")
                ),
            });
            Ok(())
        },
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PayloadStop {
    FindingsExhausted,
    EdgeLimit,
    DepthLimit,
}

// Only the current DFS path is retained; path and call stack are bounded by
// the canonical YAML nesting limit, not payload width.
fn visit_payload_values(
    value: &Value,
    path: &mut Vec<String>,
    max_depth: usize,
    visit: &mut impl FnMut(&[String], &Value) -> Result<(), PayloadStop>,
) -> Result<(), PayloadStop> {
    if path.len() > max_depth {
        return Err(PayloadStop::DepthLimit);
    }
    visit(path, value)?;
    match value {
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                path.push(index.to_string());
                let result = visit_payload_values(value, path, max_depth, visit);
                path.pop();
                result?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                path.push(key.clone());
                let result = visit_payload_values(value, path, max_depth, visit);
                path.pop();
                result?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn template_matches(template: &[String], actual: &[String]) -> bool {
    template.len() == actual.len()
        && template
            .iter()
            .zip(actual)
            .all(|(expected, actual)| expected == "*" || expected == actual)
}

fn looks_like_exact_id(value: &str) -> bool {
    static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();
    let validator = VALIDATOR.get_or_init(|| {
        let resource = SchemaBundle::resources()
            .iter()
            .find(|resource| resource.name == "common.schema.json")
            .expect("embedded common schema");
        let common: Value = serde_json::from_slice(resource.bytes).expect("valid embedded schema");
        let ids = common
            .pointer("/$defs/ids/$defs")
            .and_then(Value::as_object)
            .expect("canonical ID definitions");
        // Use the frozen patterns, including node_ extension revisions, without
        // imposing timestamp assumptions or mistaking arbitrary prefixes for IDs.
        let schema = serde_json::json!({"anyOf": ids.values().collect::<Vec<_>>()});
        jsonschema::options()
            .with_draft(Draft::Draft202012)
            .offline()
            .build(&schema)
            .expect("valid canonical exact-ID patterns")
    });
    validator.is_valid(&Value::String(value.into()))
}

fn validate_event_provenance(index: &ProjectIndex, findings: &mut ContentFindings) {
    for object in index.object_values() {
        if findings.cancelled() {
            break;
        }
        let has_events = object
            .value
            .pointer("/source/events")
            .and_then(Value::as_array)
            .is_some_and(|events| !events.is_empty());
        let unsupported_mode = matches!(object.object_type, ObjectType::ClaimChain)
            && matches!(
                object
                    .value
                    .pointer("/validation_policy/execution_provenance")
                    .and_then(Value::as_str),
                Some("event-backed" | "artifact-and-event")
            );
        if has_events || unsupported_mode {
            findings.begin(
                Owner::Object(object.id.clone()),
                Phase::EventProvenance,
                None,
                None,
                &[],
            );
            findings.state(Completeness::Verified);
            findings.push(|| {
                Finding::new(
                    "RP_E_EVENT_PROVENANCE_UNSUPPORTED",
                    "layer_a_unsupported",
                    Severity::Error,
                    "event-backed provenance is unsupported in Phase 1A",
                    Some(object.source_file.clone()),
                    if has_events {
                        "/source/events"
                    } else {
                        "/validation_policy/execution_provenance"
                    },
                )
            });
        }
    }
}

fn extension_invalid(project: &crate::ObjectRecord, message: &'static str) -> Finding {
    Finding::new(
        "RP_E_EXTENSION_SCHEMA_INVALID",
        "extension_schema",
        Severity::Error,
        message,
        Some(project.source_file.clone()),
        "/schema_policy/allowed_extensions",
    )
}

fn extension_schema_path(namespace: &str) -> Option<String> {
    let (name, version) = namespace.rsplit_once('/')?;
    if !version.starts_with('v') {
        return None;
    }
    Some(format!(
        ".research/schemas/{}/{}.schema.json",
        name.replace('.', "/"),
        version
    ))
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

#[derive(Clone, Copy)]
enum ExtensionSchemaError {
    Forbidden,
    SchemaCount,
    Bytes,
    RefDepth,
    Invalid,
}

#[allow(clippy::too_many_arguments)]
fn prepare_extension_schema(
    root: &Path,
    root_path: &str,
    mut schema: Value,
    limits: ProjectLimits,
    aggregate: &mut usize,
    schema_files: &mut usize,
    findings: &mut ContentFindings,
    namespace: &str,
    consumers: &[std::sync::Arc<str>],
) -> Result<(Value, Registry<'static>), ExtensionSchemaError> {
    let mut loaded = BTreeMap::new();
    rewrite_local_refs(
        root,
        root_path,
        &mut schema,
        limits,
        aggregate,
        schema_files,
        &mut loaded,
        0,
        findings,
        namespace,
        consumers,
    )?;
    let mut builder = Registry::new().draft(Draft::Draft202012);
    for (path, schema) in loaded {
        builder = builder
            .add(extension_schema_uri(&path), schema)
            .map_err(|_| ExtensionSchemaError::Invalid)?;
    }
    let registry = builder
        .prepare()
        .map_err(|_| ExtensionSchemaError::Invalid)?;
    Ok((schema, registry))
}

#[allow(clippy::too_many_arguments)]
fn rewrite_local_refs(
    root: &Path,
    current_path: &str,
    value: &mut Value,
    limits: ProjectLimits,
    aggregate: &mut usize,
    schema_files: &mut usize,
    loaded: &mut BTreeMap<String, Value>,
    depth: usize,
    findings: &mut ContentFindings,
    namespace: &str,
    consumers: &[std::sync::Arc<str>],
) -> Result<(), ExtensionSchemaError> {
    if depth > limits.extension_ref_depth {
        return Err(ExtensionSchemaError::RefDepth);
    }
    match value {
        Value::Array(values) => {
            for value in values {
                rewrite_local_refs(
                    root,
                    current_path,
                    value,
                    limits,
                    aggregate,
                    schema_files,
                    loaded,
                    depth,
                    findings,
                    namespace,
                    consumers,
                )?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if matches!(key.as_str(), "$ref" | "$dynamicRef") {
                    let reference = value.as_str().ok_or(ExtensionSchemaError::Invalid)?;
                    if reference.starts_with('#') {
                        continue;
                    }
                    let (relative, fragment) = reference.split_once('#').unwrap_or((reference, ""));
                    let path = resolve_schema_path(current_path, relative)?;
                    if !loaded.contains_key(&path) {
                        if *schema_files >= limits.extension_schemas {
                            return Err(ExtensionSchemaError::SchemaCount);
                        }
                        let parent_observation = findings.active;
                        findings.context(&path, Phase::ExtensionSchema, Some(namespace), consumers);
                        let bytes = secure_read(
                            root,
                            &path,
                            limits.extension_schema_aggregate_bytes,
                            &findings.output.execution,
                        )
                        .map_err(|error| {
                            findings.state(read_completeness(error));
                            match error {
                                SecureReadError::UnsafePath | SecureReadError::Symlink => {
                                    ExtensionSchemaError::Forbidden
                                }
                                SecureReadError::TooLarge => ExtensionSchemaError::Bytes,
                                _ => ExtensionSchemaError::Invalid,
                            }
                        })?;
                        findings.state(Completeness::Verified);
                        findings.hash(&raw_sha256(&bytes));
                        *aggregate = aggregate.saturating_add(bytes.len());
                        *schema_files += 1;
                        if *aggregate > limits.extension_schema_aggregate_bytes {
                            return Err(ExtensionSchemaError::Bytes);
                        }
                        let mut nested: Value = serde_json::from_slice(&bytes)
                            .map_err(|_| ExtensionSchemaError::Invalid)?;
                        loaded.insert(path.clone(), Value::Null);
                        rewrite_local_refs(
                            root,
                            &path,
                            &mut nested,
                            limits,
                            aggregate,
                            schema_files,
                            loaded,
                            depth + 1,
                            findings,
                            namespace,
                            consumers,
                        )?;
                        loaded.insert(path.clone(), nested);
                        findings.active = parent_observation;
                    }
                    *value = Value::String(if fragment.is_empty() {
                        extension_schema_uri(&path)
                    } else {
                        format!("{}#{fragment}", extension_schema_uri(&path))
                    });
                } else {
                    rewrite_local_refs(
                        root,
                        current_path,
                        value,
                        limits,
                        aggregate,
                        schema_files,
                        loaded,
                        depth,
                        findings,
                        namespace,
                        consumers,
                    )?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn resolve_schema_path(
    current_path: &str,
    reference: &str,
) -> Result<String, ExtensionSchemaError> {
    let lower = reference.to_ascii_lowercase();
    if reference.is_empty()
        || reference.starts_with('/')
        || reference.contains('\\')
        || reference.contains('?')
        || reference.contains(':')
        || lower.contains("%2f")
        || lower.contains("%5c")
        || lower.contains("%2e")
    {
        return Err(ExtensionSchemaError::Forbidden);
    }
    let mut components: Vec<&str> = current_path.split('/').collect();
    components.pop();
    for component in reference.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop().ok_or(ExtensionSchemaError::Forbidden)?;
            }
            value => components.push(value),
        }
    }
    let path = components.join("/");
    if !path.starts_with(".research/schemas/") || !path.ends_with(".schema.json") {
        return Err(ExtensionSchemaError::Forbidden);
    }
    ProjectPath::new(path.clone()).map_err(|_| ExtensionSchemaError::Forbidden)?;
    Ok(path)
}

fn extension_schema_uri(path: &str) -> String {
    format!("https://research-provenance.invalid/project/{path}")
}

fn path_or_content_finding(
    error: SecureReadError,
    default_code: &'static str,
    default_family: &'static str,
    object: &crate::ObjectRecord,
    pointer: &str,
) -> Finding {
    let (code, family, message) = match error {
        SecureReadError::Stopped(stop) => return stop.finding(),
        SecureReadError::UnsafePath => (
            "RP_E_PATH_PROJECT_ESCAPE",
            "path_containment",
            "path escapes or encodes traversal outside the Project",
        ),
        SecureReadError::Symlink => (
            "RP_E_PATH_SYMLINK_FORBIDDEN",
            "path_containment",
            "symlinks are forbidden for contained content",
        ),
        SecureReadError::NonRegular => (
            "RP_E_PATH_NON_REGULAR_FILE",
            "path_containment",
            "contained content is not a regular file",
        ),
        SecureReadError::Changed => (
            "RP_E_ARTIFACT_CHANGED_DURING_VERIFICATION",
            "artifact_integrity",
            "contained file changed during verification",
        ),
        SecureReadError::TooLarge => (
            "RP_E_RESOURCE_ARTIFACT_SIZE_EXCEEDED",
            "resource_limit",
            "contained file exceeds its byte budget",
        ),
        SecureReadError::AggregateTooLarge => (
            "RP_E_RESOURCE_ARTIFACT_AGGREGATE_EXCEEDED",
            "resource_limit",
            "contained file exceeds the remaining aggregate artifact byte budget",
        ),
        SecureReadError::ReportTooLarge => (
            "RP_E_RESOURCE_REPORT_SIZE_EXCEEDED",
            "resource_limit",
            "report document byte limit exceeded",
        ),
        SecureReadError::CaptureTooLarge => (
            "RP_E_RESOURCE_REPORT_CAPTURE_EXCEEDED",
            "resource_limit",
            "aggregate report capture byte limit exceeded",
        ),
        SecureReadError::Missing => (default_code, default_family, "contained file was not found"),
    };
    Finding::new(
        code,
        family,
        Severity::Error,
        message,
        Some(object.source_file.clone()),
        pointer,
    )
}

fn raw_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SecureReadError {
    Stopped(crate::ExecutionStop),
    UnsafePath,
    Symlink,
    NonRegular,
    Changed,
    TooLarge,
    AggregateTooLarge,
    ReportTooLarge,
    CaptureTooLarge,
    Missing,
}

fn read_completeness(error: SecureReadError) -> Completeness {
    match error {
        SecureReadError::TooLarge
        | SecureReadError::AggregateTooLarge
        | SecureReadError::ReportTooLarge
        | SecureReadError::CaptureTooLarge => Completeness::ResourceLimit,
        _ => Completeness::Unavailable,
    }
}

struct ReportCapture<'a> {
    bytes: Vec<u8>,
    max_bytes: usize,
    remaining: &'a mut usize,
}

#[cfg(target_os = "linux")]
fn secure_hash(
    root: &Path,
    path: &str,
    max_bytes: usize,
    remaining: &mut usize,
    capture: Option<&mut ReportCapture<'_>>,
    execution: &crate::ExecutionBudget,
) -> Result<(usize, String), SecureReadError> {
    use std::fs::File;

    use rustix::fs::{CWD, FileType, Mode, OFlags, ResolveFlags, fstat, openat, openat2};

    execution.checkpoint().map_err(SecureReadError::Stopped)?;
    ProjectPath::new(path.to_string()).map_err(|_| SecureReadError::UnsafePath)?;
    let root_fd = openat(
        CWD,
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| SecureReadError::Missing)?;
    let fd = openat2(
        &root_fd,
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
    )
    .map_err(|error| match error {
        rustix::io::Errno::LOOP => SecureReadError::Symlink,
        rustix::io::Errno::XDEV => SecureReadError::UnsafePath,
        _ => SecureReadError::Missing,
    })?;
    let before = fstat(&fd).map_err(|_| SecureReadError::Missing)?;
    if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile {
        return Err(SecureReadError::NonRegular);
    }
    let expected_size = usize::try_from(before.st_size).map_err(|_| SecureReadError::TooLarge)?;
    let mut file = File::from(fd);
    #[cfg(test)]
    read_audit::record(path, capture.is_some());
    let (observed_size, digest) = metered_hash_capture(
        &mut file,
        expected_size,
        max_bytes,
        remaining,
        capture,
        execution,
    )?;
    #[cfg(test)]
    read_audit::after_hash();
    let after = fstat(&file).map_err(|_| SecureReadError::Changed)?;
    if observed_size != expected_size
        || before.st_dev != after.st_dev
        || before.st_ino != after.st_ino
        || before.st_size != after.st_size
        || before.st_mtime != after.st_mtime
        || before.st_mtime_nsec != after.st_mtime_nsec
        || before.st_ctime != after.st_ctime
        || before.st_ctime_nsec != after.st_ctime_nsec
    {
        return Err(SecureReadError::Changed);
    }
    #[cfg(test)]
    read_audit::after_verified();
    execution.checkpoint().map_err(SecureReadError::Stopped)?;
    Ok((observed_size, digest))
}

// Read the stat-observed size, never the manifest declaration. Completion here is
// provisional: secure_hash must still check the descriptor's metadata. This avoids
// an unbudgeted EOF probe at exact/zero limits, including when a file grows.
#[cfg(test)]
fn metered_hash(
    reader: &mut impl std::io::Read,
    expected_size: usize,
    max_bytes: usize,
    remaining: &mut usize,
) -> Result<(usize, String), SecureReadError> {
    metered_hash_capture(
        reader,
        expected_size,
        max_bytes,
        remaining,
        None,
        &crate::ExecutionBudget::default(),
    )
}

fn metered_hash_capture(
    reader: &mut impl std::io::Read,
    expected_size: usize,
    max_bytes: usize,
    remaining: &mut usize,
    mut capture: Option<&mut ReportCapture<'_>>,
    execution: &crate::ExecutionBudget,
) -> Result<(usize, String), SecureReadError> {
    execution.checkpoint().map_err(SecureReadError::Stopped)?;
    if expected_size > max_bytes {
        return Err(SecureReadError::TooLarge);
    }
    if expected_size > *remaining {
        return Err(SecureReadError::AggregateTooLarge);
    }
    if let Some(capture) = &mut capture {
        if expected_size > capture.max_bytes {
            return Err(SecureReadError::ReportTooLarge);
        }
        if expected_size > *capture.remaining {
            return Err(SecureReadError::CaptureTooLarge);
        }
        // Stat, item, aggregate, document and capture budgets all precede allocation.
        capture.bytes.reserve_exact(expected_size);
    }
    let mut hasher = Sha256::new();
    let mut observed_size = 0;
    let mut buffer = [0_u8; 64 * 1024];
    while observed_size < expected_size {
        execution.checkpoint().map_err(SecureReadError::Stopped)?;
        let allowance = buffer
            .len()
            .min(expected_size - observed_size)
            .min(max_bytes - observed_size)
            .min(*remaining);
        let count = reader
            .read(&mut buffer[..allowance])
            .map_err(|_| SecureReadError::Missing)?;
        if count == 0 {
            return Err(SecureReadError::Changed);
        }
        // Charge immediately, before any later I/O or verification can fail.
        *remaining -= count;
        observed_size += count;
        execution.checkpoint().map_err(SecureReadError::Stopped)?;
        hasher.update(&buffer[..count]);
        if let Some(capture) = &mut capture {
            *capture.remaining -= count;
            capture.bytes.extend_from_slice(&buffer[..count]);
        }
    }
    Ok((observed_size, format!("sha256:{:x}", hasher.finalize())))
}

#[cfg(not(target_os = "linux"))]
fn secure_hash(
    _root: &Path,
    _path: &str,
    _max_bytes: usize,
    _remaining: &mut usize,
    _capture: Option<&mut ReportCapture<'_>>,
    _execution: &crate::ExecutionBudget,
) -> Result<(usize, String), SecureReadError> {
    Err(SecureReadError::Missing)
}

// Narratives, policies and schemas use the artifact descriptor/stability/metering
// path too. Captured bytes are consumed immediately, never retained by observations.
fn secure_read(
    root: &Path,
    path: &str,
    max_bytes: usize,
    execution: &crate::ExecutionBudget,
) -> Result<Vec<u8>, SecureReadError> {
    let mut remaining = max_bytes;
    let mut captured = max_bytes;
    let mut capture = ReportCapture {
        bytes: Vec::new(),
        max_bytes,
        remaining: &mut captured,
    };
    secure_hash(
        root,
        path,
        max_bytes,
        &mut remaining,
        Some(&mut capture),
        execution,
    )?;
    Ok(capture.bytes)
}

#[cfg(test)]
#[path = "content_observation_tests.rs"]
mod content_observation_tests;

#[cfg(test)]
#[path = "report_acquisition_tests.rs"]
mod report_acquisition_tests;

#[cfg(test)]
mod read_audit {
    use std::cell::RefCell;
    thread_local! {
        static EVENTS: RefCell<Vec<(String, bool)>> = const { RefCell::new(Vec::new()) };
        static AFTER_HASH: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
        static AFTER_VERIFIED: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }
    pub fn on_after_verified(hook: impl FnOnce() + 'static) {
        AFTER_VERIFIED.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    }
    pub fn after_verified() {
        let hook = AFTER_VERIFIED.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
    }
    pub fn on_after_hash(hook: impl FnOnce() + 'static) {
        AFTER_HASH.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    }
    pub fn after_hash() {
        let hook = AFTER_HASH.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
    }
    pub fn reset() {
        EVENTS.with(|events| events.borrow_mut().clear());
    }
    pub fn record(path: &str, capture: bool) {
        EVENTS.with(|events| events.borrow_mut().push((path.into(), capture)));
    }
    pub fn events() -> Vec<(String, bool)> {
        EVENTS.with(|events| events.borrow().clone())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod execution_fifo_tests {
    use super::*;
    #[test]
    fn fifo_open_child() {
        // Parent launches only this exact test with one private argument. No env hooks.
        let args: Vec<_> = std::env::args().collect();
        let Some(path) = args.iter().find_map(|a| a.strip_prefix("--logfile=")) else {
            return;
        };
        let root = Path::new(path).parent().unwrap();
        assert_eq!(
            secure_read(root, "fifo", 10, &crate::ExecutionBudget::default()),
            Err(SecureReadError::NonRegular)
        );
    }
    #[test]
    fn fifo_open_is_bounded_in_subprocess() {
        let root = std::env::temp_dir().join(format!("rp-budget-fifo-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        rustix::fs::mkfifoat(
            rustix::fs::CWD,
            root.join("fifo"),
            rustix::fs::Mode::from_raw_mode(0o600),
        )
        .unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("content::execution_fifo_tests::fifo_open_child")
            .arg(format!("--logfile={}", root.join("test.log").display()))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let start = std::time::Instant::now();
        let result = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if start.elapsed() > std::time::Duration::from_secs(5) {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            result.is_some_and(|status| status.success()),
            "FIFO target open blocked or child failed"
        );
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    use serde_json::json;
    use std::io::{self, Read};

    #[test]
    fn execution_hash_stops_after_first_chunk_without_refunding_bytes() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        struct Reader {
            token: Arc<AtomicBool>,
            calls: usize,
        }
        impl Read for Reader {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                self.calls += 1;
                bytes.fill(0);
                self.token.store(true, Ordering::Release);
                Ok(bytes.len())
            }
        }
        let token = Arc::new(AtomicBool::new(false));
        let execution =
            crate::ExecutionBudget::new(std::time::Duration::from_secs(60), token.clone());
        let mut reader = Reader { token, calls: 0 };
        let mut remaining = 128 * 1024;
        let result = metered_hash_capture(
            &mut reader,
            128 * 1024,
            128 * 1024,
            &mut remaining,
            None,
            &execution,
        );
        assert_eq!(
            result,
            Err(SecureReadError::Stopped(crate::ExecutionStop::Interrupted))
        );
        assert_eq!(reader.calls, 1);
        assert_eq!(remaining, 64 * 1024);
    }

    pub(super) struct CountingReader {
        bytes: Vec<u8>,
        consumed: usize,
        pub(super) requests: Vec<usize>,
        pub(super) error_at: Option<usize>,
        grow: bool,
    }

    impl CountingReader {
        pub(super) fn new(bytes: &[u8]) -> Self {
            Self {
                bytes: bytes.into(),
                consumed: 0,
                requests: Vec::new(),
                error_at: None,
                grow: false,
            }
        }
    }

    impl Read for CountingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.requests.push(buffer.len());
            if self.error_at == Some(self.consumed) {
                return Err(io::Error::other("injected read failure"));
            }
            if self.grow {
                self.bytes.extend_from_slice(b"growing");
            }
            let count = buffer
                .len()
                .min(2)
                .min(self.bytes.len() - self.consumed)
                .min(self.error_at.unwrap_or(usize::MAX) - self.consumed);
            buffer[..count].copy_from_slice(&self.bytes[self.consumed..self.consumed + count]);
            self.consumed += count;
            Ok(count)
        }
    }

    #[test]
    fn metered_reads_reject_metadata_before_io_and_preserve_error_charges() {
        for (size, item, aggregate, expected_error) in [
            (9, 8, 10, SecureReadError::TooLarge),
            (9, 10, 8, SecureReadError::AggregateTooLarge),
        ] {
            let mut reader = CountingReader::new(b"123456789");
            let mut remaining = aggregate;
            assert_eq!(
                metered_hash(&mut reader, size, item, &mut remaining),
                Err(expected_error)
            );
            assert!(reader.requests.is_empty());
            assert_eq!(remaining, aggregate);
        }
        for error in [false, true] {
            let mut reader = CountingReader::new(b"123");
            if error {
                reader.error_at = Some(3);
            }
            let mut remaining = 8;
            assert_eq!(
                metered_hash(&mut reader, 8, 8, &mut remaining),
                Err(if error {
                    SecureReadError::Missing
                } else {
                    SecureReadError::Changed
                })
            );
            assert_eq!(remaining, 5);
            assert_eq!(reader.consumed, 3);
            let mut next = CountingReader::new(b"123456");
            assert_eq!(
                metered_hash(&mut next, 6, 8, &mut remaining),
                Err(SecureReadError::AggregateTooLarge)
            );
            assert!(next.requests.is_empty());
        }
    }

    #[test]
    fn metered_reads_bound_every_request_even_for_growing_input_and_exact_zero() {
        for grow in [false, true] {
            let mut reader = CountingReader::new(b"12345678");
            reader.grow = grow;
            let mut remaining = 8;
            // Digest is provisional until secure_hash checks post-read metadata.
            let (size, digest) = metered_hash(&mut reader, 8, 8, &mut remaining).unwrap();
            assert_eq!((size, digest), (8, raw_sha256(b"12345678")));
            assert_eq!(remaining, 0);
            assert_eq!(reader.requests, [8, 6, 4, 2]);
            assert_eq!(reader.consumed, 8);
        }
        let mut empty = CountingReader::new(b"");
        assert_eq!(
            metered_hash(&mut empty, 0, 0, &mut 0).unwrap(),
            (0, raw_sha256(b""))
        );
        assert!(empty.requests.is_empty());
    }

    #[test]
    fn extension_collection_never_allocates_overflow_edge_or_visits_tail() {
        let object = crate::ObjectRecord {
            ordinal: 0,
            logical_id: None,
            id: "proj_01J00000000000000000000001".into(),
            object_type: ObjectType::Project,
            source_file: ProjectPath::new(".research/project.yaml").unwrap(),
            value: json!({}),
        };
        let declarations = semantic_declarations(&json!({"x-rp-semantic-references": [{
            "pointer_template": "/targets/*", "target_schema": "rp/node-revision/v1"
        }]}))
        .unwrap();
        for allowance in [0, 1] {
            let mut references = Vec::with_capacity(allowance);
            let capacity = references.capacity();
            let mut findings = ContentFindings::new(false);
            let payload = json!({"targets": ["qst_01J00000000000000000000010", "qst_01J00000000000000000000010", false]});
            assert_eq!(
                collect_extension_references(
                    &object,
                    "fixture.synthetic/v1",
                    &payload,
                    &declarations,
                    &mut references,
                    &mut findings,
                    allowance
                ),
                Err(PayloadStop::EdgeLimit)
            );
            assert_eq!(references.len(), allowance);
            assert_eq!(
                references.capacity(),
                capacity,
                "must stop before overflow allocation"
            );
            assert!(findings.output.is_empty(), "must not visit malformed tail");
        }
    }

    #[test]
    fn findings_overflow_stops_extension_payload_walk() {
        let object = crate::ObjectRecord {
            ordinal: 0,
            id: "qst_test".into(),
            logical_id: None,
            object_type: crate::ObjectType::Node("Question".into()),
            source_file: crate::ProjectPath::new("object.yaml").unwrap(),
            value: json!({}),
        };
        let mut findings = ContentFindings::with_budget(false, crate::findings::Findings::new(1));
        let payload = json!(vec!["qst_01J00000000000000000000010"; 100]);
        let result = collect_extension_references(
            &object,
            "test/v1",
            &payload,
            &[],
            &mut Vec::new(),
            &mut findings,
            100,
        );
        assert!(
            result.is_err(),
            "diagnostic overflow must cancel the payload visitor"
        );
        assert!(findings.cancelled());
    }

    #[test]
    fn payload_dfs_is_ordered_short_circuiting_and_depth_bounded() {
        let payload = json!({"a/b": [1, {"~": 2}], "z": false});
        let mut paths = Vec::new();
        let result = visit_payload_values(&payload, &mut Vec::new(), 64, &mut |path, _| {
            paths.push(path.join("/"));
            if paths.len() == 4 {
                Err(PayloadStop::EdgeLimit)
            } else {
                Ok(())
            }
        });
        assert_eq!(result, Err(PayloadStop::EdgeLimit));
        assert_eq!(paths, ["", "a/b", "a/b/0", "a/b/1"]);
        let mut visits = 0;
        assert_eq!(
            visit_payload_values(&json!([[[1]]]), &mut Vec::new(), 1, &mut |_, _| {
                visits += 1;
                Ok(())
            }),
            Err(PayloadStop::DepthLimit)
        );
        assert_eq!(visits, 2);
    }
}
