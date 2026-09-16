use crate::findings::Findings;
use std::{collections::BTreeMap, fmt, path::Path};

use serde_json::Value;

use crate::{
    Finding, ObjectRecord, ObjectType, ProjectIndex, ProjectPath, ReferenceEdge,
    ReferenceExpectation, SchemaBundle, Severity, YamlLimits,
};

#[derive(Clone, Copy, Debug)]
pub struct ProjectLimits {
    /// Diagnostic retention including the marker; clamped to 1..=1000.
    pub findings: usize,
    pub canonical_files: usize,
    pub scanned_directories: usize,
    /// Maximum names inventoried per directory, including noncanonical/invalid names.
    /// Clamped to 0..=100000; overflow rejects the whole scan before prefix processing.
    pub directory_entries: usize,
    pub yaml_bytes_per_object: usize,
    pub whole_project_yaml_bytes: usize,
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub traversal_depth: usize,
    pub local_artifact_bytes_per_item: usize,
    pub local_artifact_aggregate_bytes: usize,
    pub extension_schemas: usize,
    pub extension_schema_aggregate_bytes: usize,
    pub extension_ref_depth: usize,
}

impl Default for ProjectLimits {
    fn default() -> Self {
        Self {
            findings: crate::DEFAULT_FINDINGS_LIMIT,
            canonical_files: 100_000,
            scanned_directories: 20_000,
            directory_entries: 100_000,
            yaml_bytes_per_object: 4_194_304,
            whole_project_yaml_bytes: 536_870_912,
            graph_nodes: 100_000,
            graph_edges: 1_000_000,
            traversal_depth: 4_096,
            local_artifact_bytes_per_item: 1_073_741_824,
            local_artifact_aggregate_bytes: 4_294_967_296,
            extension_schemas: 256,
            extension_schema_aggregate_bytes: 16_777_216,
            extension_ref_depth: 32,
        }
    }
}

#[derive(Debug)]
pub enum ProjectLoadError {
    Io(String),
    Schema(String),
    UnsupportedPlatform,
}

impl fmt::Display for ProjectLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "project I/O failed: {message}"),
            Self::Schema(message) => write!(
                formatter,
                "embedded schema initialization failed: {message}"
            ),
            Self::UnsupportedPlatform => formatter.write_str(
                "secure project loading requires Linux openat2 containment on this platform",
            ),
        }
    }
}

impl std::error::Error for ProjectLoadError {}

#[derive(Debug)]
pub struct ValidationReport {
    pub findings: Vec<Finding>,
    had_error: bool,
    complete: bool,
    finding_output: crate::findings::FindingOutput,
    pub canonical_object_count: usize,
    pub canonical_paths: Vec<ProjectPath>,
    pub stage3_ran: bool,
    pub index: Option<ProjectIndex>,
}

impl ValidationReport {
    /// Consume the report for CLI/query diagnostics without granting a fresh budget.
    /// The public findings transcript must not have been modified before this call.
    #[must_use]
    pub fn into_command_parts(self) -> (crate::FindingOutput, Option<ProjectIndex>) {
        (self.finding_output.restore(self.findings), self.index)
    }

    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.complete
    }
    #[must_use]
    pub const fn had_error(&self) -> bool {
        self.had_error
    }
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.complete && !self.had_error && self.findings.is_empty()
    }
}

#[allow(clippy::missing_errors_doc)]
pub fn validate_project(root: &Path) -> Result<ValidationReport, ProjectLoadError> {
    validate_project_with_limits(root, ProjectLimits::default())
}

pub fn validate_project_with_limits(
    root: &Path,
    limits: ProjectLimits,
) -> Result<ValidationReport, ProjectLoadError> {
    validate_project_with_budget(root, limits, &crate::ExecutionBudget::default())
}

#[allow(clippy::missing_errors_doc)]
pub fn validate_project_with_budget(
    root: &Path,
    limits: ProjectLimits,
    execution: &crate::ExecutionBudget,
) -> Result<ValidationReport, ProjectLoadError> {
    #[cfg(target_os = "linux")]
    {
        validate_project_linux(root, limits, execution)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (root, limits);
        Err(ProjectLoadError::UnsupportedPlatform)
    }
}

/// Deterministic test-only injection between read completion and descriptor stat.
/// Thread-local and scoped: unused hooks are cleared on return or panic.
#[cfg(test)]
pub(crate) mod canonical_read_test {
    use super::ProjectPath;
    use std::{cell::RefCell, fs::File};
    type Hook = (ProjectPath, Box<dyn FnOnce(&File)>);
    thread_local! {
        static AFTER_READ: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            AFTER_READ.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }
    pub(crate) fn with_hook<T>(
        path: ProjectPath,
        hook: impl FnOnce(&File) + 'static,
        action: impl FnOnce() -> T,
    ) -> T {
        AFTER_READ.with(|slot| {
            assert!(slot.borrow().is_none(), "nested canonical read hook");
            *slot.borrow_mut() = Some((path, Box::new(hook)));
        });
        let _reset = Reset;
        action()
    }
    pub(super) fn after_read(path: &ProjectPath, file: &File) {
        let hook = AFTER_READ.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.as_ref().is_some_and(|(target, _)| target == path) {
                slot.take()
            } else {
                None
            }
        });
        if let Some((_, hook)) = hook {
            hook(file);
        }
    }
}

