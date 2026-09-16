mod common;

use std::fs;

use common::{TempProject, fixture_root, mutate_object, parse_yaml_file, replace, suite_file};
use rp_core::{
    ProjectLimits, ProjectPath, SchemaBundle, Severity, initialize_project, validate_project,
    validate_project_with_limits,
};
use serde_json::{Value, json};

#[test]
fn all_six_positive_fixture_projects_load_and_validate() {
    let fixtures = [
        ("claim-chain-confirmatory-v1", 42),
        ("claim-chain-evidential-v1", 24),
        ("claim-chain-minimum-v1", 12),
        ("freshness-v1", 8),
        ("overview-v1", 48),
        ("access-closure-v1", 51),
    ];

    for (name, expected_count) in fixtures {
        let report = validate_project(&fixture_root(name)).expect("load fixture project");
        assert!(report.findings.is_empty(), "{name}: {:?}", report.findings);
        assert!(report.stage3_ran, "{name}: Stage 3 did not run");
        assert_eq!(report.canonical_object_count, expected_count, "{name}");
        assert_eq!(
            report.index.as_ref().unwrap().len(),
            expected_count,
            "{name}"
        );
    }
}

#[test]
fn no_id_noncanonical_document_never_enters_the_canonical_index() {
    let policy = json!({
        "schema": "rp/freshness-policy/v1", "version": 1,
        "default_status": "unknown", "rules": []
    });
    assert!(policy.get("id").is_none());
    assert_noncanonical_document_rejected(&policy);
}

#[test]
fn id_bearing_noncanonical_document_never_enters_the_canonical_index() {
    let overlay = parse_yaml_file(&suite_file(
        "overview-v1",
        "mutations/overlay-closed-schema-violation.yaml",
    ));
    assert!(overlay.get("id").is_some());
    assert_noncanonical_document_rejected(&overlay);
}

fn assert_noncanonical_document_rejected(document: &Value) {
    let schemas = SchemaBundle::new().unwrap();
    // These are valid contracts, not malformed canonical objects.
    schemas
        .validate(document, ProjectPath::new("contract.yaml").unwrap())
        .unwrap();
    for relative in ["project.yaml", "references/noncanonical.yaml"] {
        let project = TempProject::empty();
        initialize_project(project.path()).unwrap();
        fs::write(
            project.research(relative),
            serde_json::to_vec(document).unwrap(),
        )
        .unwrap();
        let report = validate_project(project.path()).unwrap();
        assert!(!report.stage3_ran, "{relative}: {:?}", report.findings);
        assert!(report.index.is_none());
        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        let finding = &report.findings[0];
        assert_eq!(finding.error_code, "RP_E_OBJECT_SCHEMA_DISPATCH_UNKNOWN");
        assert_eq!(finding.finding_family, "schema_dispatch");
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(
            finding.source_file.as_ref().unwrap().as_str(),
            format!(".research/{relative}")
        );
        assert_eq!(finding.json_pointer, "/schema");
    }
}

#[test]
fn project_descriptor_requires_project_schema_before_semantics() {
    let project = TempProject::empty();
    initialize_project(project.path()).unwrap();
    fs::write(
        project.research("project.yaml"),
        serde_json::to_vec(&external_reference()).unwrap(),
    )
    .unwrap();
    let report = validate_project(project.path()).unwrap();
    assert!(!report.stage3_ran, "{:?}", report.findings);
    assert!(report.index.is_none());
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    let finding = &report.findings[0];
    assert_eq!(finding.error_code, "RP_E_SCHEMA_CONST");
    assert_eq!(finding.finding_family, "schema_const");
    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(
        finding.source_file.as_ref().unwrap().as_str(),
        ".research/project.yaml"
    );
    assert_eq!(finding.json_pointer, "/schema");
}

#[test]
fn ordinary_wrong_layout_remains_a_stage_three_type_mismatch() {
    let project = TempProject::empty();
    initialize_project(project.path()).unwrap();
    fs::write(
        project.research("artifacts/reference--ref_01J00000000000000000000003.yaml"),
        serde_json::to_vec(&external_reference()).unwrap(),
    )
    .unwrap();
    let report = validate_project(project.path()).unwrap();
    assert!(report.stage3_ran);
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    assert_eq!(
        report.findings[0].error_code,
        "RP_E_REFERENCE_TYPE_MISMATCH"
    );
    assert_eq!(report.findings[0].finding_family, "reference_integrity");
    assert_eq!(report.index.unwrap().len(), 2);
}

