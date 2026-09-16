use std::collections::BTreeMap;

use serde_json::{Map, Number, Value};
use yaml_rust2::{
    parser::{Event, Parser},
    scanner::{Marker, Scanner, TScalarStyle, TokenType},
};

use crate::{Finding, ProjectPath, Severity};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YamlLimits {
    pub max_file_bytes: usize,
    pub max_depth: usize,
    pub max_scalar_bytes: usize,
    pub max_mapping_keys: usize,
    pub max_sequence_items: usize,
}

impl Default for YamlLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 4 * 1024 * 1024,
            max_depth: 64,
            max_scalar_bytes: 256 * 1024,
            max_mapping_keys: 4_096,
            max_sequence_items: 65_536,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    pub byte_offset: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedYaml {
    pub value: Value,
    spans: BTreeMap<String, SourceSpan>,
}

impl ParsedYaml {
    #[must_use]
    pub fn span(&self, json_pointer: &str) -> Option<SourceSpan> {
        self.spans.get(json_pointer).copied()
    }
}

pub fn parse_restricted_yaml(
    bytes: &[u8],
    source_file: ProjectPath,
    limits: &YamlLimits,
) -> Result<ParsedYaml, Finding> {
    parse_restricted_yaml_with_budget(
        bytes,
        source_file,
        limits,
        &crate::ExecutionBudget::default(),
    )
}

pub fn parse_restricted_yaml_with_budget(
    bytes: &[u8],
    source_file: ProjectPath,
    limits: &YamlLimits,
    execution: &crate::ExecutionBudget,
) -> Result<ParsedYaml, Finding> {
    execution
        .checkpoint()
        .map_err(crate::ExecutionStop::finding)?;
    let result = parse_inner(bytes, source_file, limits, execution);
    execution
        .checkpoint()
        .map_err(crate::ExecutionStop::finding)?;
    result
}

fn parse_inner(
    bytes: &[u8],
    source_file: ProjectPath,
    limits: &YamlLimits,
    execution: &crate::ExecutionBudget,
) -> Result<ParsedYaml, Finding> {
    if bytes.len() > limits.max_file_bytes {
        return Err(resource_finding(
            "RP_E_RESOURCE_FILE_SIZE_EXCEEDED",
            "YAML file exceeds the configured byte limit",
            source_file,
            "",
        ));
    }

    let (bytes, bom_bytes) = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        (&bytes[3..], 3)
    } else {
        (bytes, 0)
    };
    let input = std::str::from_utf8(bytes).map_err(|_| {
        parser_finding(
            "RP_E_YAML_INVALID_UTF8",
            "YAML input is not valid UTF-8",
            source_file.clone(),
            "",
        )
    })?;
    if input.starts_with('\u{feff}') {
        return Err(parser_finding(
            "RP_E_YAML_INVALID_UTF8",
            "YAML input contains more than one UTF-8 BOM",
            source_file,
            "",
        ));
    }

    lexical_preflight(input, &source_file, limits, execution)?;
    construct(input, bom_bytes, source_file, limits, execution)
}