#[derive(Clone, Debug)]
enum ExpectedLayout {
    Project,
    Node(Option<&'static str>),
    Relation,
    Assessment,
    Thread,
    ThreadBinding,
    ClaimChain,
    ExternalReference,
    Artifact,
    ResearchRun,
    Invalid,
}

impl ExpectedLayout {
    fn accepts(&self, actual: &ObjectType) -> bool {
        match (self, actual) {
            (Self::Project, ObjectType::Project)
            | (Self::Node(None), ObjectType::Node(_))
            | (Self::Relation, ObjectType::Relation)
            | (Self::Assessment, ObjectType::Assessment)
            | (Self::Thread, ObjectType::Thread)
            | (Self::ThreadBinding, ObjectType::ThreadBinding)
            | (Self::ClaimChain, ObjectType::ClaimChain)
            | (Self::ExternalReference, ObjectType::ExternalReference)
            | (Self::Artifact, ObjectType::Artifact)
            | (Self::ResearchRun, ObjectType::ResearchRun) => true,
            (Self::Node(Some(expected)), ObjectType::Node(actual)) => *expected == actual.as_ref(),
            _ => false,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::Node(_) => "NodeRevision",
            Self::Relation => "ScientificRelationRevision",
            Self::Assessment => "Assessment",
            Self::Thread => "ResearchThread",
            Self::ThreadBinding => "ThreadBinding",
            Self::ClaimChain => "ClaimChainSnapshot",
            Self::ExternalReference => "ExternalReference",
            Self::Artifact => "ArtifactManifest",
            Self::ResearchRun => "ResearchRun",
            Self::Invalid => "an object in a canonical v1 directory",
        }
    }
}

#[derive(Debug)]
struct CanonicalFile {
    relative: String,
    path: ProjectPath,
    expected: ExpectedLayout,
    expected_size: usize,
}

#[cfg(target_os = "linux")]
struct CanonicalScan {
    research_fd: Option<std::os::fd::OwnedFd>,
    files: Vec<CanonicalFile>,
    findings: Findings,
}

#[derive(Debug)]
struct ParsedObject {
    path: ProjectPath,
    expected: ExpectedLayout,
    value: Value,
}

#[cfg(target_os = "linux")]
fn validate_project_linux(
    root: &Path,
    limits: ProjectLimits,
    execution: &crate::ExecutionBudget,
) -> Result<ValidationReport, ProjectLoadError> {
    use std::fs::File;

    use rustix::fs::{FileType, Mode, OFlags, ResolveFlags, fstat, openat2};

    let scan = scan_canonical_files(root, limits, execution)?;
    let canonical_object_count = scan.files.len();
    let canonical_paths = scan.files.iter().map(|file| file.path.clone()).collect();
    let mut findings = scan.findings;

    if findings.cancelled() {
        let (findings, finding_output, had_error, complete) = findings.into_output().report_parts();
        return Ok(ValidationReport {
            findings,
            finding_output,
            had_error,
            complete,
            canonical_object_count,
            canonical_paths,
            stage3_ran: false,
            index: None,
        });
    }
    let schema_bundle =
        SchemaBundle::new().map_err(|error| ProjectLoadError::Schema(error.to_string()))?;
    let catalog: Value = serde_json::from_slice(
        SchemaBundle::resources()
            .iter()
            .find(|resource| resource.name == "schema-catalog.json")
            .expect("embedded schema catalog exists")
            .bytes,
    )
    .map_err(|error| ProjectLoadError::Schema(error.to_string()))?;
    let canonical_schemas = catalog["schemas"]
        .as_object()
        .expect("embedded catalog defines canonical schemas");
    let mut parsed_objects = Vec::with_capacity(scan.files.len());
    let mut total_bytes = 0_usize;
    for file in scan.files {
        if findings.cancelled() {
            break;
        }
        let Some(research_fd) = scan.research_fd.as_ref() else {
            break;
        };
        let fd = match openat2(
            research_fd,
            file.relative.as_str(),
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
        ) {
            Ok(fd) => fd,
            Err(error) => {
                findings.push(|| {
                    Finding::new(
                        "RP_E_IO_READ_FAILED",
                        "io",
                        Severity::Error,
                        format!("secure canonical file open failed ({error})"),
                        Some(file.path),
                        "",
                    )
                });
                continue;
            }
        };
        let before = fstat(&fd).map_err(|error| {
            ProjectLoadError::Io(format!("cannot inspect canonical file ({error})"))
        })?;
        if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile {
            findings.push(|| {
                path_finding(
                    "RP_E_PATH_NON_REGULAR_FILE",
                    "path_containment",
                    "canonical object is not a regular file",
                    Some(file.path),
                )
            });
            continue;
        }
        let size = usize::try_from(before.st_size).unwrap_or(usize::MAX);
        if size > limits.yaml_bytes_per_object {
            findings.push(|| {
                resource_finding(
                    "RP_E_RESOURCE_FILE_SIZE_EXCEEDED",
                    "canonical YAML file size limit exceeded",
                    Some(file.path),
                )
            });
            continue;
        }
        total_bytes = total_bytes.saturating_add(size);
        if total_bytes > limits.whole_project_yaml_bytes {
            findings.push(|| {
                resource_finding(
                    "RP_E_RESOURCE_TOTAL_BYTES_EXCEEDED",
                    "whole-project YAML byte limit exceeded",
                    Some(file.path),
                )
            });
            break;
        }
        let mut bytes = Vec::with_capacity(size);
        let mut opened = File::from(fd);
        let mut chunk = [0_u8; 64 * 1024];
        while bytes.len() <= limits.yaml_bytes_per_object {
            let allowance = chunk
                .len()
                .min(limits.yaml_bytes_per_object + 1 - bytes.len());
            match execution.read_chunk(&mut opened, &mut chunk[..allowance]) {
                Ok(0) => break,
                Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                Err(crate::execution::ExecutionReadError::Stopped(stop)) => {
                    debug_assert_eq!(execution.checkpoint(), Err(stop));
                    break;
                }
                Err(crate::execution::ExecutionReadError::Io(error)) => {
                    return Err(ProjectLoadError::Io(format!(
                        "cannot read canonical file ({error})"
                    )));
                }
            }
        }
        if findings.cancelled() {
            break;
        }
        #[cfg(test)]
        canonical_read_test::after_read(&file.path, &opened);
        let after = fstat(&opened).map_err(|error| {
            ProjectLoadError::Io(format!("cannot re-inspect canonical file ({error})"))
        })?;
        if size != file.expected_size
            || before.st_dev != after.st_dev
            || before.st_ino != after.st_ino
            || before.st_size != after.st_size
            || before.st_mtime != after.st_mtime
            || before.st_mtime_nsec != after.st_mtime_nsec
            || before.st_ctime != after.st_ctime
            || before.st_ctime_nsec != after.st_ctime_nsec
            || bytes.len() != size
        {
            findings.push(|| {
                Finding::new(
                    "RP_E_IO_READ_FAILED",
                    "io",
                    Severity::Error,
                    "canonical file changed while it was being read",
                    Some(file.path),
                    "",
                )
            });
            continue;
        }
        match crate::yaml::parse_restricted_yaml_with_budget(
            &bytes,
            file.path.clone(),
            &YamlLimits::default(),
            execution,
        ) {
            Ok(parsed) => {
                let schema_id = parsed.value.get("schema").and_then(Value::as_str);
                // The shared bundle also validates contracts; canonical dispatch must
                // use only the catalog's Layer B schemas, regardless of ID presence.
                if !schema_id.is_some_and(|id| canonical_schemas.contains_key(id)) {
                    findings.push(|| {
                        Finding::new(
                            "RP_E_OBJECT_SCHEMA_DISPATCH_UNKNOWN",
                            "schema_dispatch",
                            Severity::Error,
                            "object schema is missing or is not a canonical Layer B schema",
                            Some(file.path),
                            "/schema",
                        )
                    });
                    continue;
                }
                // This fixed entrypoint requires Project's schema const at stage 2;
                // ordinary canonical directory/type consistency remains stage 3.
                if matches!(file.expected, ExpectedLayout::Project)
                    && schema_id != Some("rp/project/v1")
                {
                    findings.push(|| {
                        Finding::new(
                            "RP_E_SCHEMA_CONST",
                            "schema_const",
                            Severity::Error,
                            ".research/project.yaml requires schema rp/project/v1",
                            Some(file.path),
                            "/schema",
                        )
                    });
                    continue;
                }
                if schema_bundle.validate_into(&parsed.value, file.path.clone(), &mut findings) {
                    parsed_objects.push(ParsedObject {
                        path: file.path,
                        expected: file.expected,
                        value: parsed.value,
                    });
                }
            }
            Err(finding) => findings.push(|| finding),
        }
    }

    let project_descriptors: Vec<_> = parsed_objects
        .iter()
        .filter(|object| {
            object.value.get("schema").and_then(Value::as_str) == Some("rp/project/v1")
        })
        .collect();
    if project_descriptors.len() > 1 {
        findings.push(|| {
            Finding::new(
                "RP_E_PROJECT_DESCRIPTOR_MULTIPLE",
                "project_discovery",
                Severity::Error,
                "more than one Project descriptor was found below .research",
                project_descriptors.get(1).map(|object| object.path.clone()),
                "/schema",
            )
        });
    }

    if !findings.is_empty() || findings.cancelled() {
        let (findings, finding_output, had_error, complete) = findings.into_output().report_parts();
        return Ok(ValidationReport {
            findings,
            had_error,
            complete,
            finding_output,
            canonical_object_count,
            canonical_paths,
            stage3_ran: false,
            index: None,
        });
    }

    let (index, semantic_findings) =
        build_and_validate_index(root, parsed_objects, limits, &schema_bundle, findings);
    let (semantic_findings, finding_output, had_error, complete) =
        semantic_findings.into_output().report_parts();
    Ok(ValidationReport {
        findings: semantic_findings,
        had_error,
        complete,
        finding_output,
        canonical_object_count,
        canonical_paths,
        stage3_ran: true,
        index: complete.then_some(index),
    })
}

#[cfg(target_os = "linux")]
fn scan_canonical_files(
    root: &Path,
    limits: ProjectLimits,
    execution: &crate::ExecutionBudget,
) -> Result<CanonicalScan, ProjectLoadError> {
    use std::collections::{BTreeSet, VecDeque};

    use rustix::fs::{
        AtFlags, CWD, Dir, FileType, Mode, OFlags, ResolveFlags, fstat, openat, openat2, statat,
    };

    let mut findings = Findings::with_execution(limits.findings, execution.clone());
    if findings.cancelled() {
        return Ok(CanonicalScan {
            research_fd: None,
            files: Vec::new(),
            findings,
        });
    }
    let root_fd = openat(
        CWD,
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| ProjectLoadError::Io(format!("cannot open project root ({error})")))?;

    let resolve = ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS;
    match statat(&root_fd, ".research", AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::Symlink => {
            findings.push(|| {
                path_finding(
                    "RP_E_PATH_SYMLINK_FORBIDDEN",
                    "path_containment",
                    ".research must not be a symlink",
                    None,
                )
            });
            return Ok(CanonicalScan {
                research_fd: None,
                files: Vec::new(),
                findings,
            });
        }
        Ok(stat) if FileType::from_raw_mode(stat.st_mode) != FileType::Directory => {
            findings.push(|| {
                path_finding(
                    "RP_E_PATH_NON_REGULAR_FILE",
                    "path_containment",
                    ".research must be a directory",
                    None,
                )
            });
            return Ok(CanonicalScan {
                research_fd: None,
                files: Vec::new(),
                findings,
            });
        }
        Ok(_) | Err(rustix::io::Errno::NOENT) => {}
        Err(error) => {
            return Err(ProjectLoadError::Io(format!(
                "cannot inspect .research ({error})"
            )));
        }
    }
    let research_fd = match openat2(
        &root_fd,
        ".research",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        resolve,
    ) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => {
            findings.push(|| {
                Finding::new(
                    "RP_E_PROJECT_DESCRIPTOR_NOT_FOUND",
                    "project_discovery",
                    Severity::Error,
                    ".research/project.yaml was not found",
                    None,
                    "",
                )
            });
            return Ok(CanonicalScan {
                research_fd: None,
                files: Vec::new(),
                findings,
            });
        }
        Err(rustix::io::Errno::NOSYS) => return Err(ProjectLoadError::UnsupportedPlatform),
        Err(error) => {
            return Err(ProjectLoadError::Io(format!(
                "cannot securely open .research ({error})"
            )));
        }
    };

