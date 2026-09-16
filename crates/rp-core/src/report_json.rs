//! Strict, resource-bounded JSON parsing for derived reports (not schema validation).

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

/// Per-document budgets. Requests above the defaults are clamped to those hard ceilings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportJsonLimits {
    pub max_file_bytes: usize,
    pub max_depth: usize,
    pub max_scalar_bytes: usize,
    pub max_mapping_keys: usize,
    pub max_sequence_items: usize,
    pub max_total_nodes: usize,
}

impl Default for ReportJsonLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 4 * 1024 * 1024,
            max_depth: 64,
            max_scalar_bytes: 256 * 1024,
            max_mapping_keys: 4_096,
            max_sequence_items: 65_536,
            max_total_nodes: 131_072,
        }
    }
}

/// Parser errors deliberately contain no document text, keys, or schema Finding codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReportJsonError {
    Stopped(crate::ExecutionStop),
    DocumentTooLarge,
    BomForbidden,
    InvalidUtf8,
    InvalidJson,
    RootNotMapping,
    DuplicateKey,
    DepthExceeded,
    ScalarTooLarge,
    TooManyMappingKeys,
    TooManySequenceItems,
    TooManyNodes,
}

impl fmt::Display for ReportJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::Stopped(stop) = self {
            return stop.fmt(f);
        }
        f.write_str(match self {
            Self::Stopped(_) => unreachable!(),
            Self::DocumentTooLarge => "report JSON exceeds the document byte limit",
            Self::BomForbidden => "report JSON must not start with a UTF-8 BOM",
            Self::InvalidUtf8 => "report JSON is not valid UTF-8",
            Self::InvalidJson => "report must contain exactly one valid JSON document",
            Self::RootNotMapping => "report JSON root must be an object",
            Self::DuplicateKey => "report JSON contains a duplicate decoded object key",
            Self::DepthExceeded => "report JSON exceeds the container depth limit",
            Self::ScalarTooLarge => "report JSON exceeds the decoded string byte limit",
            Self::TooManyMappingKeys => "report JSON exceeds the per-object key limit",
            Self::TooManySequenceItems => "report JSON exceeds the per-array item limit",
            Self::TooManyNodes => "report JSON exceeds the total value node limit",
        })
    }
}

impl std::error::Error for ReportJsonError {}

/// Parse one UTF-8 JSON object, without schema dispatch or report binding.
///
/// Limits are enforced while constructing values, not by walking a completed tree.
/// The root object has depth one; only objects and arrays increase depth. Every
/// container and scalar value consumes one node; keys consume the per-map and
/// decoded string budgets instead. String sizes are measured after JSON unescaping.
/// Numbers retain the existing serde_json integer/f64 semantics (no arbitrary precision).
/// Invalid syntax, overflowing/nonfinite numbers and trailing data are `InvalidJson`.
/// No error retains serde diagnostics that could disclose document contents.
pub fn parse_report_json(
    bytes: &[u8],
    limits: &ReportJsonLimits,
) -> Result<Value, ReportJsonError> {
    let mut remaining = usize::MAX;
    parse_report_json_metered(bytes, limits, &mut remaining)
}

// Debit each attempted value before deserialization/allocation. The caller's
// project allowance survives every exit, including syntax and trailing-data errors.
pub(crate) fn parse_report_json_metered(
    bytes: &[u8],
    limits: &ReportJsonLimits,
    remaining_nodes: &mut usize,
) -> Result<Value, ReportJsonError> {
    parse_report_json_with_budget(
        bytes,
        limits,
        remaining_nodes,
        &crate::ExecutionBudget::default(),
    )
}
pub(crate) fn parse_report_json_with_budget(
    bytes: &[u8],
    limits: &ReportJsonLimits,
    remaining_nodes: &mut usize,
    execution: &crate::ExecutionBudget,
) -> Result<Value, ReportJsonError> {
    execution.checkpoint().map_err(ReportJsonError::Stopped)?;
    let ceilings = ReportJsonLimits::default();
    let limits = ReportJsonLimits {
        max_file_bytes: limits.max_file_bytes.min(ceilings.max_file_bytes),
        max_depth: limits.max_depth.min(ceilings.max_depth),
        max_scalar_bytes: limits.max_scalar_bytes.min(ceilings.max_scalar_bytes),
        max_mapping_keys: limits.max_mapping_keys.min(ceilings.max_mapping_keys),
        max_sequence_items: limits.max_sequence_items.min(ceilings.max_sequence_items),
        max_total_nodes: limits.max_total_nodes.min(ceilings.max_total_nodes),
    };
    if bytes.len() > limits.max_file_bytes {
        return Err(ReportJsonError::DocumentTooLarge);
    }
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(ReportJsonError::BomForbidden);
    }
    let input = std::str::from_utf8(bytes).map_err(|_| ReportJsonError::InvalidUtf8)?;
    let mut state = State {
        limits,
        nodes: 0,
        remaining_nodes,
        error: None,
        execution,
    };
    let mut deserializer = serde_json::Deserializer::from_str(input);
    // Retain serde_json's recursion guard as defense in depth. Our hard ceiling
    // of 64 containers is lower and applies equally to arrays and objects.
    let value = ValueSeed {
        state: &mut state,
        depth: 0,
        full_array: false,
    }
    .deserialize(&mut deserializer)
    .map_err(|_| state.error.unwrap_or(ReportJsonError::InvalidJson))?;
    deserializer
        .end()
        .map_err(|_| ReportJsonError::InvalidJson)?;
    execution.checkpoint().map_err(ReportJsonError::Stopped)?;
    Ok(value)
}

