use crate::findings::Findings;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Serialize;
use serde_json::Value;

use crate::{Finding, ObjectType, ProjectIndex, Severity, jcs_sha256};

#[cfg(test)]
#[path = "claim_membership_tests.rs"]
mod membership_tests;

#[cfg(test)]
#[path = "claim_v1_tests.rs"]
mod v1_tests;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceClosureEntry {
    pub object_type: Box<str>,
    pub id: Box<str>,
    pub canonical_digest: Box<str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimChainReport {
    pub snapshot_id: Box<str>,
    pub profile: Box<str>,
    /// Claim checks; attached reports additionally require complete passed subject,
    /// C2 acquisition, exact body binding and safe equal labels. Reportless semantics
    /// are unchanged. This is not overall project validity or export permission.
    pub valid: bool,
    pub source_entries: Vec<SourceClosureEntry>,
    pub source_sha256: Box<str>,
}

pub(crate) fn validate_claim_chains(
    index: &ProjectIndex,
    verified_artifacts: &BTreeSet<Box<str>>,
    mut findings: Findings,
) -> (BTreeMap<Box<str>, ClaimChainReport>, Findings) {
    let mut reports = BTreeMap::new();
    for chain in index
        .object_values()
        .filter(|object| matches!(object.object_type, ObjectType::ClaimChain))
    {
        if findings.cancelled() {
            break;
        }
        let (report, chain_findings) =
            validate_one_claim_chain(index, chain, verified_artifacts, findings.fork());
        reports.insert(chain.id.as_ref().into(), report);
        findings.absorb(chain_findings);
    }
    (reports, findings)
}

pub(crate) fn validate_one_claim_chain(
    index: &ProjectIndex,
    chain: &crate::ObjectRecord,
    verified_artifacts: &BTreeSet<Box<str>>,
    mut findings: Findings,
) -> (ClaimChainReport, Findings) {
    let profile = chain
        .value
        .pointer("/validation_policy/profile")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let nodes = string_array(&chain.value, "/node_revisions");
    let relations = string_array(&chain.value, "/relation_revisions");
    let endpoints_selected = validate_minimum(index, chain, &nodes, &relations, &mut findings);
    if endpoints_selected && matches!(profile, "evidential-v1" | "confirmatory-v1") {
        validate_evidential(index, chain, &nodes, &relations, &mut findings);
    }
    if endpoints_selected && profile == "confirmatory-v1" {
        validate_confirmatory(
            index,
            chain,
            &nodes,
            &relations,
            verified_artifacts,
            &mut findings,
        );
    }
    let (source_entries, source_sha256, unresolved) = source_closure(index, &nodes, &relations);
    if !unresolved.is_empty() {
        findings.push(|| {
            chain_finding(
                chain,
                "RP_E_SOURCE_CLOSURE_UNRESOLVABLE",
                "source_closure_integrity",
                format!("source closure contains unresolved IDs: {unresolved:?}"),
                "/source_closure_sha256",
            )
        });
    }
    if chain
        .value
        .get("source_closure_sha256")
        .and_then(Value::as_str)
        != Some(source_sha256.as_str())
    {
        findings.push(|| {
            chain_finding(
                chain,
                "RP_E_SOURCE_CLOSURE_DIGEST_MISMATCH",
                "source_closure_integrity",
                "claim-chain source closure digest does not match the selected canonical objects",
                "/source_closure_sha256",
            )
        });
    }
    let report = ClaimChainReport {
        snapshot_id: chain.id.as_ref().into(),
        profile: profile.into(),
        valid: !findings.had_error() && !findings.cancelled(),
        source_entries,
        source_sha256: source_sha256.into(),
    };
    (report, findings)
}

fn validate_minimum(
    index: &ProjectIndex,
    chain: &crate::ObjectRecord,
    nodes: &[String],
    relations: &[String],
    findings: &mut Findings,
) -> bool {
    if findings.cancelled() {
        return false;
    }
    let selected_nodes: BTreeSet<&str> = nodes.iter().map(String::as_str).collect();
    let mut endpoints_selected = true;
    for (position, root) in string_array(&chain.value, "/root_node_revisions")
        .iter()
        .enumerate()
    {
        // Do not short-circuit: each resolved unselected endpoint gets a finding.
        endpoints_selected &= validate_endpoint_membership(
            index,
            chain,
            root,
            &selected_nodes,
            &format!("/root_node_revisions/{position}"),
            findings,
        );
    }
    endpoints_selected &= validate_endpoint_membership(
        index,
        chain,
        chain.value["target_node_revision"]
            .as_str()
            .unwrap_or_default(),
        &selected_nodes,
        "/target_node_revision",
        findings,
    );
    let mut edges = Vec::new();
    for relation_id in relations {
        if findings.cancelled() {
            break;
        }
        let Some(relation) = index.get(relation_id) else {
            continue;
        };
        if relation.value.get("relation_state").and_then(Value::as_str) != Some("active") {
            findings.push(|| {
                chain_finding(
                    chain,
                    "RP_E_CLAIM_CHAIN_RELATION_NOT_ACTIVE",
                    "relation_state",
                    format!("selected relation {relation_id} is not active"),
                    "/relation_revisions",
                )
            });
        }
        let Some(from) = relation.value.get("from_revision").and_then(Value::as_str) else {
            continue;
        };
        let Some(to) = relation.value.get("to_revision").and_then(Value::as_str) else {
            continue;
        };
        if !selected_nodes.contains(from) || !selected_nodes.contains(to) {
            findings.push(|| {
                chain_finding(
                    chain,
                    "RP_E_CLAIM_CHAIN_ENDPOINT_NOT_SELECTED",
                    "endpoint_containment",
                    format!("selected relation {relation_id} has an unselected endpoint"),
                    "/node_revisions",
                )
            });
        } else {
            edges.push((from.to_string(), to.to_string()));
        }
    }

    if weak_component_count(nodes, &edges, &index.execution) != 1 {
        findings.push(|| {
            chain_finding(
                chain,
                "RP_E_CLAIM_CHAIN_MULTIPLE_COMPONENTS",
                "graph_connectivity",
                "selected claim-chain graph is not exactly one weak component",
                "/node_revisions",
            )
        });
    }
    if directed_cycle(nodes, &edges, &index.execution) {
        findings.push(|| {
            chain_finding(
                chain,
                "RP_E_CLAIM_CHAIN_DIRECTED_CYCLE",
                "claim_chain_cycle",
                "selected claim-chain graph contains a directed cycle",
                "/relation_revisions",
            )
        });
    }
    endpoints_selected
}

fn validate_endpoint_membership(
    index: &ProjectIndex,
    chain: &crate::ObjectRecord,
    id: &str,
    selected_nodes: &BTreeSet<&str>,
    pointer: &str,
    findings: &mut Findings,
) -> bool {
    if findings.cancelled() {
        return false;
    }
    if !index
        .get(id)
        .is_some_and(|object| matches!(object.object_type, ObjectType::Node(_)))
    {
        // Reference validation owns unresolved/type-mismatch diagnostics. These
        // endpoints still cannot satisfy the prerequisite for profile inference.
        return false;
    }
    if !selected_nodes.contains(id) {
        findings.push(|| {
            chain_finding(
                chain,
                "RP_E_CLAIM_CHAIN_ENDPOINT_NOT_SELECTED",
                "endpoint_containment",
                format!("claim-chain endpoint {id} is not selected in node_revisions"),
                pointer,
            )
        });
        return false;
    }
    true
}

fn validate_evidential(
    index: &ProjectIndex,
    chain: &crate::ObjectRecord,
    nodes: &[String],
    relations: &[String],
    findings: &mut Findings,
) {
    if findings.cancelled() {
        return;
    }
    let target = chain
        .value
        .get("target_node_revision")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let edges = selected_relations(index, relations);
    let evidence: Vec<_> = nodes
        .iter()
        .filter(|id| {
            index.get(id).is_some_and(|object| {
                matches!(
                    object.object_type,
                    ObjectType::Node(ref kind)
                        if matches!(kind.as_ref(), "Dataset" | "Observation" | "Measurement" | "PaperClaim")
                )
            })
        })
        .collect();
    let allowed = [
        "supports",
        "weakens",
        "contradicts",
        "consistent-with",
        "derived-from",
        "generated",
    ];
    let every_evidence_reaches_target = evidence
        .iter()
        .all(|source| path_to_target(source, target, &edges, &index.execution, |_| true));
    let has_allowed_target_path = evidence.iter().any(|source| {
        path_to_target(source, target, &edges, &index.execution, |relation_type| {
            allowed.contains(&relation_type)
        })
    });
    if !every_evidence_reaches_target || !has_allowed_target_path {
        let (code, family, message) = if every_evidence_reaches_target {
            (
                "RP_E_EVIDENTIAL_RELATION_NOT_ALLOWED",
                "evidential_relation_whitelist",
                "target paths do not contain an allowed evidential relation",
            )
        } else {
            (
                "RP_E_EVIDENTIAL_TARGET_PATH_MISSING",
                "evidential_target_path",
                "at least one selected evidence node has no directed path to the target",
            )
        };
        findings.push(|| chain_finding(chain, code, family, message, "/target_node_revision"));
    }

    if evidence.len() >= 2 {
        let synthesis_valid = nodes.iter().any(|id| {
            index.get(id).is_some_and(|object| {
                matches!(object.object_type, ObjectType::Node(ref kind) if kind.as_ref() == "Synthesis")
                    && edges
                        .iter()
                        .filter(|edge| edge.relation_type == "input-to" && edge.to == *id)
                        .map(|edge| edge.from.as_str())
                        .filter(|source| evidence.iter().any(|candidate| candidate.as_str() == *source))
                        .collect::<BTreeSet<_>>()
                        .len()
                        >= 2
                    && edges.iter().any(|edge| {
                        edge.relation_type == "generated" && edge.from == *id && edge.to == target
                    })
            })
        });
        if !synthesis_valid || flat_synthesis_bypass(index, nodes, relations, target, findings) {
            findings.push(|| {
                chain_finding(
                    chain,
                    "RP_E_EVIDENTIAL_SYNTHESIS_REQUIRED",
                    "evidential_synthesis_requirement",
                    "multiple evidence inputs require an explicit connected Synthesis",
                    "/relation_revisions",
                )
            });
        }
    }
}

/// Partial v1 coverage only: one flat final Synthesis plus direct evidence
/// evaluations of its target. Never infer groups from all selected evidence or
/// force nested/generated results, shared inputs or separate outputs into it.
fn flat_synthesis_bypass(
    index: &ProjectIndex,
    nodes: &[String],
    relations: &[String],
    target: &str,
    findings: &Findings,
) -> bool {
    if findings.cancelled() {
        return false;
    }
    let selected: BTreeSet<&str> = nodes.iter().map(String::as_str).collect();
    let evidence = |id: &str| {
        index.get(id).is_some_and(|o| {
            matches!(&o.object_type, ObjectType::Node(k)
                if matches!(k.as_ref(), "Dataset" | "Observation" | "Measurement" | "PaperClaim"))
        })
    };
    let mut edges = Vec::new();
    for id in relations {
        if findings.cancelled() {
            return false;
        }
        let Some(r) = index.get(id) else { return false };
        if r.value["relation_state"] != "active" {
            // The primary relation-state diagnosis owns malformed shapes.
            return false;
        }
        edges.extend(selected_relations(index, std::slice::from_ref(id)));
    }
    if edges.iter().any(|e| {
        !selected.contains(e.from.as_str())
            || !selected.contains(e.to.as_str())
            || e.relation_type == "generated" && evidence(&e.to)
    }) {
        return false;
    }
    let finals: BTreeSet<_> = edges
        .iter()
        .filter(|e| {
            e.relation_type == "generated"
                && e.to == target
                && index.get(&e.from).is_some_and(
                    |o| matches!(&o.object_type, ObjectType::Node(k) if k.as_ref() == "Synthesis"),
                )
        })
        .map(|e| e.from.as_str())
        .collect();
    if finals.len() != 1 {
        return false;
    }
    let synthesis = *finals.first().unwrap();
    let inputs: BTreeSet<_> = edges
        .iter()
        .filter(|e| e.relation_type == "input-to" && e.to == synthesis && evidence(&e.from))
        .map(|e| e.from.as_str())
        .collect();
    if inputs.len() < 2
        || edges.iter().any(|e| {
            e.relation_type == "generated" && e.from == synthesis && e.to != target
                || e.relation_type == "input-to"
                    && inputs.contains(e.from.as_str())
                    && e.to != synthesis
        })
    {
        return false;
    }
    for edge in &edges {
        if findings.cancelled() {
            return false;
        }
        if edge.to == target && evidence(&edge.from)
            && matches!(edge.relation_type.as_str(), "supports" | "weakens" | "contradicts" | "consistent-with")
            && !inputs.contains(edge.from.as_str())
            // Another explicit context for D is outside this flat repair.
            && !edges.iter().any(|other| {
                other.from == edge.from && (other.relation_type == "input-to"
                    || other.to != target && matches!(other.relation_type.as_str(),
                        "supports" | "weakens" | "contradicts" | "consistent-with"))
            })
        {
            return true;
        }
    }
    false
}

fn validate_confirmatory(
    index: &ProjectIndex,
    chain: &crate::ObjectRecord,
    nodes: &[String],
    relations: &[String],
    verified_artifacts: &BTreeSet<Box<str>>,
    findings: &mut Findings,
) {
    if findings.cancelled() {
        return;
    }
    let roots = string_array(&chain.value, "/root_node_revisions");
    let target = chain
        .value
        .get("target_node_revision")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let root_valid = roots.len() == 1
        && index.get(&roots[0]).is_some_and(|object| {
            matches!(object.object_type, ObjectType::Node(ref kind) if kind.as_ref() == "Question")
        });
    let target_valid = index.get(target).is_some_and(|object| {
        matches!(object.object_type, ObjectType::Node(ref kind) if kind.as_ref() == "Conclusion")
    });
    if !root_valid || !target_valid {
        findings.push(|| {
            chain_finding(
                chain,
                "RP_E_CONFIRMATORY_ROOT_KIND",
                "confirmatory_root_kind",
                "confirmatory chains require exactly one Question root and one Conclusion target",
                "/root_node_revisions",
            )
        });
    }

    let active_relations: Vec<_> = relations
        .iter()
        .filter(|id| {
            index.get(id).is_some_and(|relation| {
                relation.value.get("relation_state").and_then(Value::as_str) == Some("active")
            })
        })
        .cloned()
        .collect();
    let edges = selected_relations(index, &active_relations);
    let selected: BTreeSet<&str> = nodes.iter().map(String::as_str).collect();
    let kind_is = |id: &str, kinds: &[&str]| {
        index.get(id).is_some_and(|object| {
            matches!(&object.object_type, ObjectType::Node(kind) if kinds.contains(&kind.as_ref()))
        })
    };
    // Missing selected relation endpoints already have a primary minimum finding.
    let edge_membership_valid = edges
        .iter()
        .all(|e| selected.contains(e.from.as_str()) && selected.contains(e.to.as_str()));
    let required_kinds = ["Hypothesis", "Prediction", "Test"]
        .iter()
        .all(|kind| nodes.iter().any(|id| kind_is(id, &[*kind])));
    let backbone = edges.iter().any(|motivation| {
        motivation.relation_type == "motivates"
            && kind_is(&motivation.from, &["Question"])
            && kind_is(&motivation.to, &["Hypothesis"])
            && roots.first().is_some_and(|root| motivation.from == *root)
            && edges.iter().any(|prediction| {
                prediction.relation_type == "predicts"
                    && kind_is(&prediction.to, &["Prediction"])
                    && prediction.from == motivation.to
                    && edges.iter().any(|tested| {
                        tested.relation_type == "tested-by"
                            && kind_is(&tested.to, &["Test"])
                            && tested.from == prediction.to
                    })
            })
    });
    if !backbone || (!required_kinds && edge_membership_valid) {
        findings.push(|| {
            chain_finding(
                chain,
                "RP_E_CONFIRMATORY_BACKBONE_MISSING",
                "confirmatory_backbone",
                "confirmatory motivates/predicts/tested-by backbone is incomplete",
                "/relation_revisions",
            )
        });
    }

    let results: Vec<_> = nodes
        .iter()
        .filter_map(|id| {
            let object = index.get(id)?;
            matches!(
                object.object_type,
                ObjectType::Node(ref kind) if matches!(kind.as_ref(), "Observation" | "Measurement")
            )
            .then_some(object)
        })
        .collect();
    if results.is_empty() {
        findings.push(|| {
            chain_finding(
                chain,
                "RP_E_CONFIRMATORY_RESULT_REQUIRED",
                "confirmatory_required_result",
                "confirmatory chains require at least one selected Observation or Measurement result",
                "/node_revisions",
            )
        });
    } else if !findings.cancelled() {
        let scientific_evaluation = |edge: &SelectedEdge| {
            matches!(
                edge.relation_type.as_str(),
                "supports" | "weakens" | "contradicts" | "consistent-with"
            ) && kind_is(&edge.from, &["Observation", "Measurement", "Synthesis"])
                && (kind_is(&edge.to, &["Hypothesis", "Prediction"])
                    || edge.to == target && target_valid)
        };
        let evaluation = edges.iter().any(|edge| {
            selected.contains(edge.from.as_str())
                && selected.contains(edge.to.as_str())
                && scientific_evaluation(edge)
        });
        // An inactive or unselected-endpoint evaluation is already diagnosed by
        // minimum. Do not turn that primary failure into an absence cascade.
        let evaluation_prerequisite_failed = !evaluation
            && selected_relations(index, relations)
                .iter()
                .any(scientific_evaluation);
        if !evaluation && !evaluation_prerequisite_failed {
            findings.push(|| {
                chain_finding(
                    chain,
                    "RP_E_CONFIRMATORY_EVALUATION_REQUIRED",
                    "confirmatory_scientific_evaluation",
                    "confirmatory chains require a selected active scientific evaluation from a result or Synthesis to Hypothesis, Prediction, or the target Conclusion",
                    "/relation_revisions",
                )
            });
        }
    }
    for result in results {
        if findings.cancelled() {
            break;
        }
        let mappings: Vec<_> = edges
            .iter()
            .filter(|edge| edge.relation_type == "result-of" && edge.from == result.id.as_ref())
            .filter(|edge| {
                nodes.contains(&edge.to)
                    && index.get(&edge.to).is_some_and(|test| {
                        matches!(test.object_type, ObjectType::Node(ref kind) if kind.as_ref() == "Test")
                    })
            })
            .collect();
        if mappings.is_empty() {
            findings.push(|| {
                chain_finding(
                    chain,
                    "RP_E_CONFIRMATORY_RESULT_TEST_REQUIRED",
                    "confirmatory_result_mapping",
                    format!(
                        "confirmatory result {} has no selected result-of Test",
                        result.id
                    ),
                    "/relation_revisions",
                )
            });
        }
        let result_time = result
            .value
            .pointer("/temporal/recorded_at")
            .and_then(Value::as_str)
            .or_else(|| result.value.get("created_at").and_then(Value::as_str));
        for mapping in mappings {
            if findings.cancelled() {
                break;
            }
            let Some(test) = index.get(&mapping.to) else {
                continue;
            };
            let predictions: Vec<_> = edges
                .iter()
                .filter(|edge| {
                    edge.relation_type == "tested-by"
                        && edge.to == mapping.to
                        && nodes.contains(&edge.from)
                })
                .filter_map(|edge| index.get(&edge.from))
                .filter(|prediction| {
                    matches!(prediction.object_type, ObjectType::Node(ref kind) if kind.as_ref() == "Prediction")
                })
                .collect();
            if predictions.is_empty() && !backbone {
                // The missing backbone already explains the absent Prediction links.
                // Preserve the frozen primary diagnostic without cascading per-result errors.
                continue;
            }
            if predictions.is_empty() {
                findings.push(|| chain_finding(
                    chain,
                    "RP_E_CONFIRMATORY_RESULT_TEST_REQUIRED",
                    "confirmatory_result_mapping",
                    format!(
                        "confirmatory result {} maps to Test {} without a selected active Prediction link",
                        result.id, test.id
                    ),
                    "/relation_revisions",
                ));
                continue;
            }
            let test_time = test.value.get("created_at").and_then(Value::as_str);
            if result_time.is_some_and(|result_time| {
                test_time.is_some_and(|test_time| !strictly_precedes(test_time, result_time))
                    || predictions.iter().any(|prediction| {
                        prediction
                            .value
                            .get("created_at")
                            .and_then(Value::as_str)
                            .is_some_and(|time| !strictly_precedes(time, result_time))
                    })
            }) {
                let offending = if test_time.is_some_and(|test_time| {
                    !strictly_precedes(test_time, result_time.unwrap_or_default())
                }) {
                    test
                } else {
                    predictions
                        .iter()
                        .copied()
                        .find(|prediction| {
                            prediction
                                .value
                                .get("created_at")
                                .and_then(Value::as_str)
                                .is_some_and(|time| {
                                    !strictly_precedes(time, result_time.unwrap_or_default())
                                })
                        })
                        .unwrap_or(test)
                };
                findings.push(|| {
                    Finding::new(
                        "RP_E_CONFIRMATORY_PREDECLARATION_ORDER",
                        "confirmatory_temporal_predeclaration",
                        Severity::Error,
                        format!("predeclaration does not precede result {}", result.id),
                        Some(offending.source_file.clone()),
                        "/created_at",
                    )
                });
            }
        }

        if chain
            .value
            .pointer("/validation_policy/execution_provenance")
            .and_then(Value::as_str)
            == Some("artifact-backed")
        {
            let directly_verified = result
                .value
                .pointer("/source/artifacts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .any(|id| verified_artifacts.contains(id));
            if !directly_verified {
                findings.push(|| {
                    Finding::new(
                        "RP_E_CONFIRMATORY_ARTIFACT_REQUIRED",
                        "confirmatory_provenance",
                        Severity::Error,
                        format!(
                            "confirmatory result {} lacks a directly verified file Artifact",
                            result.id
                        ),
                        Some(result.source_file.clone()),
                        "/source/artifacts",
                    )
                });
            }
        }
    }
}

#[derive(Clone)]
struct SelectedEdge {
    from: String,
    to: String,
    relation_type: String,
}

fn selected_relations(index: &ProjectIndex, relations: &[String]) -> Vec<SelectedEdge> {
    relations
        .iter()
        .filter_map(|id| index.get(id))
        .filter_map(|relation| {
            Some(SelectedEdge {
                from: relation.value.get("from_revision")?.as_str()?.to_string(),
                to: relation.value.get("to_revision")?.as_str()?.to_string(),
                relation_type: relation.value.get("type")?.as_str()?.to_string(),
            })
        })
        .collect()
}

fn path_to_target(
    source: &str,
    target: &str,
    edges: &[SelectedEdge],
    execution: &crate::ExecutionBudget,
    allowed_final: impl Fn(&str) -> bool,
) -> bool {
    let mut queue = VecDeque::from([(source.to_string(), false)]);
    let mut visited = BTreeSet::new();
    while let Some((node, has_allowed)) = queue.pop_front() {
        if execution.checkpoint().is_err() {
            return false;
        }
        if !visited.insert((node.clone(), has_allowed)) {
            continue;
        }
        if node == target && has_allowed {
            return true;
        }
        for edge in edges
            .iter()
            .take_while(|_| execution.checkpoint().is_ok())
            .filter(|edge| edge.from == node)
        {
            queue.push_back((
                edge.to.clone(),
                has_allowed || allowed_final(&edge.relation_type),
            ));
        }
    }
    false
}

pub(crate) fn source_closure(
    index: &ProjectIndex,
    nodes: &[String],
    relations: &[String],
) -> (Vec<SourceClosureEntry>, String, Vec<Box<str>>) {
    let mut ids: BTreeSet<Box<str>> = nodes
        .iter()
        .chain(relations)
        .map(|id| id.clone().into_boxed_str())
        .collect();
    let mut queue: VecDeque<_> = ids.iter().cloned().collect();
    while let Some(id) = queue.pop_front() {
        if index.execution.checkpoint().is_err() {
            break;
        }
        let Some(object) = index.get(&id) else {
            continue;
        };
        for pointer in [
            "/source/revisions",
            "/source/relations",
            "/source/artifacts",
            "/source/external_references",
        ] {
            for dependency in object
                .value
                .pointer(pointer)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let dependency: Box<str> = dependency.into();
                if ids.insert(dependency.clone()) {
                    queue.push_back(dependency);
                }
            }
        }
    }
    let unresolved: Vec<_> = ids
        .iter()
        .filter(|id| index.get(id).is_none())
        .cloned()
        .collect();
    let mut entries: Vec<_> = ids
        .into_iter()
        .take_while(|_| index.execution.checkpoint().is_ok())
        .filter_map(|id| {
            let object = index.get(&id)?;
            Some(SourceClosureEntry {
                object_type: object.value.get("schema")?.as_str()?.into(),
                id,
                canonical_digest: jcs_sha256(&object.value).ok()?.into(),
            })
        })
        .collect();
    entries
        .sort_by(|left, right| (&left.object_type, &left.id).cmp(&(&right.object_type, &right.id)));
    let digest = jcs_sha256(&entries).expect("source closure entries are JCS serializable");
    (entries, digest, unresolved)
}

fn strictly_precedes(left: &str, right: &str) -> bool {
    let Ok(left) = left.parse::<jiff::Timestamp>() else {
        return false;
    };
    let Ok(right) = right.parse::<jiff::Timestamp>() else {
        return false;
    };
    left < right
}

fn string_array(value: &Value, pointer: &str) -> Vec<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn weak_component_count(
    nodes: &[String],
    edges: &[(String, String)],
    execution: &crate::ExecutionBudget,
) -> usize {
    let mut remaining: BTreeSet<_> = nodes.iter().map(String::as_str).collect();
    let mut count = 0;
    while let Some(start) = remaining.pop_first() {
        if execution.checkpoint().is_err() {
            return count;
        }
        count += 1;
        let mut queue = VecDeque::from([start]);
        while let Some(node) = queue.pop_front() {
            for (from, to) in edges {
                if execution.checkpoint().is_err() {
                    return count;
                }
                let neighbour = if from == node {
                    Some(to.as_str())
                } else if to == node {
                    Some(from.as_str())
                } else {
                    None
                };
                if let Some(neighbour) = neighbour
                    && remaining.remove(neighbour)
                {
                    queue.push_back(neighbour);
                }
            }
        }
    }
    count
}

fn directed_cycle(
    nodes: &[String],
    edges: &[(String, String)],
    execution: &crate::ExecutionBudget,
) -> bool {
    let mut indegree: BTreeMap<&str, usize> = nodes.iter().map(|node| (node.as_str(), 0)).collect();
    for (_, to) in edges {
        if let Some(value) = indegree.get_mut(to.as_str()) {
            *value += 1;
        }
    }
    let mut queue: VecDeque<_> = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree == 0).then_some(*node))
        .collect();
    let mut visited = 0;
    while let Some(node) = queue.pop_front() {
        if execution.checkpoint().is_err() {
            return false;
        }
        visited += 1;
        for (_, to) in edges
            .iter()
            .take_while(|_| execution.checkpoint().is_ok())
            .filter(|(from, _)| from == node)
        {
            if let Some(degree) = indegree.get_mut(to.as_str()) {
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(to);
                }
            }
        }
    }
    visited != nodes.len()
}

fn chain_finding(
    chain: &crate::ObjectRecord,
    code: &'static str,
    family: &'static str,
    message: impl Into<String>,
    pointer: &str,
) -> Finding {
    Finding::new(
        code,
        family,
        Severity::Error,
        message,
        Some(chain.source_file.clone()),
        pointer,
    )
}
