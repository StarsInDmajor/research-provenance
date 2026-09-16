use rp_core::{canonicalize_jcs, jcs_sha256};
use serde_json::json;

#[test]
fn matches_the_rfc_8785_serialization_vector() {
    let value: serde_json::Value = serde_json::from_str(
        r#"{"numbers":[333333333.33333329,1E30,4.50,2e-3,0.000000000000000000000000001],"string":"€$\u000f\nA'B\"\\\"/","literals":[null,true,false]}"#,
    )
    .unwrap();

    let canonical = String::from_utf8(canonicalize_jcs(&value).unwrap()).unwrap();
    assert_eq!(
        canonical,
        "{\"literals\":[null,true,false],\"numbers\":[333333333.3333333,1e+30,4.5,0.002,1e-27],\"string\":\"€$\\u000f\\nA'B\\\"\\\\\\\"/\"}"
    );
}

#[test]
fn orders_object_keys_by_utf16_code_units() {
    let value = json!({
        "\u{20ac}": "euro",
        "\r": "carriage return",
        "\u{fb33}": "hebrew",
        "1": "one",
        "😀": "grinning face",
        "\u{0080}": "control",
        "ö": "latin",
    });

    let canonical = String::from_utf8(canonicalize_jcs(&value).unwrap()).unwrap();
    assert_eq!(
        canonical,
        "{\"\\r\":\"carriage return\",\"1\":\"one\",\"\":\"control\",\"ö\":\"latin\",\"€\":\"euro\",\"😀\":\"grinning face\",\"דּ\":\"hebrew\"}"
    );
}

#[test]
fn prefixes_the_sha256_of_the_canonical_bytes() {
    let first = json!({"b": 2, "a": 1});
    let second = json!({"a": 1, "b": 2});

    assert_eq!(
        jcs_sha256(&first).unwrap(),
        "sha256:43258cff783fe7036d8a43033f830adfc60ec037382473548ac742b888292777"
    );
    assert_eq!(jcs_sha256(&first).unwrap(), jcs_sha256(&second).unwrap());
}
