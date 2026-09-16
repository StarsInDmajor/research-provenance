use rp_core::{ProjectPath, YamlLimits, parse_restricted_yaml};
use serde_json::json;

fn parse(input: &[u8]) -> Result<rp_core::ParsedYaml, rp_core::Finding> {
    parse_restricted_yaml(
        input,
        ProjectPath::new(".research/project.yaml").unwrap(),
        &YamlLimits::default(),
    )
}

fn assert_code(input: &[u8], code: &str) -> rp_core::Finding {
    let finding = parse(input).expect_err("input must be rejected");
    assert_eq!(finding.error_code, code);
    finding
}

#[test]
fn accepts_utf8_with_one_optional_bom_and_rejects_invalid_utf8() {
    let parsed = parse(b"\xef\xbb\xbfschema: rp/project/v1\n").unwrap();
    assert_eq!(parsed.value, json!({"schema": "rp/project/v1"}));

    assert_code(b"schema: \xff\n", "RP_E_YAML_INVALID_UTF8");
}

#[test]
fn requires_exactly_one_non_null_mapping_document() {
    assert_code(b"---\na: 1\n---\nb: 2\n", "RP_E_YAML_MULTIPLE_DOCUMENTS");
    assert_code(b"- a\n- b\n", "RP_E_YAML_ROOT_NOT_MAPPING");
    assert_code(b"null\n", "RP_E_YAML_ROOT_NOT_MAPPING");
}

#[test]
fn rejects_duplicate_keys_before_construction() {
    let finding = assert_code(b"a: 1\na: 2\n", "RP_E_YAML_DUPLICATE_KEY");
    assert_eq!(finding.json_pointer, "/a");
    assert_eq!(
        finding.source_file.unwrap().as_str(),
        ".research/project.yaml"
    );
}

#[test]
fn rejects_anchors_aliases_merge_keys_tags_and_complex_keys() {
    assert_code(b"a: &x value\nb: *x\n", "RP_E_YAML_ALIAS_FORBIDDEN");
    assert_code(b"<<: {a: b}\n", "RP_E_YAML_MERGE_KEY_FORBIDDEN");
    assert_code(b"a: !custom value\n", "RP_E_YAML_CUSTOM_TAG_FORBIDDEN");
    assert_code(b"? [a, b]\n: value\n", "RP_E_YAML_COMPLEX_KEY_FORBIDDEN");
}

#[test]
fn constructs_yaml_1_2_core_scalars_without_yaml_1_1_booleans() {
    let parsed = parse(
        br#"null_a: null
null_b: ~
true_value: TRUE
false_value: false
decimal: 12
leading_decimal: 012
hexadecimal: 0x10
octal: 0o10
float_value: 1.5
exponent: 1e2
yaml_11_yes: yes
yaml_11_on: on
quoted_true: "true"
"#,
    )
    .unwrap();

    assert_eq!(
        parsed.value,
        json!({
            "null_a": null,
            "null_b": null,
            "true_value": true,
            "false_value": false,
            "decimal": 12,
            "leading_decimal": 12,
            "hexadecimal": 16,
            "octal": 8,
            "float_value": 1.5,
            "exponent": 100.0,
            "yaml_11_yes": "yes",
            "yaml_11_on": "on",
            "quoted_true": "true"
        })
    );
}

#[test]
fn flow_depth_preflight_ignores_brackets_inside_block_scalars() {
    let text = format!("statement: |\n  {}\n", "[".repeat(100));
    let parsed = parse(text.as_bytes()).unwrap();

    assert_eq!(parsed.value["statement"], format!("{}\n", "[".repeat(100)));
}

#[test]
fn enforces_depth_scalar_and_collection_limits() {
    let source = ProjectPath::new(".research/project.yaml").unwrap();

    let depth_error = parse_restricted_yaml(
        b"a: [[[[[value]]]]]\n",
        source.clone(),
        &YamlLimits {
            max_depth: 4,
            ..YamlLimits::default()
        },
    )
    .unwrap_err();
    assert_eq!(
        depth_error.error_code,
        "RP_E_RESOURCE_NESTING_DEPTH_EXCEEDED"
    );

    let scalar_error = parse_restricted_yaml(
        b"a: abcde\n",
        source.clone(),
        &YamlLimits {
            max_scalar_bytes: 4,
            ..YamlLimits::default()
        },
    )
    .unwrap_err();
    assert_eq!(
        scalar_error.error_code,
        "RP_E_RESOURCE_SCALAR_LENGTH_EXCEEDED"
    );

    let mapping_error = parse_restricted_yaml(
        b"a: 1\nb: 2\n",
        source.clone(),
        &YamlLimits {
            max_mapping_keys: 1,
            ..YamlLimits::default()
        },
    )
    .unwrap_err();
    assert_eq!(
        mapping_error.error_code,
        "RP_E_RESOURCE_MAPPING_KEYS_EXCEEDED"
    );

    let sequence_error = parse_restricted_yaml(
        b"a: [1, 2]\n",
        source,
        &YamlLimits {
            max_sequence_items: 1,
            ..YamlLimits::default()
        },
    )
    .unwrap_err();
    assert_eq!(
        sequence_error.error_code,
        "RP_E_RESOURCE_SEQUENCE_ITEMS_EXCEEDED"
    );
}

#[test]
fn records_stable_one_based_value_spans() {
    let parsed = parse(b"alpha:\n  beta: value\n").unwrap();
    let span = parsed.span("/alpha/beta").expect("value span");

    assert_eq!(span.line, 2);
    assert_eq!(span.column, 9);
    assert_eq!(span.byte_offset, 15);
}

#[test]
fn malformed_deep_input_returns_a_bounded_finding_without_panicking() {
    let input = format!("a: {}", "[".repeat(1_000));
    let result = std::panic::catch_unwind(|| parse(input.as_bytes()));

    assert!(result.is_ok(), "restricted parser must not panic");
    assert_eq!(
        result.unwrap().unwrap_err().error_code,
        "RP_E_RESOURCE_NESTING_DEPTH_EXCEEDED"
    );
}
