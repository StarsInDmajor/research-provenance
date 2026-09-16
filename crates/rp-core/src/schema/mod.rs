use std::{collections::BTreeMap, fmt};

use jsonschema::{Draft, Registry, Validator, error::ValidationErrorKind};
use serde_json::{Value, json};

use crate::findings::Findings;
use crate::{Finding, ProjectPath, Severity};

#[cfg(test)]
thread_local! {
    static ERROR_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static ERROR_CONVERSIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

const BASE_URI: &str = "https://research-provenance.invalid/schemas/v1/";

pub struct EmbeddedResource {
    pub name: &'static str,
    pub bytes: &'static [u8],
}

macro_rules! resource {
    ($name:literal) => {
        EmbeddedResource {
            name: $name,
            bytes: include_bytes!(concat!("../../../../resources/schemas/v1/", $name)),
        }
    };
}

static RESOURCES: &[EmbeddedResource] = &[
    resource!("README.md"),
    resource!("access-closure-expectations.schema.json"),
    resource!("artifact-manifest.schema.json"),
    resource!("assessment.schema.json"),
    resource!("benchmark-budget.yaml"),
    resource!("claim-chain-snapshot.schema.json"),
    resource!("claim-chain-validation-report.schema.json"),
    resource!("cli-contract.yaml"),
    resource!("cli-data.schema.json"),
    resource!("cli-result.schema.json"),
    resource!("common.schema.json"),
    resource!("external-reference.schema.json"),
    resource!("finding-registry.yaml"),
    resource!("finding.schema.json"),
    resource!("fixture-expectations.schema.json"),
    resource!("fixture-object.schema.json"),
    resource!("freshness-policy.schema.json"),
    resource!("layer-b.schema.json"),
    resource!("node-revision.schema.json"),
    resource!("presentation-order.yaml"),
    resource!("project.schema.json"),
    resource!("research-run.schema.json"),
    resource!("research-thread.schema.json"),
    resource!("resource-limits.yaml"),
    resource!("scale-generator-contract.yaml"),
    resource!("scale-manifest.schema.json"),
    resource!("schema-catalog.json"),
    resource!("scientific-relation-revision.schema.json"),
    resource!("security-review.md"),
    resource!("test-overlay.schema.json"),
    resource!("thread-binding.schema.json"),
    resource!("validation-stages.yaml"),
];

const DISPATCH: &[(&str, &str)] = &[
    ("rp/project/v1", "project.schema.json"),
    ("rp/node-revision/v1", "node-revision.schema.json"),
    ("rp/assessment/v1", "assessment.schema.json"),
    (
        "rp/scientific-relation-revision/v1",
        "scientific-relation-revision.schema.json",
    ),
    ("rp/research-thread/v1", "research-thread.schema.json"),
    ("rp/thread-binding/v1", "thread-binding.schema.json"),
    (
        "rp/claim-chain-snapshot/v1",
        "claim-chain-snapshot.schema.json",
    ),
    ("rp/external-reference/v1", "external-reference.schema.json"),
    ("rp/artifact-manifest/v1", "artifact-manifest.schema.json"),
    ("rp/research-run/v1", "research-run.schema.json"),
    (
        "rp/claim-chain-validation-report/v1",
        "claim-chain-validation-report.schema.json",
    ),
    ("rp/finding/v1", "finding.schema.json"),
    ("rp/cli-result/v1", "cli-result.schema.json"),
    ("rp/cli-data/v1", "cli-data.schema.json"),
    ("rp/test-overlay/v1", "test-overlay.schema.json"),
    (
        "rp/fixture-expectations/v1",
        "fixture-expectations.schema.json",
    ),
    (
        "rp/access-closure-expectations/v1",
        "access-closure-expectations.schema.json",
    ),
    ("rp/freshness-policy/v1", "freshness-policy.schema.json"),
    ("rp/scale-manifest/v1", "scale-manifest.schema.json"),
];

const BUILT_IN_KINDS: &[&str] = &[
    "Dataset",
    "Question",
    "Hypothesis",
    "Observation",
    "Measurement",
    "Method",
    "Test",
    "Prediction",
    "Synthesis",
    "Interpretation",
    "Decision",
    "Conclusion",
    "Blocker",
    "NextAction",
    "PaperClaim",
];

pub struct SchemaBundle {
    validators: BTreeMap<&'static str, Validator>,
    node_validators: BTreeMap<&'static str, Validator>,
}

impl SchemaBundle {
    #[must_use]
    pub const fn resources() -> &'static [EmbeddedResource] {
        RESOURCES
    }

    #[allow(clippy::missing_errors_doc)]
    pub fn new() -> Result<Self, SchemaBundleError> {
        let schemas = parse_embedded_json()?;
        for schema in schemas.values() {
            check_references(schema)?;
        }

        let mut builder = Registry::new().draft(Draft::Draft202012);
        for (name, schema) in &schemas {
            builder = builder
                .add(format!("{BASE_URI}{name}"), schema.clone())
                .map_err(SchemaBundleError::from_display)?;
        }
        let registry = builder.prepare().map_err(SchemaBundleError::from_display)?;

        let mut validators = BTreeMap::new();
        for &(schema_id, name) in DISPATCH {
            let schema = schemas
                .get(name)
                .ok_or_else(|| SchemaBundleError::new(format!("missing embedded schema {name}")))?;
            validators.insert(schema_id, compile(schema, name, &registry)?);
        }

        let node_schema = schemas
            .get("node-revision.schema.json")
            .ok_or_else(|| SchemaBundleError::new("missing embedded node schema"))?;
        let node_validators = compile_node_validators(node_schema, &registry)?;

        Ok(Self {
            validators,
            node_validators,
        })
    }

    #[allow(clippy::missing_errors_doc)]
    pub fn check_offline_references(&self, schema: &Value) -> Result<(), SchemaBundleError> {
        let _ = self;
        check_references(schema)
    }

    #[allow(clippy::missing_errors_doc)]
    pub fn validate(&self, instance: &Value, source_file: ProjectPath) -> Result<(), Vec<Finding>> {
        let mut findings = Findings::default();
        let valid = self.validate_into(instance, source_file, &mut findings);
        let (mut findings, _, _) = findings.finish();
        findings.sort_by(|a, b| {
            (a.json_pointer.as_str(), a.error_code).cmp(&(b.json_pointer.as_str(), b.error_code))
        });
        if valid { Ok(()) } else { Err(findings) }
    }

    /// Boolean checks do not construct a discarded diagnostic transcript.
    pub(crate) fn is_valid_with_budget(
        &self,
        instance: &Value,
        execution: &crate::ExecutionBudget,
    ) -> bool {
        if execution.checkpoint().is_err() {
            return false;
        }
        let valid = instance
            .get("schema")
            .and_then(Value::as_str)
            .and_then(|id| self.validators.get(id))
            .is_some_and(|validator| validator.is_valid(instance));
        execution.checkpoint().is_ok() && valid
    }

    pub(crate) fn validate_into(
        &self,
        instance: &Value,
        source_file: ProjectPath,
        sink: &mut Findings,
    ) -> bool {
        if sink.cancelled() {
            return false;
        }
        let mut findings = sink.fork();
        let Some(schema_id) = instance.get("schema").and_then(Value::as_str) else {
            findings.push(|| schema_dispatch_finding(source_file));
            sink.absorb(findings);
            return false;
        };
        let Some(mut validator) = self.validators.get(schema_id) else {
            findings.push(|| schema_dispatch_finding(source_file));
            sink.absorb(findings);
            return false;
        };

        if schema_id == "rp/node-revision/v1"
            && let Some(kind) = instance.get("kind").and_then(Value::as_str)
            && let Some(kind_validator) = self.node_validators.get(kind)
        {
            validator = kind_validator;
        }

        let assessment_target_mismatch = assessment_target_mismatch(instance);
        let claim_chain_logical_pointers = || claim_chain_logical_pointers(instance);
        let declassification_present = instance.get("declassification_attestation").is_some();

        if assessment_target_mismatch {
            findings.push(|| {
                Finding::new(
                    "RP_E_SCHEMA_TARGET_ID_TYPE",
                    "schema_target_id_type",
                    Severity::Error,
                    "assessment target type and ID prefix disagree",
                    Some(source_file.clone()),
                    "/target",
                )
            });
        }
        for pointer in claim_chain_logical_pointers() {
            if findings.cancelled() {
                break;
            }
            findings.push(|| {
                Finding::new(
                    "RP_E_CLAIM_CHAIN_REQUIRES_EXACT_REVISION",
                    "claim_chain_identity",
                    Severity::Error,
                    "claim-chain selections require exact typed revision IDs",
                    Some(source_file.clone()),
                    pointer,
                )
            });
        }
        if declassification_present {
            findings.push(|| {
                Finding::new(
                    "RP_E_DECLASSIFICATION_UNSUPPORTED",
                    "deferred_governance_schema",
                    Severity::Error,
                    "declassification attestations are unsupported in v1",
                    Some(source_file.clone()),
                    "/declassification_attestation",
                )
            });
        }
        // Contextual diagnostics are admitted first, so a wide logical-selection
        // failure cancels before generic iteration. Check cancellation before next(),
        // not after eager conversion. Suppressed errors are not retained; internal
        // schema branch work remains a separate evaluation-budget concern.
        // jsonschema 0.54 eagerly collects a Vec inside iter_errors. Only the
        // boundaries are cooperative; neither branch work nor that allocation is.
        if findings.cancelled() {
            sink.absorb(findings);
            return false;
        }
        let mut errors = validator.iter_errors(instance);
        while !findings.cancelled() {
            let Some(error) = errors.next() else {
                break;
            };
            #[cfg(test)]
            ERROR_VISITS.with(|count| count.set(count.get() + 1));
            let pointer = error.instance_path().to_string();
            let is_assessment_override =
                assessment_target_mismatch && pointer.starts_with("/target");
            let is_claim_chain_override = logical_claim_pointer(instance, &pointer);
            let is_declassification_override = declassification_present
                && matches!(error.kind(), ValidationErrorKind::AdditionalProperties { unexpected }
                    if unexpected.iter().any(|p| p == "declassification_attestation"));
            if !(is_assessment_override || is_claim_chain_override || is_declassification_override)
            {
                findings.collect_errors(std::iter::once(error), |error| {
                    schema_finding(&error, source_file.clone())
                });
            }
        }
        findings.sort_and_dedup();
        let valid = !findings.had_error() && !findings.cancelled();
        sink.absorb(findings);
        valid
    }
}