fn lexical_preflight(
    input: &str,
    source_file: &ProjectPath,
    limits: &YamlLimits,
    execution: &crate::ExecutionBudget,
) -> Result<(), Finding> {
    preflight_flow_depth(input, source_file, limits, execution)?;

    let mut scanner = Scanner::new(input.chars());
    let mut depth = 0_usize;

    for token in scanner.by_ref() {
        execution
            .checkpoint()
            .map_err(crate::ExecutionStop::finding)?;
        match token.1 {
            TokenType::BlockMappingStart
            | TokenType::BlockSequenceStart
            | TokenType::FlowMappingStart
            | TokenType::FlowSequenceStart => {
                depth += 1;
                if depth > limits.max_depth {
                    return Err(resource_finding(
                        "RP_E_RESOURCE_NESTING_DEPTH_EXCEEDED",
                        "YAML nesting exceeds the configured depth limit",
                        source_file.clone(),
                        "",
                    ));
                }
            }
            TokenType::BlockEnd | TokenType::FlowMappingEnd | TokenType::FlowSequenceEnd => {
                depth = depth.saturating_sub(1);
            }
            TokenType::Scalar(_, scalar) if scalar.len() > limits.max_scalar_bytes => {
                return Err(resource_finding(
                    "RP_E_RESOURCE_SCALAR_LENGTH_EXCEEDED",
                    "YAML scalar exceeds the configured byte limit",
                    source_file.clone(),
                    "",
                ));
            }
            TokenType::Anchor(_) | TokenType::Alias(_) => {
                return Err(parser_finding(
                    "RP_E_YAML_ALIAS_FORBIDDEN",
                    "YAML anchors and aliases are forbidden",
                    source_file.clone(),
                    "",
                ));
            }
            TokenType::Tag(_, _) | TokenType::TagDirective(_, _) => {
                return Err(parser_finding(
                    "RP_E_YAML_CUSTOM_TAG_FORBIDDEN",
                    "YAML tags are forbidden",
                    source_file.clone(),
                    "",
                ));
            }
            _ => {}
        }
    }

    if scanner.get_error().is_some() {
        return Err(parser_finding(
            "RP_E_YAML_ROOT_NOT_MAPPING",
            "YAML syntax could not be constructed as a mapping",
            source_file.clone(),
            "",
        ));
    }
    Ok(())
}

