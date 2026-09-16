use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
};

use rp_core::{
    ProjectLimits, ProjectPath, SchemaBundle, canonicalize_jcs, validate_project_with_limits,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const EPOCH_MS: u64 = 1_767_225_600_000;
const AS_OF: &str = "2027-01-01T00:00:00Z";
const GENERATOR_VERSION: &str = "rp/scale-generator/v1";
const ACCESS_WEIGHTS: &[(&str, u64)] = &[
    ("public", 10),
    ("internal", 60),
    ("restricted", 25),
    ("exclusive", 5),
];
const COMPARTMENTS: &[&str] = &[
    "analysis-alpha",
    "collaboration-beta",
    "control-room",
    "embargo-2027",
    "instrument-gamma",
    "lineage-delta",
    "pipeline-epsilon",
    "proposal-zeta",
];
const KINDS: &[KindSpec] = &[
    KindSpec::new("Dataset", "dset", "datasets", "dataset", "dataset", 8),
    KindSpec::new("Question", "qst", "questions", "question", "question", 7),
    KindSpec::new(
        "Hypothesis",
        "hyp",
        "hypotheses",
        "hypothesis",
        "hypothesis",
        10,
    ),
    KindSpec::new(
        "Prediction",
        "pred",
        "predictions",
        "prediction",
        "prediction",
        6,
    ),
    KindSpec::new("Method", "mth", "methods", "method", "method", 7),
    KindSpec::new("Test", "tst", "tests", "test", "test", 6),
    KindSpec::new(
        "Observation",
        "obs",
        "observations",
        "observation",
        "observation",
        12,
    ),
    KindSpec::new(
        "Measurement",
        "meas",
        "measurements",
        "measurement",
        "measurement",
        18,
    ),
    KindSpec::new("Synthesis", "syn", "syntheses", "synthesis", "synthesis", 7),
    KindSpec::new(
        "Interpretation",
        "int",
        "interpretations",
        "interpretation",
        "interpretation",
        6,
    ),
    KindSpec::new("Decision", "dec", "decisions", "decision", "decision", 3),
    KindSpec::new(
        "Conclusion",
        "con",
        "conclusions",
        "conclusion",
        "conclusion",
        4,
    ),
    KindSpec::new("Blocker", "blk", "blockers", "blocker", "blocker", 2),
    KindSpec::new(
        "NextAction",
        "nxt",
        "next-actions",
        "next-action",
        "next_action",
        3,
    ),
    KindSpec::new(
        "PaperClaim",
        "clm",
        "paper-claims",
        "paper-claim",
        "paper_claim",
        1,
    ),
];
const RELATION_TYPES: &[&str] = &[
    "derived-from",
    "has-part",
    "input-to",
    "generated",
    "cites",
    "implements",
    "supports",
    "weakens",
    "contradicts",
    "consistent-with",
    "provides-prior",
    "motivates",
    "predicts",
    "tested-by",
    "result-of",
    "observed-in",
    "reproduces",
    "fails-to-reproduce",
    "validated-by",
    "requires",
    "blocked-by",
    "enables",
    "next-step",
    "depends-on",
];
const BINDING_ROLES: &[&str] = &[
    "primary",
    "alternative",
    "diagnostic",
    "sensitivity",
    "exploratory",
    "replication",
    "counterfactual",
    "failed",
    "historical",
    "follow-up",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile {
    Smoke,
    Workstation,
    Stress,
}

impl Profile {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Workstation => "workstation",
            Self::Stress => "stress",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "smoke" => Some(Self::Smoke),
            "workstation" => Some(Self::Workstation),
            "stress" => Some(Self::Stress),
            _ => None,
        }
    }

    #[must_use]
    pub fn canonical_counts(self) -> BTreeMap<String, usize> {
        let values = match self {
            Self::Smoke => [1, 5, 25, 50, 1_000, 2_500, 250, 25, 1_000, 20],
            Self::Workstation => [1, 50, 250, 500, 10_000, 30_000, 5_000, 200, 15_000, 100],
            Self::Stress => [
                1, 200, 1_000, 2_500, 50_000, 150_000, 25_000, 1_000, 75_000, 500,
            ],
        };
        [
            "project",
            "research_run",
            "external_reference",
            "artifact_manifest",
            "node_revision",
            "scientific_relation_revision",
            "assessment",
            "research_thread",
            "thread_binding",
            "claim_chain_snapshot",
        ]
        .into_iter()
        .zip(values)
        .map(|(key, value)| (key.to_string(), value))
        .collect()
    }

    #[must_use]
    pub fn total_objects(self) -> usize {
        self.canonical_counts().values().sum()
    }
}

#[derive(Clone, Debug)]
pub struct GenerateOptions {
    pub profile: Profile,
    pub seed: u64,
    pub output: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PathDigestEntry {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScaleManifest {
    pub schema: String,
    pub generator_version: String,
    pub profile: String,
    pub seed: u64,
    pub as_of: String,
    pub object_counts: BTreeMap<String, usize>,
    pub relation_type_counts: BTreeMap<String, usize>,
    pub kind_counts: BTreeMap<String, usize>,
    pub access_counts: BTreeMap<String, usize>,
    pub sorted_path_sha256_entries: Vec<PathDigestEntry>,
    pub aggregate_sha256: String,
}

#[derive(Debug)]
pub struct GeneratorError(String);

impl fmt::Display for GeneratorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for GeneratorError {}

pub struct CounterStream {
    seed: u64,
    name: String,
    counter: u64,
    block: [u8; 32],
    offset: usize,
}

impl CounterStream {
    #[allow(clippy::missing_errors_doc)]
    pub fn new(seed: u64, name: &str) -> Result<Self, GeneratorError> {
        if name.is_empty()
            || !name.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_/".contains(&byte)
            })
        {
            return Err(GeneratorError(
                "stream name is not a stable ASCII slug".to_string(),
            ));
        }
        Ok(Self {
            seed,
            name: name.to_string(),
            counter: 0,
            block: [0; 32],
            offset: 32,
        })
    }

    #[must_use]
    pub fn take(&mut self, count: usize) -> Vec<u8> {
        let mut output = Vec::with_capacity(count);
        while output.len() < count {
            if self.offset == self.block.len() {
                let input = format!(
                    "rp-scale-generator/v1\0{}\0{}\0{}",
                    self.seed, self.name, self.counter
                );
                self.block
                    .copy_from_slice(&Sha256::digest(input.as_bytes()));
                self.counter = self.counter.checked_add(1).expect("counter overflow");
                self.offset = 0;
            }
            let available = (count - output.len()).min(self.block.len() - self.offset);
            output.extend_from_slice(&self.block[self.offset..self.offset + available]);
            self.offset += available;
        }
        output
    }
}