    let research_stat = fstat(&research_fd)
        .map_err(|error| ProjectLoadError::Io(format!("cannot inspect .research ({error})")))?;
    let mut visited = BTreeSet::from([(research_stat.st_dev, research_stat.st_ino)]);
    let mut queue = VecDeque::from([String::new()]);
    let mut directories = 1_usize;
    let mut discovered = Vec::new();
    let mut limit_reached = false;

    while let Some(directory) = queue.pop_front() {
        if findings.cancelled() {
            break;
        }
        let directory_fd = if directory.is_empty() {
            openat2(
                &research_fd,
                ".",
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
                resolve,
            )
        } else {
            openat2(
                &research_fd,
                directory.as_str(),
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
                resolve,
            )
        }
        .map_err(|error| {
            ProjectLoadError::Io(format!("cannot open scanned directory ({error})"))
        })?;
        let mut entries = Vec::new();
        let dir = Dir::read_from(&directory_fd)
            .map_err(|error| ProjectLoadError::Io(format!("cannot read directory ({error})")))?;
        for entry in dir {
            if findings.cancelled() {
                break;
            }
            let entry = entry.map_err(|error| {
                ProjectLoadError::Io(format!("cannot enumerate directory ({error})"))
            })?;
            let name_bytes = entry.file_name().to_bytes();
            if matches!(name_bytes, b"." | b"..") {
                continue;
            }
            #[cfg(test)]
            scan_inventory_tests::visited();
            if entries.len()
                == limits
                    .directory_entries
                    .min(ProjectLimits::default().directory_entries)
            {
                // Readdir order is unspecified. Do not sort/process an arbitrary
                // buffered prefix, count the tail, or attribute the overflowing name.
                findings.push(|| {
                    resource_finding(
                        "RP_E_RESOURCE_DIRECTORY_ENTRIES_EXCEEDED",
                        "directory inventory entry limit exceeded; canonical scan aborted",
                        None,
                    )
                });
                return Ok(CanonicalScan {
                    research_fd: Some(research_fd),
                    files: Vec::new(),
                    findings,
                });
            }
            entries.push(name_bytes.to_vec());
            #[cfg(test)]
            scan_inventory_tests::buffered(entries.len());
        }
        entries.sort();

        for name_bytes in entries {
            if findings.cancelled() {
                break;
            }
            let Ok(name) = String::from_utf8(name_bytes) else {
                findings.push(|| {
                    path_finding(
                        "RP_E_PATH_PROJECT_ESCAPE",
                        "path_containment",
                        "non-UTF-8 names are forbidden below .research",
                        None,
                    )
                });
                continue;
            };
            let relative = if directory.is_empty() {
                name.clone()
            } else {
                format!("{directory}/{name}")
            };
            let project_path = ProjectPath::new(format!(".research/{relative}"));
            let path_for_finding = project_path.as_ref().ok().cloned();
            if project_path.is_err() {
                findings.push(|| {
                    path_finding(
                        "RP_E_PATH_PROJECT_ESCAPE",
                        "path_containment",
                        "path is not a safe normalized project-relative path",
                        None,
                    )
                });
                continue;
            }

            let stat = statat(&directory_fd, name.as_str(), AtFlags::SYMLINK_NOFOLLOW).map_err(
                |error| ProjectLoadError::Io(format!("cannot inspect directory entry ({error})")),
            )?;
            match FileType::from_raw_mode(stat.st_mode) {
                FileType::Symlink => findings.push(|| {
                    path_finding(
                        "RP_E_PATH_SYMLINK_FORBIDDEN",
                        "path_containment",
                        "symlinks are forbidden below .research",
                        path_for_finding,
                    )
                }),
                FileType::Directory => {
                    directories += 1;
                    if directories > limits.scanned_directories {
                        findings.push(|| {
                            resource_finding(
                                "RP_E_RESOURCE_SCANNED_DIRECTORIES_EXCEEDED",
                                "scanned directory limit exceeded",
                                path_for_finding,
                            )
                        });
                        limit_reached = true;
                        break;
                    }
                    if !visited.insert((stat.st_dev, stat.st_ino)) {
                        findings.push(|| {
                            path_finding(
                                "RP_E_PATH_PROJECT_ESCAPE",
                                "path_containment",
                                "directory filesystem identity was visited more than once",
                                path_for_finding,
                            )
                        });
                    } else {
                        queue.push_back(relative);
                    }
                }
                FileType::RegularFile => {
                    if let Some(expected) = expected_layout(&relative) {
                        let path = project_path.expect("checked project path");
                        let size = usize::try_from(stat.st_size).unwrap_or(usize::MAX);
                        // Discovery includes oversized files and the descriptor; size checks
                        // must not let them bypass the canonical-file scan budget.
                        discovered.push((relative, path, expected, size));
                        if discovered.len() > limits.canonical_files {
                            findings.push(|| {
                                resource_finding(
                                    "RP_E_RESOURCE_SCANNED_FILES_EXCEEDED",
                                    "canonical file limit exceeded",
                                    path_for_finding,
                                )
                            });
                            limit_reached = true;
                            break;
                        }
                    }
                }
                _ => findings.push(|| {
                    path_finding(
                        "RP_E_PATH_NON_REGULAR_FILE",
                        "path_containment",
                        "only regular files and directories are allowed below .research",
                        path_for_finding,
                    )
                }),
            }
        }
        if limit_reached {
            break;
        }
    }

    discovered.sort_by(|left, right| left.1.cmp(&right.1));
    if !discovered
        .iter()
        .any(|(relative, _, _, _)| relative == "project.yaml")
    {
        findings.push(|| {
            Finding::new(
                "RP_E_PROJECT_DESCRIPTOR_NOT_FOUND",
                "project_discovery",
                Severity::Error,
                ".research/project.yaml was not found",
                None,
                "",
            )
        });
    }

    // A truncated scan must not read, parse, or account bytes for a partial project.
    if limit_reached || findings.cancelled() {
        return Ok(CanonicalScan {
            research_fd: Some(research_fd),
            files: Vec::new(),
            findings,
        });
    }

