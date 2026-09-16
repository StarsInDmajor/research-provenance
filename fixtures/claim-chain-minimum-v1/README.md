# Research Provenance Fixture: claim-chain-minimum-v1

> Status: **G7-approved Phase 0 fixture baseline; frozen for authorized Phase 1 conformance**
>
> Nature: **deliberately synthetic, modular, and domain-neutral**

## 1. Purpose

The `claim-chain-minimum-v1` fixture module tests the baseline `minimum-v1` ClaimChain profile. It verifies foundational structural contracts for formal claim chain snapshots without imposing evidential or confirmatory workflow requirements.

It verifies that:
1. Every selected node and relation in the snapshot references an exact immutable frozen revision ID (never a mutable logical ID);
2. The selected subgraph forms a weakly connected single component with no disconnected orphan nodes;
3. Directed cycles are strictly rejected;
4. All selected relations are in the `active` state (invalidated relations are rejected as formal support);
5. Both endpoints of every selected relation are included in the selected node revision set;
6. Source closure is deterministically computed from the selected nodes and relations.

`expected-claim-chain.yaml` is the noncanonical machine-readable oracle for the
selection, structural findings, and exact `rp/source-closure/v1` entries/digest.

## 2. Materialized Positive Coverage

| Entity | Materialized Count | Phase 0 Baseline Role |
|---|---:|---|
| Project root (`project.yaml`) | 1 | Project boundary declaration (`slug: claim-chain-minimum-v1`) |
| ExternalReference | 1 | Synthetic public structural protocol source |
| Question NodeRevision | 2 | One selected root plus one unselected connectivity-overlay node |
| Hypothesis NodeRevision | 1 | Selected exact structural hypothesis revision |
| Conclusion NodeRevision | 1 | Selected target conclusion |
| ScientificRelationRevision | 5 | 3 selected active edges plus unselected cycle and invalidated overlay candidates |
| ClaimChainSnapshot | 1 | Exact frozen selection with `profile: minimum-v1` |

The positive selected graph is `Question → Hypothesis → Conclusion` plus one
direct acyclic `Question → Conclusion` edge. The two unselected relations and
one unselected Question exist only to keep every negative descriptor a single
mutation of the Snapshot.

## 3. Claim Chain Policy

Defined in `claim-chain-policy.yaml`:
- `profile: minimum-v1`
- `identity.references: exact-frozen-revision-ids`
- `identity.allow_logical_ids: false`
- `shape.connectivity: weak`
- `shape.require_single_component: true`
- `shape.directed_cycles: reject`
- `relations.require_active: true`
- `relations.allow_invalidated_as_formal_support: false`
- `validation_report.required: false`

## 4. Materialized Single-Mutation Overlays

The test harness applies single-mutation overlays over `valid/`:

| Overlay ID | Single Mutation | Expected Finding Family |
|---|---|---|
| `overlay-claim-chain-logical-id` | Snapshot replaces one exact selected relation revision ID with logical ID `relation-minimum-direct-path` | Claim chain identity / exactness |
| `overlay-disconnected-node` | Snapshot selects an additional node revision that has no relations to the rest of the chain | Graph connectivity / single component |
| `overlay-invalidated-relation` | Snapshot selects a relation revision whose `relation_state` is `invalidated` | Relation state / formal support |
| `overlay-claim-chain-cycle` | Snapshot swaps one selected direct edge for an existing reverse dependency, producing `Question → Hypothesis → Conclusion → Question` | ClaimChain directed-cycle rejection |
| `overlay-missing-endpoint` | Snapshot removes the intermediate Hypothesis while root and target remain selected | Relation endpoint containment |

## 5. MVP Boundaries

- **No GovernanceAttestation or governance standing in MVP**: Both are deferred to post-MVP.
- **No DeclassificationAttestation in MVP**: Access closure is calculated monotonically; downgrade is rejected unconditionally.