#[allow(clippy::missing_errors_doc)]
pub fn bounded_integer(
    stream: &mut CounterStream,
    minimum: u64,
    maximum: u64,
) -> Result<u64, GeneratorError> {
    let width = maximum
        .checked_sub(minimum)
        .filter(|width| *width > 0)
        .ok_or_else(|| GeneratorError("bounded integer range is empty".to_string()))?;
    let domain = 1_u128 << 64;
    let limit = domain - (domain % u128::from(width));
    loop {
        let bytes: [u8; 8] = stream
            .take(8)
            .try_into()
            .expect("counter stream returned eight bytes");
        let value = u64::from_be_bytes(bytes);
        if u128::from(value) < limit {
            return Ok(minimum + value % width);
        }
    }
}

#[allow(clippy::missing_errors_doc)]
pub fn largest_remainder(
    total: usize,
    weights: &BTreeMap<String, u64>,
) -> Result<BTreeMap<String, usize>, GeneratorError> {
    if weights.is_empty() || weights.values().any(|weight| *weight == 0) {
        return Err(GeneratorError("weights must be positive".to_string()));
    }
    let sum: u128 = weights.values().map(|weight| u128::from(*weight)).sum();
    let mut allocated = BTreeMap::new();
    let mut remainders = Vec::new();
    let mut assigned = 0_usize;
    for (key, weight) in weights {
        let numerator = (total as u128) * u128::from(*weight);
        let floor = usize::try_from(numerator / sum)
            .map_err(|_| GeneratorError("apportionment overflow".to_string()))?;
        assigned += floor;
        allocated.insert(key.clone(), floor);
        remainders.push((numerator % sum, key.clone()));
    }
    remainders.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    for (_, key) in remainders.into_iter().take(total - assigned) {
        *allocated.get_mut(&key).expect("allocated key exists") += 1;
    }
    Ok(allocated)
}

#[allow(clippy::missing_errors_doc)]
pub fn typed_id(
    seed: u64,
    schema: &str,
    prefix: &str,
    ordinal: u64,
) -> Result<String, GeneratorError> {
    let timestamp = EPOCH_MS
        .checked_add(ordinal)
        .filter(|value| *value < (1_u64 << 48))
        .ok_or_else(|| GeneratorError("ULID timestamp overflow".to_string()))?;
    let mut stream = CounterStream::new(seed, &format!("ulid/{schema}/{ordinal}"))?;
    let randomness: [u8; 10] = stream
        .take(10)
        .try_into()
        .expect("counter stream returned ten bytes");
    Ok(format!("{prefix}_{}", encode_ulid(timestamp, randomness)))
}

#[allow(clippy::missing_errors_doc)]
pub fn canonical_path(
    kind: &str,
    role: &str,
    ordinal: usize,
    id: &str,
) -> Result<String, GeneratorError> {
    let directory = KINDS
        .iter()
        .find(|spec| spec.kind == kind)
        .map(|spec| spec.directory)
        .ok_or_else(|| GeneratorError("unknown built-in kind".to_string()))?;
    Ok(format!(
        ".research/records/{directory}/{role}-{ordinal:06}--{id}.yaml"
    ))
}

#[allow(clippy::missing_errors_doc)]
pub fn generate(options: GenerateOptions) -> Result<ScaleManifest, GeneratorError> {
    prepare_output(&options.output)?;
    let result = generate_inner(&options);
    if result.is_err() {
        let _ = fs::remove_dir_all(&options.output);
    }
    result
}

fn generate_inner(options: &GenerateOptions) -> Result<ScaleManifest, GeneratorError> {
    let counts = options.profile.canonical_counts();
    let mut writer = ProjectWriter::new(
        options,
        AccessAllocator::new(options.profile.total_objects(), options.seed)?,
    );
    writer.write_project()?;
    writer.write_runs(counts["research_run"])?;
    writer.write_references(counts["external_reference"])?;
    writer.write_artifacts(counts["artifact_manifest"])?;
    writer.write_nodes(counts["node_revision"])?;
    writer.write_relations(counts["scientific_relation_revision"])?;
    writer.write_assessments(counts["assessment"])?;
    writer.write_threads(counts["research_thread"])?;
    writer.write_bindings(counts["thread_binding"])?;
    writer.write_claim_chains(counts["claim_chain_snapshot"])?;
    writer.write_freshness_policy()?;
    writer.recompute_claim_digests()?;
    writer.self_validate()?;

    let entries = digest_tree(&options.output)?;
    let aggregate_sha256 = sha256(&canonicalize_jcs(&entries).map_err(|error| {
        GeneratorError(format!("cannot canonicalize manifest entries: {error}"))
    })?);
    let manifest = ScaleManifest {
        schema: "rp/scale-manifest/v1".to_string(),
        generator_version: GENERATOR_VERSION.to_string(),
        profile: options.profile.as_str().to_string(),
        seed: options.seed,
        as_of: AS_OF.to_string(),
        object_counts: counts,
        relation_type_counts: writer.relation_type_counts,
        kind_counts: writer.kind_counts,
        access_counts: writer.access.counts.clone(),
        sorted_path_sha256_entries: entries,
        aggregate_sha256,
    };
    SchemaBundle::new()
        .map_err(|error| GeneratorError(error.to_string()))?
        .validate(
            &serde_json::to_value(&manifest).map_err(|error| GeneratorError(error.to_string()))?,
            ProjectPath::new("scale-manifest.json").expect("static manifest path"),
        )
        .map_err(|findings| GeneratorError(format!("invalid scale manifest: {findings:?}")))?;
    let mut bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|error| GeneratorError(error.to_string()))?;
    bytes.push(b'\n');
    fs::write(options.output.join("scale-manifest.json"), bytes)
        .map_err(|error| GeneratorError(format!("cannot write scale manifest: {error}")))?;
    Ok(manifest)
}