    let mut files = Vec::with_capacity(discovered.len());
    let mut total_bytes = 0_usize;
    for (relative, path, expected, expected_size) in discovered {
        if findings.cancelled() {
            break;
        }
        if expected_size > limits.yaml_bytes_per_object {
            findings.push(|| {
                resource_finding(
                    "RP_E_RESOURCE_FILE_SIZE_EXCEEDED",
                    "canonical YAML file size limit exceeded",
                    Some(path),
                )
            });
            continue;
        }
        total_bytes = total_bytes.saturating_add(expected_size);
        if total_bytes > limits.whole_project_yaml_bytes {
            findings.push(|| {
                resource_finding(
                    "RP_E_RESOURCE_TOTAL_BYTES_EXCEEDED",
                    "whole-project YAML byte limit exceeded",
                    Some(path),
                )
            });
            break;
        }
        files.push(CanonicalFile {
            relative,
            path,
            expected,
            expected_size,
        });
    }
    Ok(CanonicalScan {
        research_fd: Some(research_fd),
        files,
        findings,
    })
}

fn path_finding(
    code: &'static str,
    family: &'static str,
    message: &'static str,
    source_file: Option<ProjectPath>,
) -> Finding {
    Finding::new(code, family, Severity::Error, message, source_file, "")
}

fn resource_finding(
    code: &'static str,
    message: &'static str,
    source_file: Option<ProjectPath>,
) -> Finding {
    Finding::new(
        code,
        "resource_limit",
        Severity::Error,
        message,
        source_file,
        "",
    )
}

fn expected_layout(relative: &str) -> Option<ExpectedLayout> {
    if relative == "project.yaml" {
        return Some(ExpectedLayout::Project);
    }
    if !relative.ends_with(".yaml") {
        return None;
    }
    let parts: Vec<_> = relative.split('/').collect();
    match parts.as_slice() {
        ["records", kind_directory, _] => Some(ExpectedLayout::Node(node_kind_for_directory(
            kind_directory,
        ))),
        ["relations", _] => Some(ExpectedLayout::Relation),
        ["assessments", _] => Some(ExpectedLayout::Assessment),
        ["threads", _] => Some(ExpectedLayout::Thread),
        ["thread-bindings", _] => Some(ExpectedLayout::ThreadBinding),
        ["claim-chains", _] => Some(ExpectedLayout::ClaimChain),
        ["references", _] => Some(ExpectedLayout::ExternalReference),
        ["artifacts", _] => Some(ExpectedLayout::Artifact),
        ["runs", _] => Some(ExpectedLayout::ResearchRun),
        [first, ..]
            if matches!(
                *first,
                "records"
                    | "relations"
                    | "assessments"
                    | "threads"
                    | "thread-bindings"
                    | "claim-chains"
                    | "references"
                    | "artifacts"
                    | "runs"
            ) =>
        {
            Some(ExpectedLayout::Invalid)
        }
        _ => None,
    }
}

fn node_kind_for_directory(directory: &str) -> Option<&'static str> {
    match directory {
        "datasets" => Some("Dataset"),
        "questions" => Some("Question"),
        "hypotheses" => Some("Hypothesis"),
        "observations" => Some("Observation"),
        "measurements" => Some("Measurement"),
        "methods" => Some("Method"),
        "tests" => Some("Test"),
        "predictions" => Some("Prediction"),
        "syntheses" => Some("Synthesis"),
        "interpretations" => Some("Interpretation"),
        "decisions" => Some("Decision"),
        "conclusions" => Some("Conclusion"),
        "blockers" => Some("Blocker"),
        "next-actions" => Some("NextAction"),
        "paper-claims" => Some("PaperClaim"),
        _ => None,
    }
}

fn build_and_validate_index(
    root: &Path,
    objects: Vec<ParsedObject>,
    limits: ProjectLimits,
    schema_bundle: &SchemaBundle,
    mut findings: Findings,
) -> (ProjectIndex, Findings) {
    let mut index = ProjectIndex::default();
    index.validation_limits = limits;
    index.execution = findings.execution.clone();
    let mut unique_identity = true;

    if objects.len() > limits.graph_nodes {
        findings.push(|| {
            resource_finding(
                "RP_E_RESOURCE_GRAPH_NODES_EXCEEDED",
                "graph node limit exceeded",
                None,
            )
        });
        return (index, findings);
    }

    let tracking = objects.iter().any(|o| {
        o.value.get("schema").and_then(Value::as_str) == Some("rp/claim-chain-snapshot/v1")
            && o.value
                .get("validation_report")
                .is_some_and(|v| !v.is_null())
    });
    let mut provenance =
        crate::content_observations::ContentFindings::with_budget(tracking, findings.fork());
    for object in objects {
        if findings.cancelled() {
            break;
        }
        let mut canonical_findings = findings.fork();
        let id = object
            .value
            .get("id")
            .and_then(Value::as_str)
            .expect("schema-valid canonical object has an ID")
            .to_owned();
        let object_type = ObjectType::from_value(&object.value)
            .expect("schema-valid canonical object has a supported type");

        if !object.expected.accepts(&object_type) {
            canonical_findings.push(|| {
                Finding::new(
                    "RP_E_REFERENCE_TYPE_MISMATCH",
                    "reference_integrity",
                    Severity::Error,
                    format!(
                        "canonical layout expects {}, but object is {}",
                        object.expected.label(),
                        object_type.label()
                    ),
                    Some(object.path.clone()),
                    "/schema",
                )
            });
        }
        if !matches!(object.expected, ExpectedLayout::Project)
            && !filename_matches_id(&object.path, &id)
        {
            canonical_findings.push(|| {
                Finding::new(
                    "RP_E_OBJECT_FILENAME_ID_MISMATCH",
                    "object_identity",
                    Severity::Error,
                    "canonical filename suffix does not match the object ID",
                    Some(object.path.clone()),
                    "/id",
                )
            });
        }
        if object.value.get("record_state").and_then(Value::as_str) == Some("draft") {
            canonical_findings.push(|| {
                Finding::new(
                    "RP_E_CANONICAL_DRAFT_FORBIDDEN",
                    "canonical_state",
                    Severity::Error,
                    "draft records are forbidden in the canonical .research tree",
                    Some(object.path.clone()),
                    "/record_state",
                )
            });
        }

        let record = ObjectRecord {
            ordinal: index.len(),
            id: id.clone().into(),
            logical_id: object
                .value
                .get("logical_id")
                .and_then(Value::as_str)
                .map(Into::into),
            object_type,
            source_file: object.path.clone(),
            value: object.value,
        };
        provenance.begin(
            crate::content_observations::Owner::Object(record.id.clone()),
            crate::content_observations::Phase::Canonical,
            None,
            None,
            &[],
        );
        provenance.state(
            if record.value.get("record_state").and_then(Value::as_str) == Some("draft") {
                crate::content_observations::Completeness::Unavailable
            } else {
                crate::content_observations::Completeness::Verified
            },
        );
        provenance.absorb(canonical_findings);
        let next_output = findings.fork();
        findings.absorb(std::mem::replace(&mut provenance.output, next_output));
        if index.get(&id).is_some() {
            unique_identity = false;
            index.invalidate_subject_graph();
            findings.push(|| {
                Finding::new(
                    "RP_E_OBJECT_DUPLICATE_ID",
                    "object_identity",
                    Severity::Error,
                    format!("object ID {id} appears in more than one canonical file"),
                    Some(object.path),
                    "/id",
                )
            });
        } else {
            index.insert(record);
        }
    }

    let next_output = findings.fork();
    findings.absorb(std::mem::replace(&mut provenance.output, next_output));
    if findings.cancelled() {
        return (index, findings);
    }
    index.assign_ordinals();
    let mut references = Vec::new();
    for object in index.object_values() {
        if findings.cancelled() {
            break;
        }
        collect_references(object, &mut references);
        if references.len() > limits.graph_edges {
            findings.push(|| {
                resource_finding(
                    "RP_E_RESOURCE_GRAPH_EDGES_EXCEEDED",
                    "graph edge limit exceeded",
                    None,
                )
            });
            return (index.empty_with_context(), findings);
        }
    }
    validate_references(&index, &references, &mut findings);
    validate_semantics(&index, limits, &mut findings);
    if findings.cancelled() {
        return (index, findings);
    }
    index.derive_heads();

    if let Some(store) = &mut provenance.store {
        // Stage 1/2 completed before entry. Every canonical object was visited and
        // projected at its producing owner; duplicate identity still blocks readiness.
        store.pipeline_provenance_complete = unique_identity && store.coverage_complete;
    }
    let content = crate::content::validate_content_with_observations(
        root,
        &index,
        limits,
        schema_bundle,
        limits.graph_edges - references.len(),
        provenance,
    );
    index.content_observations = content.observations;
    findings.absorb(content.findings);
    if findings.cancelled() {
        return (index, findings);
    }
    if content.extension_references_exhausted {
        // Never publish a partial reference set or compute access from it.
        return (index.empty_with_context(), findings);
    }
    validate_references(&index, &content.extension_references, &mut findings);
    references.extend(content.extension_references);
    index.set_references(references);
    index.set_verified_artifacts(content.verified_artifacts.clone());
    index.set_freshness_policy(content.freshness_policy);
    index.set_unbound_reports(content.unbound_reports);

    let (access, access_findings) = crate::access::compute_access(&index, findings.fork());
    findings.absorb(access_findings);
    if findings.cancelled() {
        return (index, findings);
    }
    index.set_access(access);

    let (claim_chains, claim_findings) =
        crate::claim::validate_claim_chains(&index, &content.verified_artifacts, findings.fork());
    findings.absorb(claim_findings);
    index.set_claim_chains(claim_chains);
    // This records graph availability only, not content completeness or validity.
    if unique_identity && !findings.cancelled() {
        index.mark_subject_graph_ready();
    }
    if tracking {
        let bindings = crate::report_binding::bind_reports(&index, &mut findings);
        index.set_report_bindings(bindings);
    }
    (index, findings)
}

