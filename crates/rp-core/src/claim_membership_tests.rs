use super::*;
use crate::{ObjectRecord, ProjectPath};
use serde_json::json;

#[test]
fn resolved_wrong_object_type_never_emits_membership_or_profile_cascades() {
    // Public project loading rejects non-Node ID prefixes at Stage 2. Construct
    // an index here to pin the Stage 3 defensive type guard independently.
    for profile in ["minimum-v1", "evidential-v1", "confirmatory-v1"] {
        for (wrong_root, wrong_target) in [(true, false), (false, true), (true, true)] {
            let mut index = ProjectIndex::default();
            let records = [
                (
                    "root",
                    ObjectType::Node("Question".into()),
                    json!({"schema": "rp/node-revision/v1"}),
                ),
                (
                    "target",
                    ObjectType::Node("Conclusion".into()),
                    json!({"schema": "rp/node-revision/v1"}),
                ),
                (
                    "wrong",
                    ObjectType::Artifact,
                    json!({"schema": "rp/artifact-manifest/v1"}),
                ),
                (
                    "edge",
                    ObjectType::Relation,
                    json!({"schema": "rp/scientific-relation-revision/v1", "relation_state": "active", "from_revision": "root", "to_revision": "target", "type": "derived-from"}),
                ),
            ];
            for (ordinal, (id, object_type, value)) in records.into_iter().enumerate() {
                index.insert(ObjectRecord {
                    ordinal,
                    id: id.into(),
                    logical_id: None,
                    object_type,
                    source_file: ProjectPath::new(format!("{id}.yaml")).unwrap(),
                    value,
                });
            }
            let (_, digest, _) =
                source_closure(&index, &["root".into(), "target".into()], &["edge".into()]);
            index.insert(ObjectRecord {
                ordinal: 4,
                id: "chain".into(),
                logical_id: None,
                object_type: ObjectType::ClaimChain,
                source_file: ProjectPath::new("chain.yaml").unwrap(),
                value: json!({
                    "root_node_revisions": [if wrong_root { "wrong" } else { "root" }],
                    "target_node_revision": if wrong_target { "wrong" } else { "target" },
                    "node_revisions": ["root", "target"], "relation_revisions": ["edge"],
                    "validation_policy": {"profile": profile}, "source_closure_sha256": digest
                }),
            });
            let (_, findings) = validate_claim_chains(&index, &BTreeSet::new(), Default::default());
            assert_eq!(
                findings.as_slice(),
                [],
                "{profile}: {wrong_root}/{wrong_target}"
            );
        }
    }
}