struct State<'a> {
    execution: &'a crate::ExecutionBudget,
    limits: ReportJsonLimits,
    nodes: usize,
    remaining_nodes: &'a mut usize,
    // Keep typed failures out of serde's string-based custom error channel.
    error: Option<ReportJsonError>,
}

impl State<'_> {
    fn reject<T, E: de::Error>(&mut self, error: ReportJsonError) -> Result<T, E> {
        self.error = Some(error);
        Err(E::custom("report JSON rejected"))
    }

    fn string<E: de::Error>(&mut self, value: &str) -> Result<String, E> {
        // serde_json may first unescape into its scratch buffer, bounded by the
        // document byte ceiling. Check before allocating a retained key/Value.
        if value.len() > self.limits.max_scalar_bytes {
            return self.reject(ReportJsonError::ScalarTooLarge);
        }
        Ok(value.to_owned())
    }
}

struct ValueSeed<'a, 'b> {
    state: &'a mut State<'b>,
    // Number of ancestor containers, including an enclosing map or array.
    depth: usize,
    full_array: bool,
}

impl<'de> DeserializeSeed<'de> for ValueSeed<'_, '_> {
    type Value = Value;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        if let Err(stop) = self.state.execution.checkpoint() {
            return self.state.reject(ReportJsonError::Stopped(stop));
        }
        // SeqAccess calls this only when another item exists, so an empty array
        // is valid at a zero item budget and no excess subtree gets constructed.
        if self.full_array {
            return self.state.reject(ReportJsonError::TooManySequenceItems);
        }
        if self.state.nodes >= self.state.limits.max_total_nodes || *self.state.remaining_nodes == 0
        {
            return self.state.reject(ReportJsonError::TooManyNodes);
        }
        self.state.nodes += 1;
        *self.state.remaining_nodes -= 1;
        deserializer.deserialize_any(self)
    }
}

impl ValueSeed<'_, '_> {
    fn scalar<E: de::Error>(&mut self) -> Result<(), E> {
        if self.depth == 0 {
            return self.state.reject(ReportJsonError::RootNotMapping);
        }
        Ok(())
    }

    fn container<E: de::Error>(&mut self) -> Result<(), E> {
        if self.depth >= self.state.limits.max_depth {
            return self.state.reject(ReportJsonError::DepthExceeded);
        }
        Ok(())
    }
}

impl<'de> Visitor<'de> for ValueSeed<'_, '_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E: de::Error>(mut self) -> Result<Value, E> {
        self.scalar()?;
        Ok(Value::Null)
    }

    fn visit_bool<E: de::Error>(mut self, value: bool) -> Result<Value, E> {
        self.scalar()?;
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(mut self, value: i64) -> Result<Value, E> {
        self.scalar()?;
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E: de::Error>(mut self, value: u64) -> Result<Value, E> {
        self.scalar()?;
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E: de::Error>(mut self, value: f64) -> Result<Value, E> {
        self.scalar()?;
        match Number::from_f64(value) {
            Some(number) => Ok(Value::Number(number)),
            None => self.state.reject(ReportJsonError::InvalidJson),
        }
    }

    fn visit_str<E: de::Error>(mut self, value: &str) -> Result<Value, E> {
        self.scalar()?;
        self.state.string(value).map(Value::String)
    }

    fn visit_map<A: MapAccess<'de>>(mut self, mut map: A) -> Result<Value, A::Error> {
        self.container()?;
        let mut values = Map::new();
        while let Some(key) = map.next_key_seed(KeySeed {
            full_map: values.len() >= self.state.limits.max_mapping_keys,
            state: self.state,
        })? {
            // Keys are already decoded. Check before reading the value, and
            // before Map::insert could silently replace a prior entry.
            if values.contains_key(&key) {
                return self.state.reject(ReportJsonError::DuplicateKey);
            }
            let value = map.next_value_seed(ValueSeed {
                state: self.state,
                depth: self.depth + 1,
                full_array: false,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }

    fn visit_seq<A: SeqAccess<'de>>(mut self, mut sequence: A) -> Result<Value, A::Error> {
        self.scalar()?; // Arrays are values but cannot be the root report.
        self.container()?;
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(ValueSeed {
            full_array: values.len() >= self.state.limits.max_sequence_items,
            state: self.state,
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
}

struct KeySeed<'a, 'b> {
    state: &'a mut State<'b>,
    full_map: bool,
}

impl<'de> DeserializeSeed<'de> for KeySeed<'_, '_> {
    type Value = String;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        if self.full_map {
            return self.state.reject(ReportJsonError::TooManyMappingKeys);
        }
        deserializer.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for KeySeed<'_, '_> {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON object key")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<String, E> {
        self.state.string(value)
    }
}

#[cfg(test)]
#[path = "report_json_tests.rs"]
mod tests;