fn filename_matches_id(path: &ProjectPath, id: &str) -> bool {
    let Some(filename) = path.as_str().rsplit('/').next() else {
        return false;
    };
    let Some(stem) = filename.strip_suffix(".yaml") else {
        return false;
    };
    stem.rsplit_once("--")
        .is_some_and(|(slug, suffix)| suffix == id && is_filename_slug(slug))
}

fn is_filename_slug(slug: &str) -> bool {
    !slug.is_empty()
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !slug.contains("--")
}

fn collect_references(object: &ObjectRecord, references: &mut Vec<ReferenceEdge>) {
    collect_source_references(object, references);
    match &object.object_type {
        ObjectType::Project | ObjectType::ExternalReference | ObjectType::Artifact => {}
        ObjectType::Node(_) => {
            collect_parent_ids(object, ReferenceExpectation::Node, references);
            collect_optional(
                object,
                "/scope/research_run_id",
                ReferenceExpectation::ResearchRun,
                references,
            );
            collect_array(
                object,
                "/scope/dataset_revisions",
                ReferenceExpectation::NodeKind("Dataset".into()),
                references,
            );
            collect_optional(
                object,
                "/observation/method/protocol_reference",
                ReferenceExpectation::ExternalReference,
                references,
            );
            collect_optional(
                object,
                "/measurement/method/protocol_reference",
                ReferenceExpectation::ExternalReference,
                references,
            );
            collect_array(
                object,
                "/method/protocol_references",
                ReferenceExpectation::ExternalReference,
                references,
            );
            collect_array(
                object,
                "/test/target_revision_ids",
                ReferenceExpectation::Node,
                references,
            );
            collect_optional(
                object,
                "/test/method_revision",
                ReferenceExpectation::NodeKind("Method".into()),
                references,
            );
            collect_array(
                object,
                "/next_action/dependencies",
                ReferenceExpectation::Node,
                references,
            );
            collect_optional(
                object,
                "/paper_claim/publication_reference",
                ReferenceExpectation::ExternalReference,
                references,
            );
        }
        ObjectType::Relation => {
            collect_optional(
                object,
                "/from_revision",
                ReferenceExpectation::Node,
                references,
            );
            collect_optional(
                object,
                "/to_revision",
                ReferenceExpectation::Node,
                references,
            );
            collect_parent_ids(object, ReferenceExpectation::Relation, references);
        }
        ObjectType::Assessment => {
            collect_optional(
                object,
                "/target/id",
                assessment_target_expectation(&object.value),
                references,
            );
            collect_array(
                object,
                "/supersedes_assessments",
                ReferenceExpectation::Assessment,
                references,
            );
            collect_array(
                object,
                "/quantitative_assessment/source_measurements",
                ReferenceExpectation::NodeKind("Measurement".into()),
                references,
            );
        }
        ObjectType::Thread => {
            collect_optional(
                object,
                "/root_question_revision",
                ReferenceExpectation::NodeKind("Question".into()),
                references,
            );
            collect_optional(
                object,
                "/parent_thread_id",
                ReferenceExpectation::Thread,
                references,
            );
            collect_optional(
                object,
                "/forked_from_thread_id",
                ReferenceExpectation::Thread,
                references,
            );
        }
        ObjectType::ThreadBinding => {
            collect_optional(
                object,
                "/thread_id",
                ReferenceExpectation::Thread,
                references,
            );
            collect_optional(object, "/target/id", ReferenceExpectation::Node, references);
            collect_parent_ids(object, ReferenceExpectation::ThreadBinding, references);
        }
        ObjectType::ClaimChain => {
            collect_optional(
                object,
                "/thread_id",
                ReferenceExpectation::Thread,
                references,
            );
            collect_array(
                object,
                "/root_node_revisions",
                ReferenceExpectation::Node,
                references,
            );
            collect_optional(
                object,
                "/target_node_revision",
                ReferenceExpectation::Node,
                references,
            );
            collect_array(
                object,
                "/node_revisions",
                ReferenceExpectation::Node,
                references,
            );
            collect_array(
                object,
                "/relation_revisions",
                ReferenceExpectation::Relation,
                references,
            );
            collect_optional(
                object,
                "/validation_report",
                ReferenceExpectation::Artifact,
                references,
            );
        }
        ObjectType::ResearchRun => {
            collect_optional(
                object,
                "/project_id",
                ReferenceExpectation::Project,
                references,
            );
            collect_optional(
                object,
                "/parent_run_id",
                ReferenceExpectation::ResearchRun,
                references,
            );
            collect_array(
                object,
                "/planned_inputs/revisions",
                ReferenceExpectation::Node,
                references,
            );
            collect_array(
                object,
                "/planned_inputs/artifacts",
                ReferenceExpectation::Artifact,
                references,
            );
        }
    }
}

fn collect_source_references(object: &ObjectRecord, references: &mut Vec<ReferenceEdge>) {
    collect_array(
        object,
        "/source/revisions",
        ReferenceExpectation::Node,
        references,
    );
    collect_array(
        object,
        "/source/relations",
        ReferenceExpectation::Relation,
        references,
    );
    collect_array(
        object,
        "/source/artifacts",
        ReferenceExpectation::Artifact,
        references,
    );
    collect_array(
        object,
        "/source/external_references",
        ReferenceExpectation::ExternalReference,
        references,
    );
}

fn collect_parent_ids(
    object: &ObjectRecord,
    expected: ReferenceExpectation,
    references: &mut Vec<ReferenceEdge>,
) {
    let Some(parents) = object
        .value
        .pointer("/revision/parents")
        .and_then(Value::as_array)
    else {
        return;
    };
    for (index, parent) in parents.iter().enumerate() {
        if let Some(id) = parent.get("id").and_then(Value::as_str) {
            references.push(reference(
                object,
                id,
                expected.clone(),
                format!("/revision/parents/{index}/id"),
            ));
        }
    }
}

fn collect_optional(
    object: &ObjectRecord,
    pointer: &str,
    expected: ReferenceExpectation,
    references: &mut Vec<ReferenceEdge>,
) {
    if let Some(id) = object.value.pointer(pointer).and_then(Value::as_str) {
        references.push(reference(object, id, expected, pointer.to_string()));
    }
}

