//! Private bounded C3 expected bodies. Not a public writer or disclosure API.
use std::{
    collections::{BTreeSet, VecDeque},
    io::{self, Write},
};

use crate::findings::Findings;
use crate::{ObjectType, ProjectIndex, ProjectLimits, ReferenceExpectation};

const MAX_OBJECTS: usize = 128;
const MAX_EDGES: usize = 1_024;
const MAX_GRAPH_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SubjectUnavailable {
    Stopped(crate::ExecutionStop),
    IndexIncomplete,
    Budget,
    ContentIncomplete,
    Attribution,
    OwnReport,
    WireContract,
}

/// One meter per batch, never one fresh meter per snapshot/report pair.
/// Conservative subject admission bounds; these do not implement global cancellation.
pub(crate) struct SubjectBudget {
    pub findings: Findings,
    pub remaining_work: usize,
    pub remaining_bytes: usize,
}

impl Default for SubjectBudget {
    fn default() -> Self {
        Self {
            findings: Findings::default(),
            remaining_work: 2_000_000,
            remaining_bytes: 16 * 1024 * 1024,
        }
    }
}

impl SubjectBudget {
    fn bytes(&mut self, amount: usize) -> Result<(), SubjectUnavailable> {
        self.findings
            .execution
            .checkpoint()
            .map_err(SubjectUnavailable::Stopped)?;
        let charged = amount.min(self.remaining_bytes);
        self.remaining_bytes -= charged;
        if charged == amount {
            Ok(())
        } else {
            Err(SubjectUnavailable::Budget)
        }
    }
    pub(crate) fn work(&mut self, amount: usize) -> Result<(), SubjectUnavailable> {
        self.findings
            .execution
            .checkpoint()
            .map_err(SubjectUnavailable::Stopped)?;
        match self.remaining_work.checked_sub(amount) {
            Some(left) => {
                self.remaining_work = left;
                Ok(())
            }
            None => {
                self.remaining_work = 0;
                Err(SubjectUnavailable::Budget)
            }
        }
    }
}

pub(crate) struct SubjectGraph {
    pub index: ProjectIndex,
    pub missing: BTreeSet<(ReferenceExpectation, Box<str>)>,
    /// Preserve non-role reachability, including missing targets, for outer rejection.
    pub own_report_reached: bool,
}