struct ProjectWriter<'a> {
    options: &'a GenerateOptions,
    ordinal: u64,
    access: AccessAllocator,
    project_id: String,
    run_ids: Vec<String>,
    reference_ids: Vec<String>,
    artifact_ids: Vec<String>,
    node_ids: BTreeMap<String, Vec<String>>,
    public_node: BTreeMap<String, String>,
    public_nodes: BTreeMap<String, Vec<String>>,
    relation_ids: BTreeMap<String, Vec<String>>,
    thread_ids: Vec<String>,
    claim_paths: Vec<PathBuf>,
    kind_counts: BTreeMap<String, usize>,
    relation_type_counts: BTreeMap<String, usize>,
}

impl<'a> ProjectWriter<'a> {
    fn new(options: &'a GenerateOptions, access: AccessAllocator) -> Self {
        Self {
            options,
            ordinal: 0,
            access,
            project_id: String::new(),
            run_ids: Vec::new(),
            reference_ids: Vec::new(),
            artifact_ids: Vec::new(),
            node_ids: BTreeMap::new(),
            public_node: BTreeMap::new(),
            public_nodes: BTreeMap::new(),
            relation_ids: BTreeMap::new(),
            thread_ids: Vec::new(),
            claim_paths: Vec::new(),
            kind_counts: BTreeMap::new(),
            relation_type_counts: BTreeMap::new(),
        }
    }

    fn next_id(&mut self, schema: &str, prefix: &str) -> Result<String, GeneratorError> {
        let id = typed_id(self.options.seed, schema, prefix, self.ordinal)?;
        self.ordinal += 1;
        Ok(id)
    }

    fn write_project(&mut self) -> Result<(), GeneratorError> {
        self.project_id = self.next_id("rp/project/v1", "proj")?;
        let access = self.access.next(true)?;
        let object = json!({
            "schema": "rp/project/v1",
            "id": self.project_id,
            "slug": format!("scale-{}", self.options.profile.as_str()),
            "title": format!("Deterministic {} scale project", self.options.profile.as_str()),
            "created_at": timestamp(0),
            "schema_policy": {"core_version": "v1", "kind_registry": "rp/kinds/v1", "allowed_extensions": []},
            "repository_policy": {"visibility": "private", "allowed_remotes": []},
            "access_defaults": access,
            "validation_policy": ".research/policies/freshness-v1.yaml"
        });
        self.write_object(".research/project.yaml", &object)
    }

    fn write_runs(&mut self, count: usize) -> Result<(), GeneratorError> {
        for index in 0..count {
            let id = self.next_id("rp/research-run/v1", "run")?;
            let object = json!({
                "schema": "rp/research-run/v1",
                "id": id,
                "project_id": self.project_id,
                "title": format!("Generated run {index}"),
                "objective": "Exercise deterministic scale validation.",
                "created_at": timestamp(self.ordinal - 1),
                "created_by": actor(),
                "parent_run_id": Value::Null,
                "planned_inputs": {"revisions": [], "artifacts": []},
                "planned_outputs": ["Measurement"],
                "limitations": ["Synthetic benchmark data."],
                "access": self.access.next(false)?,
                "tags": ["scale-generated"]
            });
            self.write_object(
                &format!(".research/runs/run-{index:06}--{id}.yaml"),
                &object,
            )?;
            self.run_ids.push(id);
        }
        Ok(())
    }

    fn write_references(&mut self, count: usize) -> Result<(), GeneratorError> {
        for index in 0..count {
            let id = self.next_id("rp/external-reference/v1", "ref")?;
            let object = json!({
                "schema": "rp/external-reference/v1",
                "id": id,
                "type": "https",
                "canonical_uri": format!("https://example.invalid/rp-scale/reference/{index}"),
                "foreign_key": Value::Null,
                "display_label": format!("Generated reference {index}"),
                "retrieved_at": timestamp(self.ordinal - 1),
                "source_version": {"identifier": format!("v{index}"), "released_at": timestamp(self.ordinal - 1), "digest": Value::Null},
                "trust_tags": ["synthetic-scale"],
                "access": self.access.next(true)?
            });
            self.write_object(
                &format!(".research/references/reference-{index:06}--{id}.yaml"),
                &object,
            )?;
            self.reference_ids.push(id);
        }
        Ok(())
    }

    fn write_artifacts(&mut self, count: usize) -> Result<(), GeneratorError> {
        for index in 0..count {
            let id = self.next_id("rp/artifact-manifest/v1", "art")?;
            let mut stream = CounterStream::new(self.options.seed, &format!("artifact/{index}"))?;
            let payload = stream.take(64);
            let payload_path = format!("data/generated/artifact-{index:06}.bin");
            self.write_bytes(&payload_path, &payload)?;
            let object = json!({
                "schema": "rp/artifact-manifest/v1",
                "id": id,
                "title": format!("Generated artifact {index}"),
                "uri": format!("file:{payload_path}"),
                "media_type": "application/octet-stream",
                "size_bytes": 64,
                "sha256": sha256(&payload),
                "created_at": timestamp(self.ordinal - 1),
                "access": self.access.next(index == 0)?
            });
            self.write_object(
                &format!(".research/artifacts/artifact-{index:06}--{id}.yaml"),
                &object,
            )?;
            self.artifact_ids.push(id);
        }
        Ok(())
    }