fn collect_array(
    object: &ObjectRecord,
    pointer: &str,
    expected: ReferenceExpectation,
    references: &mut Vec<ReferenceEdge>,
) {
    let Some(values) = object.value.pointer(pointer).and_then(Value::as_array) else {
        return;
    };
    for (index, value) in values.iter().enumerate() {
        if let Some(id) = value.as_str() {
            references.push(reference(
                object,
                id,
                expected.clone(),
                format!("{pointer}/{index}"),
            ));
        }
    }
}

fn reference(
    object: &ObjectRecord,
    target_id: &str,
    expected: ReferenceExpectation,
    json_pointer: String,
) -> ReferenceEdge {
    ReferenceEdge {
        source_id: object.id.clone(),
        target_id: target_id.into(),
        target_ordinal: None,
        expected,
        json_pointer,
    }
}

fn assessment_target_expectation(value: &Value) -> ReferenceExpectation {
    match value.pointer("/target/type").and_then(Value::as_str) {
        Some("relation_revision") => ReferenceExpectation::Relation,
        _ => ReferenceExpectation::Node,
    }
}

pub(crate) fn validate_references(
    index: &ProjectIndex,
    references: &[ReferenceEdge],
    findings: &mut Findings,
) {
    for reference in references {
        if findings.cancelled() {
            break;
        }
        let source = index
            .get(&reference.source_id)
            .expect("reference source is indexed");
        match index.get(&reference.target_id) {
            Some(target) if !reference.expected.accepts(&target.object_type) => {
                findings.push(|| {
                    Finding::new(
                        "RP_E_REFERENCE_TYPE_MISMATCH",
                        "reference_integrity",
                        Severity::Error,
                        format!(
                            "reference expects {}, but target is {}",
                            reference.expected.label(),
                            target.object_type.label()
                        ),
                        Some(source.source_file.clone()),
                        reference.json_pointer.clone(),
                    )
                })
            }
            Some(_) => {}
            None if matches!(source.object_type, ObjectType::ThreadBinding)
                && matches!(reference.json_pointer.as_str(), "/thread_id" | "/target/id") =>
            {
                findings.push(|| {
                    Finding::new(
                        "RP_E_ORPHAN_THREAD_BINDING",
                        "thread_binding_integrity",
                        Severity::Error,
                        format!(
                            "thread binding target {} was not found",
                            reference.target_id
                        ),
                        Some(source.source_file.clone()),
                        reference.json_pointer.clone(),
                    )
                })
            }
            None if matches!(
                reference.expected,
                ReferenceExpectation::Node
                    | ReferenceExpectation::NodeKind(_)
                    | ReferenceExpectation::Relation
                    | ReferenceExpectation::ThreadBinding
            ) =>
            {
                findings.push(|| {
                    Finding::new(
                        "RP_E_DANGLING_REVISION_REFERENCE",
                        "reference_integrity",
                        Severity::Error,
                        format!("exact revision {} was not found", reference.target_id),
                        Some(source.source_file.clone()),
                        reference.json_pointer.clone(),
                    )
                })
            }
            None => findings.push(|| {
                Finding::new(
                    "RP_E_REFERENCE_NOT_FOUND",
                    "reference_integrity",
                    Severity::Error,
                    format!("referenced object {} was not found", reference.target_id),
                    Some(source.source_file.clone()),
                    reference.json_pointer.clone(),
                )
            }),
        }
    }
}

pub(crate) fn validate_semantics(
    index: &ProjectIndex,
    limits: ProjectLimits,
    findings: &mut Findings,
) {
    if findings.cancelled() {
        return;
    }
    validate_lineages(index, findings);
    validate_relation_compatibility(index, findings);
    validate_assessments(index, findings);
    validate_cycles(index, limits, findings);
}

fn validate_lineages(index: &ProjectIndex, findings: &mut Findings) {
    if findings.cancelled() {
        return;
    }
    for object in index.object_values() {
        if findings.cancelled() {
            break;
        }
        let Some(parents) = object
            .value
            .pointer("/revision/parents")
            .and_then(Value::as_array)
        else {
            continue;
        };
        for (position, parent_value) in parents.iter().enumerate() {
            if findings.cancelled() {
                break;
            }
            let Some(parent_id) = parent_value.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(parent) = index.get(parent_id) else {
                continue;
            };
            let pointer = format!("/revision/parents/{position}/id");
            match (&object.object_type, &parent.object_type) {
                (ObjectType::Node(kind), ObjectType::Node(parent_kind)) => {
                    if object.logical_id != parent.logical_id {
                        findings.push(|| {
                            lineage_finding(
                                "RP_E_NODE_LINEAGE_LOGICAL_ID_MISMATCH",
                                "node_lineage",
                                "node parent belongs to a different logical lineage",
                                object,
                                &pointer,
                            )
                        });
                    }
                    if kind != parent_kind {
                        findings.push(|| {
                            lineage_finding(
                                "RP_E_NODE_LINEAGE_KIND_MISMATCH",
                                "node_lineage",
                                "node parent has an incompatible kind",
                                object,
                                &pointer,
                            )
                        });
                    }
                }
                (ObjectType::Relation, ObjectType::Relation) => {
                    if object.logical_id != parent.logical_id {
                        findings.push(|| {
                            lineage_finding(
                                "RP_E_RELATION_LINEAGE_LOGICAL_ID_MISMATCH",
                                "relation_lineage",
                                "relation parent belongs to a different logical lineage",
                                object,
                                &pointer,
                            )
                        });
                    }
                    if object.value.get("type") != parent.value.get("type") {
                        findings.push(|| {
                            lineage_finding(
                                "RP_E_RELATION_LINEAGE_TYPE_MISMATCH",
                                "relation_lineage",
                                "relation parent has a different relation type",
                                object,
                                &pointer,
                            )
                        });
                    }
                }
                (ObjectType::ThreadBinding, ObjectType::ThreadBinding) => {
                    if object.logical_id != parent.logical_id {
                        findings.push(|| {
                            lineage_finding(
                                "RP_E_BINDING_LINEAGE_LOGICAL_ID_MISMATCH",
                                "binding_lineage",
                                "binding parent belongs to a different logical lineage",
                                object,
                                &pointer,
                            )
                        });
                    }
                    if object.value.get("thread_id") != parent.value.get("thread_id")
                        || object.value.get("target") != parent.value.get("target")
                    {
                        findings.push(|| {
                            lineage_finding(
                                "RP_E_BINDING_LINEAGE_CONTEXT_MISMATCH",
                                "binding_lineage",
                                "binding parent has a different thread or exact target context",
                                object,
                                &pointer,
                            )
                        });
                    }
                }
                _ => {}
            }
        }
    }
}

fn lineage_finding(
    code: &'static str,
    family: &'static str,
    message: &'static str,
    object: &ObjectRecord,
    pointer: &str,
) -> Finding {
    Finding::new(
        code,
        family,
        Severity::Error,
        message,
        Some(object.source_file.clone()),
        pointer,
    )
}

fn validate_relation_compatibility(index: &ProjectIndex, findings: &mut Findings) {
    if findings.cancelled() {
        return;
    }
    for relation in index
        .object_values()
        .filter(|object| matches!(object.object_type, ObjectType::Relation))
    {
        let Some(relation_type) = relation.value.get("type").and_then(Value::as_str) else {
            continue;
        };
        let Some(from) = relation
            .value
            .get("from_revision")
            .and_then(Value::as_str)
            .and_then(|id| index.get(id))
        else {
            continue;
        };
        let Some(to) = relation
            .value
            .get("to_revision")
            .and_then(Value::as_str)
            .and_then(|id| index.get(id))
        else {
            continue;
        };
        let (ObjectType::Node(from_kind), ObjectType::Node(to_kind)) =
            (&from.object_type, &to.object_type)
        else {
            continue;
        };
        if !relation_kinds_compatible(relation_type, from_kind, to_kind) {
            findings.push(|| Finding::new(
                "RP_E_RELATION_KIND_INCOMPATIBLE",
                "relation_compatibility",
                Severity::Error,
                format!(
                    "relation type {relation_type} is incompatible with {from_kind} -> {to_kind}"
                ),
                Some(relation.source_file.clone()),
                "/type",
            ));
        }
    }
}

