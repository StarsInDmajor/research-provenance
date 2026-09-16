use std::{
    collections::BTreeSet,
    fmt, fs,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use rp_core::{ProjectPath, SchemaBundle, YamlLimits, parse_restricted_yaml, validate_project};
use serde::Serialize;
use serde_json::Value;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FixtureOutcome {
    pub fixture: String,
    pub overlay: String,
    pub expectation_matched: bool,
    pub observed_exit_code: u8,
    pub observed_error_codes: Vec<String>,
    #[serde(skip)]
    pub materialization_mode: u32,
}

#[derive(Debug)]
pub enum HarnessError {
    DescriptorInvalid(String),
    FixtureMismatch,
    MutationCardinality,
    MutationInvalid(String),
    Io(String),
    Validation(String),
}

impl fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DescriptorInvalid(message) => {
                write!(formatter, "invalid overlay descriptor: {message}")
            }
            Self::FixtureMismatch => {
                formatter.write_str("descriptor fixture does not match baseline")
            }
            Self::MutationCardinality => {
                formatter.write_str("mutation must match exactly one target location")
            }
            Self::MutationInvalid(message) => write!(formatter, "invalid mutation: {message}"),
            Self::Io(message) | Self::Validation(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for HarnessError {}

#[allow(clippy::missing_errors_doc)]
pub fn run_fixture(
    baseline: &Path,
    descriptor_path: &Path,
) -> Result<FixtureOutcome, HarnessError> {
    let descriptor_bytes = fs::read(descriptor_path)
        .map_err(|error| HarnessError::Io(format!("cannot read descriptor: {error}")))?;
    let descriptor = parse_restricted_yaml(
        &descriptor_bytes,
        ProjectPath::new("overlay.yaml").expect("static project path"),
        &YamlLimits::default(),
    )
    .map_err(|finding| HarnessError::DescriptorInvalid(finding.error_code.to_string()))?
    .value;
    SchemaBundle::new()
        .map_err(|error| HarnessError::Validation(error.to_string()))?
        .validate(
            &descriptor,
            ProjectPath::new("overlay.yaml").expect("static project path"),
        )
        .map_err(|findings| {
            HarnessError::DescriptorInvalid(
                findings
                    .iter()
                    .map(|finding| finding.error_code)
                    .collect::<Vec<_>>()
                    .join(","),
            )
        })?;

    let fixture = descriptor
        .get("fixture")
        .and_then(Value::as_str)
        .ok_or_else(|| HarnessError::DescriptorInvalid("missing fixture".to_string()))?;
    if baseline
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        != Some(fixture)
    {
        return Err(HarnessError::FixtureMismatch);
    }

    let materialized = PrivateMaterialization::copy_from(baseline)?;
    apply_mutation(materialized.path(), &descriptor["mutation"])?;
    apply_maintenance(materialized.path(), descriptor.get("maintenance"))?;
    let report = validate_project(materialized.path())
        .map_err(|error| HarnessError::Validation(error.to_string()))?;
    let observed_exit_code = u8::from(!report.findings.is_empty());
    let observed_error_codes = report
        .findings
        .iter()
        .map(|finding| finding.error_code.to_string())
        .collect();
    let expectation_matched = findings_match(&descriptor, &report.findings, observed_exit_code);
    Ok(FixtureOutcome {
        fixture: fixture.to_string(),
        overlay: descriptor
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        expectation_matched,
        observed_exit_code,
        observed_error_codes,
        materialization_mode: fs::metadata(materialized.path())
            .map_err(|error| HarnessError::Io(error.to_string()))?
            .permissions()
            .mode()
            & 0o777,
    })
}

fn apply_mutation(root: &Path, mutation: &Value) -> Result<(), HarnessError> {
    let target = mutation
        .get("target_file")
        .and_then(Value::as_str)
        .ok_or_else(|| HarnessError::MutationInvalid("missing target_file".to_string()))?;
    ProjectPath::new(target.to_string())
        .map_err(|_| HarnessError::MutationInvalid("unsafe target_file".to_string()))?;
    let target_path = root.join(target);
    match mutation.get("mode").and_then(Value::as_str) {
        Some("structured_ast") => apply_structured_mutation(&target_path, mutation),
        Some("raw_text") => apply_raw_mutation(&target_path, mutation),
        _ => Err(HarnessError::MutationInvalid("unknown mode".to_string())),
    }
}

fn apply_structured_mutation(path: &Path, mutation: &Value) -> Result<(), HarnessError> {
    let bytes = fs::read(path).map_err(|error| HarnessError::Io(error.to_string()))?;
    let mut object = parse_restricted_yaml(
        &bytes,
        ProjectPath::new("mutation-target.yaml").expect("static path"),
        &YamlLimits::default(),
    )
    .map_err(|finding| HarnessError::MutationInvalid(finding.error_code.to_string()))?
    .value;
    let pointer = mutation
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| HarnessError::MutationInvalid("missing path".to_string()))?;
    let (parent_pointer, token) = pointer
        .rsplit_once('/')
        .ok_or_else(|| HarnessError::MutationInvalid("root mutation is forbidden".to_string()))?;
    let parent = if parent_pointer.is_empty() {
        &mut object
    } else {
        object
            .pointer_mut(parent_pointer)
            .ok_or(HarnessError::MutationCardinality)?
    };
    let token = token.replace("~1", "/").replace("~0", "~");
    let operation = mutation.get("operation").and_then(Value::as_str);
    let value = mutation.get("value").cloned();
    match (parent, operation) {
        (Value::Object(values), Some("set_value")) => {
            values.insert(
                token,
                value.ok_or_else(|| HarnessError::MutationInvalid("missing value".to_string()))?,
            );
        }
        (Value::Object(values), Some("add_field")) => {
            if values.contains_key(&token) {
                return Err(HarnessError::MutationCardinality);
            }
            values.insert(
                token,
                value.ok_or_else(|| HarnessError::MutationInvalid("missing value".to_string()))?,
            );
        }
        (Value::Object(values), Some("delete_field")) => {
            values
                .remove(&token)
                .ok_or(HarnessError::MutationCardinality)?;
        }
        (Value::Array(values), Some("set_value")) => {
            let index = token
                .parse::<usize>()
                .map_err(|_| HarnessError::MutationCardinality)?;
            *values
                .get_mut(index)
                .ok_or(HarnessError::MutationCardinality)? =
                value.ok_or_else(|| HarnessError::MutationInvalid("missing value".to_string()))?;
        }
        (Value::Array(values), Some("delete_field")) => {
            let index = token
                .parse::<usize>()
                .map_err(|_| HarnessError::MutationCardinality)?;
            if index >= values.len() {
                return Err(HarnessError::MutationCardinality);
            }
            values.remove(index);
        }
        _ => {
            return Err(HarnessError::MutationInvalid(
                "operation does not match target".to_string(),
            ));
        }
    }
    write_private(
        path,
        &serde_json::to_vec_pretty(&object)
            .map_err(|error| HarnessError::MutationInvalid(error.to_string()))?,
    )
}

fn apply_raw_mutation(path: &Path, mutation: &Value) -> Result<(), HarnessError> {
    let text = fs::read_to_string(path).map_err(|error| HarnessError::Io(error.to_string()))?;
    let search = mutation
        .get("search")
        .and_then(Value::as_str)
        .ok_or_else(|| HarnessError::MutationInvalid("missing search".to_string()))?;
    let replacement = mutation
        .get("replace")
        .and_then(Value::as_str)
        .ok_or_else(|| HarnessError::MutationInvalid("missing replace".to_string()))?;
    if text.match_indices(search).count() != 1 {
        return Err(HarnessError::MutationCardinality);
    }
    write_private(path, text.replacen(search, replacement, 1).as_bytes())
}

fn apply_maintenance(root: &Path, maintenance: Option<&Value>) -> Result<(), HarnessError> {
    let Some(paths) = maintenance
        .and_then(|value| value.get("recompute_source_closure_sha256"))
        .and_then(Value::as_array)
    else {
        return Ok(());
    };
    for path in paths.iter().filter_map(Value::as_str) {
        ProjectPath::new(path.to_string())
            .map_err(|_| HarnessError::MutationInvalid("unsafe maintenance path".to_string()))?;
        let target = root.join(path);
        let bytes = fs::read(&target).map_err(|error| HarnessError::Io(error.to_string()))?;
        let mut chain = parse_restricted_yaml(
            &bytes,
            ProjectPath::new(path.to_string()).map_err(|_| {
                HarnessError::MutationInvalid("unsafe maintenance path".to_string())
            })?,
            &YamlLimits::default(),
        )
        .map_err(|finding| HarnessError::MutationInvalid(finding.error_code.to_string()))?
        .value;
        let id = chain.get("id").and_then(Value::as_str).ok_or_else(|| {
            HarnessError::MutationInvalid("maintenance target lacks id".to_string())
        })?;
        let report =
            validate_project(root).map_err(|error| HarnessError::Validation(error.to_string()))?;
        let digest = report
            .index
            .as_ref()
            .and_then(|index| index.claim_chain_report(id))
            .map(|claim| claim.source_sha256.to_string())
            .ok_or_else(|| {
                HarnessError::Validation("cannot recompute source closure".to_string())
            })?;
        chain["source_closure_sha256"] = Value::String(digest);
        write_private(
            &target,
            &serde_json::to_vec_pretty(&chain)
                .map_err(|error| HarnessError::MutationInvalid(error.to_string()))?,
        )?;
    }
    Ok(())
}

fn findings_match(descriptor: &Value, findings: &[rp_core::Finding], exit_code: u8) -> bool {
    let expected = &descriptor["expected_failure"];
    if expected.get("exit_code").and_then(Value::as_u64) != Some(u64::from(exit_code)) {
        return false;
    }
    let expected_code = expected.get("error_code").and_then(Value::as_str);
    let expected_family = expected.get("finding_family").and_then(Value::as_str);
    let expected_file = expected.get("target_file").and_then(Value::as_str);
    let primary = findings.iter().any(|finding| {
        Some(finding.error_code) == expected_code
            && Some(finding.finding_family) == expected_family
            && finding.source_file.as_ref().map(ProjectPath::as_str) == expected_file
    });
    if !primary {
        return false;
    }
    let allowed: BTreeSet<_> = descriptor
        .get("allowed_secondary_findings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            Some((
                entry.get("error_code")?.as_str()?,
                entry.get("finding_family")?.as_str()?,
            ))
        })
        .collect();
    findings.iter().all(|finding| {
        (Some(finding.error_code) == expected_code
            && Some(finding.finding_family) == expected_family)
            || allowed.contains(&(finding.error_code, finding.finding_family))
    })
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), HarnessError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).truncate(true).mode(0o600);
    let mut file = options
        .open(path)
        .map_err(|error| HarnessError::Io(error.to_string()))?;
    use std::io::Write;
    file.write_all(bytes)
        .map_err(|error| HarnessError::Io(error.to_string()))
}

