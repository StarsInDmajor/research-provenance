# Research Provenance Fixture: access-closure-v1

> Status: **G7-approved Phase 0 fixture baseline; frozen for authorized Phase 1 conformance**
>
> Nature: **deliberately synthetic, modular, and domain-neutral**

## 1. Purpose

The `access-closure-v1` fixture tests the complete `rp/access-closure/v1`
contract across provenance and structural dependencies. It verifies:

1. the strict lattice `public < internal < restricted < exclusive`;
2. sorted, unique lowercase-slug compartments;
3. the distinction between the dependency-only **required floor** and the
   declared-label-inclusive **effective access**;
4. transitive level maximum and compartment union;
5. revision-parent, relation endpoint/parent, Assessment target/supersession,
   Thread root/parent/fork, ThreadBinding target/thread, and ClaimChain
   selection dependencies;
6. export eligibility against both a level ceiling and authorized compartments;
7. unconditional downgrade rejection and the absence of declassification in
   MVP.

`access-policy.yaml` is Phase 0 design data describing the exact traversal and
export contract. `expected-access-closure.yaml` is a noncanonical test oracle,
not a research fact source.

### Phase 1 implementation erratum

The original synthetic baseline used cross-target `supersedes_assessments`
edges solely to exercise access closure. That contradicted the stronger frozen
Phase 1 rule that Assessment supersession is confined to the exact lane keyed
by target, scope, assessor, and review assurance. The fixture now adds
`asm_01J00000000000000000000073` as a same-lane restricted predecessor for
`asm_01J00000000000000000000071`; Assessment 71 supersedes that predecessor,
and Assessment 72 no longer carries an unrelated cross-target supersession.
This is an implementation erratum to synthetic test data, not a relaxation of
Assessment lane semantics.

## 2. Materialized Positive Coverage

The simulated Project root contains **51 one-object-per-file records**. Fifty
carry object-level access labels; Project uses its separate `access_defaults`
contract. The additional record is the Phase 1 implementation erratum above;
the remaining records retain the G7-approved Phase 0 baseline content.

| Entity | Count | Access-closure responsibility |
|---|---:|---|
| Project | 1 | Project boundary and internal defaults |
| ExternalReference | 1 | Public dependency floor |
| ArtifactManifest | 3 | Internal, restricted partner, and exclusive control bytes |
| Dataset NodeRevision | 3 | Provenance joins from Artifact and public protocol |
| Question NodeRevision | 4 | Public/internal/restricted/exclusive Thread roots and controls |
| Hypothesis NodeRevision | 4 | Restricted/exclusive revision lineage and parent-only control |
| Method NodeRevision | 1 | Intentionally restrictive `analysis-alpha` compartment |
| Synthesis NodeRevision | 2 | Restricted and exclusive multi-branch joins |
| Conclusion NodeRevision | 2 | Transitive restricted and exclusive outputs |
| ScientificRelationRevision | 17 | Endpoint and relation-parent closure, plus valid chain edges |
| Assessment | 4 | Target and same-lane supersession closure |
| ResearchThread | 4 | Root, parent, and fork closure |
| ThreadBinding | 3 | Thread and target closure |
| ClaimChainSnapshot | 2 | Restricted and exclusive selected-graph closure |

## 3. Required Floor and Effective Access

The object being validated is excluded from its dependency floor:

```text
required_floor.level        = max(transitive dependency levels)
required_floor.compartments = union(transitive dependency compartments)
```

The effective label includes the object's own declaration:

```text
effective = join(declared, required_floor)
```

A valid object must declare a level at least as restrictive as the floor and a
superset of all required compartments. Therefore `effective == declared` for
every valid baseline object, while an object may intentionally be stricter than
its dependencies.

Key positive branches:

