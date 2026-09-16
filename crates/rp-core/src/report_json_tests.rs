use super::*;

#[test]
fn project_node_meter_charges_failures_and_stops_before_overflow() {
    let mut remaining = 3;
    assert_eq!(
        parse_report_json_metered(
            b"{\"a\":0,\"a\":1}",
            &ReportJsonLimits::default(),
            &mut remaining
        ),
        Err(ReportJsonError::DuplicateKey)
    );
    assert_eq!(remaining, 1);
    assert_eq!(
        parse_report_json_metered(b"{\"b\":0}", &ReportJsonLimits::default(), &mut remaining),
        Err(ReportJsonError::TooManyNodes)
    );
    assert_eq!(remaining, 0);
    assert_eq!(
        parse_report_json_metered(b"{}", &ReportJsonLimits::default(), &mut remaining),
        Err(ReportJsonError::TooManyNodes)
    );
    let mut exact = 2;
    assert!(
        parse_report_json_metered(b"{\"a\":0}", &ReportJsonLimits::default(), &mut exact).is_ok()
    );
    assert_eq!(exact, 0);
    let mut syntax = 3;
    assert_eq!(
        parse_report_json_metered(b"{\"a\":", &ReportJsonLimits::default(), &mut syntax),
        Err(ReportJsonError::InvalidJson)
    );
    assert_eq!(syntax, 1);
}

use serde_json::json;

fn parse(input: &str) -> Result<Value, ReportJsonError> {
    parse_report_json(input.as_bytes(), &ReportJsonLimits::default())
}

fn limits() -> ReportJsonLimits {
    ReportJsonLimits::default()
}

fn check_boundary(exact: &str, over: &str, limits: &ReportJsonLimits, error: ReportJsonError) {
    assert!(parse_report_json(exact.as_bytes(), limits).is_ok());
    assert_eq!(parse_report_json(over.as_bytes(), limits), Err(error));
}

fn nested(depth: usize, kind: usize) -> String {
    let mut input = String::from("{\"x\":");
    let mut closes = String::new();
    for level in 1..depth {
        if kind == 0 || (kind == 2 && level % 2 == 0) {
            input.push_str("{\"x\":");
            closes.push('}');
        } else {
            input.push('[');
            closes.push(']');
        }
    }
    input.push_str("null");
    input.extend(closes.chars().rev());
    input.push('}');
    input
}

fn mapping(keys: usize) -> String {
    let entries: Vec<_> = (0..keys).map(|n| format!("\"k{n}\":null")).collect();
    format!("{{{}}}", entries.join(","))
}

fn array(items: usize) -> String {
    format!("[{}]", vec!["null"; items].join(","))
}