    fn write_nodes(&mut self, count: usize) -> Result<(), GeneratorError> {
        let weights: BTreeMap<_, _> = KINDS
            .iter()
            .map(|kind| (kind.kind.to_string(), kind.weight))
            .collect();
        let allocations = largest_remainder(count, &weights)?;
        let mut global_index = 0_usize;
        for kind in KINDS {
            let kind_count = allocations[kind.kind];
            self.kind_counts.insert(kind.kind.to_string(), kind_count);
            let roles = largest_remainder(
                kind_count,
                &BTreeMap::from([
                    ("branch".to_string(), 8),
                    ("linear".to_string(), 20),
                    ("merge".to_string(), 2),
                    ("root".to_string(), 70),
                ]),
            )?;
            let root_count = roles["root"];
            let linear_count = roles["linear"];
            let branch_count = roles["branch"];
            let merge_count = roles["merge"];
            let mut ids = Vec::with_capacity(kind_count);
            let main_logical = format!("{}-main-lineage", kind.role);
            let main = self.make_node(kind, 0, &main_logical, &[], global_index, true)?;
            self.public_node.insert(kind.kind.to_string(), main.clone());
            self.public_nodes
                .entry(kind.kind.to_string())
                .or_default()
                .push(main.clone());
            ids.push(main.clone());
            global_index += 1;
            for index in 1..root_count {
                let logical = format!("{}-root-{index:06}", kind.role);
                ids.push(self.make_node(kind, index, &logical, &[], global_index, false)?);
                global_index += 1;
            }
            let mut linear_parent = main.clone();
            for index in 0..linear_count {
                let id = self.make_node(
                    kind,
                    root_count + index,
                    &main_logical,
                    &[linear_parent.clone()],
                    global_index,
                    true,
                )?;
                linear_parent = id.clone();
                self.public_nodes
                    .entry(kind.kind.to_string())
                    .or_default()
                    .push(id.clone());
                ids.push(id);
                global_index += 1;
            }
            let mut first_branch = None;
            for index in 0..branch_count {
                let force_public = index == 0;
                let id = self.make_node(
                    kind,
                    root_count + linear_count + index,
                    &main_logical,
                    std::slice::from_ref(&main),
                    global_index,
                    force_public,
                )?;
                first_branch.get_or_insert_with(|| id.clone());
                if force_public {
                    self.public_nodes
                        .entry(kind.kind.to_string())
                        .or_default()
                        .push(id.clone());
                }
                ids.push(id);
                global_index += 1;
            }
            for index in 0..merge_count {
                let parents = if let Some(branch) = &first_branch {
                    vec![linear_parent.clone(), branch.clone()]
                } else {
                    vec![main.clone(), linear_parent.clone()]
                };
                ids.push(self.make_node(
                    kind,
                    root_count + linear_count + branch_count + index,
                    &main_logical,
                    &parents,
                    global_index,
                    false,
                )?);
                global_index += 1;
            }
            self.node_ids.insert(kind.kind.to_string(), ids);
        }
        Ok(())
    }

    fn make_node(
        &mut self,
        kind: &KindSpec,
        local_index: usize,
        logical_id: &str,
        parents: &[String],
        global_index: usize,
        force_public: bool,
    ) -> Result<String, GeneratorError> {
        let id = self.next_id("rp/node-revision/v1", kind.prefix)?;
        let mut source = json!({"external_references": [self.reference_ids[0]]});
        if kind.kind == "Measurement" {
            source["artifacts"] = json!([self.artifact_ids[0]]);
        }
        let mut object = json!({
            "schema": "rp/node-revision/v1",
            "id": id,
            "logical_id": logical_id,
            "kind": kind.kind,
            "record_state": "frozen",
            "title": format!("Generated {} {local_index}", kind.kind),
            "statement": format!("Synthetic {} statement {local_index}.", kind.kind),
            "scope": {"statement": "Deterministic scale workload only."},
            "assumptions": [],
            "limitations": ["Synthetic benchmark object."],
            "revision": {
                "parents": parents.iter().map(|parent| json!({"id": parent, "change_type": "clarification"})).collect::<Vec<_>>(),
                "summary": "Generated immutable revision."
            },
            "created_at": timestamp(self.ordinal - 1),
            "created_by": actor(),
            "source": source,
            "access": self.access.next(force_public)?,
            "tags": ["scale-generated"]
        });
        object[kind.payload_key] = node_payload(kind.kind, &self.public_node, &self.reference_ids)?;
        if global_index < 4 {
            let (tag, temporal) = match global_index {
                0 => (
                    "scale-fresh",
                    json!({"reviewed_at": "2026-01-01T00:00:00Z", "review_due_at": "2027-06-01T00:00:00Z"}),
                ),
                1 => (
                    "scale-review-due",
                    json!({"reviewed_at": "2026-01-01T00:00:00Z", "review_due_at": "2027-01-01T00:00:00Z"}),
                ),
                2 => (
                    "scale-stale",
                    json!({"effective_until": "2026-12-31T00:00:00Z", "review_due_at": "2027-06-01T00:00:00Z"}),
                ),
                _ => ("scale-unknown", json!({})),
            };
            object["tags"] = json!([tag]);
            object["temporal"] = temporal;
        }
        let path = canonical_path(kind.kind, kind.role, local_index, &id)?;
        self.write_object(&path, &object)?;
        Ok(id)
    }

    fn write_relations(&mut self, count: usize) -> Result<(), GeneratorError> {
        let active_count = count * 95 / 100;
        let mut active_by_type: BTreeMap<&str, (String, String)> = BTreeMap::new();
        let mut out_degree = BTreeMap::<String, usize>::new();
        for index in 0..count {
            let relation_type = RELATION_TYPES[index % RELATION_TYPES.len()];
            let id = self.next_id("rp/scientific-relation-revision/v1", "rel")?;
            let (from, to) = compatible_pair(
                relation_type,
                index / RELATION_TYPES.len(),
                &self.public_nodes,
            )?;
            let degree = out_degree.entry(from.clone()).or_default();
            *degree += 1;
            if *degree > 64 {
                return Err(GeneratorError(
                    "generated relation out-degree exceeds 64".to_string(),
                ));
            }
            let invalidated = index >= active_count;
            let (logical_id, parents) = if invalidated {
                let (parent_id, parent_logical) =
                    active_by_type.get(relation_type).ok_or_else(|| {
                        GeneratorError("relation type lacks active parent".to_string())
                    })?;
                (
                    parent_logical.clone(),
                    vec![json!({"id": parent_id, "change_type": "evidence_incorporation"})],
                )
            } else {
                (format!("relation-{relation_type}-{index:06}"), Vec::new())
            };
            if !invalidated {
                active_by_type
                    .entry(relation_type)
                    .or_insert_with(|| (id.clone(), logical_id.clone()));
            }
            let object = json!({
                "schema": "rp/scientific-relation-revision/v1",
                "id": id,
                "logical_id": logical_id,
                "record_state": "frozen",
                "type": relation_type,
                "from_revision": from,
                "to_revision": to,
                "scope": {"statement": "Synthetic compatible scale relation.", "conditions": [], "exclusions": []},
                "assertion_mode": "asserted",
                "relation_state": if invalidated {"invalidated"} else {"active"},
                "rationale": "Exercise deterministic relation graph validation.",
                "revision": {"parents": parents, "summary": "Generated relation revision."},
                "created_at": timestamp(self.ordinal - 1),
                "created_by": actor(),
                "source": {"external_references": [self.reference_ids[0]]},
                "access": self.access.next(index < RELATION_TYPES.len())?
            });
            self.write_object(
                &format!(".research/relations/relation-{index:06}--{id}.yaml"),
                &object,
            )?;
            self.relation_ids
                .entry(relation_type.to_string())
                .or_default()
                .push(id);
            *self
                .relation_type_counts
                .entry(relation_type.to_string())
                .or_default() += 1;
        }
        Ok(())
    }

