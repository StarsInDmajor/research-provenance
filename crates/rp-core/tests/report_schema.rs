//! Synthetic wire examples only: no report generator, signatures, or binding verification.
mod common;

use std::{fs, path::PathBuf, sync::OnceLock};

use common::TempProject;
use rp_core::{
    ProjectPath, ReportJsonLimits, SchemaBundle, Severity, initialize_project, validate_project,
};
use serde_json::{Value, json};

const REPORT: &str = "rp/claim-chain-validation-report/v1";
const FILE: &str = "claim-chain-validation-report.schema.json";
const SCOPE: &str = "claim-chain-subject-pre-binding/v1";
const ULID: &str = "01J00000000000000000000001";
const TYPES: &[(&str, &str)] = &[
    ("rp/project/v1", "proj"),
    ("rp/node-revision/v1", "qst"),
    ("rp/scientific-relation-revision/v1", "rel"),
    ("rp/assessment/v1", "asm"),
    ("rp/research-thread/v1", "thd"),
    ("rp/thread-binding/v1", "tbd"),
    ("rp/claim-chain-snapshot/v1", "cch"),
    ("rp/external-reference/v1", "ref"),
    ("rp/artifact-manifest/v1", "art"),
    ("rp/research-run/v1", "run"),
];

fn bundle() -> &'static SchemaBundle {
    static BUNDLE: OnceLock<SchemaBundle> = OnceLock::new();
    BUNDLE.get_or_init(|| SchemaBundle::new().expect("production offline bundle compiles"))
}

fn digest() -> String {
    format!("sha256:{}", "0".repeat(64))
}

fn entry(object_type: &str, prefix: &str) -> Value {
    json!({"object_type": object_type, "id": format!("{prefix}_{ULID}"),
        "canonical_digest": digest()})
}

fn finding(severity: Severity, context: bool) -> Value {
    let owner = if context {
        json!({"kind": "context", "path": ".research/policies/freshness-v1.yaml"})
    } else {
        json!({"kind": "object", "id": format!("cch_{ULID}")})
    };
    json!({"owner": owner, "error_code": "RP_E_SCHEMA_REQUIRED_PROPERTY",
        "finding_family": "schema_required_property", "severity": severity,
        "json_pointer": "/extensions/example~1v1/a~0b"})
}

fn passed() -> Value {
    json!({
        "schema": REPORT, "scope": SCOPE,
        "subject": {"id": format!("cch_{ULID}"), "canonical_digest": digest(),
            "validation_policy": {"profile": "minimum-v1", "version": 1}},
        "selected_nodes": [entry("rp/node-revision/v1", "qst")],
        "selected_relations": [entry("rp/scientific-relation-revision/v1", "rel")],
        "source_closure": {"algorithm": "rp/source-closure/v1",
            "entries": [entry("rp/node-revision/v1", "qst")],
            "sha256": digest(), "unresolved_ids": []},
        "validation_dependencies": TYPES.iter().map(|(kind, prefix)| entry(kind, prefix)).collect::<Vec<_>>(),
        "validation_context": [{"path": ".research/schemas/example/v1.schema.json", "raw_sha256": digest()}],
        "validator": {"id": "rp-core", "ruleset": SCOPE, "fingerprint": digest()},
        "outcome": {"passed": true, "findings": []}
    })
}

fn failed() -> Value {
    let mut value = passed();
    value["outcome"] = json!({"passed": false, "findings": [finding(Severity::Error, false)]});
    value
}

fn assert_valid(value: &Value) {
    let result = bundle().validate(value, ProjectPath::new("report.json").unwrap());
    assert!(result.is_ok(), "synthetic wire rejected: {result:?}");
}

fn assert_invalid(value: &Value, label: &str) {
    let result = bundle().validate(value, ProjectPath::new("report.json").unwrap());
    assert!(result.is_err(), "accepted invalid wire: {label}");
    assert!(
        result
            .unwrap_err()
            .iter()
            .all(|finding| { finding.error_code != "RP_E_OBJECT_SCHEMA_DISPATCH_UNKNOWN" }),
        "dispatch failure is not behavioral schema rejection: {label}"
    );
}

