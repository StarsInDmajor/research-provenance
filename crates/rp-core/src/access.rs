use crate::findings::Findings;
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::{BTreeSet, VecDeque},
    sync::Arc,
};

use serde::Serialize;
use serde_json::Value;

use crate::{Finding, ObjectRecord, ObjectType, ProjectIndex, ReferenceEdge, Severity};

#[derive(Clone, Debug, Eq, PartialEq)]
struct AccessEdge {
    order: u8,
    label: Cow<'static, str>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessLevel {
    Public,
    Internal,
    Restricted,
    Exclusive,
}

impl AccessLevel {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "public" => Some(Self::Public),
            "internal" => Some(Self::Internal),
            "restricted" => Some(Self::Restricted),
            "exclusive" => Some(Self::Exclusive),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AccessLabel {
    pub level: AccessLevel,
    pub compartments: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DependencyStep {
    pub from_id: Box<str>,
    pub edge: String,
    pub to_id: Box<str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessReport {
    pub declared: AccessLabel,
    pub required_floor: AccessLabel,
    pub effective: AccessLabel,
    pub valid: bool,
    pub dependency_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportRequest {
    pub level_ceiling: AccessLevel,
    pub allowed_compartments: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportDecision {
    pub eligible: bool,
    pub effective_access: AccessLabel,
    pub request: ExportRequest,
}

#[derive(Clone, Debug)]
enum CompactCompartments {
    Bits(u64),
    Many(Box<[u32]>),
}

impl CompactCompartments {
    fn from_indexes(indexes: impl IntoIterator<Item = usize>, vocabulary_len: usize) -> Self {
        if vocabulary_len <= 64 {
            let mut bits = 0_u64;
            for index in indexes {
                bits |= 1_u64 << index;
            }
            Self::Bits(bits)
        } else {
            let mut indexes: Vec<_> = indexes
                .into_iter()
                .filter_map(|index| u32::try_from(index).ok())
                .collect();
            indexes.sort_unstable();
            indexes.dedup();
            Self::Many(indexes.into_boxed_slice())
        }
    }

    fn indexes(&self) -> Box<dyn Iterator<Item = usize> + '_> {
        match self {
            Self::Bits(bits) => {
                let bits = *bits;
                Box::new((0..64).filter(move |index| bits & (1_u64 << index) != 0))
            }
            Self::Many(indexes) => Box::new(indexes.iter().map(|index| *index as usize)),
        }
    }

    fn is_subset(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bits(left), Self::Bits(right)) => left & !right == 0,
            _ => self
                .indexes()
                .all(|index| other.indexes().any(|other| other == index)),
        }
    }

    fn union(&self, other: &Self, vocabulary_len: usize) -> Self {
        match (self, other) {
            (Self::Bits(left), Self::Bits(right)) => Self::Bits(left | right),
            _ => Self::from_indexes(self.indexes().chain(other.indexes()), vocabulary_len),
        }
    }
}

#[derive(Clone, Debug)]
struct StoredAccessReport {
    declared_level: AccessLevel,
    declared_compartments: CompactCompartments,
    required_level: AccessLevel,
    required_compartments: CompactCompartments,
    effective_level: AccessLevel,
    effective_compartments: CompactCompartments,
    valid: bool,
    dependency_count: usize,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct AccessStore {
    compartment_names: Vec<Box<str>>,
    reports: Vec<Option<StoredAccessReport>>,
}

impl AccessStore {
    pub(crate) fn report(&self, ordinal: usize) -> Option<AccessReport> {
        let report = self.reports.get(ordinal)?.as_ref()?;
        Some(AccessReport {
            declared: self.label(report.declared_level, &report.declared_compartments),
            required_floor: self.label(report.required_level, &report.required_compartments),
            effective: self.label(report.effective_level, &report.effective_compartments),
            valid: report.valid,
            dependency_count: report.dependency_count,
        })
    }

    fn label(&self, level: AccessLevel, compartments: &CompactCompartments) -> AccessLabel {
        AccessLabel {
            level,
            compartments: compartments
                .indexes()
                .map(|index| self.compartment_names[index].to_string())
                .collect(),
        }
    }
}

pub(crate) fn compute_access(
    index: &ProjectIndex,
    mut findings: Findings,
) -> (AccessStore, Findings) {
    let mut closure = AccessClosure::new(index);
    let mut reports: Vec<Option<StoredAccessReport>> =
        (0..closure.objects.len()).map(|_| None).collect();

    for (ordinal, report_slot) in reports.iter_mut().enumerate() {
        if findings.cancelled() {
            break;
        }
        let object = closure.objects[ordinal];
        let Some(declared) = closure.labels[ordinal].clone() else {
            continue;
        };
        if !compartments_are_sorted(&object.value) {
            findings.push(|| {
                Finding::new(
                    "RP_E_ACCESS_COMPARTMENTS_UNSORTED",
                    "access_compartment_normalization",
                    Severity::Error,
                    "access compartments must be sorted lexicographically",
                    Some(object.source_file.clone()),
                    "/access/compartments",
                )
            });
        }

        let (dependency_count, required_level, required_compartments) = closure.summary(ordinal);
        let level_valid = declared.level >= required_level;
        let compartments_valid = required_compartments.is_subset(&declared.compartments);
        let valid = level_valid && compartments_valid;
        let effective_level = required_level.max(declared.level);
        let effective_compartments = declared
            .compartments
            .union(&required_compartments, closure.compartment_names.len());

        if !valid {
            let declared_label = closure.label(declared.level, &declared.compartments);
            let required_floor = closure.label(required_level, &required_compartments);
            let (code, family) =
                primary_access_code(object, index, &declared_label, &required_floor, level_valid);
            let missing: Vec<_> = required_floor
                .compartments
                .difference(&declared_label.compartments)
                .cloned()
                .collect();
            findings.push(|| Finding::new(
                code,
                family,
                Severity::Error,
                format!(
                    "declared access does not dominate dependency floor; required level {:?}, missing compartments {:?}",
                    required_floor.level, missing
                ),
                Some(object.source_file.clone()),
                "/access",
            ));
        }

        *report_slot = Some(StoredAccessReport {
            declared_level: declared.level,
            declared_compartments: declared.compartments.clone(),
            required_level,
            required_compartments,
            effective_level,
            effective_compartments,
            valid,
            dependency_count,
        });
    }

    let compartment_names = closure
        .compartment_names
        .iter()
        .map(|name| (*name).into())
        .collect();
    (
        AccessStore {
            compartment_names,
            reports,
        },
        findings,
    )
}

#[derive(Clone)]
struct CompactAccess {
    level: AccessLevel,
    compartments: CompactCompartments,
}

struct AccessClosure<'a> {
    index: &'a ProjectIndex,
    objects: Vec<&'a ObjectRecord>,
    labels: Vec<Option<CompactAccess>>,
    compartment_names: Vec<&'a str>,
    visited: Vec<u32>,
    compartment_marks: Vec<u32>,
    queue: VecDeque<usize>,
    generation: u32,
}

impl<'a> AccessClosure<'a> {
    fn new(index: &'a ProjectIndex) -> Self {
        let objects: Vec<_> = index.object_values().collect();
        let compartment_names: Vec<_> = objects
            .iter()
            .filter_map(|object| access_value(object))
            .flat_map(|access| {
                access
                    .get("compartments")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let labels = objects
            .iter()
            .map(|object| {
                let access = access_value(object)?;
                let level = AccessLevel::parse(access.get("level")?.as_str()?)?;
                let compartments = CompactCompartments::from_indexes(
                    access
                        .get("compartments")?
                        .as_array()?
                        .iter()
                        .filter_map(Value::as_str)
                        .filter_map(|name| compartment_names.binary_search(&name).ok()),
                    compartment_names.len(),
                );
                Some(CompactAccess {
                    level,
                    compartments,
                })
            })
            .collect();
        let object_count = objects.len();
        let compartment_count = compartment_names.len();
        Self {
            index,
            objects,
            labels,
            compartment_names,
            visited: vec![0; object_count],
            compartment_marks: vec![0; compartment_count],
            queue: VecDeque::new(),
            generation: 0,
        }
    }

    fn label(&self, level: AccessLevel, compartments: &CompactCompartments) -> AccessLabel {
        AccessLabel {
            level,
            compartments: compartments
                .indexes()
                .map(|index| self.compartment_names[index].to_string())
                .collect(),
        }
    }

    fn summary(&mut self, start: usize) -> (usize, AccessLevel, CompactCompartments) {
        self.generation += 1;
        let generation = self.generation;
        self.queue.clear();
        self.visited[start] = generation;
        self.queue.push_back(start);
        let mut dependency_count = 0;
        let mut level = AccessLevel::Public;
        while let Some(source) = self.queue.pop_front() {
            if self.index.execution.checkpoint().is_err() {
                break;
            }
            for reference in self.index.references_from(&self.objects[source].id) {
                if self.index.execution.checkpoint().is_err() {
                    break;
                }
                let Some(target) = reference
                    .target_ordinal
                    .map(|ordinal| ordinal.get() as usize - 1)
                else {
                    continue;
                };
                if self.visited[target] == generation {
                    continue;
                }
                self.visited[target] = generation;
                dependency_count += 1;
                if let Some(label) = &self.labels[target] {
                    level = level.max(label.level);
                    for compartment in label.compartments.indexes() {
                        self.compartment_marks[compartment] = generation;
                    }
                }
                self.queue.push_back(target);
            }
        }
        let compartments = CompactCompartments::from_indexes(
            self.compartment_marks
                .iter()
                .enumerate()
                .filter(|(_, marked)| **marked == generation)
                .map(|(index, _)| index),
            self.compartment_names.len(),
        );
        (dependency_count, level, compartments)
    }
}

fn access_value(object: &ObjectRecord) -> Option<&Value> {
    if matches!(object.object_type, ObjectType::Project) {
        object.value.get("access_defaults")
    } else {
        object.value.get("access")
    }
}

fn access_label(object: &ObjectRecord) -> Option<AccessLabel> {
    let access = access_value(object)?;
    let level = AccessLevel::parse(access.get("level")?.as_str()?)?;
    let compartments = access
        .get("compartments")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    Some(AccessLabel {
        level,
        compartments,
    })
}

fn compartments_are_sorted(value: &Value) -> bool {
    let Some(values) = value
        .pointer("/access/compartments")
        .and_then(Value::as_array)
    else {
        return true;
    };
    let strings: Vec<_> = values.iter().filter_map(Value::as_str).collect();
    strings.windows(2).all(|pair| pair[0] < pair[1])
}

// Policy order is needed only for requested explanations and access-error selection.
// Closure summaries use the stored semantic order without allocating an edge list.
fn policy_references_from<'a>(index: &'a ProjectIndex, id: &str) -> Vec<&'a ReferenceEdge> {
    let Some(source) = index.get(id) else {
        return Vec::new();
    };
    let mut references: Vec<_> = index.references_from(id).collect();
    references.sort_by(|left, right| compare_reference_edges(source, left, right));
    references
}

fn compare_reference_edges(
    source: &ObjectRecord,
    left: &ReferenceEdge,
    right: &ReferenceEdge,
) -> Ordering {
    let left_edge = access_edge(source, &left.json_pointer);
    let right_edge = access_edge(source, &right.json_pointer);
    (
        left_edge.order,
        left_edge.label.as_ref(),
        left.target_id.as_ref(),
        left.json_pointer.as_str(),
    )
        .cmp(&(
            right_edge.order,
            right_edge.label.as_ref(),
            right.target_id.as_ref(),
            right.json_pointer.as_str(),
        ))
}

fn access_edge(source: &ObjectRecord, pointer: &str) -> AccessEdge {
    let known = |order, label| AccessEdge {
        order,
        label: Cow::Borrowed(label),
    };
    let array_edge =
        |prefix: &str, order, label| pointer.starts_with(prefix).then(|| known(order, label));
    if let Some(edge) = array_edge("/source/revisions/", 0, "source.revisions") {
        return edge;
    }
    if let Some(edge) = array_edge("/source/relations/", 1, "source.relations") {
        return edge;
    }
    if let Some(edge) = array_edge(
        "/source/external_references/",
        2,
        "source.external_references",
    ) {
        return edge;
    }
    if let Some(edge) = array_edge("/source/artifacts/", 3, "source.artifacts") {
        return edge;
    }
    if pointer.starts_with("/revision/parents/") {
        return match source.object_type {
            ObjectType::Node(_) => known(5, "revision.parents"),
            ObjectType::Relation => known(8, "relation.revision.parents"),
            ObjectType::ThreadBinding => known(20, "binding.revision.parents"),
            _ => AccessEdge {
                order: 40,
                label: Cow::Owned(pointer.to_string()),
            },
        };
    }
    match (&source.object_type, pointer) {
        (ObjectType::Relation, "/from_revision") => known(6, "relation.from_revision"),
        (ObjectType::Relation, "/to_revision") => known(7, "relation.to_revision"),
        (ObjectType::Assessment, "/target/id") => known(9, "assessment.target"),
        (ObjectType::Assessment, value) if value.starts_with("/supersedes_assessments/") => {
            known(10, "assessment.supersedes_assessments")
        }
        (ObjectType::Assessment, value)
            if value.starts_with("/quantitative_assessment/source_measurements/") =>
        {
            known(11, "assessment.quantitative_assessment.source_measurements")
        }
        (ObjectType::Thread, "/root_question_revision") => {
            known(12, "thread.root_question_revision")
        }
        (ObjectType::Thread, "/parent_thread_id") => known(13, "thread.parent_thread_id"),
        (ObjectType::Thread, "/forked_from_thread_id") => known(14, "thread.forked_from_thread_id"),
        (ObjectType::ThreadBinding, "/thread_id") => known(15, "binding.thread_id"),
        (ObjectType::ThreadBinding, "/target/id") => known(16, "binding.target"),
        (ObjectType::ClaimChain, "/thread_id") => known(17, "claim_chain.thread_id"),
        (ObjectType::ClaimChain, value) if value.starts_with("/root_node_revisions/") => {
            known(18, "claim_chain.root_node_revisions")
        }
        (ObjectType::ClaimChain, "/target_node_revision") => {
            known(19, "claim_chain.target_node_revision")
        }
        (ObjectType::ClaimChain, value) if value.starts_with("/node_revisions/") => {
            known(20, "claim_chain.node_revisions")
        }
        (ObjectType::ClaimChain, value) if value.starts_with("/relation_revisions/") => {
            known(21, "claim_chain.relation_revisions")
        }
        (ObjectType::ClaimChain, "/validation_report") => {
            known(22, "claim_chain.validation_report")
        }
        (ObjectType::Node(_), "/scope/research_run_id") => known(23, "node.scope.research_run_id"),
        (ObjectType::Node(_), value) if value.starts_with("/scope/dataset_revisions/") => {
            known(24, "node.scope.dataset_revisions")
        }
        (ObjectType::Node(_), "/observation/method/protocol_reference") => {
            known(25, "node.observation.method.protocol_reference")
        }
        (ObjectType::Node(_), "/measurement/method/protocol_reference") => {
            known(26, "node.measurement.method.protocol_reference")
        }
        (ObjectType::Node(_), value) if value.starts_with("/method/protocol_references/") => {
            known(27, "node.method.protocol_references")
        }
        (ObjectType::Node(_), value) if value.starts_with("/test/target_revision_ids/") => {
            known(28, "node.test.target_revision_ids")
        }
        (ObjectType::Node(_), "/test/method_revision") => known(29, "node.test.method_revision"),
        (ObjectType::Node(_), value) if value.starts_with("/next_action/dependencies/") => {
            known(30, "node.next_action.dependencies")
        }
        (ObjectType::Node(_), "/paper_claim/publication_reference") => {
            known(31, "node.paper_claim.publication_reference")
        }
        (ObjectType::ResearchRun, "/project_id") => known(35, "research_run.project_id"),
        (ObjectType::ResearchRun, "/parent_run_id") => known(36, "research_run.parent_run_id"),
        (ObjectType::ResearchRun, value) if value.starts_with("/planned_inputs/revisions/") => {
            known(37, "research_run.planned_inputs.revisions")
        }
        (ObjectType::ResearchRun, value) if value.starts_with("/planned_inputs/artifacts/") => {
            known(38, "research_run.planned_inputs.artifacts")
        }
        (_, value) if value.starts_with("/extensions/") => {
            known(39, "extension_schema_declared_semantic_references")
        }
        _ => AccessEdge {
            order: 40,
            label: Cow::Owned(pointer.to_string()),
        },
    }
}

pub(crate) fn ordered_dependency_explanation(
    index: &ProjectIndex,
    start: &str,
) -> Option<Vec<DependencyStep>> {
    index.access_report(start)?;
    let mut visited = BTreeSet::new();
    let mut explanation = Vec::new();
    let mut queue = VecDeque::from([Arc::<str>::from(start)]);
    while let Some(source) = queue.pop_front() {
        if index.execution.checkpoint().is_err() {
            return None;
        }
        let source_object = index.get(&source)?;
        for reference in policy_references_from(index, &source) {
            if index.execution.checkpoint().is_err() {
                return None;
            }
            let target = &reference.target_id;
            if target.as_ref() == start || !visited.insert(target.clone()) {
                continue;
            }
            if index.get(target).is_none() {
                continue;
            }
            explanation.push(DependencyStep {
                from_id: source.as_ref().into(),
                edge: access_edge(source_object, &reference.json_pointer)
                    .label
                    .into_owned(),
                to_id: target.as_ref().into(),
            });
            queue.push_back(target.clone());
        }
    }
    Some(explanation)
}

fn primary_access_code(
    object: &ObjectRecord,
    index: &ProjectIndex,
    declared: &AccessLabel,
    required: &AccessLabel,
    level_valid: bool,
) -> (&'static str, &'static str) {
    let deficits = |target: &ObjectRecord| {
        access_label(target).is_some_and(|label| {
            (!level_valid && label.level > declared.level)
                || !label.compartments.is_subset(&declared.compartments)
        })
    };
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    for reference in policy_references_from(index, &object.id) {
        queue.push_back((
            reference.target_id.clone(),
            access_edge(object, &reference.json_pointer),
        ));
    }
    let mut deficit_edge = None;
    while let Some((target, first_edge)) = queue.pop_front() {
        if index.execution.checkpoint().is_err() {
            break;
        }
        if !visited.insert(target.clone()) {
            continue;
        }
        if index.get(&target).is_some_and(&deficits) {
            deficit_edge = Some(first_edge);
            break;
        }
        for reference in policy_references_from(index, &target) {
            queue.push_back((reference.target_id.clone(), first_edge.clone()));
        }
    }
    if let Some(edge) = deficit_edge {
        return match edge.label.as_ref() {
            "relation.from_revision" | "relation.to_revision" => (
                "RP_E_ACCESS_STRUCTURAL_ENDPOINT",
                "access_structural_endpoint",
            ),
            "revision.parents" => ("RP_E_ACCESS_REVISION_PARENT", "access_revision_parent"),
            "relation.revision.parents" => {
                ("RP_E_ACCESS_RELATION_PARENT", "access_relation_parent")
            }
            "assessment.target" => ("RP_E_ACCESS_ASSESSMENT_TARGET", "access_assessment_target"),
            "assessment.supersedes_assessments" => (
                "RP_E_ACCESS_ASSESSMENT_SUPERSESSION",
                "access_assessment_supersession",
            ),
            "thread.root_question_revision" => ("RP_E_ACCESS_THREAD_ROOT", "access_thread_root"),
            "thread.parent_thread_id" | "thread.forked_from_thread_id" => {
                ("RP_E_ACCESS_THREAD_PARENT", "access_thread_parent")
            }
            "binding.thread_id" | "binding.target" => {
                ("RP_E_ACCESS_BINDING_TARGET", "access_binding_target")
            }
            edge if edge.starts_with("claim_chain.") => (
                "RP_E_ACCESS_CLAIM_SELECTION",
                "access_claim_chain_selection",
            ),
            _ => generic_access_code(level_valid),
        };
    }
    let _ = required;
    generic_access_code(level_valid)
}

const fn generic_access_code(level_valid: bool) -> (&'static str, &'static str) {
    if level_valid {
        ("RP_E_ACCESS_COMPARTMENT_MISSING", "access_compartment_drop")
    } else {
        ("RP_E_ACCESS_LEVEL_DOWNGRADE", "access_level_downgrade")
    }
}