fn external_reference() -> Value {
    json!({
        "schema": "rp/external-reference/v1", "id": "ref_01J00000000000000000000003",
        "type": "https", "canonical_uri": "https://example.invalid/reference",
        "foreign_key": null, "display_label": "Reference", "retrieved_at": "2026-01-01T00:00:00Z",
        "source_version": {"identifier": "v1"}, "trust_tags": [],
        "access": {"level": "internal", "compartments": []}
    })
}

fn extension_edge_project() -> TempProject {
    let project = TempProject::empty();
    initialize_project(project.path()).unwrap();
    fs::write(
        project.research("references/reference--ref_01J00000000000000000000003.yaml"),
        serde_json::to_vec(&external_reference()).unwrap(),
    )
    .unwrap();
    add_extension_edges(&project);
    project
}

fn add_extension_edges(project: &TempProject) {
    let schema_path = project.research("schemas/loader/refs/v1.schema.json");
    fs::create_dir_all(schema_path.parent().unwrap()).unwrap();
    fs::write(
        schema_path,
        serde_json::to_vec(&json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object", "required": ["targets"], "additionalProperties": false,
            "properties": {"targets": {"type": "array", "items": {"type": "string"}}},
            "x-rp-semantic-references": [{
                "pointer_template": "/targets/*", "target_schema": "rp/external-reference/v1"
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    mutate_object(&project.research("project.yaml"), |object| {
        object["schema_policy"]["allowed_extensions"]
            .as_array_mut()
            .unwrap()
            .push(json!("loader.refs/v1"));
        object["extensions"]["loader.refs/v1"] = json!({"targets": [
            "ref_01J00000000000000000000003", "ref_01J00000000000000000000003"
        ]});
    });
}

fn assert_edge_budget(project: &TempProject, boundary: usize) {
    let report = validate_project_with_limits(
        project.path(),
        ProjectLimits {
            graph_edges: boundary - 1,
            ..ProjectLimits::default()
        },
    )
    .unwrap();
    assert!(report.stage3_ran);
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    assert_eq!(
        report.findings[0].error_code,
        "RP_E_RESOURCE_GRAPH_EDGES_EXCEEDED"
    );
    assert_eq!(report.findings[0].finding_family, "resource_limit");
    let index = report.index.unwrap();
    assert!(index.is_empty(), "over-budget index must not be published");
    assert_eq!(
        index.ids().flat_map(|id| index.references_from(id)).count(),
        0
    );

    let report = validate_project_with_limits(
        project.path(),
        ProjectLimits {
            graph_edges: boundary,
            ..ProjectLimits::default()
        },
    )
    .unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    let index = report.index.unwrap();
    assert_eq!(
        index.ids().flat_map(|id| index.references_from(id)).count(),
        boundary
    );
    let project_id = parse_yaml_file(&project.research("project.yaml"))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(index.access_report(&project_id).unwrap().dependency_count >= 1);
}

#[test]
fn extension_only_edges_obey_budget_and_inclusive_boundary() {
    assert_edge_budget(&extension_edge_project(), 2);
}

#[test]
fn combined_builtin_and_extension_edges_obey_budget_and_inclusive_boundary() {
    let project = TempProject::copy_fixture("overview-v1");
    let baseline = validate_project(project.path()).unwrap();
    assert!(baseline.findings.is_empty());
    let index = baseline.index.unwrap();
    let builtin_count = index.ids().flat_map(|id| index.references_from(id)).count();
    assert!(builtin_count > 2);
    add_extension_edges(&project);
    assert_edge_budget(&project, builtin_count + 2);
}

#[test]
fn canonical_files_are_scanned_in_normalized_deterministic_order() {
    let first = validate_project(&fixture_root("overview-v1"))
        .expect("first load")
        .canonical_paths;
    let second = validate_project(&fixture_root("overview-v1"))
        .expect("second load")
        .canonical_paths;

    assert_eq!(first, second);
    assert!(first.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        first[0].as_str(),
        ".research/assessments/candidate-support-relation-assessment--asm_01J00000000000000000000024.yaml"
    );
    assert_eq!(
        first.last().unwrap().as_str(),
        ".research/threads/primary-efficacy--thd_01J00000000000000000000031.yaml"
    );
}

#[test]
fn filename_id_duplicate_id_and_frozen_state_are_stage_three_findings() {
    let project = TempProject::copy_fixture("freshness-v1");
    let original = project
        .research("records/questions/fresh-review-window--qst_01J00000000000000000000110.yaml");
    let mismatch = project
        .research("records/questions/fresh-review-window--qst_01J00000000000000000000119.yaml");
    fs::rename(&original, &mismatch).expect("rename canonical object");

    let duplicate =
        project.research("records/questions/duplicate--qst_01J00000000000000000000111.yaml");
    fs::copy(
        project
            .research("records/questions/review-due-window--qst_01J00000000000000000000111.yaml"),
        duplicate,
    )
    .expect("copy duplicate canonical object");

    replace(&mismatch, "record_state: frozen", "record_state: draft");

    let report = validate_project(project.path()).expect("validate mutated project");
    assert!(report.stage3_ran);
    let codes: Vec<_> = report
        .findings
        .iter()
        .map(|finding| finding.error_code)
        .collect();
    assert!(codes.contains(&"RP_E_OBJECT_FILENAME_ID_MISMATCH"));
    assert!(codes.contains(&"RP_E_OBJECT_DUPLICATE_ID"));
    assert!(codes.contains(&"RP_E_CANONICAL_DRAFT_FORBIDDEN"));
}

#[test]
fn parser_or_schema_failure_gates_all_project_semantics() {
    let project = TempProject::copy_fixture("freshness-v1");
    let broken = project
        .research("records/questions/fresh-review-window--qst_01J00000000000000000000119.yaml");
    fs::rename(
        project
            .research("records/questions/fresh-review-window--qst_01J00000000000000000000110.yaml"),
        &broken,
    )
    .expect("rename canonical object");
    fs::write(&broken, b"schema: rp/node-revision/v1\nid: [\n").expect("write malformed YAML");

    let report = validate_project(project.path()).expect("validate malformed project");
    assert!(!report.stage3_ran);
    assert!(report.index.is_none());
    assert!(report.findings.iter().any(|finding| {
        finding.error_code.starts_with("RP_E_YAML_")
            || finding.error_code.starts_with("RP_E_SCHEMA_")
    }));
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.error_code == "RP_E_OBJECT_FILENAME_ID_MISMATCH")
    );
}

#[test]
fn schema_failure_also_gates_stage_three_without_identity_secondaries() {
    let project = TempProject::copy_fixture("freshness-v1");
    let broken = project
        .research("records/questions/fresh-review-window--qst_01J00000000000000000000119.yaml");
    fs::rename(
        project
            .research("records/questions/fresh-review-window--qst_01J00000000000000000000110.yaml"),
        &broken,
    )
    .expect("rename canonical object");
    replace(
        &broken,
        "title: Fresh review window control",
        "title_removed_for_schema_test: Fresh review window control",
    );

    let report = validate_project(project.path()).expect("validate schema-invalid project");
    assert!(!report.stage3_ran);
    assert!(report.index.is_none());
    assert!(report.findings.iter().any(|finding| {
        matches!(
            finding.error_code,
            "RP_E_SCHEMA_REQUIRED_PROPERTY" | "RP_E_CLOSED_SCHEMA_UNKNOWN_PROPERTY"
        )
    }));
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.error_code == "RP_E_OBJECT_FILENAME_ID_MISMATCH")
    );
}