fn mutation(base: &Value, pointer: &str, replacement: Value) {
    let mut changed = base.clone();
    *changed
        .pointer_mut(pointer)
        .expect("existing mutation target") = replacement;
    assert_invalid(&changed, pointer);
}

#[test]
fn synthetic_passed_failed_and_unresolved_forms_are_valid() {
    assert_valid(&passed());
    assert_valid(&failed());
    let mut value = passed();
    for severity in [Severity::Warning, Severity::Info] {
        for context in [false, true] {
            value["outcome"]["findings"] = json!([finding(severity, context)]);
            assert_valid(&value);
        }
    }
    value = failed();
    value["outcome"]["findings"] = json!([
        finding(Severity::Warning, true),
        finding(Severity::Error, true),
        finding(Severity::Info, false)
    ]);
    for pointer in [
        "/selected_nodes/0/canonical_digest",
        "/selected_relations/0/canonical_digest",
        "/validation_dependencies/0/canonical_digest",
    ] {
        *value.pointer_mut(pointer).unwrap() = Value::Null;
    }
    value["source_closure"]["unresolved_ids"] = json!([format!("proj_{ULID}")]);
    assert_valid(&value);
    for field in [
        "selected_nodes",
        "selected_relations",
        "validation_dependencies",
        "validation_context",
    ] {
        value[field] = json!([]);
    }
    value["source_closure"]["entries"] = json!([]);
    assert_valid(&value);
}

#[test]
fn policy_reuses_all_existing_v1_combinations_without_defaults() {
    for profile in ["minimum-v1", "evidential-v1", "confirmatory-v1"] {
        for mode in [
            None,
            Some("none"),
            Some("artifact-backed"),
            Some("event-backed"),
            Some("artifact-and-event"),
        ] {
            let mut value = passed();
            value["subject"]["validation_policy"]["profile"] = json!(profile);
            if let Some(mode) = mode {
                value["subject"]["validation_policy"]["execution_provenance"] = json!(mode);
            }
            let before = value.clone();
            assert_valid(&value);
            assert_eq!(
                value, before,
                "schema validation must not insert policy defaults"
            );
        }
    }
    for policy in [
        json!({"profile": "evidential-v2", "version": 1}),
        json!({"profile": "minimum-v1", "version": 2}),
        json!({"profile": "minimum-v1", "version": "1"}),
        json!({"profile": "minimum-v1", "version": 1, "execution_provenance": null}),
        json!({"profile": "minimum-v1", "version": 1, "execution_provenance": "automatic"}),
    ] {
        mutation(&passed(), "/subject/validation_policy", policy);
    }
}

#[test]
fn all_mappings_are_closed_and_all_declared_fields_are_required() {
    let mut value = failed();
    value["subject"]["validation_policy"]["execution_provenance"] = json!("none");
    for pointer in [
        "",
        "/subject",
        "/subject/validation_policy",
        "/selected_nodes/0",
        "/selected_relations/0",
        "/source_closure",
        "/source_closure/entries/0",
        "/validation_dependencies/0",
        "/validation_context/0",
        "/validator",
        "/outcome",
        "/outcome/findings/0",
        "/outcome/findings/0/owner",
    ] {
        let keys: Vec<_> = value
            .pointer(pointer)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        for key in keys {
            if key == "execution_provenance" || key == "schema" {
                continue; // Optional policy key; missing schema is dispatch, not behavioral RED.
            }
            let mut changed = value.clone();
            changed
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(&key);
            assert_invalid(&changed, &format!("missing {pointer}/{key}"));
        }
        let mut changed = value.clone();
        changed
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unexpected".into(), json!(true));
        assert_invalid(&changed, &format!("extra {pointer}/unexpected"));
    }
    value["outcome"]["findings"][0]["owner"] = finding(Severity::Error, true)["owner"].clone();
    for owner in [
        json!({"kind": "context"}),
        json!({"path": "policy.yaml"}),
        json!({"kind": "context", "path": "policy.yaml", "id": format!("cch_{ULID}")}),
    ] {
        mutation(&value, "/outcome/findings/0/owner", owner);
    }
}