fn assessment_target_mismatch(instance: &Value) -> bool {
    if instance.get("schema").and_then(Value::as_str) != Some("rp/assessment/v1") {
        return false;
    }
    let Some(target) = instance.get("target") else {
        return false;
    };
    let target_type = target.get("type").and_then(Value::as_str);
    let target_id = target.get("id").and_then(Value::as_str);
    match (target_type, target_id) {
        (Some("node_revision"), Some(id)) => !is_node_revision_id(id),
        (Some("relation_revision"), Some(id)) => !id.starts_with("rel_"),
        _ => false,
    }
}

fn is_node_revision_id(id: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "dset_", "qst_", "hyp_", "pred_", "obs_", "meas_", "mth_", "tst_", "syn_", "int_", "dec_",
        "con_", "blk_", "nxt_", "clm_", "node_",
    ];
    PREFIXES.iter().any(|prefix| id.starts_with(prefix))
}

fn logical_claim_pointer(instance: &Value, pointer: &str) -> bool {
    if instance.get("schema").and_then(Value::as_str) != Some("rp/claim-chain-snapshot/v1") {
        return false;
    }
    let mut segments = pointer.split('/').skip(1);
    let Some(field @ ("node_revisions" | "relation_revisions")) = segments.next() else {
        return false;
    };
    let Some(index) = segments.next().and_then(|s| s.parse::<usize>().ok()) else {
        return false;
    };
    instance
        .get(field)
        .and_then(Value::as_array)
        .and_then(|a| a.get(index))
        .and_then(Value::as_str)
        .is_some_and(|id| !id.contains('_'))
}