| Target | Required / effective result |
|---|---|
| Public protocol | `public`, `[]` |
| Internal Dataset | `internal`, `[]` after including its own stricter declaration |
| Restricted partner Dataset | `restricted`, `[partner-beta]` |
| Restricted Synthesis / Conclusion | `restricted`, `[analysis-alpha, partner-beta]` |
| Exclusive control Dataset | `exclusive`, `[control-room]` |
| Exclusive Synthesis / Conclusion | `exclusive`, `[analysis-alpha, control-room, partner-beta]` |
| Child Hypothesis with restricted parent and public direct source | `restricted`, `[partner-beta]` |
| Child Relation with restricted parent and internal endpoints | `restricted`, `[lineage-alpha]` |
| Assessment superseding a same-lane restricted Assessment | `restricted`, `[partner-beta]` |
| Internal ThreadBinding targeting restricted Hypothesis | `restricted`, `[partner-beta]` |

## 4. ClaimChain and Export Oracles

The restricted minimum-profile chain has a 13-entry source closure:

```text
sha256:2adca4be1773f43b3c7a0e32cdfebdf1e86fe262e217d3c8e976a7031cef624f
```

The exclusive minimum-profile chain has a 13-entry source closure:

```text
sha256:c860fc5352c7e15faf06fb8241c0fe3e93e8682c5cf5e9b38a8a9ab495780f4c
```

Export eligibility is computed from effective access:

```text
effective.level <= request.level_ceiling
and
effective.compartments subset-of request.allowed_compartments
```

The oracle includes positive and negative public, internal, restricted, and
exclusive queries. In particular:

- restricted Conclusion + only `partner-beta` is denied because
  `analysis-alpha` is missing;
- restricted Conclusion + both compartments is allowed;
- exclusive Conclusion is denied under a restricted ceiling even when all
  compartments are supplied;
- exclusive Conclusion is allowed only with an exclusive ceiling and all three
  compartments.

## 5. Materialized Single-Mutation Overlays

Fifteen overlays exercise each dependency class without copying complete
Project trees:

| Overlay | Primary Finding |
|---|---|
| `overlay-invalid-access-downgrade` | Restricted provenance level downgrade |
| `overlay-compartment-drop` | Missing dependency compartment |
| `overlay-exclusive-downgrade` | Exclusive-to-restricted downgrade |
| `overlay-relation-endpoint-leak` | Relation endpoint omitted from source still raises floor |
| `overlay-revision-parent-leak` | NodeRevision parent closure |
| `overlay-relation-parent-leak` | RelationRevision parent closure |
| `overlay-assessment-target-leak` | Assessment target closure |
| `overlay-assessment-supersession-leak` | Assessment supersession closure |
| `overlay-thread-root-leak` | ResearchThread root closure |
| `overlay-thread-parent-leak` | ResearchThread parent/fork closure |
| `overlay-binding-target-leak` | ThreadBinding thread/target closure |
| `overlay-claim-selection-leak` | ClaimChain selected node/relation/thread closure |
| `overlay-unknown-access-level` | Access level enum rejection |
| `overlay-unsorted-compartments` | Compartment normalization |
| `overlay-declassification-in-mvp` | Deferred governance field rejection |

Overlays that change an object included in a ClaimChain source closure declare
`maintenance.recompute_source_closure_sha256` as required by the suite overlay
materialization contract.

## 6. Physical Artifacts

| File | Size | SHA-256 |
|---|---:|---|
| `standard-summary.csv` | 250 bytes | `159cb654c2db9fdeafd8a168e0bb6efbb57770a345a20b26f75f819898606174` |
| `partner-summary.csv` | 183 bytes | `54826794331eb2bffa42f21508b6eef589334ca029fad9242b69bd5d2fcc086c` |
| `control-summary.csv` | 54 bytes | `fcac7f0c2e9d157aceadcb5fdb0935a410998b245ddb1e78d1e3f9033c8ab487` |

All use project-contained `file:data/*.csv` URIs.

## 7. MVP Boundaries

- Governance standing and `GovernanceAttestation` remain post-MVP.
- `DeclassificationAttestation` is not a valid v1 object or field.
- No access downgrade or compartment release has an exception in MVP.
- The fixture tests accidental disclosure prevention, not adversarial secrecy
  against an attacker who already controls the repository or local shell.
