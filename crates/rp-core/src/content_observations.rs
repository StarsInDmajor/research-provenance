//! Private C3 same-read observations consumed by the scoped evaluator.
//! Compact finding retention and all owner/path/hash reservations are bounded before cloning.
use crate::{Finding, Severity};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    Object(Arc<str>),
    Context(Box<str>),
    Outer,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Canonical,
    Policy,
    Narrative,
    Artifact,
    ExtensionSchema,
    ExtensionPayload,
    EventProvenance,
    OuterReport,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Completeness {
    Verified,
    Mismatch,
    IdentifierOnly,
    Unavailable,
    ResourceLimit,
    Ambiguous,
}
impl Completeness {
    pub fn byte_complete(self) -> bool {
        matches!(self, Self::Verified | Self::Mismatch)
    }
}
#[derive(Clone, Debug)]
pub(crate) struct CompactFinding {
    pub error_code: &'static str,
    pub finding_family: &'static str,
    pub severity: Severity,
    pub json_pointer: Box<str>,
}
#[derive(Clone, Debug)]
pub(crate) struct Observation {
    pub owner: Owner,
    pub phase: Phase,
    pub path: Option<Box<str>>,
    pub namespace: Option<Box<str>>,
    pub consumers: Vec<Arc<str>>,
    pub raw_sha256: Option<Box<str>>,
    pub completeness: Completeness,
    pub findings: Vec<CompactFinding>,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct ObservationLimits {
    pub records: usize,
    pub bytes: usize,
    pub owners: usize,
    pub findings: usize,
}
impl Default for ObservationLimits {
    fn default() -> Self {
        Self {
            records: 4096,
            bytes: 4 * 1024 * 1024,
            owners: 16384,
            findings: 16384,
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct ObservationStore {
    pub records: Vec<Observation>,
    /// False on overflow, early stop or a visit that could not be represented.
    /// This is coverage only: inspect each record's completeness as well.
    pub coverage_complete: bool,
    pub pipeline_provenance_complete: bool,
    pub retention_exhausted: bool,
    remaining: ObservationLimits,
    // First actual hash per normalized path; None is sticky ambiguity. No last-writer wins.
    hashes: BTreeMap<Box<str>, Option<Box<str>>>,
}
impl ObservationStore {
    pub fn new(limits: ObservationLimits) -> Self {
        let ceiling = ObservationLimits::default();
        Self {
            records: Vec::new(),
            coverage_complete: true,
            pipeline_provenance_complete: false,
            retention_exhausted: false,
            remaining: ObservationLimits {
                records: limits.records.min(ceiling.records),
                bytes: limits.bytes.min(ceiling.bytes),
                owners: limits.owners.min(ceiling.owners),
                findings: limits.findings.min(ceiling.findings),
            },
            hashes: BTreeMap::new(),
        }
    }
    fn reserve(&mut self, bytes: usize, owners: usize, records: usize, findings: usize) -> bool {
        if bytes > self.remaining.bytes
            || owners > self.remaining.owners
            || records > self.remaining.records
            || findings > self.remaining.findings
        {
            self.coverage_complete = false;
            self.retention_exhausted = true;
            self.remaining = ObservationLimits {
                records: 0,
                bytes: 0,
                owners: 0,
                findings: 0,
            };
            return false;
        }
        self.remaining.bytes -= bytes;
        self.remaining.owners -= owners;
        self.remaining.records -= records;
        self.remaining.findings -= findings;
        true
    }
    pub fn begin(
        &mut self,
        owner: Owner,
        phase: Phase,
        path: Option<&str>,
        namespace: Option<&str>,
        consumers: &[Arc<str>],
    ) -> Option<usize> {
        let owner_bytes = match &owner {
            Owner::Object(id) => id.len(),
            Owner::Context(path) => path.len(),
            Owner::Outer => 0,
        };
        let bytes = consumers.iter().fold(
            owner_bytes
                .saturating_add(path.map_or(0, str::len))
                .saturating_add(namespace.map_or(0, str::len)),
            |n, id| n.saturating_add(id.len()),
        );
        if !self.reserve(
            bytes
                .saturating_add(std::mem::size_of::<Observation>())
                .saturating_add(
                    consumers
                        .len()
                        .saturating_mul(std::mem::size_of::<Arc<str>>()),
                ),
            1 + consumers.len(),
            1,
            0,
        ) {
            return None;
        }
        // Secure paths may contain redundant separators accepted by ProjectPath;
        // collapse them lexically, never canonicalize/reopen the filesystem.
        let path = path.map(|p| {
            let mut normalized = String::with_capacity(p.len());
            for segment in p.split('/').filter(|s| !s.is_empty() && *s != ".") {
                if !normalized.is_empty() {
                    normalized.push('/');
                }
                normalized.push_str(segment);
            }
            normalized.into_boxed_str()
        });
        let owner = match (owner, &path) {
            (Owner::Context(_), Some(path)) => Owner::Context(path.clone()),
            (owner, _) => owner,
        };
        let key = self.records.len();
        self.records.push(Observation {
            owner,
            phase,
            path,
            namespace: namespace.map(Into::into),
            consumers: consumers.to_vec(),
            raw_sha256: None,
            completeness: Completeness::Unavailable,
            findings: Vec::new(),
        });
        Some(key)
    }
    // Reserve the temporary shared-consumer list too, before cloning any IDs.
    // begin() separately charges each retained context (including transitive refs).
    pub fn consumers<'a>(
        &mut self,
        ids: impl Iterator<Item = &'a Arc<str>> + Clone,
    ) -> Vec<Arc<str>> {
        let (count, bytes) = ids.clone().fold((0_usize, 0_usize), |(n, b), id| {
            (
                n.saturating_add(1),
                b.saturating_add(id.len())
                    .saturating_add(std::mem::size_of::<Arc<str>>()),
            )
        });
        if !self.reserve(bytes, count, 0, 0) {
            return Vec::new();
        }
        ids.cloned().collect()
    }
    pub fn state(&mut self, key: Option<usize>, state: Completeness) {
        if let Some(r) = key.and_then(|i| self.records.get_mut(i)) {
            if !matches!(
                r.completeness,
                Completeness::Ambiguous | Completeness::ResourceLimit
            ) {
                r.completeness = state;
            }
        }
    }
    pub fn hash(&mut self, key: Option<usize>, hash: &str) {
        let Some(key) = key else {
            return;
        };
        let path = self.records[key].path.as_deref();
        let new_path = path.is_some_and(|p| !self.hashes.contains_key(p));
        let bytes = hash.len().saturating_add(if new_path {
            path.map_or(0, str::len) + hash.len() + 64
        } else {
            0
        });
        if !self.reserve(bytes, 0, 0, 0) {
            self.state(Some(key), Completeness::ResourceLimit);
            return;
        }
        let path = self.records[key].path.clone();
        let ambiguous = if let Some(path) = path.as_deref() {
            match self.hashes.get_mut(path) {
                Some(previous) if previous.as_deref() != Some(hash) => {
                    *previous = None;
                    true
                }
                Some(_) => false,
                None => {
                    self.hashes.insert(path.into(), Some(hash.into()));
                    false
                }
            }
        } else {
            false
        };
        if ambiguous {
            for record in &mut self.records {
                if record.path == path {
                    record.raw_sha256 = None;
                    record.completeness = Completeness::Ambiguous;
                }
            }
        } else {
            self.records[key].raw_sha256 = Some(hash.into());
        }
    }
    pub fn finding(&mut self, key: Option<usize>, finding: &Finding) {
        let Some(key) = key else {
            return;
        };
        if !self.reserve(
            finding.json_pointer.len() + std::mem::size_of::<CompactFinding>(),
            0,
            0,
            1,
        ) {
            self.state(Some(key), Completeness::ResourceLimit);
            return;
        }
        if finding.finding_family == "resource_limit" {
            self.state(Some(key), Completeness::ResourceLimit);
        } else if self.records[key].completeness == Completeness::Verified {
            self.state(Some(key), Completeness::Mismatch);
        }
        self.records[key].findings.push(CompactFinding {
            error_code: finding.error_code,
            finding_family: finding.finding_family,
            severity: finding.severity,
            json_pointer: finding.json_pointer.as_str().into(),
        });
    }
}

/// The external transcript stays unchanged. Scope is assigned at the producing callsite,
/// never inferred from source_file. Only compact, budgeted projections are retained.
pub(crate) struct ContentFindings {
    pub output: crate::findings::Findings,
    pub store: Option<ObservationStore>,
    pub active: Option<usize>,
}
#[cfg(test)]
thread_local! {
    static TEST_LIMITS: std::cell::Cell<Option<ObservationLimits>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
pub(crate) fn test_limits(limits: ObservationLimits) {
    TEST_LIMITS.with(|slot| slot.set(Some(limits)));
}

impl ContentFindings {
    #[cfg(test)]
    pub fn new(enabled: bool) -> Self {
        Self::with_budget(enabled, crate::findings::Findings::default())
    }
    pub fn with_budget(enabled: bool, output: crate::findings::Findings) -> Self {
        let limits = ObservationLimits::default();
        #[cfg(test)]
        let limits = TEST_LIMITS.with(|slot| slot.take()).unwrap_or(limits);
        Self {
            output,
            store: enabled.then(|| ObservationStore::new(limits)),
            active: None,
        }
    }
    pub fn begin(
        &mut self,
        owner: Owner,
        phase: Phase,
        path: Option<&str>,
        namespace: Option<&str>,
        consumers: &[Arc<str>],
    ) {
        self.active = self
            .store
            .as_mut()
            .and_then(|s| s.begin(owner, phase, path, namespace, consumers));
    }
    pub fn context(
        &mut self,
        path: &str,
        phase: Phase,
        namespace: Option<&str>,
        consumers: &[Arc<str>],
    ) {
        self.active = self.store.as_mut().and_then(|store| {
            // Charge the temporary owner before allocating it; begin additionally
            // reserves the full retained record. Failed reservations are not refunded.
            if !store.reserve(path.len(), 1, 0, 0) {
                return None;
            }
            store.begin(
                Owner::Context(path.into()),
                phase,
                Some(path),
                namespace,
                consumers,
            )
        });
    }
    pub fn state(&mut self, state: Completeness) {
        if let Some(s) = &mut self.store {
            s.state(self.active, state);
        }
    }
    pub fn hash(&mut self, hash: &str) {
        if let Some(s) = &mut self.store {
            s.hash(self.active, hash);
        }
    }
    pub fn incomplete(&mut self) {
        if let Some(s) = &mut self.store {
            s.coverage_complete = false;
        }
    }
    pub fn cancelled(&self) -> bool {
        self.output.cancelled()
    }
    pub fn push(&mut self, make: impl FnOnce() -> Finding) {
        let store = &mut self.store;
        let active = self.active;
        self.output.push(|| {
            let finding = make();
            if let Some(s) = store {
                s.finding(active, &finding);
            }
            finding
        });
        if self.cancelled() {
            self.incomplete();
        }
    }
    pub fn absorb(&mut self, findings: crate::findings::Findings) {
        if let Some(s) = &mut self.store {
            for finding in findings.as_slice() {
                s.finding(self.active, finding);
            }
        }
        self.output.absorb(findings);
        if self.cancelled() {
            self.incomplete();
        }
    }
}