    fn write_assessments(&mut self, count: usize) -> Result<(), GeneratorError> {
        let node_targets = count * 80 / 100;
        let node = self.public_node["Hypothesis"].clone();
        let relation = self.relation_ids["supports"][0].clone();
        let statuses = ["proposed", "supported", "challenged", "inconclusive"];
        for index in 0..count {
            let id = self.next_id("rp/assessment/v1", "asm")?;
            let target_node = index < node_targets;
            let object = json!({
                "schema": "rp/assessment/v1",
                "id": id,
                "target": {"type": if target_node {"node_revision"} else {"relation_revision"}, "id": if target_node {&node} else {&relation}},
                "assessment_scope": "generated benchmark assessment",
                "epistemic_status": statuses[index % statuses.len()],
                "evidence_evaluation": {
                    "evidence_quality": "medium", "agreement": "mixed", "independence": "partially_independent",
                    "directness": "direct", "decision_confidence": "medium",
                    "rationale": "Synthetic assessment for deterministic workload.", "limitations": []
                },
                "quantitative_assessment": Value::Null,
                "assessed_at": timestamp(self.ordinal - 1),
                "assessed_by": actor(),
                "source": {"external_references": [self.reference_ids[0]]},
                "supersedes_assessments": [],
                "review_assurance": "machine-validated",
                "access": self.access.next(false)?
            });
            self.write_object(
                &format!(".research/assessments/assessment-{index:06}--{id}.yaml"),
                &object,
            )?;
        }
        Ok(())
    }

    fn write_threads(&mut self, count: usize) -> Result<(), GeneratorError> {
        let root_question = self.public_node["Question"].clone();
        for index in 0..count {
            let id = self.next_id("rp/research-thread/v1", "thd")?;
            let parent = (index > 0).then(|| self.thread_ids[0].clone());
            let object = json!({
                "schema": "rp/research-thread/v1",
                "id": id,
                "title": format!("Generated thread {index}"),
                "objective": "Exercise deterministic contextual research navigation.",
                "root_question_revision": root_question,
                "parent_thread_id": parent,
                "forked_from_thread_id": if index > 0 && index % 2 == 0 {Some(self.thread_ids[0].clone())} else {None},
                "fork_reason": if index > 0 {Some("Synthetic nested branch.")} else {None},
                "created_at": timestamp(self.ordinal - 1),
                "created_by": actor(),
                "access": self.access.next(index < 2)?
            });
            self.write_object(
                &format!(".research/threads/thread-{index:06}--{id}.yaml"),
                &object,
            )?;
            self.thread_ids.push(id);
        }
        Ok(())
    }

    fn write_bindings(&mut self, count: usize) -> Result<(), GeneratorError> {
        let public_targets: Vec<_> = KINDS
            .iter()
            .map(|kind| self.public_node[kind.kind].clone())
            .collect();
        for index in 0..count {
            let id = self.next_id("rp/thread-binding/v1", "tbd")?;
            let thread_id = if index == 1 && self.thread_ids.len() > 1 {
                &self.thread_ids[1]
            } else {
                &self.thread_ids[0]
            };
            let target = &public_targets[index % public_targets.len()];
            let object = json!({
                "schema": "rp/thread-binding/v1",
                "id": id,
                "logical_id": format!("binding-{index:06}"),
                "record_state": "frozen",
                "thread_id": thread_id,
                "target": {"type": "node_revision", "id": target},
                "role": BINDING_ROLES[index % BINDING_ROLES.len()],
                "rationale": "Synthetic contextual binding.",
                "revision": {"parents": [], "summary": "Generated binding revision."},
                "created_at": timestamp(self.ordinal - 1),
                "created_by": actor(),
                "access": self.access.next(false)?
            });
            self.write_object(
                &format!(".research/thread-bindings/binding-{index:06}--{id}.yaml"),
                &object,
            )?;
        }
        Ok(())
    }

    fn write_claim_chains(&mut self, count: usize) -> Result<(), GeneratorError> {
        let profiles = largest_remainder(
            count,
            &BTreeMap::from([
                ("confirmatory-v1".to_string(), 15),
                ("evidential-v1".to_string(), 35),
                ("minimum-v1".to_string(), 50),
            ]),
        )?;
        let mut profile_sequence = Vec::new();
        for (profile, count) in profiles {
            profile_sequence.extend(std::iter::repeat_n(profile, count));
        }
        profile_sequence.sort();
        for (index, profile) in profile_sequence.into_iter().enumerate() {
            let id = self.next_id("rp/claim-chain-snapshot/v1", "cch")?;
            let (nodes, relations, target, execution) = self.claim_selection(&profile);
            let object = json!({
                "schema": "rp/claim-chain-snapshot/v1",
                "id": id,
                "thread_id": self.thread_ids[0],
                "title": format!("Generated {profile} chain {index}"),
                "root_node_revisions": [self.public_node["Question"]],
                "target_node_revision": target,
                "validation_policy": {"profile": profile, "version": 1, "execution_provenance": execution},
                "node_revisions": nodes,
                "relation_revisions": relations,
                "source_closure_sha256": format!("sha256:{}", "0".repeat(64)),
                "validation_report": Value::Null,
                "created_at": timestamp(self.ordinal - 1),
                "created_by": actor(),
                "access": self.access.next(false)?
            });
            let relative = format!(".research/claim-chains/chain-{index:06}--{id}.yaml");
            self.write_object(&relative, &object)?;
            self.claim_paths.push(self.options.output.join(relative));
        }
        Ok(())
    }