#[test]
fn local_constants_types_ids_and_digests_are_strict() {
    let value = passed();
    for (pointer, replacement) in [
        ("/scope", json!("project-global/v1")),
        ("/subject/id", json!(format!("qst_{ULID}"))),
        ("/subject/id", json!("cch_not-a-ulid")),
        ("/subject", Value::Null),
        ("/validator/id", json!("other-validator")),
        ("/validator/ruleset", json!("other/v1")),
        ("/source_closure/algorithm", json!("rp/source-closure/v2")),
        ("/selected_nodes", json!({})),
        ("/selected_relations", Value::Null),
        ("/source_closure/entries", json!(false)),
        ("/source_closure/unresolved_ids", json!({})),
        ("/validation_dependencies", json!("none")),
        ("/validation_context", json!({})),
        ("/outcome/passed", json!("true")),
        ("/outcome/findings", Value::Null),
    ] {
        mutation(&value, pointer, replacement);
    }
    for pointer in [
        "/subject/canonical_digest",
        "/selected_nodes/0/canonical_digest",
        "/selected_relations/0/canonical_digest",
        "/source_closure/entries/0/canonical_digest",
        "/source_closure/sha256",
        "/validation_dependencies/0/canonical_digest",
        "/validation_context/0/raw_sha256",
        "/validator/fingerprint",
    ] {
        for bad in [
            json!("sha256:abc"),
            json!(format!("sha256:{}", "A".repeat(64))),
            json!("0".repeat(64)),
            json!(42),
        ] {
            mutation(&value, pointer, bad);
        }
    }
    for pointer in [
        "/subject/canonical_digest",
        "/source_closure/entries/0/canonical_digest",
        "/source_closure/sha256",
        "/validation_context/0/raw_sha256",
        "/validator/fingerprint",
    ] {
        mutation(&value, pointer, Value::Null);
    }
}

#[test]
fn canonical_tuple_union_checks_every_type_id_pair_and_excludes_layer_a() {
    let value = passed();
    for (kind, prefix) in TYPES {
        for pointer in ["/validation_dependencies/0", "/source_closure/entries/0"] {
            let mut valid = value.clone();
            *valid.pointer_mut(pointer).unwrap() = entry(kind, prefix);
            assert_valid(&valid);
            for (other_kind, other_prefix) in TYPES {
                if kind != other_kind {
                    let mut wrong = entry(kind, other_prefix);
                    mutation(&value, pointer, wrong.clone());
                    wrong["canonical_digest"] = Value::Null;
                    mutation(&value, pointer, wrong);
                }
            }
        }
        let mut owner = failed();
        owner["outcome"]["findings"][0]["owner"]["id"] = json!(format!("{prefix}_{ULID}"));
        assert_valid(&owner);
    }
    for prefix in [
        "dset", "qst", "hyp", "obs", "meas", "mth", "tst", "pred", "syn", "int", "dec", "con",
        "blk", "nxt", "clm", "node",
    ] {
        let mut valid = value.clone();
        valid["selected_nodes"][0] = entry("rp/node-revision/v1", prefix);
        assert_valid(&valid);
    }
    mutation(
        &value,
        "/selected_nodes/0",
        entry("rp/scientific-relation-revision/v1", "rel"),
    );
    mutation(
        &value,
        "/selected_relations/0",
        entry("rp/node-revision/v1", "qst"),
    );
    for (kind, prefix) in [
        ("rp/event/v1", "ev"),
        ("rp/model/v1", "model"),
        (REPORT, "cch"),
        ("rp/unknown/v1", "qst"),
        ("node_revision", "qst"),
    ] {
        for pointer in [
            "/selected_nodes/0",
            "/selected_relations/0",
            "/source_closure/entries/0",
            "/validation_dependencies/0",
        ] {
            mutation(&value, pointer, entry(kind, prefix));
        }
    }
    for bad in [
        format!("ev_{ULID}"),
        format!("model_{ULID}"),
        "logical-node".into(),
        format!("qst_{}", "I".repeat(26)),
    ] {
        mutation(&value, "/source_closure/unresolved_ids", json!([bad]));
        mutation(&failed(), "/outcome/findings/0/owner/id", json!(bad));
    }
}