fn relation_kinds_compatible(relation: &str, from: &str, to: &str) -> bool {
    let any = || true;
    let in_set = |kind: &str, allowed: &[&str]| allowed.contains(&kind);
    match relation {
        "derived-from" | "depends-on" => any(),
        "has-part" => {
            in_set(from, &["Dataset", "Method", "Synthesis"])
                && in_set(
                    to,
                    &[
                        "Dataset",
                        "Method",
                        "Observation",
                        "Measurement",
                        "PaperClaim",
                    ],
                )
        }
        "input-to" => {
            in_set(
                from,
                &[
                    "Dataset",
                    "Observation",
                    "Measurement",
                    "PaperClaim",
                    "Method",
                ],
            ) && to == "Synthesis"
        }
        "generated" => {
            from == "Synthesis"
                && in_set(
                    to,
                    &[
                        "Observation",
                        "Measurement",
                        "Interpretation",
                        "Decision",
                        "Conclusion",
                    ],
                )
        }
        "cites" => to == "PaperClaim",
        "implements" => in_set(from, &["Method", "Test"]) && in_set(to, &["Prediction", "Test"]),
        "supports" | "weakens" | "contradicts" | "consistent-with" => {
            in_set(
                from,
                &["Observation", "Measurement", "PaperClaim", "Synthesis"],
            ) && in_set(
                to,
                &["Hypothesis", "Prediction", "Interpretation", "Conclusion"],
            )
        }
        "provides-prior" => {
            in_set(
                from,
                &["Dataset", "Observation", "Measurement", "PaperClaim"],
            ) && in_set(to, &["Hypothesis", "Method", "Test"])
        }
        "motivates" => from == "Question" && to == "Hypothesis",
        "predicts" => from == "Hypothesis" && to == "Prediction",
        "tested-by" => in_set(from, &["Hypothesis", "Prediction"]) && to == "Test",
        "result-of" => in_set(from, &["Observation", "Measurement"]) && to == "Test",
        "observed-in" => in_set(from, &["Observation", "Measurement"]) && to == "Dataset",
        "reproduces" | "fails-to-reproduce" => {
            in_set(from, &["Observation", "Measurement", "Test"])
                && in_set(to, &["Observation", "Measurement", "Test"])
        }
        "validated-by" => {
            in_set(from, &["Dataset", "Observation", "Measurement", "Method"])
                && in_set(to, &["Method", "Test"])
        }
        "requires" => in_set(
            from,
            &["Method", "Test", "Synthesis", "Decision", "NextAction"],
        ),
        "blocked-by" => in_set(from, &["Decision", "NextAction"]) && to == "Blocker",
        "enables" => {
            in_set(
                from,
                &[
                    "Dataset",
                    "Observation",
                    "Measurement",
                    "Method",
                    "Test",
                    "Synthesis",
                    "Decision",
                    "Conclusion",
                ],
            ) && in_set(
                to,
                &["Method", "Test", "Synthesis", "Decision", "NextAction"],
            )
        }
        "next-step" => {
            in_set(
                from,
                &[
                    "Question",
                    "Hypothesis",
                    "Interpretation",
                    "Decision",
                    "Blocker",
                    "Conclusion",
                    "NextAction",
                ],
            ) && to == "NextAction"
        }
        _ => false,
    }
}

fn validate_assessments(index: &ProjectIndex, findings: &mut Findings) {
    if findings.cancelled() {
        return;
    }
    for assessment in index
        .object_values()
        .filter(|object| matches!(object.object_type, ObjectType::Assessment))
    {
        let Some(supersedes) = assessment
            .value
            .get("supersedes_assessments")
            .and_then(Value::as_array)
        else {
            continue;
        };
        for (position, superseded) in supersedes.iter().enumerate() {
            if findings.cancelled() {
                break;
            }
            let Some(parent) = superseded.as_str().and_then(|id| index.get(id)) else {
                continue;
            };
            if assessment_lane(&assessment.value) != assessment_lane(&parent.value) {
                findings.push(|| {
                    Finding::new(
                        "RP_E_ASSESSMENT_SUPERSESSION_LANE_MISMATCH",
                        "assessment_supersession",
                        Severity::Error,
                        "assessment supersession crosses an independent assessment lane",
                        Some(assessment.source_file.clone()),
                        format!("/supersedes_assessments/{position}"),
                    )
                });
            }
        }
    }
}

fn assessment_lane(value: &Value) -> [Option<&str>; 6] {
    [
        value.pointer("/target/type").and_then(Value::as_str),
        value.pointer("/target/id").and_then(Value::as_str),
        value.get("assessment_scope").and_then(Value::as_str),
        value.pointer("/assessed_by/type").and_then(Value::as_str),
        value.pointer("/assessed_by/id").and_then(Value::as_str),
        value.get("review_assurance").and_then(Value::as_str),
    ]
}

#[derive(Clone, Copy)]
enum CycleClass {
    Node,
    Relation,
    Binding,
    Thread,
    Assessment,
}

fn validate_cycles(index: &ProjectIndex, limits: ProjectLimits, findings: &mut Findings) {
    if findings.cancelled() {
        return;
    }
    for class in [
        CycleClass::Node,
        CycleClass::Relation,
        CycleClass::Binding,
        CycleClass::Thread,
        CycleClass::Assessment,
    ] {
        if findings.cancelled() {
            break;
        }
        let cycle_id = match find_cycle(index, class, limits.traversal_depth) {
            CycleSearch::Stopped => return,
            CycleSearch::Acyclic => continue,
            CycleSearch::Cycle(cycle_id) => cycle_id,
            CycleSearch::DepthExceeded => {
                findings.push(|| {
                    resource_finding(
                        "RP_E_RESOURCE_TRAVERSAL_DEPTH_EXCEEDED",
                        "graph traversal depth limit exceeded",
                        None,
                    )
                });
                return;
            }
        };
        let (code, family, message) = match class {
            CycleClass::Node => (
                "RP_E_REVISION_DAG_CYCLE",
                "revision_dag_cycle",
                "node revision lineage contains a cycle",
            ),
            CycleClass::Relation => (
                "RP_E_RELATION_DAG_CYCLE",
                "relation_dag_cycle",
                "relation revision lineage contains a cycle",
            ),
            CycleClass::Binding => (
                "RP_E_BINDING_DAG_CYCLE",
                "binding_dag_cycle",
                "thread binding lineage contains a cycle",
            ),
            CycleClass::Thread => (
                "RP_E_THREAD_DAG_CYCLE",
                "thread_dag_cycle",
                "research thread hierarchy contains a cycle",
            ),
            CycleClass::Assessment => (
                "RP_E_ASSESSMENT_SUPERSESSION_CYCLE",
                "assessment_supersession",
                "assessment supersession contains a cycle",
            ),
        };
        findings.push(|| {
            Finding::new(
                code,
                family,
                Severity::Error,
                message,
                index
                    .get(&cycle_id)
                    .map(|object| object.source_file.clone()),
                "",
            )
        });
    }
}

fn belongs_to_cycle_class(object: &ObjectRecord, class: CycleClass) -> bool {
    matches!(
        (class, &object.object_type),
        (CycleClass::Node, ObjectType::Node(_))
            | (CycleClass::Relation, ObjectType::Relation)
            | (CycleClass::Binding, ObjectType::ThreadBinding)
            | (CycleClass::Thread, ObjectType::Thread)
            | (CycleClass::Assessment, ObjectType::Assessment)
    )
}