fn claim_chain_logical_pointers(instance: &Value) -> impl Iterator<Item = String> + '_ {
    ["node_revisions", "relation_revisions"]
        .into_iter()
        .filter(|_| {
            instance.get("schema").and_then(Value::as_str) == Some("rp/claim-chain-snapshot/v1")
        })
        .flat_map(|field| {
            instance
                .get(field)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(move |(index, value)| {
                    let value = value.as_str()?;
                    (!value.contains('_')).then(|| format!("/{field}/{index}"))
                })
        })
}

fn parse_embedded_json() -> Result<BTreeMap<&'static str, Value>, SchemaBundleError> {
    RESOURCES
        .iter()
        .filter(|resource| resource.name.ends_with(".json"))
        .map(|resource| {
            serde_json::from_slice(resource.bytes)
                .map(|schema| (resource.name, schema))
                .map_err(SchemaBundleError::from_display)
        })
        .collect()
}

fn compile(
    schema: &Value,
    name: &str,
    registry: &Registry<'_>,
) -> Result<Validator, SchemaBundleError> {
    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .with_base_uri(format!("{BASE_URI}{name}"))
        .with_registry(registry)
        .offline()
        .should_validate_formats(true)
        .should_ignore_unknown_formats(false)
        .build(schema)
        .map_err(SchemaBundleError::from_display)
}