impl SubjectGraph {
    pub fn build(
        full: &ProjectIndex,
        snapshot_id: &str,
        budget: &mut SubjectBudget,
    ) -> Result<Self, SubjectUnavailable> {
        if !full.subject_graph_ready() {
            return Err(SubjectUnavailable::IndexIncomplete);
        }
        let snapshot = full
            .get(snapshot_id)
            .filter(|o| o.object_type == ObjectType::ClaimChain)
            .ok_or(SubjectUnavailable::IndexIncomplete)?;
        // Charge the descriptor search rather than hiding an O(project) scan per pair.
        budget.work(full.len())?;
        let mut projects = full
            .objects()
            .filter(|o| o.object_type == ObjectType::Project);
        let project = projects.next().ok_or(SubjectUnavailable::IndexIncomplete)?;
        if projects.next().is_some() {
            return Err(SubjectUnavailable::IndexIncomplete);
        }
        let mut visited = BTreeSet::from([snapshot.id.clone(), project.id.clone()]);
        let mut queue: VecDeque<_> = visited.iter().cloned().collect();
        let mut index = ProjectIndex::default();
        index.validation_limits = full.validation_limits;
        index.execution = budget.findings.execution.clone();
        let mut references = Vec::new();
        let mut missing: BTreeSet<(ReferenceExpectation, Box<str>)> = BTreeSet::new();
        let mut bytes_left = MAX_GRAPH_BYTES;
        while let Some(id) = queue.pop_front() {
            budget.work(1)?;
            let object = full.get(&id).ok_or(SubjectUnavailable::IndexIncomplete)?;
            // Count bounded serialization without allocating a full extra value/string.
            // Charge even failed serialization; clone only after byte admission.
            let mut counter = ByteCounter {
                remaining: bytes_left.min(budget.remaining_bytes),
                used: 0,
            };
            let serialized = serde_json::to_writer(&mut counter, &object.value);
            bytes_left -= counter.used;
            budget.remaining_bytes -= counter.used;
            if serialized.is_err() {
                return Err(SubjectUnavailable::Budget);
            }
            for reference in full.references_from(&id) {
                budget.work(1)?;
                if object.object_type == ObjectType::ClaimChain
                    && reference.json_pointer == "/validation_report"
                {
                    continue;
                }
                if references.len() == MAX_EDGES {
                    return Err(SubjectUnavailable::Budget);
                }
                let edge_bytes = reference
                    .json_pointer
                    .len()
                    .saturating_add(reference.target_id.len())
                    .saturating_add(reference.source_id.len())
                    .saturating_add(match &reference.expected {
                        ReferenceExpectation::NodeKind(kind) => kind.len(),
                        _ => 0,
                    });
                let charged = edge_bytes.min(bytes_left).min(budget.remaining_bytes);
                bytes_left -= charged;
                budget.remaining_bytes -= charged;
                if charged != edge_bytes {
                    return Err(SubjectUnavailable::Budget);
                }
                if full.get(&reference.target_id).is_some() {
                    if !visited.contains(&reference.target_id) {
                        if visited.len() == MAX_OBJECTS {
                            return Err(SubjectUnavailable::Budget);
                        }
                        visited.insert(reference.target_id.clone());
                        queue.push_back(reference.target_id.clone());
                    }
                } else {
                    missing.insert((
                        reference.expected.clone(),
                        reference.target_id.as_ref().into(),
                    ));
                }
                references.push(reference.clone());
            }
            index.insert(object.clone());
        }
        // Reserve all-pairs access traversal and semantic passes before evaluating.
        budget.work(
            index.len().saturating_mul(
                index
                    .len()
                    .saturating_add(references.len())
                    .saturating_add(1),
            ),
        )?;
        index.assign_ordinals();
        index.set_references(references);
        index.derive_heads();
        let own_report_reached = snapshot
            .value
            .get("validation_report")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| {
                index.get(id).is_some() || missing.iter().any(|(_, missing)| missing.as_ref() == id)
            });
        Ok(Self {
            index,
            missing,
            own_report_reached,
        })
    }

    /// Only graph prerequisites. Findings are freshly evaluated on V, never
    /// selected out of a global transcript. No `passed` or completeness claim.
    pub fn prerequisites(
        &self,
        limits: ProjectLimits,
        mut findings: Findings,
    ) -> Result<(crate::access::AccessStore, Findings), SubjectUnavailable> {
        for object in self.index.objects() {
            let edges: Vec<_> = self.index.references_from(&object.id).cloned().collect();
            crate::project::validate_references(&self.index, &edges, &mut findings);
        }
        crate::project::validate_semantics(&self.index, limits, &mut findings);
        if findings
            .iter()
            .any(|f| f.finding_family == "resource_limit")
        {
            return Err(SubjectUnavailable::Budget);
        }
        let (access, access_findings) = crate::access::compute_access(&self.index, findings.fork());
        findings.absorb(access_findings);
        if findings.cancelled() {
            return Err(SubjectUnavailable::Budget);
        }
        Ok((access, findings))
    }
}

struct ByteCounter {
    remaining: usize,
    used: usize,
}
impl Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            self.used += self.remaining;
            self.remaining = 0;
            return Err(io::Error::other("subject byte budget exhausted"));
        }
        self.remaining -= bytes.len();
        self.used += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Internal expected bytes only. Not an attachment verdict or permission to disclose.
pub(crate) struct ExpectedSubjectReport {
    pub body: serde_json::Value,
    /// Join of computed role-cut effective labels for ALL V, including Project.
    /// Context is governed by Project and its consuming objects in V; their effective
    /// labels (and finding owners) are included, never candidate-supplied labels.
    pub disclosure_floor: crate::AccessLabel,
}

