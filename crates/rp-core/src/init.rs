use std::{fmt, path::Path};

#[cfg(target_os = "linux")]
use rustix::fs::{
    AtFlags, CWD, Mode, OFlags, RenameFlags, ResolveFlags, fstat, fsync, mkdirat, openat, openat2,
    renameat_with, statat, unlinkat,
};
#[cfg(target_os = "linux")]
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::OwnedFd,
};

use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitReport {
    pub project_id: String,
    pub created_paths: Vec<String>,
}

#[derive(Debug)]
pub enum InitError {
    Stopped(crate::ExecutionStop),
    Conflict,
    Io(String),
}

impl fmt::Display for InitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stopped(stop) => stop.fmt(formatter),
            Self::Conflict => {
                formatter.write_str(".research already exists; refusing to overwrite")
            }
            Self::Io(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for InitError {}

const DIRECTORIES: &[&str] = &[
    ".research",
    ".research/artifacts",
    ".research/assessments",
    ".research/claim-chains",
    ".research/notes",
    ".research/policies",
    ".research/records",
    ".research/records/blockers",
    ".research/records/conclusions",
    ".research/records/datasets",
    ".research/records/decisions",
    ".research/records/hypotheses",
    ".research/records/interpretations",
    ".research/records/measurements",
    ".research/records/methods",
    ".research/records/next-actions",
    ".research/records/observations",
    ".research/records/paper-claims",
    ".research/records/predictions",
    ".research/records/questions",
    ".research/records/syntheses",
    ".research/records/tests",
    ".research/references",
    ".research/relations",
    ".research/runs",
    ".research/schemas",
    ".research/thread-bindings",
    ".research/threads",
];

pub fn initialize_project(root: &Path) -> Result<InitReport, InitError> {
    initialize_project_with_budget(root, &crate::ExecutionBudget::default())
}

/// Publish one private `.research` subtree without modifying existing entries.
/// Linux openat2/renameat2 support is required. Existing root modes are untouched.
/// A root renamed after the final identity check remains the descriptor-bound
/// destination; the replacement at its old pathname is never used for writes.
#[allow(clippy::missing_errors_doc)]
pub fn initialize_project_with_budget(
    root: &Path,
    execution: &crate::ExecutionBudget,
) -> Result<InitReport, InitError> {
    execution.checkpoint().map_err(InitError::Stopped)?;
    #[cfg(target_os = "linux")]
    {
        initialize_linux_with_budget(root, &mut |_| Ok(()), execution)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = root;
        Err(InitError::Io(
            "secure initialization requires Linux openat2 and renameat2".into(),
        ))
    }
}

fn project_descriptor(root: &Path) -> (String, String) {
    let now = jiff::Timestamp::now();
    let project_id = init_project_id(root, now.as_millisecond());
    let slug = project_slug(root);
    let title = slug
        .split('-')
        .map(|part| {
            let mut characters = part.chars();
            match characters.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + characters.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let project = format!(
        "schema: rp/project/v1\nid: {project_id}\nslug: '{slug}'\ntitle: '{title}'\ncreated_at: '{}'\nschema_policy:\n  core_version: v1\n  kind_registry: rp/kinds/v1\n  allowed_extensions: []\nrepository_policy:\n  visibility: private\n  allowed_remotes: []\naccess_defaults:\n  level: internal\n  compartments: []\n",
        now,
    );
    (project_id, project)
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    Staged,
    DirectoryCreated,
    PartialWrite,
    FileSync,
    DirectorySync,
    BeforePublish,
    PublishChecked,
    Published,
}

#[cfg(target_os = "linux")]
fn io_error(error: impl fmt::Display) -> InitError {
    InitError::Io(format!("cannot initialize project: {error}"))
}

#[cfg(target_os = "linux")]
fn open_directory(fd: &OwnedFd, path: &Path) -> Result<OwnedFd, rustix::io::Errno> {
    openat2(
        fd,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
    )
}

#[cfg(target_os = "linux")]
fn same_entry(parent: &OwnedFd, name: &str, fd: &OwnedFd) -> bool {
    match (statat(parent, name, AtFlags::SYMLINK_NOFOLLOW), fstat(fd)) {
        (Ok(named), Ok(opened)) => named.st_dev == opened.st_dev && named.st_ino == opened.st_ino,
        _ => false,
    }
}

// Bounded ownership journal: no directory enumeration or recursive path cleanup.
// Each removal uses its pinned parent and checks the entry's recorded identity.
// Replacements and nonempty directories are left alone rather than reclaimed.
#[cfg(target_os = "linux")]
struct OwnedEntry {
    parent: OwnedFd,
    name: String,
    fd: OwnedFd,
    directory: bool,
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct Staging(Vec<OwnedEntry>);

#[cfg(target_os = "linux")]
impl Staging {
    fn directory(&mut self, parent: &OwnedFd, name: &str) -> Result<OwnedFd, InitError> {
        let parent = parent.try_clone().map_err(io_error)?;
        mkdirat(&parent, name, Mode::from_raw_mode(0o700)).map_err(io_error)?;
        let fd = open_directory(&parent, Path::new(name)).map_err(io_error)?;
        let result = fd.try_clone().map_err(io_error);
        self.0.push(OwnedEntry {
            parent,
            name: name.into(),
            fd,
            directory: true,
        });
        result
    }
}

#[cfg(target_os = "linux")]
impl Drop for Staging {
    fn drop(&mut self) {
        for entry in self.0.iter().rev() {
            if same_entry(&entry.parent, &entry.name, &entry.fd) {
                let flags = if entry.directory {
                    AtFlags::REMOVEDIR
                } else {
                    AtFlags::empty()
                };
                let _ = unlinkat(&entry.parent, entry.name.as_str(), flags);
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
fn initialize_linux(
    root: &Path,
    hook: &mut impl FnMut(Step) -> std::io::Result<()>,
) -> Result<InitReport, InitError> {
    initialize_linux_with_budget(root, hook, &crate::ExecutionBudget::default())
}
#[cfg(target_os = "linux")]
fn initialize_linux_with_budget(
    root: &Path,
    hook: &mut impl FnMut(Step) -> std::io::Result<()>,
    execution: &crate::ExecutionBudget,
) -> Result<InitReport, InitError> {
    execution.checkpoint().map_err(InitError::Stopped)?;
    use std::{os::unix::ffi::OsStrExt, path::Component};
    // Bound path work, reject traversal rather than normalizing through it.
    if root.as_os_str().is_empty()
        || root.as_os_str().as_bytes().len() > 4096
        || root.components().count() > 256
        || root.components().any(|c| matches!(c, Component::ParentDir))
    {
        return Err(io_error("unsafe or oversized init path"));
    }
    let anchor = openat(
        CWD,
        if root.is_absolute() { "/" } else { "." },
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(io_error)?;
    let relative = if root.is_absolute() {
        root.strip_prefix("/").map_err(io_error)?
    } else {
        root
    };
    let relative = if relative.as_os_str().is_empty() {
        Path::new(".")
    } else {
        relative
    };
    let root_fd = match open_directory(&anchor, relative) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => {
            let name = relative
                .file_name()
                .ok_or_else(|| io_error("missing final root component"))?;
            let parent = relative
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let parent_fd = open_directory(&anchor, parent).map_err(io_error)?;
            // Only the intended final component may be created. Never chmod or
            // remove this root: a failure may leave an empty private directory.
            match mkdirat(&parent_fd, name, Mode::from_raw_mode(0o700)) {
                Ok(()) | Err(rustix::io::Errno::EXIST) => (),
                Err(error) => return Err(io_error(error)),
            }
            open_directory(&parent_fd, Path::new(name)).map_err(io_error)?
        }
        Err(error) => return Err(io_error(error)),
    };
    match statat(&root_fd, ".research", AtFlags::SYMLINK_NOFOLLOW) {
        Ok(_) => return Err(InitError::Conflict),
        Err(rustix::io::Errno::NOENT) => (),
        Err(error) => return Err(io_error(error)),
    }

    execution.checkpoint().map_err(InitError::Stopped)?;
    let mut staging = Staging::default();
    let mut entropy = File::open("/dev/urandom").map_err(io_error)?;
    let mut stage = None;
    for _ in 0..8 {
        execution.checkpoint().map_err(InitError::Stopped)?;
        let mut bytes = [0_u8; 16];
        entropy.read_exact(&mut bytes).map_err(io_error)?;
        let name = format!(".rp-init-{:032x}", u128::from_ne_bytes(bytes));
        // Reserve exclusively. A collision never becomes owned cleanup input.
        match mkdirat(&root_fd, name.as_str(), Mode::from_raw_mode(0o700)) {
            Ok(()) => {
                let fd = open_directory(&root_fd, Path::new(&name)).map_err(io_error)?;
                staging.0.push(OwnedEntry {
                    parent: root_fd.try_clone().map_err(io_error)?,
                    name,
                    fd: fd.try_clone().map_err(io_error)?,
                    directory: true,
                });
                stage = Some(fd);
                break;
            }
            Err(rustix::io::Errno::EXIST) => (),
            Err(error) => return Err(io_error(error)),
        }
    }
    let stage = stage.ok_or_else(|| io_error("staging name attempts exhausted"))?;
    hook(Step::Staged).map_err(io_error)?;
    execution.checkpoint().map_err(InitError::Stopped)?;
    let tree = staging.directory(&stage, "tree")?;
    for directory in &DIRECTORIES[1..] {
        let relative = Path::new(
            directory
                .strip_prefix(".research/")
                .expect("static directory prefix"),
        );
        let parent = relative
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent_fd = open_directory(&tree, parent).map_err(io_error)?;
        staging.directory(
            &parent_fd,
            relative
                .file_name()
                .and_then(|n| n.to_str())
                .expect("static directory name"),
        )?;
        hook(Step::DirectoryCreated).map_err(io_error)?;
        execution.checkpoint().map_err(InitError::Stopped)?;
    }
    let (project_id, project) = project_descriptor(root);
    let fd = openat(
        &tree,
        "project.yaml",
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map_err(io_error)?;
    staging.0.push(OwnedEntry {
        parent: tree.try_clone().map_err(io_error)?,
        name: "project.yaml".into(),
        fd: fd.try_clone().map_err(io_error)?,
        directory: false,
    });
    let mut file = File::from(fd);
    let split = project.len() / 2;
    file.write_all(&project.as_bytes()[..split])
        .map_err(io_error)?;
    hook(Step::PartialWrite).map_err(io_error)?;
    execution.checkpoint().map_err(InitError::Stopped)?;
    file.write_all(&project.as_bytes()[split..])
        .map_err(io_error)?;
    hook(Step::FileSync).map_err(io_error)?;
    execution.checkpoint().map_err(InitError::Stopped)?;
    file.sync_all().map_err(io_error)?;
    for entry in staging.0.iter().rev().filter(|e| e.directory) {
        hook(Step::DirectorySync).map_err(io_error)?;
        execution.checkpoint().map_err(InitError::Stopped)?;
        fsync(&entry.fd).map_err(io_error)?;
    }
    hook(Step::BeforePublish).map_err(io_error)?;
    execution.checkpoint().map_err(InitError::Stopped)?;
    // Re-resolve from the pinned anchor only to detect pathname substitution;
    // all writes and publication still use the original descriptors.
    let current = open_directory(&anchor, relative).map_err(io_error)?;
    let expected = fstat(&root_fd).map_err(io_error)?;
    let observed = fstat(&current).map_err(io_error)?;
    if expected.st_dev != observed.st_dev
        || expected.st_ino != observed.st_ino
        || !same_entry(&stage, "tree", &tree)
    {
        return Err(io_error("init target changed before publication"));
    }
    hook(Step::PublishChecked).map_err(io_error)?;
    execution.checkpoint().map_err(InitError::Stopped)?;
    renameat_with(
        &stage,
        "tree",
        &root_fd,
        ".research",
        RenameFlags::NOREPLACE,
    )
    .map_err(|error| {
        if error == rustix::io::Errno::EXIST {
            InitError::Conflict
        } else {
            io_error(error)
        }
    })?;
    // Publication is the commit point for this one subtree. Disarm its journal
    // immediately, retaining only the empty staging wrapper for cleanup.
    staging.0.truncate(1);
    drop(staging);
    hook(Step::Published).map_err(io_error)?;
    execution.checkpoint().map_err(InitError::Stopped)?;
    // A post-publication sync error leaves the complete subtree in place.
    fsync(&root_fd).map_err(io_error)?;
    execution.checkpoint().map_err(InitError::Stopped)?;
    let mut created_paths: Vec<_> = DIRECTORIES.iter().map(|p| (*p).to_string()).collect();
    created_paths.push(".research/project.yaml".into());
    created_paths.sort();
    Ok(InitReport {
        project_id,
        created_paths,
    })
}

#[cfg(all(test, target_os = "linux"))]
#[path = "init_repair_tests.rs"]
mod repair_tests;

fn project_slug(root: &Path) -> String {
    let raw = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("research-project");
    let mut slug = String::new();
    let mut separator = false;
    for character in raw.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            separator = false;
        } else {
            separator = true;
        }
    }
    if slug.is_empty() {
        "research-project".to_string()
    } else {
        slug
    }
}

fn init_project_id(root: &Path, milliseconds: i64) -> String {
    let timestamp = u64::try_from(milliseconds.max(0)).unwrap_or_default();
    let material = format!(
        "rp-init/v1\0{}\0{}\0{}",
        root.to_string_lossy(),
        milliseconds,
        std::process::id()
    );
    let digest = Sha256::digest(material.as_bytes());
    let mut randomness = [0_u8; 10];
    randomness.copy_from_slice(&digest[..10]);
    format!("proj_{}", encode_ulid(timestamp, randomness))
}

pub(crate) fn encode_ulid(timestamp_ms: u64, randomness: [u8; 10]) -> String {
    let mut bytes = [0_u8; 16];
    let timestamp = timestamp_ms.to_be_bytes();
    bytes[..6].copy_from_slice(&timestamp[2..]);
    bytes[6..].copy_from_slice(&randomness);
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut output = String::with_capacity(26);
    for group in 0..26 {
        let mut value = 0_u8;
        for offset in 0..5 {
            let bit = group * 5 + offset;
            value <<= 1;
            if bit >= 2 {
                let source_bit = bit - 2;
                value |= (bytes[source_bit / 8] >> (7 - source_bit % 8)) & 1;
            }
        }
        output.push(char::from(ALPHABET[usize::from(value)]));
    }
    output
}