fn preflight_flow_depth(
    input: &str,
    source_file: &ProjectPath,
    limits: &YamlLimits,
    execution: &crate::ExecutionBudget,
) -> Result<(), Finding> {
    let mut depth = 0_usize;
    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut escaped = false;
    let mut block_parent_indent = None;

    for line in input.split_inclusive('\n') {
        execution
            .checkpoint()
            .map_err(crate::ExecutionStop::finding)?;
        let indentation = line
            .chars()
            .take_while(|character| *character == ' ')
            .count();
        if let Some(parent_indent) = block_parent_indent {
            if line.trim().is_empty() || indentation > parent_indent {
                continue;
            }
            block_parent_indent = None;
        }

        let mut prefix = String::new();
        let mut characters = line.chars().peekable();
        while let Some(character) = characters.next() {
            execution
                .checkpoint()
                .map_err(crate::ExecutionStop::finding)?;
            if double_quoted {
                if escaped {
                    escaped = false;
                } else if character == '\\' {
                    escaped = true;
                } else if character == '"' {
                    double_quoted = false;
                }
                continue;
            }
            if single_quoted {
                if character == '\'' {
                    if characters.peek() == Some(&'\'') {
                        let _ = characters.next();
                    } else {
                        single_quoted = false;
                    }
                }
                continue;
            }

            match character {
                '#' => break,
                '"' => double_quoted = true,
                '\'' => single_quoted = true,
                '|' | '>'
                    if prefix.trim_end().ends_with(':') || prefix.trim_end().ends_with('-') =>
                {
                    block_parent_indent = Some(indentation);
                    break;
                }
                '[' | '{' => {
                    depth += 1;
                    if depth > limits.max_depth {
                        return Err(resource_finding(
                            "RP_E_RESOURCE_NESTING_DEPTH_EXCEEDED",
                            "YAML nesting exceeds the configured depth limit",
                            source_file.clone(),
                            "",
                        ));
                    }
                }
                ']' | '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
            prefix.push(character);
        }
    }
    Ok(())
}

#[derive(Debug)]
enum Container {
    Mapping {
        pointer: String,
        values: Map<String, Value>,
        pending_key: Option<String>,
    },
    Sequence {
        pointer: String,
        values: Vec<Value>,
    },
}

fn construct(
    input: &str,
    bom_bytes: usize,
    source_file: ProjectPath,
    limits: &YamlLimits,
    execution: &crate::ExecutionBudget,
) -> Result<ParsedYaml, Finding> {
    let mut parser = Parser::new_from_str(input);
    let mut stack = Vec::<Container>::new();
    let mut root = None;
    let mut spans = BTreeMap::new();
    let mut document_count = 0_usize;

    loop {
        execution
            .checkpoint()
            .map_err(crate::ExecutionStop::finding)?;
        let (event, marker) = parser.next_token().map_err(|_| {
            parser_finding(
                "RP_E_YAML_ROOT_NOT_MAPPING",
                "YAML syntax could not be constructed as a mapping",
                source_file.clone(),
                "",
            )
        })?;

        match event {
            Event::StreamStart | Event::Nothing => {}
            Event::StreamEnd => break,
            Event::DocumentStart => {
                document_count += 1;
                if document_count > 1 {
                    return Err(parser_finding(
                        "RP_E_YAML_MULTIPLE_DOCUMENTS",
                        "exactly one YAML document is required",
                        source_file,
                        "",
                    ));
                }
            }
            Event::DocumentEnd => {}
            Event::Alias(_) => {
                return Err(parser_finding(
                    "RP_E_YAML_ALIAS_FORBIDDEN",
                    "YAML anchors and aliases are forbidden",
                    source_file,
                    "",
                ));
            }
            Event::Scalar(scalar, style, anchor, tag) => {
                if anchor != 0 {
                    return Err(parser_finding(
                        "RP_E_YAML_ALIAS_FORBIDDEN",
                        "YAML anchors and aliases are forbidden",
                        source_file,
                        "",
                    ));
                }
                if tag.is_some() {
                    return Err(parser_finding(
                        "RP_E_YAML_CUSTOM_TAG_FORBIDDEN",
                        "YAML tags are forbidden",
                        source_file,
                        "",
                    ));
                }

                if let Some(Container::Mapping {
                    pointer,
                    values,
                    pending_key,
                }) = stack.last_mut()
                    && pending_key.is_none()
                {
                    if scalar == "<<" {
                        return Err(parser_finding(
                            "RP_E_YAML_MERGE_KEY_FORBIDDEN",
                            "YAML merge keys are forbidden",
                            source_file,
                            &child_pointer(pointer, &scalar),
                        ));
                    }
                    if values.contains_key(&scalar) {
                        return Err(parser_finding(
                            "RP_E_YAML_DUPLICATE_KEY",
                            "duplicate YAML mapping key",
                            source_file,
                            &child_pointer(pointer, &scalar),
                        ));
                    }
                    if values.len() >= limits.max_mapping_keys {
                        return Err(resource_finding(
                            "RP_E_RESOURCE_MAPPING_KEYS_EXCEEDED",
                            "YAML mapping exceeds the configured key limit",
                            source_file,
                            pointer,
                        ));
                    }
                    *pending_key = Some(scalar);
                    continue;
                }

                let value = construct_scalar(&scalar, style);
                attach_value(
                    value,
                    marker,
                    input,
                    bom_bytes,
                    &mut stack,
                    &mut root,
                    &mut spans,
                    &source_file,
                    limits,
                )?;
            }
            Event::MappingStart(anchor, tag) => {
                reject_collection_properties(anchor, tag.is_some(), &source_file)?;
                if mapping_expects_key(&stack) {
                    return Err(parser_finding(
                        "RP_E_YAML_COMPLEX_KEY_FORBIDDEN",
                        "YAML complex mapping keys are forbidden",
                        source_file,
                        current_pointer(&stack),
                    ));
                }
                let pointer = next_pointer(&stack);
                spans.insert(pointer.clone(), span(marker, input, bom_bytes));
                stack.push(Container::Mapping {
                    pointer,
                    values: Map::new(),
                    pending_key: None,
                });
            }
            Event::SequenceStart(anchor, tag) => {
                reject_collection_properties(anchor, tag.is_some(), &source_file)?;
                if stack.is_empty() {
                    return Err(parser_finding(
                        "RP_E_YAML_ROOT_NOT_MAPPING",
                        "top-level YAML value must be a mapping",
                        source_file,
                        "",
                    ));
                }
                if mapping_expects_key(&stack) {
                    return Err(parser_finding(
                        "RP_E_YAML_COMPLEX_KEY_FORBIDDEN",
                        "YAML complex mapping keys are forbidden",
                        source_file,
                        current_pointer(&stack),
                    ));
                }
                let pointer = next_pointer(&stack);
                spans.insert(pointer.clone(), span(marker, input, bom_bytes));
                stack.push(Container::Sequence {
                    pointer,
                    values: Vec::new(),
                });
            }
            Event::MappingEnd => {
                let Some(Container::Mapping {
                    values,
                    pending_key,
                    ..
                }) = stack.pop()
                else {
                    return Err(parser_finding(
                        "RP_E_YAML_ROOT_NOT_MAPPING",
                        "invalid YAML mapping boundary",
                        source_file,
                        "",
                    ));
                };
                if pending_key.is_some() {
                    return Err(parser_finding(
                        "RP_E_YAML_ROOT_NOT_MAPPING",
                        "YAML mapping key has no value",
                        source_file,
                        "",
                    ));
                }
                attach_completed(
                    Value::Object(values),
                    &mut stack,
                    &mut root,
                    &source_file,
                    limits,
                )?;
            }
            Event::SequenceEnd => {
                let Some(Container::Sequence { values, .. }) = stack.pop() else {
                    return Err(parser_finding(
                        "RP_E_YAML_ROOT_NOT_MAPPING",
                        "invalid YAML sequence boundary",
                        source_file,
                        "",
                    ));
                };
                attach_completed(
                    Value::Array(values),
                    &mut stack,
                    &mut root,
                    &source_file,
                    limits,
                )?;
            }
        }
    }

    let Some(value @ Value::Object(_)) = root else {
        return Err(parser_finding(
            "RP_E_YAML_ROOT_NOT_MAPPING",
            "top-level YAML value must be a non-null mapping",
            source_file,
            "",
        ));
    };

    Ok(ParsedYaml { value, spans })
}

#[allow(clippy::too_many_arguments)]
fn attach_value(
    value: Value,
    marker: Marker,
    input: &str,
    bom_bytes: usize,
    stack: &mut [Container],
    root: &mut Option<Value>,
    spans: &mut BTreeMap<String, SourceSpan>,
    source_file: &ProjectPath,
    limits: &YamlLimits,
) -> Result<(), Finding> {
    let pointer = next_pointer(stack);
    spans.insert(pointer, span(marker, input, bom_bytes));
    attach_completed(value, stack, root, source_file, limits)
}

fn attach_completed(
    value: Value,
    stack: &mut [Container],
    root: &mut Option<Value>,
    source_file: &ProjectPath,
    limits: &YamlLimits,
) -> Result<(), Finding> {
    match stack.last_mut() {
        Some(Container::Mapping {
            values,
            pending_key,
            ..
        }) => {
            let Some(key) = pending_key.take() else {
                return Err(parser_finding(
                    "RP_E_YAML_COMPLEX_KEY_FORBIDDEN",
                    "YAML complex mapping keys are forbidden",
                    source_file.clone(),
                    "",
                ));
            };
            values.insert(key, value);
        }
        Some(Container::Sequence { pointer, values }) => {
            if values.len() >= limits.max_sequence_items {
                return Err(resource_finding(
                    "RP_E_RESOURCE_SEQUENCE_ITEMS_EXCEEDED",
                    "YAML sequence exceeds the configured item limit",
                    source_file.clone(),
                    pointer,
                ));
            }
            values.push(value);
        }
        None => {
            if root.replace(value).is_some() {
                return Err(parser_finding(
                    "RP_E_YAML_MULTIPLE_DOCUMENTS",
                    "exactly one YAML document is required",
                    source_file.clone(),
                    "",
                ));
            }
        }
    }
    Ok(())
}

fn reject_collection_properties(
    anchor: usize,
    has_tag: bool,
    source_file: &ProjectPath,
) -> Result<(), Finding> {
    if anchor != 0 {
        return Err(parser_finding(
            "RP_E_YAML_ALIAS_FORBIDDEN",
            "YAML anchors and aliases are forbidden",
            source_file.clone(),
            "",
        ));
    }
    if has_tag {
        return Err(parser_finding(
            "RP_E_YAML_CUSTOM_TAG_FORBIDDEN",
            "YAML tags are forbidden",
            source_file.clone(),
            "",
        ));
    }
    Ok(())
}

fn mapping_expects_key(stack: &[Container]) -> bool {
    matches!(
        stack.last(),
        Some(Container::Mapping {
            pending_key: None,
            ..
        })
    )
}

fn current_pointer(stack: &[Container]) -> &str {
    match stack.last() {
        Some(Container::Mapping { pointer, .. } | Container::Sequence { pointer, .. }) => pointer,
        None => "",
    }
}

fn next_pointer(stack: &[Container]) -> String {
    match stack.last() {
        Some(Container::Mapping {
            pointer,
            pending_key: Some(key),
            ..
        }) => child_pointer(pointer, key),
        Some(Container::Sequence { pointer, values }) => {
            child_pointer(pointer, &values.len().to_string())
        }
        _ => String::new(),
    }
}

fn child_pointer(parent: &str, segment: &str) -> String {
    let escaped = segment.replace('~', "~0").replace('/', "~1");
    format!("{parent}/{escaped}")
}

fn span(marker: Marker, input: &str, bom_bytes: usize) -> SourceSpan {
    let byte_in_input = input
        .char_indices()
        .nth(marker.index())
        .map_or(input.len(), |(offset, _)| offset);
    SourceSpan {
        byte_offset: bom_bytes + byte_in_input,
        line: marker.line(),
        column: marker.col() + 1,
    }
}

fn construct_scalar(scalar: &str, style: TScalarStyle) -> Value {
    if style != TScalarStyle::Plain {
        return Value::String(scalar.to_string());
    }

    match scalar {
        "" | "~" | "null" | "Null" | "NULL" => Value::Null,
        "true" | "True" | "TRUE" => Value::Bool(true),
        "false" | "False" | "FALSE" => Value::Bool(false),
        _ => construct_number(scalar).unwrap_or_else(|| Value::String(scalar.to_string())),
    }
}

fn construct_number(scalar: &str) -> Option<Value> {
    let normalized = scalar.replace('_', "");
    let (negative, unsigned) = normalized
        .strip_prefix('-')
        .map_or((false, normalized.as_str()), |rest| (true, rest));
    let unsigned = unsigned.strip_prefix('+').unwrap_or(unsigned);

    let radix_integer = if let Some(digits) = unsigned.strip_prefix("0x") {
        u64::from_str_radix(digits, 16).ok()
    } else if let Some(digits) = unsigned.strip_prefix("0o") {
        u64::from_str_radix(digits, 8).ok()
    } else if unsigned.chars().all(|character| character.is_ascii_digit()) && !unsigned.is_empty() {
        unsigned.parse::<u64>().ok()
    } else {
        None
    };

    if let Some(integer) = radix_integer {
        if negative {
            let signed = i128::from(integer).checked_neg()?;
            return i64::try_from(signed)
                .ok()
                .map(Number::from)
                .map(Value::Number);
        }
        return Some(Value::Number(Number::from(integer)));
    }

    let is_float_syntax =
        (unsigned.contains('.') || unsigned.contains('e') || unsigned.contains('E'))
            && unsigned.chars().any(|character| character.is_ascii_digit());
    if is_float_syntax {
        return normalized
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number);
    }
    None
}

fn parser_finding(
    code: &'static str,
    message: &'static str,
    source_file: ProjectPath,
    json_pointer: &str,
) -> Finding {
    Finding::new(
        code,
        "yaml_parser_compliance",
        Severity::Error,
        message,
        Some(source_file),
        json_pointer,
    )
}

fn resource_finding(
    code: &'static str,
    message: &'static str,
    source_file: ProjectPath,
    json_pointer: &str,
) -> Finding {
    Finding::new(
        code,
        "resource_limit",
        Severity::Error,
        message,
        Some(source_file),
        json_pointer,
    )
}