fn cycle_targets(object: &ObjectRecord, class: CycleClass) -> Vec<&str> {
    #[cfg(test)]
    graph_semantics::TARGET_BUILDS.with(|count| count.set(count.get() + 1));
    let mut targets = Vec::new();
    match class {
        CycleClass::Node | CycleClass::Relation | CycleClass::Binding => {
            if let Some(parents) = object
                .value
                .pointer("/revision/parents")
                .and_then(Value::as_array)
            {
                targets.extend(
                    parents
                        .iter()
                        .filter_map(|parent| parent.get("id").and_then(Value::as_str)),
                );
            }
        }
        CycleClass::Thread => {
            targets.extend(
                ["parent_thread_id", "forked_from_thread_id"]
                    .into_iter()
                    .filter_map(|field| object.value.get(field).and_then(Value::as_str)),
            );
        }
        CycleClass::Assessment => {
            if let Some(values) = object
                .value
                .get("supersedes_assessments")
                .and_then(Value::as_array)
            {
                targets.extend(values.iter().filter_map(Value::as_str));
            }
        }
    }
    targets.sort_unstable();
    targets
}

enum CycleSearch {
    Stopped,
    Acyclic,
    Cycle(Box<str>),
    DepthExceeded,
}

fn find_cycle(index: &ProjectIndex, class: CycleClass, depth_limit: usize) -> CycleSearch {
    let mut colors: BTreeMap<&str, u8> = index
        .object_values()
        .filter(|object| belongs_to_cycle_class(object, class))
        .map(|object| (object.id.as_ref(), 0))
        .collect();
    for object in index
        .object_values()
        .filter(|object| belongs_to_cycle_class(object, class))
    {
        let start = object.id.as_ref();
        if colors.get(start) != Some(&0) {
            continue;
        }
        let mut stack = vec![(start, cycle_targets(object, class).into_iter())];
        colors.insert(start, 1);
        while let Some((node, mut neighbours)) = stack.pop() {
            if index.execution.checkpoint().is_err() {
                return CycleSearch::Stopped;
            }
            if stack.len() > depth_limit {
                return CycleSearch::DepthExceeded;
            }
            let Some(neighbour) = neighbours.next() else {
                colors.insert(node, 2);
                continue;
            };
            // Retain each frame's sorted targets across child visits.
            stack.push((node, neighbours));
            match colors.get(neighbour).copied().unwrap_or(2) {
                0 => {
                    colors.insert(neighbour, 1);
                    let targets = index
                        .get(neighbour)
                        .map(|object| cycle_targets(object, class))
                        .unwrap_or_default();
                    stack.push((neighbour, targets.into_iter()));
                }
                1 => return CycleSearch::Cycle(neighbour.into()),
                _ => {}
            }
        }
    }
    CycleSearch::Acyclic
}

#[cfg(test)]
mod graph_semantics {
    use super::*;
    use serde_json::json;

    thread_local! {
        pub(super) static TARGET_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    fn node_graph(edges: &[(&str, &[&str])]) -> ProjectIndex {
        let mut index = ProjectIndex::default();
        for (id, parents) in edges {
            index.insert(ObjectRecord {
                ordinal: index.len(),
                id: (*id).into(),
                logical_id: None,
                object_type: ObjectType::Node("question".into()),
                source_file: ProjectPath::new(format!(".research/{id}.yaml")).unwrap(),
                value: json!({ "revision": { "parents": parents.iter().map(|id| json!({"id": id})).collect::<Vec<_>>() } }),
            });
        }
        index
    }

    #[test]
    fn cycle_targets_are_built_once_per_active_frame() {
        let leaves: Vec<_> = (0..128).map(|i| format!("b{i:03}")).collect();
        let targets: Vec<_> = leaves.iter().rev().map(String::as_str).collect();
        let mut edges = vec![("a", targets.as_slice())];
        edges.extend(leaves.iter().map(|id| (id.as_str(), &[][..])));
        let index = node_graph(&edges);
        TARGET_BUILDS.with(|count| count.set(0));
        assert!(matches!(
            find_cycle(&index, CycleClass::Node, 1),
            CycleSearch::Acyclic
        ));
        TARGET_BUILDS.with(|count| assert_eq!(count.get(), index.len()));
    }

    #[test]
    fn cycle_targets_keep_sorted_order_when_frames_resume() {
        let index = node_graph(&[
            ("a", &["d", "b"]),
            ("b", &["c"]),
            ("c", &["b"]),
            ("d", &["a"]),
        ]);
        match find_cycle(&index, CycleClass::Node, 2) {
            CycleSearch::Cycle(id) => assert_eq!(id.as_ref(), "b"),
            _ => panic!("expected lexically first cycle target"),
        }
    }

    #[test]
    fn cycle_depth_counts_active_ancestors_not_resumed_siblings() {
        let index = node_graph(&[("a", &["c", "b"]), ("b", &[]), ("c", &["d"]), ("d", &[])]);
        assert!(matches!(
            find_cycle(&index, CycleClass::Node, 1),
            CycleSearch::DepthExceeded
        ));
        assert!(matches!(
            find_cycle(&index, CycleClass::Node, 2),
            CycleSearch::Acyclic
        ));
        let isolated = node_graph(&[("a", &[])]);
        assert!(matches!(
            find_cycle(&isolated, CycleClass::Node, 0),
            CycleSearch::Acyclic
        ));
    }
}

#[cfg(all(test, target_os = "linux"))]
mod scan_inventory_tests {
    struct TempProject(std::path::PathBuf);
    impl TempProject {
        fn empty() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "rp-cap-inventory-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn research(&self, name: &str) -> std::path::PathBuf {
            self.0.join(".research").join(name)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    use super::*;
    use std::{
        cell::Cell,
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::symlink},
    };

    thread_local! {
        static PEAK: Cell<usize> = const { Cell::new(0) };
        static VISITS: Cell<usize> = const { Cell::new(0) };
    }
    pub(super) fn visited() {
        VISITS.with(|n| n.set(n.get() + 1));
    }
    pub(super) fn buffered(count: usize) {
        PEAK.with(|n| n.set(n.get().max(count)));
    }
    #[test]
    fn directory_inventory_is_bounded_before_clone_and_permutation_independent() {
        let mut outputs = Vec::new();
        for reverse in [false, true] {
            let project = TempProject::empty();
            std::fs::create_dir(project.research("")).unwrap();
            let mut names = vec![
                OsString::from("z-link"),
                OsString::from_vec(vec![0xff]),
                OsString::from("project.yaml"),
                OsString::from("a.yaml"),
            ];
            if reverse {
                names.reverse();
            }
            for name in names {
                if name == "z-link" {
                    symlink("missing", project.research("").join(name)).unwrap();
                } else {
                    std::fs::write(project.research("").join(name), b"[malformed").unwrap();
                }
            }
            PEAK.with(|n| n.set(0));
            VISITS.with(|n| n.set(0));
            let report = validate_project_with_limits(
                project.path(),
                ProjectLimits {
                    directory_entries: 2,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                PEAK.with(Cell::get),
                2,
                "must reject excess before cloning a name"
            );
            assert_eq!(VISITS.with(Cell::get), 3, "must not count unbounded tail");
            assert_eq!(report.findings.len(), 1);
            assert_eq!(
                report.findings[0].error_code,
                "RP_E_RESOURCE_DIRECTORY_ENTRIES_EXCEEDED"
            );
            assert!(report.findings[0].source_file.is_none());
            assert!(report.index.is_none() && !report.stage3_ran);
            assert!(report.canonical_paths.is_empty());
            outputs.push(report.findings);
        }
        assert_eq!(outputs[0], outputs[1]);
    }
    #[test]
    fn directory_inventory_exact_boundary_and_zero() {
        let project = TempProject::empty();
        std::fs::create_dir(project.research("")).unwrap();
        std::fs::write(project.research("project.yaml"), b"[malformed").unwrap();
        for (limit, expected_peak, overflow) in [(0, 0, true), (1, 1, false)] {
            PEAK.with(|n| n.set(0));
            VISITS.with(|n| n.set(0));
            let report = validate_project_with_limits(
                project.path(),
                ProjectLimits {
                    directory_entries: limit,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(PEAK.with(Cell::get), expected_peak);
            assert_eq!(
                report
                    .findings
                    .iter()
                    .any(|f| f.error_code == "RP_E_RESOURCE_DIRECTORY_ENTRIES_EXCEEDED"),
                overflow
            );
        }
    }
}