#[test]
fn accepts_json_controls_and_preserves_serde_json_number_semantics() {
    let input = r#" {"numbers":[-9223372036854775808,18446744073709551615,18446744073709551616,333333333.33333329,1E30,4.50,2e-3,-0,1e-999],"text":"€\u0000\n\t\"\\\/\ud83d\ude00","values":[null,true,false,{},[]]} "#;
    let actual = parse(input).unwrap();
    let expected: Value = serde_json::from_str(input).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        crate::canonicalize_jcs(&actual).unwrap(),
        crate::canonicalize_jcs(&expected).unwrap()
    );
    assert_eq!(parse(" \r\n\t{} \r\n\t").unwrap(), json!({}));
    assert!(parse(r#"{"a":{"x":1},"b":{"x":2},"é":0,"e\u0301":1,"A":2}"#).is_ok());
    assert!(parse(r#"{"text":"NaN Infinity --- # // /* {} [] \ufeff"}"#).is_ok());
}

#[test]
fn rejects_duplicate_decoded_keys_at_every_container_position() {
    for input in [
        r#"{"a":1,"a":2}"#,
        r#"{"a":1,"\u0061":2}"#,
        r#"{"\u0061":1,"a":2}"#,
        r#"{"é":1,"\u00e9":2}"#,
        r#"{"😀":1,"\ud83d\ude00":2}"#,
        r#"{"\/":1,"/":2}"#,
        r#"{"\n":1,"\u000a":2}"#,
        r#"{"":1,"":2}"#,
        r#"{"nested":{"a":1,"a":2}}"#,
        r#"{"nested":[{"a":1,"a":2}]}"#,
    ] {
        assert_eq!(parse(input), Err(ReportJsonError::DuplicateKey));
    }
}

#[test]
fn rejects_duplicate_before_reading_or_inserting_its_value() {
    assert_eq!(
        parse(r#"{"a":1,"a":NOT_JSON}"#),
        Err(ReportJsonError::DuplicateKey)
    );
}

#[test]
fn requires_a_mapping_root() {
    for input in [
        "[]", "[{}]", "null", "true", "false", "42", "1.2", "\"text\"",
    ] {
        assert_eq!(parse(input), Err(ReportJsonError::RootNotMapping));
    }
}

#[test]
fn rejects_non_json_syntax_nonfinite_overflow_and_trailing_data() {
    for input in [
        "",
        " ",
        "---\na: 1",
        "a: 1",
        "{'a':1}",
        "{a:1}",
        "{\"a\":1,}",
        "{\"a\":[1,]}",
        "{\"a\":NaN}",
        "{\"a\":Infinity}",
        "{\"a\":-Infinity}",
        "{\"a\":1e400}",
        "{\"a\":-1e400}",
        "{\"a\":01}",
        "{\"a\":+1}",
        "{\"a\":0x10}",
        "{\"a\":.1}",
        "{\"a\":1.}",
        "{\"a\":True}",
        "{}{}",
        "{} null",
        "{} trailing",
        "{}\0",
        "{}\u{a0}",
        "{}\u{feff}",
        "// comment\n{}",
        "/* comment */{}",
        "{}// comment",
        "{} # comment",
        "{\"a\":\"\n\"}",
        "{\"a\":\"\u{0}\"}",
        r#"{"a":"\x41"}"#,
        r#"{"a":"\ud800"}"#,
        r#"{"a":"\udc00"}"#,
        "{\"a\":",
        "{",
    ] {
        assert_eq!(
            parse(input),
            Err(ReportJsonError::InvalidJson),
            "input case: {input:?}"
        );
    }
    assert_eq!(
        parse(&format!("{{\"n\":{}}}", "9".repeat(400))),
        Err(ReportJsonError::InvalidJson)
    );
}

#[test]
fn rejects_bom_and_invalid_utf8_even_in_trailing_data() {
    assert_eq!(parse("\u{feff}{}"), Err(ReportJsonError::BomForbidden));
    assert_eq!(
        parse("\u{feff}\u{feff}{}"),
        Err(ReportJsonError::BomForbidden)
    );
    for bytes in [
        b"{\"a\":\"\xff\"}".as_slice(),
        b"{\"\xff\":1}",
        b"{}\xff",
        b"\xff\xfe{}",
    ] {
        assert_eq!(
            parse_report_json(bytes, &limits()),
            Err(ReportJsonError::InvalidUtf8)
        );
    }
}

#[test]
fn document_byte_limit_is_inclusive_and_counts_whitespace() {
    let custom = ReportJsonLimits {
        max_file_bytes: 3,
        ..limits()
    };
    check_boundary("{} ", "{}  ", &custom, ReportJsonError::DocumentTooLarge);
    let exact = format!("{{}}{}", " ".repeat(limits().max_file_bytes - 2));
    check_boundary(
        &exact,
        &(exact.clone() + " "),
        &limits(),
        ReportJsonError::DocumentTooLarge,
    );
}

#[test]
fn depth_limit_counts_objects_arrays_and_mixed_containers_not_scalars() {
    for kind in 0..3 {
        let custom = ReportJsonLimits {
            max_depth: 3,
            ..limits()
        };
        check_boundary(
            &nested(3, kind),
            &nested(4, kind),
            &custom,
            ReportJsonError::DepthExceeded,
        );
        check_boundary(
            &nested(64, kind),
            &nested(65, kind),
            &limits(),
            ReportJsonError::DepthExceeded,
        );
    }
    let custom = ReportJsonLimits {
        max_depth: 1,
        ..limits()
    };
    check_boundary(
        r#"{"x":null}"#,
        r#"{"x":[]}"#,
        &custom,
        ReportJsonError::DepthExceeded,
    );
    assert!(parse_report_json(b"{}", &custom).is_ok());
}

#[test]
fn scalar_limit_uses_decoded_utf8_bytes_for_values_and_keys() {
    let custom = ReportJsonLimits {
        max_scalar_bytes: 4,
        ..limits()
    };
    for (exact, over) in [
        (r#"{"x":"abcd"}"#, r#"{"x":"abcde"}"#),
        (r#"{"x":"\ud83d\ude00"}"#, r#"{"x":"\ud83d\ude00a"}"#),
        (r#"{"x":"éé"}"#, r#"{"x":"ééa"}"#),
        (r#"{"\ud83d\ude00":0}"#, r#"{"\ud83d\ude00a":0}"#),
        (r#"{"abcd":0}"#, r#"{"abcde":0}"#),
    ] {
        check_boundary(exact, over, &custom, ReportJsonError::ScalarTooLarge);
    }
    let size = limits().max_scalar_bytes;
    for key in [false, true] {
        let document = |n| {
            if key {
                format!("{{\"{}\":0}}", "a".repeat(n))
            } else {
                format!("{{\"x\":\"{}\"}}", "a".repeat(n))
            }
        };
        check_boundary(
            &document(size),
            &document(size + 1),
            &limits(),
            ReportJsonError::ScalarTooLarge,
        );
    }
    let exact = format!("{{\"x\":\"{}\"}}", "\\u0061".repeat(size));
    let over = format!("{{\"x\":\"{}a\"}}", "\\u0061".repeat(size));
    check_boundary(&exact, &over, &limits(), ReportJsonError::ScalarTooLarge);
}

#[test]
fn mapping_key_limit_is_per_map_and_inclusive() {
    let custom = ReportJsonLimits {
        max_mapping_keys: 2,
        ..limits()
    };
    check_boundary(
        &mapping(2),
        &mapping(3),
        &custom,
        ReportJsonError::TooManyMappingKeys,
    );
    assert!(parse_report_json(br#"{"a":{"x":1,"y":2},"b":{"x":3,"y":4}}"#, &custom).is_ok());
    check_boundary(
        &mapping(4096),
        &mapping(4097),
        &limits(),
        ReportJsonError::TooManyMappingKeys,
    );
}

#[test]
fn array_item_limit_is_per_array_and_inclusive() {
    let custom = ReportJsonLimits {
        max_sequence_items: 2,
        ..limits()
    };
    check_boundary(
        r#"{"x":[1,2]}"#,
        r#"{"x":[1,2,3]}"#,
        &custom,
        ReportJsonError::TooManySequenceItems,
    );
    assert!(parse_report_json(br#"{"x":[[1,2],[3,4]]}"#, &custom).is_ok());
    check_boundary(
        &format!("{{\"x\":{}}}", array(65536)),
        &format!("{{\"x\":{}}}", array(65537)),
        &limits(),
        ReportJsonError::TooManySequenceItems,
    );
}

#[test]
fn node_limit_includes_each_value_and_container_but_not_keys() {
    let custom = ReportJsonLimits {
        max_total_nodes: 6,
        ..limits()
    };
    check_boundary(
        r#"{"a":[null,{}],"b":true,"c":"s"}"#,
        r#"{"a":[null,{}],"b":true,"c":"s","d":0}"#,
        &custom,
        ReportJsonError::TooManyNodes,
    );
    let custom = ReportJsonLimits {
        max_total_nodes: 1,
        ..limits()
    };
    check_boundary("{}", r#"{"a":{}}"#, &custom, ReportJsonError::TooManyNodes);
    let document = |last| format!("{{\"a\":{},\"b\":{}}}", array(65536), array(last));
    check_boundary(
        &document(65533),
        &document(65534),
        &limits(),
        ReportJsonError::TooManyNodes,
    );
}

#[test]
fn limits_abort_before_deserializing_excess_children() {
    for (input, custom, error) in [
        (
            r#"{"a":null,"b":INVALID}"#,
            ReportJsonLimits {
                max_mapping_keys: 1,
                ..limits()
            },
            ReportJsonError::TooManyMappingKeys,
        ),
        (
            r#"{"a":[null,INVALID]}"#,
            ReportJsonLimits {
                max_sequence_items: 1,
                ..limits()
            },
            ReportJsonError::TooManySequenceItems,
        ),
        (
            r#"{"a":INVALID}"#,
            ReportJsonLimits {
                max_total_nodes: 1,
                ..limits()
            },
            ReportJsonError::TooManyNodes,
        ),
        (
            r#"{"a":[INVALID]}"#,
            ReportJsonLimits {
                max_depth: 1,
                ..limits()
            },
            ReportJsonError::DepthExceeded,
        ),
    ] {
        assert_eq!(parse_report_json(input.as_bytes(), &custom), Err(error));
    }
}

#[test]
fn zero_limits_have_no_hidden_minimum_or_off_by_one() {
    for (input, custom, error) in [
        (
            "{}",
            ReportJsonLimits {
                max_file_bytes: 0,
                ..limits()
            },
            ReportJsonError::DocumentTooLarge,
        ),
        (
            "{}",
            ReportJsonLimits {
                max_depth: 0,
                ..limits()
            },
            ReportJsonError::DepthExceeded,
        ),
        (
            "{}",
            ReportJsonLimits {
                max_total_nodes: 0,
                ..limits()
            },
            ReportJsonError::TooManyNodes,
        ),
        (
            r#"{"":null}"#,
            ReportJsonLimits {
                max_mapping_keys: 0,
                ..limits()
            },
            ReportJsonError::TooManyMappingKeys,
        ),
        (
            r#"{"x":[null]}"#,
            ReportJsonLimits {
                max_sequence_items: 0,
                ..limits()
            },
            ReportJsonError::TooManySequenceItems,
        ),
        (
            r#"{"":"x"}"#,
            ReportJsonLimits {
                max_scalar_bytes: 0,
                ..limits()
            },
            ReportJsonError::ScalarTooLarge,
        ),
    ] {
        assert_eq!(parse_report_json(input.as_bytes(), &custom), Err(error));
    }
    assert!(
        parse_report_json(
            b"{}",
            &ReportJsonLimits {
                max_mapping_keys: 0,
                ..limits()
            }
        )
        .is_ok()
    );
    assert!(
        parse_report_json(
            br#"{"x":[]}"#,
            &ReportJsonLimits {
                max_sequence_items: 0,
                ..limits()
            }
        )
        .is_ok()
    );
    assert!(
        parse_report_json(
            br#"{"":""}"#,
            &ReportJsonLimits {
                max_scalar_bytes: 0,
                ..limits()
            }
        )
        .is_ok()
    );
}

#[test]
fn callers_cannot_raise_any_hard_ceiling() {
    let raised = ReportJsonLimits {
        max_file_bytes: usize::MAX,
        max_depth: usize::MAX,
        max_scalar_bytes: usize::MAX,
        max_mapping_keys: usize::MAX,
        max_sequence_items: usize::MAX,
        max_total_nodes: usize::MAX,
    };
    assert!(parse_report_json(b"{}", &raised).is_ok());
    for (input, error) in [
        (
            format!("{{}}{}", " ".repeat(4 * 1024 * 1024 - 1)),
            ReportJsonError::DocumentTooLarge,
        ),
        (nested(65, 2), ReportJsonError::DepthExceeded),
        (
            format!("{{\"x\":\"{}\"}}", "a".repeat(256 * 1024 + 1)),
            ReportJsonError::ScalarTooLarge,
        ),
        (mapping(4097), ReportJsonError::TooManyMappingKeys),
        (
            format!("{{\"x\":{}}}", array(65537)),
            ReportJsonError::TooManySequenceItems,
        ),
        (
            format!("{{\"a\":{},\"b\":{}}}", array(65536), array(65534)),
            ReportJsonError::TooManyNodes,
        ),
    ] {
        assert_eq!(parse_report_json(input.as_bytes(), &raised), Err(error));
    }
}

#[test]
fn errors_never_retain_payload_or_keys() {
    for input in [
        r#"{"private-payload":0,"private-payload":1}"#,
        r#"{"private-payload":INVALID}"#,
        r#""private-payload""#,
    ] {
        let error = parse(input).unwrap_err();
        assert!(!format!("{error} {error:?}").contains("private-payload"));
        assert!(std::error::Error::source(&error).is_none());
    }
}
