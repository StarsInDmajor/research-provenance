use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Value, json};

use crate::{ObjectRecord, ObjectType, ProjectIndex};

#[cfg(test)]
#[path = "navigation_execution_tests.rs"]
mod execution_review_tests;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FreshnessStatus {
    Unknown,
    Fresh,
    ReviewDue,
    Stale,
}

impl FreshnessStatus {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "unknown" => Some(Self::Unknown),
            "fresh" => Some(Self::Fresh),
            "review-due" => Some(Self::ReviewDue),
            "stale" => Some(Self::Stale),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Fresh => "fresh",
            Self::ReviewDue => "review-due",
            Self::Stale => "stale",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FreshnessEvaluation {
    pub status: FreshnessStatus,
    pub applicable_rules: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct FreshnessCounts {
    pub unknown: usize,
    pub fresh: usize,
    #[serde(rename = "review-due")]
    pub review_due: usize,
    pub stale: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OverviewDto {
    pub object_counts: BTreeMap<String, usize>,
    pub unresolved_forks: Vec<String>,
    pub blockers: Vec<String>,
    pub next_actions: Vec<String>,
    pub freshness_counts: FreshnessCounts,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ShowDto {
    pub object: Value,
    pub derived: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HistoryDto {
    pub logical_id: String,
    pub revisions: Vec<Value>,
    pub heads: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DiffChange {
    pub path: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DiffDto {
    pub revision_id_a: String,
    pub revision_id_b: String,
    pub changes: Vec<DiffChange>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryOptions {
    pub kind: Option<String>,
    pub thread_id: Option<String>,
    pub freshness: Option<FreshnessStatus>,
    pub as_of: String,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct QueryDto {
    pub rows: Vec<Value>,
    pub returned_count: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ThreadListDto {
    pub threads: Vec<Value>,
    pub returned_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ThreadShowDto {
    pub thread: Value,
    pub bindings: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SnapshotDto {
    pub project: Value,
    pub threads: Vec<Value>,
    pub objects: BTreeMap<String, Value>,
    pub heads: BTreeMap<String, Vec<String>>,
    pub relation_heads: Vec<String>,
    pub assessments: BTreeMap<String, Vec<String>>,
    pub thread_bindings: BTreeMap<String, Vec<String>>,
    pub freshness: BTreeMap<String, FreshnessStatus>,
    pub canonical_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OversizedReason {
    SemanticNodes { count: usize, limit: usize },
    ScientificRelations { count: usize, limit: usize },
    CanonicalRecords { count: usize, limit: usize },
    CanonicalBytes { bytes: usize, limit: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NavigationError {
    Stopped(crate::ExecutionStop),
    NotFound,
    InvalidAsOf,
    InvalidLimit,
    OversizedGraph(OversizedReason),
    SerializationError(String),
}

impl ProjectIndex {
    fn navigation_checkpoint(&self) -> Result<(), NavigationError> {
        self.execution
            .checkpoint()
            .map_err(NavigationError::Stopped)
    }
    pub fn freshness(&self, id: &str, as_of: &str) -> Result<FreshnessEvaluation, NavigationError> {
        self.navigation_checkpoint()?;
        let as_of = as_of
            .parse::<jiff::Timestamp>()
            .map_err(|_| NavigationError::InvalidAsOf)?;
        let object = self.get(id).ok_or(NavigationError::NotFound)?;
        let result = self.freshness_at(object, as_of);
        self.navigation_checkpoint()?;
        Ok(result)
    }

    pub fn overview(&self, as_of: &str) -> Result<OverviewDto, NavigationError> {
        self.navigation_checkpoint()?;
        let as_of = as_of
            .parse::<jiff::Timestamp>()
            .map_err(|_| NavigationError::InvalidAsOf)?;
        let mut object_counts = BTreeMap::new();
        for object in self.object_values() {
            self.navigation_checkpoint()?;
            *object_counts
                .entry(object_count_key(&object.object_type).to_string())
                .or_default() += 1;
        }
        let mut unresolved_forks: Vec<_> = self
            .lineage_heads()
            .filter(|(_, heads)| heads.len() > 1)
            .map(|(logical_id, _)| logical_id.to_string())
            .collect();
        unresolved_forks.sort();
        let blockers = self.current_node_ids("Blocker");
        let next_actions = self.current_node_ids("NextAction");
        let mut freshness_counts = FreshnessCounts::default();
        for object in self.object_values().filter(|object| {
            matches!(
                object.object_type,
                ObjectType::Node(_)
                    | ObjectType::Relation
                    | ObjectType::Assessment
                    | ObjectType::ThreadBinding
            )
        }) {
            self.navigation_checkpoint()?;
            match self.freshness_at(object, as_of).status {
                FreshnessStatus::Unknown => freshness_counts.unknown += 1,
                FreshnessStatus::Fresh => freshness_counts.fresh += 1,
                FreshnessStatus::ReviewDue => freshness_counts.review_due += 1,
                FreshnessStatus::Stale => freshness_counts.stale += 1,
            }
        }
        self.navigation_checkpoint()?;
        Ok(OverviewDto {
            object_counts,
            unresolved_forks,
            blockers,
            next_actions,
            freshness_counts,
        })
    }

    pub fn show(&self, id: &str, as_of: &str) -> Result<ShowDto, NavigationError> {
        self.navigation_checkpoint()?;
        let object = self.get(id).ok_or(NavigationError::NotFound)?;
        let freshness = self.freshness(id, as_of)?;
        let heads = object
            .logical_id
            .as_deref()
            .map_or_else(Vec::new, |logical_id| {
                self.heads_for(logical_id)
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            });
        let assessment_ids: Vec<_> = self
            .object_values()
            .take_while(|_| self.execution.checkpoint().is_ok())
            .filter(|candidate| matches!(candidate.object_type, ObjectType::Assessment))
            .filter(|candidate| {
                candidate
                    .value
                    .pointer("/target/id")
                    .and_then(Value::as_str)
                    == Some(id)
            })
            .map(|candidate| candidate.id.to_string())
            .collect();
        let binding_ids: Vec<_> = self
            .object_values()
            .take_while(|_| self.execution.checkpoint().is_ok())
            .filter(|candidate| matches!(candidate.object_type, ObjectType::ThreadBinding))
            .filter(|candidate| {
                candidate
                    .value
                    .pointer("/target/id")
                    .and_then(Value::as_str)
                    == Some(id)
            })
            .map(|candidate| candidate.id.to_string())
            .collect();
        let access = self.access_report(id);
        self.navigation_checkpoint()?;
        let dto = ShowDto {
            object: object.value.clone(),
            derived: json!({
                "is_head": heads.iter().any(|head| head == id),
                "lineage_heads": heads,
                "freshness": freshness.status,
                "assessment_ids": assessment_ids,
                "thread_binding_ids": binding_ids,
                "effective_access": access.map(|value| value.effective),
            }),
        };
        self.navigation_checkpoint()?;
        Ok(dto)
    }

    pub fn history(&self, logical_id: &str) -> Result<HistoryDto, NavigationError> {
        self.navigation_checkpoint()?;
        let mut revisions: Vec<_> = self
            .object_values()
            .take_while(|_| self.execution.checkpoint().is_ok())
            .filter(|object| object.logical_id.as_deref() == Some(logical_id))
            .map(|object| object.value.clone())
            .collect();
        self.navigation_checkpoint()?;
        if revisions.is_empty() {
            return Err(NavigationError::NotFound);
        }
        revisions.sort_by(|left, right| {
            let left_time = left.get("created_at").and_then(Value::as_str).unwrap_or("");
            let right_time = right
                .get("created_at")
                .and_then(Value::as_str)
                .unwrap_or("");
            (
                left_time,
                left.get("id").and_then(Value::as_str).unwrap_or(""),
            )
                .cmp(&(
                    right_time,
                    right.get("id").and_then(Value::as_str).unwrap_or(""),
                ))
        });
        let heads = self
            .heads_for(logical_id)
            .into_iter()
            .map(str::to_string)
            .collect();
        self.navigation_checkpoint()?;
        Ok(HistoryDto {
            logical_id: logical_id.to_string(),
            revisions,
            heads,
        })
    }

    pub fn diff(&self, id_a: &str, id_b: &str) -> Result<DiffDto, NavigationError> {
        self.navigation_checkpoint()?;
        let left = self.get(id_a).ok_or(NavigationError::NotFound)?;
        let right = self.get(id_b).ok_or(NavigationError::NotFound)?;
        let mut changes = Vec::new();
        collect_diff(
            "",
            Some(&left.value),
            Some(&right.value),
            &mut changes,
            &self.execution,
        )?;
        self.navigation_checkpoint()?;
        Ok(DiffDto {
            revision_id_a: id_a.to_string(),
            revision_id_b: id_b.to_string(),
            changes,
        })
    }

    pub fn query(&self, options: QueryOptions) -> Result<QueryDto, NavigationError> {
        self.execution
            .checkpoint()
            .map_err(NavigationError::Stopped)?;
        if options.limit == 0 || options.limit > 100_000 {
            return Err(NavigationError::InvalidLimit);
        }
        let as_of = options
            .as_of
            .parse::<jiff::Timestamp>()
            .map_err(|_| NavigationError::InvalidAsOf)?;
        let thread_targets: Option<BTreeSet<&str>> =
            options.thread_id.as_deref().map(|thread_id| {
                self.object_values()
                    .take_while(|_| self.execution.checkpoint().is_ok())
                    .filter(|object| matches!(object.object_type, ObjectType::ThreadBinding))
                    .filter(|object| {
                        object.value.get("thread_id").and_then(Value::as_str) == Some(thread_id)
                    })
                    .filter_map(|object| object.value.pointer("/target/id").and_then(Value::as_str))
                    .collect()
            });
        let mut matching = Vec::new();
        for object in self.object_values() {
            self.execution
                .checkpoint()
                .map_err(NavigationError::Stopped)?;
            if options.kind.as_deref().is_some_and(|kind| {
                !matches!(&object.object_type, ObjectType::Node(actual) if actual.as_ref() == kind)
            }) {
                continue;
            }
            if thread_targets
                .as_ref()
                .is_some_and(|targets| !targets.contains(object.id.as_ref()))
            {
                continue;
            }
            let freshness = self.freshness_at(object, as_of).status;
            if options
                .freshness
                .is_some_and(|expected| freshness != expected)
            {
                continue;
            }
            matching.push(query_row(object, freshness));
        }
        self.execution
            .checkpoint()
            .map_err(NavigationError::Stopped)?;
        let truncated = matching.len() > options.limit;
        matching.truncate(options.limit);
        Ok(QueryDto {
            returned_count: matching.len(),
            rows: matching,
            truncated,
        })
    }

    #[must_use]
    pub fn thread_list(&self, limit: usize) -> ThreadListDto {
        // Legacy infallible API; CLI uses the typed variant below.
        self.thread_list_checked(limit).unwrap_or(ThreadListDto {
            threads: Vec::new(),
            returned_count: 0,
        })
    }
    pub fn thread_list_checked(&self, limit: usize) -> Result<ThreadListDto, NavigationError> {
        self.navigation_checkpoint()?;
        let mut threads = Vec::new();
        for thread in self
            .object_values()
            .filter(|object| matches!(object.object_type, ObjectType::Thread))
            .take(limit)
        {
            self.navigation_checkpoint()?;
            let binding_count = self
                .object_values()
                .take_while(|_| self.execution.checkpoint().is_ok())
                .filter(|object| matches!(object.object_type, ObjectType::ThreadBinding))
                .filter(|object| {
                    object.value.get("thread_id").and_then(Value::as_str)
                        == Some(thread.id.as_ref())
                })
                .count();
            let child_count = self
                .object_values()
                .take_while(|_| self.execution.checkpoint().is_ok())
                .filter(|object| matches!(object.object_type, ObjectType::Thread))
                .filter(|object| {
                    object.value.get("parent_thread_id").and_then(Value::as_str)
                        == Some(thread.id.as_ref())
                })
                .count();
            threads.push(json!({
                "id": thread.id,
                "title": thread.value.get("title"),
                "status": thread.value.get("status"),
                "binding_count": binding_count,
                "child_count": child_count,
            }));
        }
        self.navigation_checkpoint()?;
        Ok(ThreadListDto {
            returned_count: threads.len(),
            threads,
        })
    }

    pub fn thread_show(&self, id: &str, as_of: &str) -> Result<ThreadShowDto, NavigationError> {
        self.navigation_checkpoint()?;
        let _ = as_of
            .parse::<jiff::Timestamp>()
            .map_err(|_| NavigationError::InvalidAsOf)?;
        let thread = self.get(id).ok_or(NavigationError::NotFound)?;
        if !matches!(thread.object_type, ObjectType::Thread) {
            return Err(NavigationError::NotFound);
        }
        let mut bindings: Vec<_> = self
            .object_values()
            .take_while(|_| self.execution.checkpoint().is_ok())
            .filter(|object| matches!(object.object_type, ObjectType::ThreadBinding))
            .filter(|object| object.value.get("thread_id").and_then(Value::as_str) == Some(id))
            .map(|object| object.value.clone())
            .collect();
        bindings.sort_by(|left, right| {
            (
                left.pointer("/target/id")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                left.get("role").and_then(Value::as_str).unwrap_or(""),
                left.get("id").and_then(Value::as_str).unwrap_or(""),
            )
                .cmp(&(
                    right
                        .pointer("/target/id")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                    right.get("role").and_then(Value::as_str).unwrap_or(""),
                    right.get("id").and_then(Value::as_str).unwrap_or(""),
                ))
        });
        self.navigation_checkpoint()?;
        let dto = ThreadShowDto {
            thread: thread.value.clone(),
            bindings,
        };
        self.navigation_checkpoint()?;
        Ok(dto)
    }

    pub fn snapshot(&self, as_of: &str) -> Result<SnapshotDto, NavigationError> {
        self.navigation_checkpoint()?;
        let as_of_ts = as_of
            .parse::<jiff::Timestamp>()
            .map_err(|_| NavigationError::InvalidAsOf)?;

        // CLONE-FREE Pass 1: admission count for records, nodes, and relations
        // Beta-1 raised the prototype ceilings so full-project graphs fit.
        // The 512-record / 100-node / 300-relation limits were alpha-era
        // prototype guards, not memory protections; the real bounds stay
        // the CountWriter byte budget and ExecutionBudget checkpoints.
        let record_count = self.len();
        if record_count > 100_000 {
            return Err(NavigationError::OversizedGraph(
                OversizedReason::CanonicalRecords {
                    count: record_count,
                    limit: 100_000,
                },
            ));
        }

        let mut node_count = 0;
        let mut relation_count = 0;
        for object in self.object_values() {
            self.navigation_checkpoint()?;
            match &object.object_type {
                ObjectType::Node(_) => {
                    node_count += 1;
                    if node_count > 20_000 {
                        return Err(NavigationError::OversizedGraph(
                            OversizedReason::SemanticNodes {
                                count: node_count,
                                limit: 20_000,
                            },
                        ));
                    }
                }
                ObjectType::Relation => {
                    relation_count += 1;
                    if relation_count > 50_000 {
                        return Err(NavigationError::OversizedGraph(
                            OversizedReason::ScientificRelations {
                                count: relation_count,
                                limit: 50_000,
                            },
                        ));
                    }
                }
                _ => {}
            }
        }

        // CLONE-FREE Pass 2: byte-budget streaming COUNT writer (no retained bytes)
        struct CountWriter<'a> {
            counted: usize,
            limit: usize,
            execution: &'a crate::ExecutionBudget,
        }
        impl std::io::Write for CountWriter<'_> {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.execution
                    .checkpoint()
                    .map_err(|stop| std::io::Error::new(std::io::ErrorKind::Interrupted, stop))?;
                self.counted = self.counted.saturating_add(buf.len());
                if self.counted > self.limit {
                    return Err(std::io::Error::other("limit_exceeded"));
                }
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                self.execution
                    .checkpoint()
                    .map_err(|stop| std::io::Error::new(std::io::ErrorKind::Interrupted, stop))?;
                Ok(())
            }
        }

        let mut writer = CountWriter {
            counted: 0,
            limit: 2_000_000,
            execution: &self.execution,
        };

        for object in self.object_values() {
            self.navigation_checkpoint()?;
            if let Err(err) = serde_json::to_writer(&mut writer, &object.value) {
                if err.is_io() {
                    let msg = err.to_string();
                    if msg.contains("limit_exceeded") {
                        return Err(NavigationError::OversizedGraph(
                            OversizedReason::CanonicalBytes {
                                bytes: writer.counted,
                                limit: 2_000_000,
                            },
                        ));
                    }
                    if let Err(stop) = self.execution.checkpoint() {
                        return Err(NavigationError::Stopped(stop));
                    }
                }
                return Err(NavigationError::SerializationError(format!("{err}")));
            }
        }

        // Collection phase
        self.navigation_checkpoint()?;
        let mut project_val: Option<Value> = None;
        let mut threads = Vec::new();
        let mut objects = BTreeMap::new();
        let mut assessments: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut thread_bindings: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut freshness = BTreeMap::new();

        for object in self.object_values() {
            self.navigation_checkpoint()?;
            match &object.object_type {
                ObjectType::Project => {
                    project_val = Some(object.value.clone());
                }
                ObjectType::Thread => {
                    threads.push(object.value.clone());
                }
                ObjectType::Assessment => {
                    if let Some(target_id) =
                        object.value.pointer("/target/id").and_then(Value::as_str)
                    {
                        assessments
                            .entry(target_id.to_string())
                            .or_default()
                            .push(object.id.to_string());
                    }
                }
                ObjectType::ThreadBinding => {
                    if let Some(target_id) =
                        object.value.pointer("/target/id").and_then(Value::as_str)
                    {
                        thread_bindings
                            .entry(target_id.to_string())
                            .or_default()
                            .push(object.id.to_string());
                    }
                }
                _ => {}
            }
            objects.insert(object.id.to_string(), object.value.clone());
            freshness.insert(
                object.id.to_string(),
                self.freshness_at(object, as_of_ts).status,
            );
        }

        let project = project_val.ok_or(NavigationError::NotFound)?;

        threads.sort_by(|left, right| {
            left.get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .cmp(right.get("id").and_then(Value::as_str).unwrap_or(""))
        });

        self.navigation_checkpoint()?;
        let mut heads = BTreeMap::new();
        for (logical_id, head_ids) in self.lineage_heads() {
            self.navigation_checkpoint()?;
            let mut head_strings: Vec<String> = head_ids.iter().map(|id| id.to_string()).collect();
            head_strings.sort();
            heads.insert(logical_id.to_string(), head_strings);
        }

        self.navigation_checkpoint()?;
        let mut relation_heads = Vec::new();
        for object in self.object_values() {
            self.navigation_checkpoint()?;
            if matches!(object.object_type, ObjectType::Relation) {
                if let Some(logical_id) = &object.logical_id {
                    if heads
                        .get(logical_id.as_ref())
                        .is_some_and(|h| h.iter().any(|id| id == object.id.as_ref()))
                    {
                        relation_heads.push(object.id.to_string());
                    }
                }
            }
        }
        relation_heads.sort();

        let canonical_count = objects.len();
        self.navigation_checkpoint()?;
        Ok(SnapshotDto {
            project,
            threads,
            objects,
            heads,
            relation_heads,
            assessments,
            thread_bindings,
            freshness,
            canonical_count,
        })
    }

    fn current_node_ids(&self, kind: &str) -> Vec<String> {
        self.object_values()
            .filter(|object| {
                matches!(&object.object_type, ObjectType::Node(actual) if actual.as_ref() == kind)
            })
            .filter(|object| {
                object.logical_id.as_deref().is_some_and(|logical_id| {
                    self.heads_for(logical_id).contains(&object.id.as_ref())
                })
            })
            .map(|object| object.id.to_string())
            .collect()
    }

    fn freshness_at(&self, object: &ObjectRecord, as_of: jiff::Timestamp) -> FreshnessEvaluation {
        let Some(policy) = self.freshness_policy() else {
            return FreshnessEvaluation {
                status: FreshnessStatus::Unknown,
                applicable_rules: Vec::new(),
            };
        };
        let tags: BTreeSet<_> = object
            .value
            .get("tags")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let applicable: Vec<_> = policy
            .get("rules")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|rule| {
                rule.pointer("/selector/tags_any")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .any(|tag| tags.contains(tag))
            })
            .collect();
        if applicable.is_empty() {
            return FreshnessEvaluation {
                status: FreshnessStatus::Unknown,
                applicable_rules: Vec::new(),
            };
        }
        let applicable_rules = applicable
            .iter()
            .filter_map(|rule| rule.get("id").and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        if timestamp_at_or_before(
            object
                .value
                .pointer("/temporal/effective_until")
                .and_then(Value::as_str),
            as_of,
        ) {
            return FreshnessEvaluation {
                status: FreshnessStatus::Stale,
                applicable_rules,
            };
        }

        let mut all_checks_available = true;
        let mut invalidated_by_source = false;
        let review_deadline = object
            .value
            .pointer("/temporal/review_due_at")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<jiff::Timestamp>().ok());
        for rule in applicable {
            if self.execution.checkpoint().is_err() {
                break;
            }
            let invalidates = rule
                .get("invalidates_on_change")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            for check in rule
                .get("required_checks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                match check {
                    "review-deadline" => {
                        if review_deadline.is_none() {
                            all_checks_available = false;
                        }
                    }
                    "external-source-version" => {
                        let outcome = self.source_version_outcome(object, as_of);
                        all_checks_available &= outcome.available;
                        invalidated_by_source |= invalidates && outcome.newer_exists;
                    }
                    "artifact-digest" => {
                        let artifacts: Vec<_> = object
                            .value
                            .pointer("/source/artifacts")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .collect();
                        // The index records verified successes, not failure reasons.
                        // In particular, identifier-only HTTPS manifests are not
                        // digest mismatches. Integrity failures remain Findings.
                        all_checks_available &= !artifacts.is_empty()
                            && artifacts.iter().all(|id| self.artifact_verified(id));
                    }
                    _ => all_checks_available = false,
                }
            }
        }
        if invalidated_by_source {
            return FreshnessEvaluation {
                status: FreshnessStatus::Stale,
                applicable_rules,
            };
        }
        // The frozen machine contract gives unavailable required checks
        // precedence over review-due, but never over an established stale condition.
        if !all_checks_available {
            return FreshnessEvaluation {
                status: FreshnessStatus::Unknown,
                applicable_rules,
            };
        }
        if review_deadline.is_some_and(|deadline| deadline <= as_of) {
            return FreshnessEvaluation {
                status: FreshnessStatus::ReviewDue,
                applicable_rules,
            };
        }
        FreshnessEvaluation {
            status: FreshnessStatus::Fresh,
            applicable_rules,
        }
    }

    fn source_version_outcome(
        &self,
        object: &ObjectRecord,
        as_of: jiff::Timestamp,
    ) -> SourceVersionOutcome {
        let pinned: Vec<_> = object
            .value
            .pointer("/source/external_references")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let mut outcome = SourceVersionOutcome {
            available: !pinned.is_empty(),
            newer_exists: false,
        };
        for id in pinned {
            if self.execution.checkpoint().is_err() {
                break;
            }
            let Some(reference) = self
                .get(id)
                .filter(|reference| matches!(reference.object_type, ObjectType::ExternalReference))
            else {
                outcome.available = false;
                continue;
            };
            let Some(released) = source_release(reference) else {
                outcome.available = false;
                continue;
            };
            for candidate in self
                .object_values()
                .take_while(|_| self.execution.checkpoint().is_ok())
                .filter(|candidate| {
                    matches!(candidate.object_type, ObjectType::ExternalReference)
                        && candidate.value.get("type") == reference.value.get("type")
                        && candidate.value.get("canonical_uri")
                            == reference.value.get("canonical_uri")
                        && candidate.value.pointer("/source_version/identifier")
                            != reference.value.pointer("/source_version/identifier")
                })
            {
                match source_release(candidate) {
                    Some(candidate) => {
                        outcome.newer_exists |= candidate > released && candidate <= as_of;
                    }
                    None => outcome.available = false,
                }
            }
        }
        outcome
    }
}

struct SourceVersionOutcome {
    available: bool,
    newer_exists: bool,
}

fn source_release(reference: &ObjectRecord) -> Option<jiff::Timestamp> {
    reference
        .value
        .pointer("/source_version/released_at")
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok())
}

fn object_count_key(object_type: &ObjectType) -> &'static str {
    match object_type {
        ObjectType::Project => "project",
        ObjectType::Node(_) => "node_revision",
        ObjectType::Relation => "scientific_relation_revision",
        ObjectType::Assessment => "assessment",
        ObjectType::Thread => "research_thread",
        ObjectType::ThreadBinding => "thread_binding",
        ObjectType::ClaimChain => "claim_chain_snapshot",
        ObjectType::ExternalReference => "external_reference",
        ObjectType::Artifact => "artifact_manifest",
        ObjectType::ResearchRun => "research_run",
    }
}

fn query_row(object: &ObjectRecord, freshness: FreshnessStatus) -> Value {
    json!({
        "id": object.id,
        "schema": object.value.get("schema"),
        "logical_id": object.logical_id,
        "kind": object.value.get("kind"),
        "title": object.value.get("title"),
        "freshness": freshness,
    })
}

fn timestamp_at_or_before(value: Option<&str>, as_of: jiff::Timestamp) -> bool {
    value
        .and_then(|value| value.parse::<jiff::Timestamp>().ok())
        .is_some_and(|value| value <= as_of)
}

fn collect_diff(
    path: &str,
    before: Option<&Value>,
    after: Option<&Value>,
    changes: &mut Vec<DiffChange>,
    execution: &crate::ExecutionBudget,
) -> Result<(), NavigationError> {
    execution.checkpoint().map_err(NavigationError::Stopped)?;
    if before == after {
        execution.checkpoint().map_err(NavigationError::Stopped)?;
        return Ok(());
    }
    match (before, after) {
        (Some(Value::Object(left)), Some(Value::Object(right))) => {
            let keys: BTreeSet<_> = left.keys().chain(right.keys()).collect();
            for key in keys {
                let pointer = format!("{path}/{}", escape_pointer(key));
                collect_diff(&pointer, left.get(key), right.get(key), changes, execution)?;
            }
        }
        (Some(Value::Array(left)), Some(Value::Array(right))) => {
            let length = left.len().max(right.len());
            for index in 0..length {
                let pointer = format!("{path}/{index}");
                collect_diff(
                    &pointer,
                    left.get(index),
                    right.get(index),
                    changes,
                    execution,
                )?;
            }
        }
        _ => changes.push(DiffChange {
            path: if path.is_empty() {
                "/".to_string()
            } else {
                path.to_string()
            },
            before: before.cloned(),
            after: after.cloned(),
        }),
    }
    execution.checkpoint().map_err(NavigationError::Stopped)
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