#[derive(serde::Serialize)]
struct Entry<'a> {
    object_type: &'a str,
    id: &'a str,
    canonical_digest: Option<String>,
}
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum WireOwner<'a> {
    Object { id: &'a str },
    Context { path: &'a str },
}
#[derive(serde::Serialize)]
struct Projected<'a> {
    owner: WireOwner<'a>,
    error_code: &'static str,
    finding_family: &'static str,
    severity: crate::Severity,
    json_pointer: &'a str,
}
impl Projected<'_> {
    fn key(&self) -> (crate::Severity, &str, &str, &str, &str, &str) {
        let (kind, identity) = match self.owner {
            WireOwner::Object { id } => ("object", id),
            WireOwner::Context { path } => ("context", path),
        };
        (
            self.severity,
            kind,
            identity,
            self.json_pointer,
            self.error_code,
            self.finding_family,
        )
    }
}
fn expected_type(expected: &ReferenceExpectation) -> &'static str {
    match expected {
        ReferenceExpectation::Project => "rp/project/v1",
        ReferenceExpectation::Node | ReferenceExpectation::NodeKind(_) => "rp/node-revision/v1",
        ReferenceExpectation::Relation => "rp/scientific-relation-revision/v1",
        ReferenceExpectation::Assessment => "rp/assessment/v1",
        ReferenceExpectation::Thread => "rp/research-thread/v1",
        ReferenceExpectation::ThreadBinding => "rp/thread-binding/v1",
        ReferenceExpectation::ClaimChain => "rp/claim-chain-snapshot/v1",
        ReferenceExpectation::Artifact => "rp/artifact-manifest/v1",
        ReferenceExpectation::ExternalReference => "rp/external-reference/v1",
        ReferenceExpectation::ResearchRun => "rp/research-run/v1",
    }
}