fn compile_node_validators(
    node_schema: &Value,
    registry: &Registry<'_>,
) -> Result<BTreeMap<&'static str, Validator>, SchemaBundleError> {
    let branches = node_schema
        .get("oneOf")
        .and_then(Value::as_array)
        .ok_or_else(|| SchemaBundleError::new("node schema has no oneOf discriminator"))?;
    let mut validators = BTreeMap::new();

    for &kind in BUILT_IN_KINDS {
        let branch = branches
            .iter()
            .find(|branch| {
                branch
                    .pointer("/properties/kind/const")
                    .and_then(Value::as_str)
                    == Some(kind)
            })
            .ok_or_else(|| {
                SchemaBundleError::new(format!("node schema has no branch for {kind}"))
            })?;
        let mut base = node_schema.clone();
        base.as_object_mut()
            .expect("node schema is an object")
            .remove("oneOf");
        base.as_object_mut()
            .expect("node schema is an object")
            .remove("$id");
        let schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "allOf": [base, branch]
        });
        validators.insert(
            kind,
            compile(&schema, "node-revision.schema.json", registry)?,
        );
    }
    Ok(validators)
}

fn check_references(schema: &Value) -> Result<(), SchemaBundleError> {
    match schema {
        Value::Array(values) => {
            for value in values {
                check_references(value)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if matches!(key.as_str(), "$ref" | "$dynamicRef") {
                    let reference = value.as_str().ok_or_else(|| {
                        SchemaBundleError::new("schema reference must be a string")
                    })?;
                    check_reference(reference)?;
                } else {
                    check_references(value)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn check_reference(reference: &str) -> Result<(), SchemaBundleError> {
    if reference.starts_with('#') {
        return Ok(());
    }
    let path = reference.split('#').next().unwrap_or_default();
    let known = RESOURCES
        .iter()
        .any(|resource| resource.name == path && path.ends_with(".json"));
    if !known || path.contains('/') || path.contains('\\') || path.contains(':') {
        return Err(SchemaBundleError::new(
            "schema reference is not an embedded offline resource",
        ));
    }
    Ok(())
}

fn schema_finding(error: &jsonschema::ValidationError<'_>, source_file: ProjectPath) -> Finding {
    #[cfg(test)]
    ERROR_CONVERSIONS.with(|count| count.set(count.get() + 1));
    let pointer = error.instance_path().to_string();
    let (code, family) = match error.kind() {
        ValidationErrorKind::AdditionalProperties { .. }
        | ValidationErrorKind::UnevaluatedProperties { .. } => (
            "RP_E_CLOSED_SCHEMA_UNKNOWN_PROPERTY",
            "closed_schema_violation",
        ),
        ValidationErrorKind::Required { .. } => {
            ("RP_E_SCHEMA_REQUIRED_PROPERTY", "schema_required_property")
        }
        ValidationErrorKind::Enum { .. } if pointer == "/access/level" => {
            ("RP_E_ACCESS_LEVEL_UNKNOWN", "access_level_enum")
        }
        ValidationErrorKind::Enum { .. } => ("RP_E_SCHEMA_ENUM", "schema_enum"),
        ValidationErrorKind::Constant { .. } => ("RP_E_SCHEMA_CONST", "schema_const"),
        ValidationErrorKind::Pattern { .. }
        | ValidationErrorKind::BacktrackLimitExceeded { .. }
        | ValidationErrorKind::RegexEngineFailure { .. } => {
            ("RP_E_SCHEMA_PATTERN", "schema_pattern")
        }
        ValidationErrorKind::Format { .. } => ("RP_E_SCHEMA_FORMAT", "schema_format"),
        ValidationErrorKind::Type { .. } => ("RP_E_SCHEMA_TYPE", "schema_type"),
        ValidationErrorKind::MinItems { .. } => ("RP_E_SCHEMA_MIN_ITEMS", "schema_min_items"),
        ValidationErrorKind::MaxItems { .. } => ("RP_E_SCHEMA_MAX_ITEMS", "schema_max_items"),
        ValidationErrorKind::MinProperties { .. } => {
            ("RP_E_SCHEMA_MIN_PROPERTIES", "schema_min_properties")
        }
        ValidationErrorKind::MaxProperties { .. } => {
            ("RP_E_SCHEMA_MAX_PROPERTIES", "schema_max_properties")
        }
        ValidationErrorKind::MinLength { .. } => ("RP_E_SCHEMA_MIN_LENGTH", "schema_min_length"),
        ValidationErrorKind::MaxLength { .. } => ("RP_E_SCHEMA_MAX_LENGTH", "schema_max_length"),
        ValidationErrorKind::UniqueItems => ("RP_E_SCHEMA_UNIQUE_ITEMS", "schema_unique_items"),
        ValidationErrorKind::OneOfNotValid { .. }
        | ValidationErrorKind::OneOfMultipleValid { .. } => {
            ("RP_E_SCHEMA_DISCRIMINATOR", "schema_discriminator")
        }
        ValidationErrorKind::Minimum { .. }
        | ValidationErrorKind::Maximum { .. }
        | ValidationErrorKind::ExclusiveMinimum { .. }
        | ValidationErrorKind::ExclusiveMaximum { .. }
        | ValidationErrorKind::MultipleOf { .. } => {
            ("RP_E_SCHEMA_NUMBER_RANGE", "schema_number_range")
        }
        _ => ("RP_E_SCHEMA_DISCRIMINATOR", "schema_discriminator"),
    };

    Finding::new(
        code,
        family,
        Severity::Error,
        schema_message(error),
        Some(source_file),
        pointer,
    )
}

fn schema_message(error: &jsonschema::ValidationError<'_>) -> String {
    match error.kind() {
        ValidationErrorKind::Required { property } => {
            format!("required property {property} is missing")
        }
        ValidationErrorKind::AdditionalProperties { unexpected }
        | ValidationErrorKind::UnevaluatedProperties { unexpected } => {
            format!(
                "unknown properties are not allowed: {}",
                unexpected.join(", ")
            )
        }
        ValidationErrorKind::Format { format } => format!("value does not satisfy format {format}"),
        ValidationErrorKind::Enum { .. } => "value is not in the allowed enumeration".to_string(),
        ValidationErrorKind::Pattern { .. } => {
            "value does not match the required pattern".to_string()
        }
        ValidationErrorKind::Type { .. } => "value has the wrong JSON type".to_string(),
        ValidationErrorKind::OneOfNotValid { .. } => {
            "value does not match the selected schema branch".to_string()
        }
        ValidationErrorKind::OneOfMultipleValid { .. } => {
            "value matches more than one schema branch".to_string()
        }
        _ => format!("JSON Schema keyword {} failed", error.kind().keyword()),
    }
}

fn schema_dispatch_finding(source_file: ProjectPath) -> Finding {
    Finding::new(
        "RP_E_OBJECT_SCHEMA_DISPATCH_UNKNOWN",
        "schema_dispatch",
        Severity::Error,
        "object schema is missing or is not in the embedded v1 catalog",
        Some(source_file),
        "/schema",
    )
}

#[derive(Debug)]
pub struct SchemaBundleError {
    message: String,
}

impl SchemaBundleError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn from_display(error: impl fmt::Display) -> Self {
        Self::new(error.to_string())
    }
}

impl fmt::Display for SchemaBundleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SchemaBundleError {}

#[cfg(test)]
mod findings_cap_tests {
    use super::*;
    #[test]
    fn schema_iterator_and_conversion_stop_on_overflow_and_stay_cancelled() {
        let bundle = SchemaBundle::new().unwrap();
        let instance = json!({"schema":"rp/node-revision/v1", "kind":"Question"});
        let mut findings = Findings::new(3);
        ERROR_VISITS.with(|n| n.set(0));
        ERROR_CONVERSIONS.with(|n| n.set(0));
        assert!(!bundle.validate_into(
            &instance,
            ProjectPath::new("instance.yaml").unwrap(),
            &mut findings
        ));
        assert_eq!(ERROR_VISITS.with(std::cell::Cell::get), 4);
        assert_eq!(ERROR_CONVERSIONS.with(std::cell::Cell::get), 3);
        assert!(!bundle.validate_into(
            &instance,
            ProjectPath::new("later.yaml").unwrap(),
            &mut findings
        ));
        assert_eq!(ERROR_VISITS.with(std::cell::Cell::get), 4);
        assert_eq!(ERROR_CONVERSIONS.with(std::cell::Cell::get), 3);
    }
    #[test]
    fn large_logical_selection_is_bounded_before_generic_schema_iteration() {
        let bundle = SchemaBundle::new().unwrap();
        let instance =
            json!({"schema":"rp/claim-chain-snapshot/v1", "node_revisions":vec!["logical"; 10000]});
        let mut findings = Findings::new(3);
        ERROR_VISITS.with(|n| n.set(0));
        assert!(!bundle.validate_into(
            &instance,
            ProjectPath::new("instance.yaml").unwrap(),
            &mut findings
        ));
        assert_eq!(ERROR_VISITS.with(std::cell::Cell::get), 0);
        let (retained, error, complete) = findings.finish();
        assert!(error && !complete);
        assert_eq!(retained.len(), 3);
        assert_eq!(
            retained[0].error_code,
            "RP_E_CLAIM_CHAIN_REQUIRES_EXACT_REVISION"
        );
    }
}