    fn claim_selection(&self, profile: &str) -> (Vec<String>, Vec<String>, String, &'static str) {
        let question = self.public_node["Question"].clone();
        let hypothesis = self.public_node["Hypothesis"].clone();
        let prediction = self.public_node["Prediction"].clone();
        let test = self.public_node["Test"].clone();
        let measurement = self.public_node["Measurement"].clone();
        let conclusion = self.public_node["Conclusion"].clone();
        match profile {
            "minimum-v1" => (
                vec![question, hypothesis, prediction, test],
                vec![
                    self.relation_ids["motivates"][0].clone(),
                    self.relation_ids["predicts"][0].clone(),
                    self.relation_ids["tested-by"][0].clone(),
                ],
                self.public_node["Test"].clone(),
                "none",
            ),
            "evidential-v1" => (
                vec![question, hypothesis, measurement, conclusion.clone()],
                vec![
                    self.relation_ids["motivates"][0].clone(),
                    self.relation_ids["depends-on"][0].clone(),
                    self.relation_ids["supports"][0].clone(),
                ],
                conclusion,
                "none",
            ),
            _ => (
                vec![
                    question,
                    hypothesis,
                    prediction,
                    test,
                    measurement,
                    conclusion.clone(),
                ],
                vec![
                    self.relation_ids["motivates"][0].clone(),
                    self.relation_ids["predicts"][0].clone(),
                    self.relation_ids["tested-by"][0].clone(),
                    self.relation_ids["result-of"][0].clone(),
                    self.relation_ids["supports"][0].clone(),
                ],
                conclusion,
                "artifact-backed",
            ),
        }
    }

    fn write_freshness_policy(&self) -> Result<(), GeneratorError> {
        let policy = b"schema: rp/freshness-policy/v1\nversion: 1\ndefault_status: unknown\nrules:\n- id: scale-fresh\n  selector: {tags_any: [scale-fresh]}\n  required_checks: [review-deadline]\n  invalidates_on_change: false\n- id: scale-review-due\n  selector: {tags_any: [scale-review-due]}\n  required_checks: [review-deadline]\n  invalidates_on_change: false\n- id: scale-stale\n  selector: {tags_any: [scale-stale]}\n  required_checks: [review-deadline]\n  invalidates_on_change: false\n- id: scale-unknown\n  selector: {tags_any: [scale-unknown]}\n  required_checks: [review-deadline]\n  invalidates_on_change: false\n";
        self.write_bytes(".research/policies/freshness-v1.yaml", policy)
    }

    fn recompute_claim_digests(&self) -> Result<(), GeneratorError> {
        let mut limits = ProjectLimits::default();
        if self.options.profile == Profile::Stress {
            limits.canonical_files = 500_000;
            limits.whole_project_yaml_bytes = 2_147_483_648;
            limits.graph_nodes = 500_000;
            limits.graph_edges = 8_000_000;
            limits.traversal_depth = 65_536;
        }
        let report = validate_project_with_limits(&self.options.output, limits)
            .map_err(|error| GeneratorError(format!("preliminary validation failed: {error}")))?;
        let index = report.index.ok_or_else(|| {
            GeneratorError(format!(
                "preliminary validation did not index project: {:?}",
                report.findings
            ))
        })?;
        for path in &self.claim_paths {
            let bytes = fs::read(path).map_err(|error| GeneratorError(error.to_string()))?;
            let parsed = rp_core::parse_restricted_yaml(
                &bytes,
                ProjectPath::new("claim.yaml").expect("static path"),
                &rp_core::YamlLimits::default(),
            )
            .map_err(|finding| GeneratorError(finding.error_code.to_string()))?;
            let mut value = parsed.value;
            let id = value["id"].as_str().expect("generated claim id");
            let digest = index
                .claim_chain_report(id)
                .ok_or_else(|| GeneratorError("generated claim report missing".to_string()))?
                .source_sha256
                .to_string();
            value["source_closure_sha256"] = Value::String(digest);
            fs::write(path, serialize_object(&value)?)
                .map_err(|error| GeneratorError(error.to_string()))?;
        }
        Ok(())
    }

    fn self_validate(&self) -> Result<(), GeneratorError> {
        let mut limits = ProjectLimits::default();
        if self.options.profile == Profile::Stress {
            limits.canonical_files = 500_000;
            limits.whole_project_yaml_bytes = 2_147_483_648;
            limits.graph_nodes = 500_000;
            limits.graph_edges = 8_000_000;
            limits.traversal_depth = 65_536;
        }
        let report = validate_project_with_limits(&self.options.output, limits)
            .map_err(|error| GeneratorError(format!("self-validation failed: {error}")))?;
        if !report.findings.is_empty() {
            return Err(GeneratorError(format!(
                "generated project is invalid: {:?}",
                report.findings
            )));
        }
        Ok(())
    }

    fn write_object(&self, relative: &str, value: &Value) -> Result<(), GeneratorError> {
        self.write_bytes(relative, &serialize_object(value)?)
    }

    fn write_bytes(&self, relative: &str, bytes: &[u8]) -> Result<(), GeneratorError> {
        let path = self.options.output.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| GeneratorError(error.to_string()))?;
        }
        if path.exists() {
            return Err(GeneratorError(format!(
                "generated path collision: {relative}"
            )));
        }
        fs::write(path, bytes).map_err(|error| GeneratorError(error.to_string()))
    }
}

struct AccessAllocator {
    labels: Vec<String>,
    cursor: usize,
    counts: BTreeMap<String, usize>,
}

impl AccessAllocator {
    fn new(total: usize, seed: u64) -> Result<Self, GeneratorError> {
        let weights = ACCESS_WEIGHTS
            .iter()
            .map(|(level, weight)| ((*level).to_string(), *weight))
            .collect();
        let counts = largest_remainder(total, &weights)?;
        let mut labels = Vec::with_capacity(total);
        for (level, count) in &counts {
            labels.extend(std::iter::repeat_n(level.clone(), *count));
        }
        let mut stream = CounterStream::new(seed, "access-labels")?;
        for index in (1..labels.len()).rev() {
            let target = usize::try_from(bounded_integer(&mut stream, 0, (index + 1) as u64)?)
                .expect("bounded index fits usize");
            labels.swap(index, target);
        }
        Ok(Self {
            labels,
            cursor: 0,
            counts,
        })
    }

    fn next(&mut self, force_public: bool) -> Result<Value, GeneratorError> {
        if force_public
            && self
                .labels
                .get(self.cursor)
                .is_some_and(|level| level != "public")
        {
            let offset = self.labels[self.cursor..]
                .iter()
                .position(|level| level == "public")
                .ok_or_else(|| GeneratorError("public access quota exhausted".to_string()))?;
            self.labels.swap(self.cursor, self.cursor + offset);
        }
        let level = self
            .labels
            .get(self.cursor)
            .ok_or_else(|| GeneratorError("access allocation exhausted".to_string()))?
            .clone();
        let compartment = COMPARTMENTS[self.cursor % COMPARTMENTS.len()];
        self.cursor += 1;
        Ok(json!({
            "level": level,
            "compartments": if matches!(level.as_str(), "restricted" | "exclusive") {vec![compartment]} else {Vec::<&str>::new()}
        }))
    }
}

