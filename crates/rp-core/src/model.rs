use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
    sync::Arc,
};

use serde_json::Value;

use crate::{
    AccessReport, ClaimChainReport, ExportDecision, ExportRequest, ProjectPath,
    access::AccessStore, jcs_sha256,
};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ObjectType {
    Project,
    Node(Box<str>),
    Relation,
    Assessment,
    Thread,
    ThreadBinding,
    ClaimChain,
    ExternalReference,
    Artifact,
    ResearchRun,
}

impl ObjectType {
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        match value.get("schema")?.as_str()? {
            "rp/project/v1" => Some(Self::Project),
            "rp/node-revision/v1" => Some(Self::Node(value.get("kind")?.as_str()?.into())),
            "rp/scientific-relation-revision/v1" => Some(Self::Relation),
            "rp/assessment/v1" => Some(Self::Assessment),
            "rp/research-thread/v1" => Some(Self::Thread),
            "rp/thread-binding/v1" => Some(Self::ThreadBinding),
            "rp/claim-chain-snapshot/v1" => Some(Self::ClaimChain),
            "rp/external-reference/v1" => Some(Self::ExternalReference),
            "rp/artifact-manifest/v1" => Some(Self::Artifact),
            "rp/research-run/v1" => Some(Self::ResearchRun),
            _ => None,
        }
    }

    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Project => "project",
            Self::Node(_) => "node revision",
            Self::Relation => "relation revision",
            Self::Assessment => "assessment",
            Self::Thread => "research thread",
            Self::ThreadBinding => "thread binding",
            Self::ClaimChain => "claim-chain snapshot",
            Self::ExternalReference => "external reference",
            Self::Artifact => "artifact manifest",
            Self::ResearchRun => "research run",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ObjectRecord {
    pub(crate) ordinal: usize,
    pub id: Arc<str>,
    pub logical_id: Option<Arc<str>>,
    pub object_type: ObjectType,
    pub source_file: ProjectPath,
    pub value: Value,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ReferenceExpectation {
    Project,
    Node,
    NodeKind(Box<str>),
    Relation,
    Assessment,
    Thread,
    ThreadBinding,
    ClaimChain,
    Artifact,
    ExternalReference,
    ResearchRun,
}

impl ReferenceExpectation {
    pub(crate) fn accepts(&self, actual: &ObjectType) -> bool {
        match (self, actual) {
            (Self::Project, ObjectType::Project)
            | (Self::Node, ObjectType::Node(_))
            | (Self::Relation, ObjectType::Relation)
            | (Self::Assessment, ObjectType::Assessment)
            | (Self::Thread, ObjectType::Thread)
            | (Self::ThreadBinding, ObjectType::ThreadBinding)
            | (Self::ClaimChain, ObjectType::ClaimChain)
            | (Self::Artifact, ObjectType::Artifact)
            | (Self::ExternalReference, ObjectType::ExternalReference)
            | (Self::ResearchRun, ObjectType::ResearchRun) => true,
            (Self::NodeKind(expected), ObjectType::Node(actual)) => expected == actual,
            _ => false,
        }
    }

    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Project => "project",
            Self::Node | Self::NodeKind(_) => "node revision",
            Self::Relation => "relation revision",
            Self::Assessment => "assessment",
            Self::Thread => "research thread",
            Self::ThreadBinding => "thread binding",
            Self::ClaimChain => "claim-chain snapshot",
            Self::Artifact => "artifact manifest",
            Self::ExternalReference => "external reference",
            Self::ResearchRun => "research run",
        }
    }

    fn fingerprint_label(&self) -> String {
        match self {
            Self::NodeKind(kind) => format!("node:{kind}"),
            _ => self.label().to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ReferenceEdge {
    pub source_id: Arc<str>,
    pub target_id: Arc<str>,
    pub(crate) target_ordinal: Option<NonZeroU32>,
    pub expected: ReferenceExpectation,
    pub json_pointer: String,
}

impl ReferenceEdge {
    // Keep the original public field order; resolved ordinals are index-local caches.
    fn semantic_key(&self) -> (&str, &str, &ReferenceExpectation, &str) {
        (
            &self.source_id,
            &self.target_id,
            &self.expected,
            &self.json_pointer,
        )
    }
}

impl PartialEq for ReferenceEdge {
    fn eq(&self, other: &Self) -> bool {
        self.semantic_key() == other.semantic_key()
    }
}

impl Eq for ReferenceEdge {}

impl PartialOrd for ReferenceEdge {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ReferenceEdge {
    fn cmp(&self, other: &Self) -> Ordering {
        self.semantic_key().cmp(&other.semantic_key())
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProjectIndex {
    pub(crate) execution: crate::ExecutionBudget,
    objects: BTreeMap<Arc<str>, ObjectRecord>,
    references: Vec<Vec<ReferenceEdge>>,
    heads: BTreeMap<Arc<str>, Vec<Arc<str>>>,
    access: AccessStore,
    claim_chains: BTreeMap<Box<str>, ClaimChainReport>,
    verified_artifacts: BTreeSet<Box<str>>,
    freshness_policy: Option<Value>,
    unbound_reports: BTreeMap<Box<str>, crate::UnboundReportCandidate>,
    report_bindings: crate::report_binding::Bindings,
    subject_graph_ready: bool,
    /// Effective caller limits belong to the read/evaluation context, not the
    /// report body. Scoped re-evaluation must never replace them with defaults.
    pub(crate) validation_limits: crate::ProjectLimits,
    pub(crate) content_observations: Option<crate::content_observations::ObservationStore>,
}

impl ProjectIndex {
    /// Discard incomplete graph data without starting a new command or replacing
    /// the caller's limits. Resource-error reports retain the empty-index contract.
    pub(crate) fn empty_with_context(&self) -> Self {
        Self {
            execution: self.execution.clone(),
            validation_limits: self.validation_limits,
            ..Self::default()
        }
    }
    /// Same command state retained after validation; never a fresh helper deadline.
    pub fn execution_budget(&self) -> &crate::ExecutionBudget {
        &self.execution
    }
    pub(crate) fn mark_subject_graph_ready(&mut self) {
        self.subject_graph_ready = true;
    }
    pub(crate) fn invalidate_subject_graph(&mut self) {
        self.subject_graph_ready = false;
    }
    pub(crate) fn subject_graph_ready(&self) -> bool {
        self.subject_graph_ready
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&ObjectRecord> {
        self.objects.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.objects.keys().map(AsRef::as_ref)
    }

    pub fn objects(&self) -> impl Iterator<Item = &ObjectRecord> {
        self.objects.values()
    }

    #[must_use]
    pub fn access_report(&self, id: &str) -> Option<AccessReport> {
        self.access.report(self.objects.get(id)?.ordinal)
    }

    #[must_use]
    pub fn access_explanation(&self, id: &str) -> Option<Vec<crate::DependencyStep>> {
        crate::access::ordered_dependency_explanation(self, id)
    }
    pub fn access_explanation_checked(
        &self,
        id: &str,
    ) -> Result<Option<Vec<crate::DependencyStep>>, crate::ExecutionStop> {
        self.execution.checkpoint()?;
        let result = crate::access::ordered_dependency_explanation(self, id);
        self.execution.checkpoint()?;
        Ok(result)
    }

    #[must_use]
    pub fn claim_chain_report(&self, id: &str) -> Option<&ClaimChainReport> {
        self.claim_chains.get(id)
    }

    #[must_use]
    pub fn artifact_verified(&self, id: &str) -> bool {
        self.verified_artifacts.contains(id)
    }

    /// An acquired wire candidate only; does not verify any snapshot/report pair.
    #[must_use]
    pub fn unbound_report_candidate(
        &self,
        artifact_id: &str,
    ) -> Option<&crate::UnboundReportCandidate> {
        self.unbound_reports.get(artifact_id)
    }

    /// Observation for this exact attached pair. None means absent or not admitted
    /// (e.g. pair overflow), never success. Does not authorize export or override
    /// overall project errors. C2 candidate availability remains a separate getter.
    #[must_use]
    pub fn report_binding(
        &self,
        snapshot_id: &str,
        artifact_id: &str,
    ) -> Option<&crate::ReportBindingObservation> {
        self.report_bindings
            .get(snapshot_id)
            .filter(|(id, _)| id.as_ref() == artifact_id)
            .map(|(_, observation)| observation)
    }

    pub(crate) fn set_report_bindings(&mut self, bindings: crate::report_binding::Bindings) {
        for (id, report) in &mut self.claim_chains {
            let attached = self.objects.get(id.as_ref()).is_some_and(|o| {
                o.value
                    .get("validation_report")
                    .is_some_and(|v| !v.is_null())
            });
            if attached {
                report.valid &= bindings
                    .get(id.as_ref())
                    .is_some_and(|(_, b)| b.permits_chain_valid());
            }
        }
        self.report_bindings = bindings;
    }

    pub(crate) fn set_unbound_reports(
        &mut self,
        reports: BTreeMap<Box<str>, crate::UnboundReportCandidate>,
    ) {
        self.unbound_reports = reports;
    }

    #[must_use]
    pub fn canonical_digest(&self, id: &str) -> Option<String> {
        self.objects
            .get(id)
            .and_then(|object| jcs_sha256(&object.value).ok())
    }

    #[must_use]
    pub fn export_check(&self, id: &str, request: ExportRequest) -> Option<ExportDecision> {
        let access = self.access_report(id)?;
        let eligible = access.effective.level <= request.level_ceiling
            && access
                .effective
                .compartments
                .is_subset(&request.allowed_compartments);
        Some(ExportDecision {
            eligible,
            effective_access: access.effective.clone(),
            request,
        })
    }

    pub fn references_from(&self, id: &str) -> impl Iterator<Item = &ReferenceEdge> {
        self.objects
            .get(id)
            .and_then(|object| self.references.get(object.ordinal))
            .into_iter()
            .flatten()
    }

    #[must_use]
    pub fn heads_for(&self, logical_id: &str) -> Vec<&str> {
        self.heads
            .get(logical_id)
            .into_iter()
            .flatten()
            .map(AsRef::as_ref)
            .collect()
    }

    #[must_use]
    pub fn reference_fingerprint(&self) -> Vec<(String, String, String, String)> {
        self.references
            .iter()
            .flatten()
            .map(|reference| {
                (
                    reference.source_id.to_string(),
                    reference.target_id.to_string(),
                    reference.expected.fingerprint_label(),
                    reference.json_pointer.clone(),
                )
            })
            .collect()
    }

    pub(crate) fn insert(&mut self, record: ObjectRecord) -> Option<ObjectRecord> {
        self.objects.insert(record.id.clone(), record)
    }

    pub(crate) fn object_values(&self) -> impl Iterator<Item = &ObjectRecord> {
        self.objects.values()
    }

    pub(crate) fn assign_ordinals(&mut self) {
        for (ordinal, object) in self.objects.values_mut().enumerate() {
            object.ordinal = ordinal;
        }
    }

    pub(crate) fn set_references(&mut self, references: Vec<ReferenceEdge>) {
        self.references = (0..self.objects.len()).map(|_| Vec::new()).collect();
        for mut reference in references {
            // Ordinals belong to this index, including unresolved targets after scoping.
            reference.target_ordinal = None;
            let source_ordinal = self
                .objects
                .get(reference.source_id.as_ref())
                .expect("reference source is indexed")
                .ordinal;
            if let Some(target) = self.objects.get(reference.target_id.as_ref()) {
                reference.target_id = target.id.clone();
                reference.target_ordinal = NonZeroU32::new(
                    u32::try_from(target.ordinal + 1)
                        .expect("project object limit keeps encoded ordinals within u32"),
                );
            }
            self.references[source_ordinal].push(reference);
        }
        for references in &mut self.references {
            references.sort();
        }
    }

    pub(crate) fn set_access(&mut self, access: AccessStore) {
        self.access = access;
    }

    pub(crate) fn set_claim_chains(&mut self, claim_chains: BTreeMap<Box<str>, ClaimChainReport>) {
        self.claim_chains = claim_chains;
    }

    pub(crate) fn set_verified_artifacts(&mut self, artifacts: BTreeSet<Box<str>>) {
        self.verified_artifacts = artifacts;
    }

    pub(crate) fn set_freshness_policy(&mut self, policy: Option<Value>) {
        self.freshness_policy = policy;
    }

    pub(crate) fn freshness_policy(&self) -> Option<&Value> {
        self.freshness_policy.as_ref()
    }

    pub(crate) fn lineage_heads(&self) -> impl Iterator<Item = (&str, &[Arc<str>])> {
        self.heads
            .iter()
            .map(|(logical_id, heads)| (logical_id.as_ref(), heads.as_slice()))
    }

    pub(crate) fn derive_heads(&mut self) {
        let mut members: BTreeMap<Arc<str>, BTreeSet<Arc<str>>> = BTreeMap::new();
        let mut parents: BTreeSet<Arc<str>> = BTreeSet::new();
        for object in self.objects.values() {
            let Some(logical_id) = &object.logical_id else {
                continue;
            };
            members
                .entry(logical_id.clone())
                .or_default()
                .insert(object.id.clone());
            let parent_pointer = match &object.object_type {
                ObjectType::Node(_) | ObjectType::Relation | ObjectType::ThreadBinding => {
                    Some("/revision/parents")
                }
                _ => None,
            };
            if let Some(pointer) = parent_pointer
                && let Some(values) = object.value.pointer(pointer).and_then(Value::as_array)
            {
                for parent in values {
                    if let Some(id) = parent.get("id").and_then(Value::as_str) {
                        parents.insert(Arc::from(id));
                    }
                }
            }
        }
        self.heads = members
            .into_iter()
            .map(|(logical_id, ids)| {
                let heads = ids.into_iter().filter(|id| !parents.contains(id)).collect();
                (logical_id, heads)
            })
            .collect();
    }
}