pub(crate) fn expected_subject_report(
    full: &ProjectIndex,
    snapshot_id: &str,
    budget: &mut SubjectBudget,
) -> Result<ExpectedSubjectReport, SubjectUnavailable> {
    use crate::content_observations::{Completeness, Owner, Phase};
    use serde::Serialize;
    use serde_json::Value;
    use std::collections::BTreeMap;
    budget
        .findings
        .execution
        .checkpoint()
        .map_err(SubjectUnavailable::Stopped)?;
    if budget.findings.cancelled() {
        return Err(SubjectUnavailable::Budget);
    }
    let graph = SubjectGraph::build(full, snapshot_id, budget)?;
    if graph.own_report_reached {
        return Err(SubjectUnavailable::OwnReport);
    }
    let store = full
        .content_observations
        .as_ref()
        .ok_or(SubjectUnavailable::ContentIncomplete)?;
    if store.retention_exhausted {
        return Err(SubjectUnavailable::Budget);
    }
    if !store.coverage_complete || !store.pipeline_provenance_complete {
        return Err(SubjectUnavailable::ContentIncomplete);
    }
    let index = &graph.index;
    let chain = index
        .get(snapshot_id)
        .ok_or(SubjectUnavailable::IndexIncomplete)?;
    let n = index.len();
    let e = index
        .objects()
        .map(|o| index.references_from(&o.id).count())
        .sum::<usize>();
    // Reserve repeated profile edge scans, pair/result/Prediction loops and path
    // traversals conservatively BEFORE calling existing unmetered helpers.
    let selected_count = |field| chain.value[field].as_array().map_or(0, Vec::len);
    let nodes = selected_count("node_revisions").saturating_add(1);
    let relations = selected_count("relation_revisions").saturating_add(1);
    budget.work(
        4_usize.saturating_mul(
            nodes
                .saturating_pow(2)
                .saturating_mul(relations.saturating_pow(2))
                .saturating_add(relations.saturating_pow(3))
                .saturating_add(n.saturating_mul(n.saturating_add(e))),
        ),
    )?;
    // Reserve helper retention before their unmetered allocations. Reference and
    // profile messages contain bounded IDs/kinds; access messages can repeat the
    // entire compartment vocabulary per object. The graph byte admission already
    // bounds canonical strings; reserve each repeated copy, not just final output.
    let mut helper_bytes = ByteCounter {
        remaining: MAX_GRAPH_BYTES,
        used: 0,
    };
    let mut vocabulary_bytes = 0_usize;
    let mut max_kind = 0;
    let mut pointer_bytes = 0_usize;
    for o in index.objects() {
        serde_json::to_writer(&mut helper_bytes, &o.value)
            .map_err(|_| SubjectUnavailable::Budget)?;
        max_kind = max_kind.max(
            o.value
                .get("kind")
                .and_then(Value::as_str)
                .map_or(0, str::len),
        );
        for c in o
            .value
            .pointer(if o.object_type == ObjectType::Project {
                "/access_defaults/compartments"
            } else {
                "/access/compartments"
            })
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            vocabulary_bytes = vocabulary_bytes.saturating_add(c.len().saturating_add(64));
        }
        pointer_bytes = index
            .references_from(&o.id)
            .fold(pointer_bytes, |b, r| b.saturating_add(r.json_pointer.len()));
    }
    let helper_slots = 32 + 8 * (n + e);
    budget.bytes(
        helper_bytes
            .used
            .saturating_mul(4)
            .saturating_add(vocabulary_bytes.saturating_mul(8 * (n + 1)))
            .saturating_add(pointer_bytes.saturating_mul(4))
            .saturating_add(helper_slots.saturating_mul(1024 + max_kind.saturating_mul(2))),
    )?;
    budget.work(vocabulary_bytes.saturating_mul(n.saturating_add(e)))?;
    // Bounded borrowed maps/sets and wire tuple slots, admitted before insertion.
    budget.bytes((store.records.len() + 8 * (n + e + 1)).saturating_mul(256))?;
    let mut source_map = BTreeMap::new();
    for o in index.objects() {
        if source_map.insert(&o.source_file, o.id.as_ref()).is_some() {
            return Err(SubjectUnavailable::Attribution);
        }
    }
    // This is a validated one-to-one map for fresh helper-scoped findings only.
    // Never consult the flattened full-project transcript (including first-cycle).
    let (access, mut findings) =
        graph.prerequisites(index.validation_limits, budget.findings.fork())?;
    let mut disclosure_floor = crate::AccessLabel {
        level: crate::AccessLevel::Public,
        compartments: BTreeSet::new(),
    };
    // Reuse the admitted prerequisite computation, not another graph traversal.
    // This covers dependencies that are not reachable from the snapshot (Project),
    // and effective floors even when a dependency's declared access is invalid.
    for object in index.objects() {
        let label = access
            .report(object.ordinal)
            .ok_or(SubjectUnavailable::Attribution)?;
        disclosure_floor.level = disclosure_floor.level.max(label.effective.level);
        disclosure_floor
            .compartments
            .extend(label.effective.compartments);
    }
    let mut verified = BTreeSet::new();
    let mut contexts = BTreeMap::new();
    let mut covered = BTreeSet::new();
    let mut projected = Vec::new();
    budget.work(store.records.len())?;
    for r in &store.records {
        budget.work(r.consumers.len() + r.findings.len())?;
        if r.phase == Phase::OuterReport {
            continue;
        }
        let owner = match &r.owner {
            Owner::Object(id) if index.get(id).is_some() => WireOwner::Object { id },
            Owner::Context(path) if r.consumers.iter().any(|id| index.get(id).is_some()) => {
                WireOwner::Context { path }
            }
            _ => continue,
        };
        if r.completeness == Completeness::ResourceLimit {
            return Err(SubjectUnavailable::Budget);
        }
        if !(r.completeness.byte_complete()
            || r.phase == Phase::Artifact && r.completeness == Completeness::IdentifierOnly)
        {
            return Err(SubjectUnavailable::ContentIncomplete);
        }
        if let Owner::Object(id) = &r.owner {
            covered.insert((id.as_ref(), r.phase as u8, r.namespace.as_deref()));
            if r.phase == Phase::Artifact && r.completeness == Completeness::Verified {
                verified.insert(id.as_ref().into());
            }
        }
        if let WireOwner::Context { path } = owner {
            let hash = r
                .raw_sha256
                .as_deref()
                .ok_or(SubjectUnavailable::ContentIncomplete)?;
            if contexts.insert(path, hash).is_some_and(|old| old != hash) {
                return Err(SubjectUnavailable::ContentIncomplete);
            }
        }
        for f in &r.findings {
            if projected.len() == 16_384 {
                return Err(SubjectUnavailable::Budget);
            }
            budget.bytes(std::mem::size_of::<Projected>() + f.json_pointer.len() + 128)?;
            projected.push(Projected {
                owner: match owner {
                    WireOwner::Object { id } => WireOwner::Object { id },
                    WireOwner::Context { path } => WireOwner::Context { path },
                },
                error_code: f.error_code,
                finding_family: f.finding_family,
                severity: f.severity,
                json_pointer: &f.json_pointer,
            });
        }
    }
    for o in index.objects() {
        let has = |phase, ns| covered.contains(&(o.id.as_ref(), phase as u8, ns));
        if !has(Phase::Canonical, None)
            || (o.value.pointer("/narrative/path").is_some() && !has(Phase::Narrative, None))
            || (o.object_type == ObjectType::Artifact && !has(Phase::Artifact, None))
        {
            return Err(SubjectUnavailable::ContentIncomplete);
        }
        if let Some(extensions) = o.value.get("extensions").and_then(Value::as_object) {
            for ns in extensions.keys() {
                if !has(Phase::ExtensionPayload, Some(ns.as_str())) {
                    return Err(SubjectUnavailable::ContentIncomplete);
                }
            }
        }
        if o.object_type == ObjectType::Project {
            for field in ["validation_policy", "redaction_policy"] {
                if let Some(path) = o.value.get(field).and_then(Value::as_str) {
                    let normalized = path
                        .split('/')
                        .filter(|s| !s.is_empty() && *s != ".")
                        .collect::<Vec<_>>()
                        .join("/");
                    if !contexts.contains_key(normalized.as_str()) {
                        return Err(SubjectUnavailable::ContentIncomplete);
                    }
                }
            }
        }
    }
    let (claim, claim_findings) =
        crate::claim::validate_one_claim_chain(index, chain, &verified, findings.fork());
    findings.absorb(claim_findings);
    if findings.cancelled() {
        return Err(SubjectUnavailable::Budget);
    }
    for f in findings.as_slice() {
        if f.finding_family == "resource_limit" {
            return Err(SubjectUnavailable::Budget);
        }
        let id = f
            .source_file
            .as_ref()
            .and_then(|path| source_map.get(path))
            .ok_or(SubjectUnavailable::Attribution)?;
        if projected.len() == 16_384 {
            return Err(SubjectUnavailable::Budget);
        }
        budget.bytes(std::mem::size_of::<Projected>() + f.json_pointer.len() + 128)?;
        projected.push(Projected {
            owner: WireOwner::Object { id },
            error_code: f.error_code,
            finding_family: f.finding_family,
            severity: f.severity,
            json_pointer: &f.json_pointer,
        });
    }
    projected.sort_by(|a, b| a.key().cmp(&b.key()));
    projected.dedup_by(|a, b| a.key() == b.key());
    let passed = !projected
        .iter()
        .any(|f| f.severity == crate::Severity::Error);
    // Borrowed IDs in the wire tuples; hash only admitted canonical values.
    let selected = |pointer: &str, ty: &'static str| -> Vec<Entry<'_>> {
        let ids: BTreeSet<_> = chain.value[pointer]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        ids.into_iter()
            .map(|id| Entry {
                object_type: ty,
                id,
                canonical_digest: index
                    .get(id)
                    .filter(|o| o.value["schema"] == ty)
                    .and_then(|o| crate::jcs_sha256(&o.value).ok()),
            })
            .collect()
    };
    let selected_nodes = selected("node_revisions", "rp/node-revision/v1");
    let selected_relations = selected("relation_revisions", "rp/scientific-relation-revision/v1");
    let mut dependencies = BTreeMap::new();
    for o in index.objects() {
        let ty = o.value["schema"]
            .as_str()
            .ok_or(SubjectUnavailable::WireContract)?;
        dependencies.insert((ty, o.id.as_ref()), index.canonical_digest(&o.id));
    }
    for (expected, id) in &graph.missing {
        dependencies.insert((expected_type(expected), id.as_ref()), None);
    }
    let validation_dependencies: Vec<_> = dependencies
        .into_iter()
        .map(|((object_type, id), canonical_digest)| Entry {
            object_type,
            id,
            canonical_digest,
        })
        .collect();
    let nodes: Vec<_> = selected_nodes.iter().map(|e| e.id.to_string()).collect();
    let relations: Vec<_> = selected_relations
        .iter()
        .map(|e| e.id.to_string())
        .collect();
    let (_, _, unresolved_ids) = crate::claim::source_closure(index, &nodes, &relations);
    #[derive(Serialize)]
    struct Context<'a> {
        path: &'a str,
        raw_sha256: &'a str,
    }
    #[derive(Serialize)]
    struct Subject<'a> {
        id: &'a str,
        canonical_digest: String,
        validation_policy: &'a Value,
    }
    #[derive(Serialize)]
    struct Closure<'a> {
        algorithm: &'static str,
        entries: &'a [crate::SourceClosureEntry],
        sha256: &'a str,
        unresolved_ids: &'a [Box<str>],
    }
    #[derive(Serialize)]
    struct Validator {
        id: &'static str,
        ruleset: &'static str,
        fingerprint: &'static str,
    }
    #[derive(Serialize)]
    struct Outcome<'a> {
        passed: bool,
        findings: &'a [Projected<'a>],
    }
    #[derive(Serialize)]
    struct Body<'a> {
        schema: &'static str,
        scope: &'static str,
        subject: Subject<'a>,
        selected_nodes: Vec<Entry<'a>>,
        selected_relations: Vec<Entry<'a>>,
        source_closure: Closure<'a>,
        validation_dependencies: Vec<Entry<'a>>,
        validation_context: Vec<Context<'a>>,
        validator: Validator,
        outcome: Outcome<'a>,
    }
    let body = Body {
        schema: "rp/claim-chain-validation-report/v1",
        scope: "claim-chain-subject-pre-binding/v1",
        subject: Subject {
            id: snapshot_id,
            canonical_digest: index
                .canonical_digest(snapshot_id)
                .ok_or(SubjectUnavailable::WireContract)?,
            validation_policy: &chain.value["validation_policy"],
        },
        selected_nodes,
        selected_relations,
        source_closure: Closure {
            algorithm: "rp/source-closure/v1",
            entries: &claim.source_entries,
            sha256: &claim.source_sha256,
            unresolved_ids: &unresolved_ids,
        },
        validation_dependencies,
        validation_context: contexts
            .into_iter()
            .map(|(path, raw_sha256)| Context { path, raw_sha256 })
            .collect(),
        validator: Validator {
            id: "rp-core",
            ruleset: "claim-chain-subject-pre-binding/v1",
            fingerprint: crate::report_fingerprint::fingerprint(),
        },
        outcome: Outcome {
            passed,
            findings: &projected,
        },
    };
    // Count streaming serialization before raw retention; then parse with the actual
    // C1 parser, which admits every Value node/string before allocating it.
    let mut counter = ByteCounter {
        remaining: (4 * 1024 * 1024).min(budget.remaining_bytes),
        used: 0,
    };
    let result = serde_json::to_writer(&mut counter, &body);
    budget.bytes(counter.used)?;
    result.map_err(|_| SubjectUnavailable::Budget)?;
    // Reserve serialization/structural equality work as well as retention. Runtime
    // comparison visits at most the expected structure/strings before inequality;
    // map lookup overhead is bounded by the strict parser's key ceiling.
    budget.work(counter.used.saturating_mul(16))?;
    // Reserve JSON storage conservatively (nodes/keys/strings) before parsing too.
    budget.bytes(counter.used.saturating_mul(16))?;
    let mut bytes = Vec::with_capacity(counter.used);
    serde_json::to_writer(&mut bytes, &body).map_err(|_| SubjectUnavailable::WireContract)?;
    let mut nodes = usize::MAX;
    let body = crate::report_json::parse_report_json_with_budget(
        &bytes,
        &crate::ReportJsonLimits::default(),
        &mut nodes,
        &budget.findings.execution,
    )
    .map_err(|error| match error {
        crate::ReportJsonError::Stopped(stop) => SubjectUnavailable::Stopped(stop),
        _ => SubjectUnavailable::Budget,
    })?;
    budget
        .findings
        .execution
        .checkpoint()
        .map_err(SubjectUnavailable::Stopped)?;
    let bundle = crate::SchemaBundle::new().map_err(|_| SubjectUnavailable::WireContract)?;
    let valid = bundle.is_valid_with_budget(&body, &budget.findings.execution);
    budget
        .findings
        .execution
        .checkpoint()
        .map_err(SubjectUnavailable::Stopped)?;
    if !valid {
        return Err(SubjectUnavailable::WireContract);
    }
    Ok(ExpectedSubjectReport {
        body,
        disclosure_floor,
    })
}