#[derive(Clone, Copy)]
struct KindSpec {
    kind: &'static str,
    prefix: &'static str,
    directory: &'static str,
    role: &'static str,
    payload_key: &'static str,
    weight: u64,
}

impl KindSpec {
    const fn new(
        kind: &'static str,
        prefix: &'static str,
        directory: &'static str,
        role: &'static str,
        payload_key: &'static str,
        weight: u64,
    ) -> Self {
        Self {
            kind,
            prefix,
            directory,
            role,
            payload_key,
            weight,
        }
    }
}

fn node_payload(
    kind: &str,
    public: &BTreeMap<String, String>,
    references: &[String],
) -> Result<Value, GeneratorError> {
    let reference = references
        .first()
        .ok_or_else(|| GeneratorError("reference pool is empty".to_string()))?;
    Ok(match kind {
        "Dataset" => {
            json!({"version": "v1", "selection": {"description": "Synthetic selection.", "criteria": []}, "variables": [{"name": "value", "role": "measure", "unit": "unit"}]})
        }
        "Question" => {
            json!({"resolution_criteria": ["Synthetic criterion."], "evidence_requirements": ["Synthetic evidence."]})
        }
        "Hypothesis" => {
            json!({"falsification_criteria": ["Synthetic falsification."], "revision_criteria": ["Synthetic revision."]})
        }
        "Observation" => {
            json!({"context": "Synthetic observation.", "method": {"description": "Generated method."}, "reproducibility_criteria": ["Repeat deterministically."]})
        }
        "Measurement" => {
            json!({"quantity": "generated-value", "value": "1", "unit": "unit", "uncertainty": {"kind": "not-reported"}, "method": {"description": "Generated measurement.", "protocol_reference": reference}})
        }
        "Method" => {
            json!({"version": "v1", "procedure": "Execute deterministic procedure.", "applicable_inputs": ["synthetic input"], "protocol_references": [reference]})
        }
        "Test" => {
            json!({"target_revision_ids": [public.get("Hypothesis").ok_or_else(|| GeneratorError("Hypothesis dependency missing".to_string()))?], "method_revision": public.get("Method").ok_or_else(|| GeneratorError("Method dependency missing".to_string()))?, "evaluation_criteria": ["Generated criterion."]})
        }
        "Prediction" => {
            json!({"expected_outcome": "Generated outcome.", "test_conditions": ["Generated condition."], "acceptance_criteria": ["Generated acceptance."]})
        }
        "Synthesis" => {
            json!({"combination_method": "Deterministic combination.", "rationale": "Synthetic synthesis."})
        }
        "Interpretation" => json!({"alternatives_considered": [], "unresolved_alternatives": []}),
        "Decision" => {
            json!({"selected_option": "generated option", "considered_alternatives": ["alternative"], "rationale": "Synthetic decision.", "reversibility": "reversible", "revisit_triggers": ["new evidence"]})
        }
        "Conclusion" => {
            json!({"residual_uncertainty": ["Synthetic uncertainty."], "use_limitations": ["Benchmark only."]})
        }
        "Blocker" => {
            json!({"blocked_target": "generated work", "blocking_condition": "Synthetic condition.", "evidence_summary": "Synthetic evidence.", "unblock_criteria": ["Resolve deterministically."]})
        }
        "NextAction" => {
            json!({"intended_outcome": "Generated follow-up.", "dependencies": [], "owner": Value::Null, "completion_criteria": ["Generated completion."]})
        }
        "PaperClaim" => {
            json!({"publication_reference": reference, "locator": {"section": "generated"}})
        }
        _ => return Err(GeneratorError("unknown kind payload".to_string())),
    })
}

fn compatible_pair(
    relation_type: &str,
    index: usize,
    nodes: &BTreeMap<String, Vec<String>>,
) -> Result<(String, String), GeneratorError> {
    let pair = match relation_type {
        "derived-from" => ("Question", "Dataset"),
        "has-part" => ("Dataset", "Measurement"),
        "input-to" => ("Measurement", "Synthesis"),
        "generated" => ("Synthesis", "Conclusion"),
        "cites" => ("Question", "PaperClaim"),
        "implements" => ("Method", "Test"),
        "supports" | "weakens" | "contradicts" | "consistent-with" => ("Measurement", "Conclusion"),
        "provides-prior" => ("Dataset", "Hypothesis"),
        "motivates" => ("Question", "Hypothesis"),
        "predicts" => ("Hypothesis", "Prediction"),
        "tested-by" => ("Prediction", "Test"),
        "result-of" => ("Measurement", "Test"),
        "observed-in" => ("Measurement", "Dataset"),
        "reproduces" | "fails-to-reproduce" => ("Measurement", "Observation"),
        "validated-by" => ("Dataset", "Method"),
        "requires" => ("Method", "Dataset"),
        "blocked-by" => ("NextAction", "Blocker"),
        "enables" => ("Dataset", "NextAction"),
        "next-step" => ("Question", "NextAction"),
        "depends-on" => ("Hypothesis", "Measurement"),
        _ => return Err(GeneratorError("unknown relation type".to_string())),
    };
    let from = &nodes[pair.0];
    let to = &nodes[pair.1];
    Ok((
        from[index % from.len()].clone(),
        to[index % to.len()].clone(),
    ))
}

fn actor() -> Value {
    json!({"type": "software", "id": "rp-scale-generator"})
}

fn timestamp(ordinal: u64) -> String {
    let total_ms = ordinal;
    let minutes = total_ms / 60_000;
    let seconds = (total_ms / 1_000) % 60;
    let milliseconds = total_ms % 1_000;
    format!("2026-01-01T00:{minutes:02}:{seconds:02}.{milliseconds:03}Z")
}

fn encode_ulid(timestamp_ms: u64, randomness: [u8; 10]) -> String {
    let mut bytes = [0_u8; 16];
    bytes[..6].copy_from_slice(&timestamp_ms.to_be_bytes()[2..]);
    bytes[6..].copy_from_slice(&randomness);
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut output = String::with_capacity(26);
    for group in 0..26 {
        let mut value = 0_u8;
        for offset in 0..5 {
            let bit = group * 5 + offset;
            value <<= 1;
            if bit >= 2 {
                let source = bit - 2;
                value |= (bytes[source / 8] >> (7 - source % 8)) & 1;
            }
        }
        output.push(char::from(ALPHABET[usize::from(value)]));
    }
    output
}