struct PrivateMaterialization {
    root: PathBuf,
}

impl PrivateMaterialization {
    fn copy_from(source: &Path) -> Result<Self, HarnessError> {
        let root = std::env::temp_dir().join(format!(
            "rp-fixture-materialization-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).map_err(|error| HarnessError::Io(error.to_string()))?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .map_err(|error| HarnessError::Io(error.to_string()))?;
        copy_directory(source, &root)?;
        Ok(Self { root })
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for PrivateMaterialization {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn copy_directory(source: &Path, target: &Path) -> Result<(), HarnessError> {
    let mut entries: Vec<_> = fs::read_dir(source)
        .map_err(|error| HarnessError::Io(error.to_string()))?
        .collect::<Result<_, _>>()
        .map_err(|error| HarnessError::Io(error.to_string()))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|error| HarnessError::Io(error.to_string()))?;
        let destination = target.join(entry.file_name());
        if file_type.is_symlink() {
            return Err(HarnessError::Io(
                "fixture symlinks are forbidden".to_string(),
            ));
        }
        if file_type.is_dir() {
            fs::create_dir(&destination).map_err(|error| HarnessError::Io(error.to_string()))?;
            fs::set_permissions(&destination, fs::Permissions::from_mode(0o700))
                .map_err(|error| HarnessError::Io(error.to_string()))?;
            copy_directory(&entry.path(), &destination)?;
        } else if file_type.is_file() {
            let bytes =
                fs::read(entry.path()).map_err(|error| HarnessError::Io(error.to_string()))?;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true).mode(0o600);
            use std::io::Write;
            options
                .open(&destination)
                .and_then(|mut file| file.write_all(&bytes))
                .map_err(|error| HarnessError::Io(error.to_string()))?;
        } else {
            return Err(HarnessError::Io(
                "fixture contains a non-regular entry".to_string(),
            ));
        }
    }
    Ok(())
}