#[test]
fn compact_findings_are_not_cli_findings_and_outcome_tracks_errors_locally() {
    mutation(&passed(), "/outcome/passed", json!(false));
    mutation(&failed(), "/outcome/passed", json!(true));
    for severity in [Severity::Warning, Severity::Info] {
        mutation(&failed(), "/outcome/findings/0/severity", json!(severity));
    }
    for (pointer, replacement) in [
        ("/outcome/findings/0/severity", json!("fatal")),
        ("/outcome/findings/0/severity", json!("Error")),
        ("/outcome/findings/0/error_code", json!("invalid")),
        ("/outcome/findings/0/finding_family", json!("bad-family")),
        ("/outcome/findings/0/owner", json!(format!("cch_{ULID}"))),
        ("/outcome/findings/0/owner/kind", json!("global")),
    ] {
        mutation(&failed(), pointer, replacement);
    }
    for key in [
        "schema",
        "message",
        "source_file",
        "details",
        "timestamp",
        "host",
        "duration",
        "environment",
    ] {
        let mut value = failed();
        value["outcome"]["findings"][0][key] = json!("forbidden");
        assert_invalid(&value, key);
    }
    let mut cli = finding(Severity::Error, false);
    cli.as_object_mut().unwrap().remove("owner");
    cli["schema"] = json!("rp/finding/v1");
    cli["message"] = json!("synthetic CLI finding");
    cli["source_file"] = json!("object.yaml");
    assert_valid(&cli);
    mutation(&failed(), "/outcome/findings/0", cli);
}

#[test]
fn json_pointer_escaping_is_rfc6901_not_a_path_or_uri_fragment() {
    for pointer in ["", "/", "//", "/a~0b/c~1d", "/~01", "/a b/研究", "/~1~0"] {
        let mut value = failed();
        value["outcome"]["findings"][0]["json_pointer"] = json!(pointer);
        assert_valid(&value);
    }
    for pointer in ["a", "#/a", "/~", "/~2", "/a~b", "/~~0"] {
        mutation(
            &failed(),
            "/outcome/findings/0/json_pointer",
            json!(pointer),
        );
    }
    mutation(&failed(), "/outcome/findings/0/json_pointer", Value::Null);
}

#[test]
fn context_paths_are_normalized_contained_literal_project_paths() {
    // No Windows reserved-name/trailing-dot rules: ProjectPath does not impose them.
    for path in [
        "policy.yaml",
        ".research/policies/v1.yaml",
        ".research/schemas/example/v1.schema.json",
        "dir with spaces/研究.json",
        "CON/policy.",
        "dir/a:b.json",
        ".../file",
        "dir/a%20b.json",
        "dir/100%done.json",
        "dir/%41.json",
        "dir/%25.json",
        "dir/%2efile.json",
    ] {
        assert!(ProjectPath::new(path).is_ok(), "{path}");
        let mut value = failed();
        value["validation_context"][0]["path"] = json!(path);
        value["outcome"]["findings"][0]["owner"] = json!({"kind": "context", "path": path});
        assert_valid(&value);
    }
    for path in [
        "",
        "/tmp/policy",
        "https://example.invalid/policy",
        "file:policy",
        "C:policy",
        "C:/policy",
        "./policy",
        "dir/./policy",
        "dir/../policy",
        "../policy",
        "dir//policy",
        "dir/policy/",
        "dir\\policy",
        "dir\0policy",
        "dir\npolicy",
        "dir\u{7f}policy",
        "%2e%2e/policy",
        "dir/%2E/policy",
        "dir/%2fpolicy",
        "dir/%5cpolicy",
        "dir/%00policy",
        "dir/%0Apolicy",
        "%252e%252e/policy",
        "dir/.%2e/policy",
        "dir/%2e./policy",
        "dir/%7fpolicy",
        "dir/%252Fpolicy",
        "dir/%25%32%65/policy",
        "dir/%2%65/policy",
        "dir/%%32e/policy",
        "dir/%%32%65/policy",
        "dir/policy\n",
    ] {
        mutation(&passed(), "/validation_context/0/path", json!(path));
        let mut value = failed();
        value["outcome"]["findings"][0]["owner"] = json!({"kind": "context", "path": path});
        assert_invalid(&value, path);
    }
}