#[test]
fn oversized_project_descriptor_is_not_reported_as_missing() {
    let project = TempProject::empty();
    fs::create_dir(project.research("")).expect("create research directory");
    fs::write(project.research("project.yaml"), b"oversized descriptor").expect("write descriptor");

    let report = validate_project_with_limits(
        project.path(),
        ProjectLimits {
            yaml_bytes_per_object: 4,
            ..ProjectLimits::default()
        },
    )
    .expect("scan oversized descriptor");
    let codes: Vec<_> = report
        .findings
        .iter()
        .map(|finding| finding.error_code)
        .collect();
    assert_eq!(codes, ["RP_E_RESOURCE_FILE_SIZE_EXCEEDED"]);
    assert!(!report.stage3_ran);
    assert!(report.index.is_none());
}

#[test]
fn canonical_file_and_total_byte_limits_fail_closed() {
    let project = TempProject::copy_fixture("freshness-v1");
    let report = validate_project_with_limits(
        project.path(),
        ProjectLimits {
            canonical_files: 2,
            ..ProjectLimits::default()
        },
    )
    .expect("bounded scan");
    assert!(!report.stage3_ran);
    assert_eq!(
        report.findings[0].error_code,
        "RP_E_RESOURCE_SCANNED_FILES_EXCEEDED"
    );

    let report = validate_project_with_limits(
        project.path(),
        ProjectLimits {
            whole_project_yaml_bytes: 64,
            ..ProjectLimits::default()
        },
    )
    .expect("bounded read");
    assert!(!report.stage3_ran);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.error_code == "RP_E_RESOURCE_TOTAL_BYTES_EXCEEDED")
    );
}