fn prepare_output(output: &Path) -> Result<(), GeneratorError> {
    if output.exists() {
        if fs::read_dir(output)
            .map_err(|error| GeneratorError(error.to_string()))?
            .next()
            .is_some()
        {
            return Err(GeneratorError("output directory is not empty".to_string()));
        }
    } else {
        fs::create_dir(output).map_err(|error| GeneratorError(error.to_string()))?;
    }
    Ok(())
}

fn digest_tree(root: &Path) -> Result<Vec<PathDigestEntry>, GeneratorError> {
    let mut paths = Vec::new();
    collect_paths(root, root, &mut paths)?;
    paths.sort();
    paths
        .into_iter()
        .filter(|path| path != "scale-manifest.json")
        .map(|path| {
            let bytes =
                fs::read(root.join(&path)).map_err(|error| GeneratorError(error.to_string()))?;
            Ok(PathDigestEntry {
                path,
                sha256: sha256(&bytes),
            })
        })
        .collect()
}

fn collect_paths(
    root: &Path,
    directory: &Path,
    output: &mut Vec<String>,
) -> Result<(), GeneratorError> {
    for entry in fs::read_dir(directory).map_err(|error| GeneratorError(error.to_string()))? {
        let entry = entry.map_err(|error| GeneratorError(error.to_string()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_paths(root, &path, output)?;
        } else if path.is_file() {
            output.push(
                path.strip_prefix(root)
                    .expect("collected path is beneath root")
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}

fn serialize_object(value: &Value) -> Result<Vec<u8>, GeneratorError> {
    let object = value
        .as_object()
        .ok_or_else(|| GeneratorError("canonical object is not a mapping".to_string()))?;
    let schema = object
        .get("schema")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut order: Vec<&str> = presentation_order(schema).to_vec();
    if schema == "rp/node-revision/v1"
        && let Some(kind) = object.get("kind").and_then(Value::as_str)
        && let Some(spec) = KINDS.iter().find(|spec| spec.kind == kind)
    {
        let position = order
            .iter()
            .position(|key| *key == "$kind_payload")
            .unwrap();
        order[position] = spec.payload_key;
    }
    let mut keys: Vec<_> = object.keys().map(String::as_str).collect();
    keys.sort();
    for key in keys {
        if !order.contains(&key) {
            order.push(key);
        }
    }
    let mut output = String::from("{\n");
    let selected: Vec<_> = order
        .into_iter()
        .filter_map(|key| object.get(key).map(|value| (key, value)))
        .collect();
    for (index, (key, value)) in selected.iter().enumerate() {
        let rendered = serde_json::to_string_pretty(value)
            .map_err(|error| GeneratorError(error.to_string()))?
            .replace('\n', "\n  ");
        output.push_str("  ");
        output.push_str(&serde_json::to_string(key).expect("key serializes"));
        output.push_str(": ");
        output.push_str(&rendered);
        if index + 1 != selected.len() {
            output.push(',');
        }
        output.push('\n');
    }
    output.push_str("}\n");
    Ok(output.into_bytes())
}

fn presentation_order(schema: &str) -> &'static [&'static str] {
    match schema {
        "rp/project/v1" => &[
            "schema",
            "id",
            "slug",
            "title",
            "created_at",
            "schema_policy",
            "repository_policy",
            "access_defaults",
            "redaction_policy",
            "validation_policy",
            "extensions",
        ],
        "rp/node-revision/v1" => &[
            "schema",
            "id",
            "logical_id",
            "kind",
            "record_state",
            "title",
            "statement",
            "scope",
            "$kind_payload",
            "assumptions",
            "limitations",
            "revision",
            "created_at",
            "created_by",
            "temporal",
            "source",
            "access",
            "tags",
            "narrative",
            "extensions",
        ],
        "rp/assessment/v1" => &[
            "schema",
            "id",
            "target",
            "assessment_scope",
            "epistemic_status",
            "evidence_evaluation",
            "quantitative_assessment",
            "assessed_at",
            "assessed_by",
            "source",
            "supersedes_assessments",
            "review_assurance",
            "access",
            "extensions",
        ],
        "rp/scientific-relation-revision/v1" => &[
            "schema",
            "id",
            "logical_id",
            "record_state",
            "type",
            "from_revision",
            "to_revision",
            "scope",
            "assertion_mode",
            "relation_state",
            "rationale",
            "revision",
            "created_at",
            "created_by",
            "source",
            "access",
            "extensions",
        ],
        "rp/research-thread/v1" => &[
            "schema",
            "id",
            "title",
            "objective",
            "root_question_revision",
            "parent_thread_id",
            "forked_from_thread_id",
            "fork_reason",
            "created_at",
            "created_by",
            "access",
            "extensions",
        ],
        "rp/thread-binding/v1" => &[
            "schema",
            "id",
            "logical_id",
            "record_state",
            "thread_id",
            "target",
            "role",
            "rationale",
            "revision",
            "created_at",
            "created_by",
            "access",
            "extensions",
        ],
        "rp/claim-chain-snapshot/v1" => &[
            "schema",
            "id",
            "thread_id",
            "title",
            "root_node_revisions",
            "target_node_revision",
            "validation_policy",
            "node_revisions",
            "relation_revisions",
            "source_closure_sha256",
            "validation_report",
            "created_at",
            "created_by",
            "access",
            "extensions",
        ],
        "rp/external-reference/v1" => &[
            "schema",
            "id",
            "type",
            "canonical_uri",
            "foreign_key",
            "display_label",
            "retrieved_at",
            "source_version",
            "trust_tags",
            "access",
            "extensions",
        ],
        "rp/artifact-manifest/v1" => &[
            "schema",
            "id",
            "title",
            "uri",
            "media_type",
            "size_bytes",
            "sha256",
            "created_at",
            "access",
            "extensions",
        ],
        "rp/research-run/v1" => &[
            "schema",
            "id",
            "project_id",
            "title",
            "objective",
            "created_at",
            "created_by",
            "parent_run_id",
            "planned_inputs",
            "planned_outputs",
            "limitations",
            "access",
            "tags",
            "extensions",
        ],
        _ => &[],
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