#[test]
fn array_maxima_match_parser_ceiling_without_claiming_aggregate_budget_enforcement() {
    let ceiling = ReportJsonLimits::default().max_sequence_items;
    assert_eq!(ceiling, 65_536);
    for pointer in [
        "/selected_nodes",
        "/selected_relations",
        "/source_closure/entries",
        "/source_closure/unresolved_ids",
        "/validation_dependencies",
        "/validation_context",
        "/outcome/findings",
    ] {
        let mut value = failed();
        value["source_closure"]["unresolved_ids"] = json!([format!("qst_{ULID}")]);
        let item = value.pointer(pointer).unwrap()[0].clone();
        *value.pointer_mut(pointer).unwrap() = json!(vec![item.clone(); ceiling]);
        // Duplicates/sort and aggregate parser budgets are NOT local schema obligations.
        assert_valid(&value);
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(item);
        assert_invalid(&value, pointer);
    }
}

#[test]
fn structurally_correct_forged_digests_still_pass_without_binding_verification() {
    // All examples are synthetic Values. No digest here is a recomputed subject result.
    for pointer in [
        "/subject/canonical_digest",
        "/selected_nodes/0/canonical_digest",
        "/selected_relations/0/canonical_digest",
        "/source_closure/entries/0/canonical_digest",
        "/source_closure/sha256",
        "/validation_dependencies/0/canonical_digest",
        "/validation_context/0/raw_sha256",
        "/validator/fingerprint",
    ] {
        let mut forged = passed();
        *forged.pointer_mut(pointer).unwrap() = json!(format!("sha256:{}", "f".repeat(64)));
        assert_valid(&forged); // C2/C3, not schema, must establish observed bytes and truth.
    }
}

#[test]
fn derived_report_is_contract_only_and_fresh_canonical_placement_fails_closed() {
    let document = passed();
    assert!(document.get("id").is_none());
    assert_valid(&document);
    for relative in [
        "project.yaml",
        "claim-chains/fresh-report.yaml",
        "references/fresh-report.yaml",
        "artifacts/fresh-report.yaml",
    ] {
        let project = TempProject::empty();
        initialize_project(project.path()).unwrap();
        let baseline = validate_project(project.path()).unwrap();
        assert!(baseline.stage3_ran);
        assert!(baseline.findings.is_empty());
        fs::write(
            project.research(relative),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        let report = validate_project(project.path()).unwrap();
        let source = format!(".research/{relative}");
        assert!(
            report
                .canonical_paths
                .iter()
                .any(|path| path.as_str() == source),
            "fresh report disappeared from loader"
        );
        assert!(!report.stage3_ran);
        assert!(report.index.is_none());
        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        let finding = &report.findings[0];
        assert_eq!(finding.error_code, "RP_E_OBJECT_SCHEMA_DISPATCH_UNKNOWN");
        assert_eq!(finding.finding_family, "schema_dispatch");
        assert_eq!(finding.severity, Severity::Error);
        assert_eq!(finding.source_file.as_ref().unwrap().as_str(), source);
        assert_eq!(finding.json_pointer, "/schema");
    }
}

#[test]
fn embedded_report_and_catalog_are_identical_and_reuse_v1_contract_properties() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/v1");
    for name in [FILE, "schema-catalog.json"] {
        let resource = SchemaBundle::resources()
            .iter()
            .find(|resource| resource.name == name)
            .unwrap();
        assert_eq!(resource.bytes, fs::read(source.join(name)).unwrap());
    }
    let catalog: Value =
        serde_json::from_slice(&fs::read(source.join("schema-catalog.json")).unwrap()).unwrap();
    assert_eq!(catalog["contracts"][REPORT], FILE);
    assert!(catalog["schemas"].get(REPORT).is_none());
    assert_eq!(catalog["schemas"].as_object().unwrap().len(), TYPES.len());
    for (kind, _) in TYPES {
        assert!(catalog["schemas"].get(kind).is_some());
    }
    let schema: Value = serde_json::from_slice(&fs::read(source.join(FILE)).unwrap()).unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(
        schema
            .pointer("/properties/subject/properties/validation_policy/$ref")
            .unwrap(),
        "claim-chain-snapshot.schema.json#/properties/validation_policy"
    );
}
