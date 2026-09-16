# Research Provenance Fixture: overview-v1

> Status: **G7-approved Phase 0 fixture baseline; frozen for authorized Phase 1 conformance**
>
> Nature: **deliberately synthetic, modular, and domain-neutral**

## 1. Purpose

The `overview-v1` fixture module tests project-wide navigation, top-down structure summarization, multi-head DAG branch/merge resolution, assessment history on unchanged node revisions, research thread role bindings, and blocker/next-action tracking.

It verifies that the Research Provenance Workbench can:
1. Parse and index project metadata, runs, references, and threads;
2. Resolve multi-parent DAG revisions without flattening history (e.g. `hypothesis-v1` branching into `v2-scope` and `v2-criteria`, then merging into `v3-merged`);
3. Represent unresolved interpretation forks where multiple competing interpretations co-exist as valid heads;
4. Maintain multiple independent assessments targeting the exact same unchanged node revision;
5. Track relation invalidation lineages without destroying historical audit trails;
6. Index first-class research threads with thread-relative node roles (e.g. `primary`, `diagnostic`, `sensitivity`, `historical`, `follow-up`);
7. Validate all previously uncovered built-in payloads (`Observation`, `Decision`,
   and `PaperClaim`) and an Assessment targeting an exact RelationRevision.

## 2. Materialized Positive Coverage

| Entity / Concept | Materialized Count | Phase 0 Baseline Role |
|---|---:|---|
| Project root (`project.yaml`) | 1 | Project boundary and core v1 policy declaration |
| ResearchRun | 1 | Top-level execution container (`run-drift-evaluation`) |
| Question NodeRevision | 1 | Root question (`question-drift`) |
| Hypothesis NodeRevision | 4 | Multi-parent DAG (`v1`, `v2-scope`, `v2-criteria`, `v3-merged`) |
| Measurement NodeRevision | 2 | Standard and stress measurements driving overview state |
| Observation NodeRevision | 1 | Reproducibility metadata and protocol-backed empirical statement |
| Interpretation NodeRevision | 2 | Competing unresolved heads (`interpretation-stress-environment`, `interpretation-stress-reference`) |
| Decision NodeRevision | 1 | Structured option, alternatives, reversibility, and revisit triggers |
| PaperClaim NodeRevision | 1 | Stable publication reference plus non-empty source locator |
| Assessment | 4 | Three concurrent node assessments plus one exact RelationRevision assessment |
| ScientificRelationRevision | 9 | Motivation, evidence, workflow, and invalidation lineage (`relation-candidate-support` active revision → invalidated revision) |
| ResearchThread | 3 | First-class threads (`primary-efficacy`, `data-integrity-diagnostic`, `environmental-sensitivity`) |
| ThreadBinding | 14 | Explicit node revision bindings with thread-scoped roles |
| Blocker NodeRevision | 1 | Unresolved dependency (`blocker-stress-adjudication`) |
| NextAction NodeRevision | 1 | Proposed follow-up action (`next-action-independent-stress`) |
| ExternalReference | 2 | Stable protocol reference and legacy summary reference |

## 3. Expected Derived State

`expected-overview.yaml` is the noncanonical machine-readable oracle for these
query results. It is not part of the research fact source.

- **Hypothesis Head**: Single resolved head `hypothesis-drift-v3-merged` with exact parents `{hypothesis-drift-v2-scope, hypothesis-drift-v2-criteria}`.
- **Interpretation Heads**: Two co-equal active heads `{interpretation-stress-environment, interpretation-stress-reference}`.
- **Assessment History**: Three distinct assessments on `hypothesis-drift-v1`; no silent collapse by timestamp.
- **Relation Lineage**: Candidate-support relation has invalidated head `rel_01J00000000000000000000044`; the logical relation has no active head, while history returns the ordered two-revision lineage.
- **Thread Scoping**: Stress measurement is `primary` in environmental sensitivity thread, but `sensitivity` in primary efficacy thread.
- **Freshness Boundary**: Revision-head currentness is tested independently. Under approved `rp/freshness/v1`, evaluation at `as_of: 2026-09-01T00:00:00Z` returns `unknown` because this fixture declares no applicable freshness policy or review deadline; it never infers freshness from head status.

## 4. Materialized Single-Mutation Overlays

The test harness materializes negative cases by applying a single-mutation overlay descriptor over `valid/`:

| Overlay ID | Single Mutation | Expected Finding Family |
|---|---|---|
| `overlay-dangling-reference` | Relation target revision ID does not exist | Reference integrity / unresolved pointer |
| `overlay-revision-cycle` | `hypothesis-drift-v2-scope` lists its merged descendant `v3-merged` as parent | Revision DAG cycle detection |
| `overlay-thread-binding-orphan` | ThreadBinding references a non-existent thread or node revision | Thread binding integrity |
| `overlay-closed-schema-violation` | Top-level YAML contains undeclared field outside `extensions` | Schema validation / closed schema |
| `overlay-duplicate-yaml-key` | YAML object contains duplicate key (e.g. repeated `access:`) | Restricted YAML parser compliance |
| `overlay-observation-payload-missing` | Required Observation reproducibility field is removed | JSON Schema required property |
| `overlay-decision-reversibility-enum` | Decision uses an undeclared reversibility value | JSON Schema enum |
| `overlay-paper-claim-empty-locator` | PaperClaim locator is an empty mapping | JSON Schema minimum properties |
| `overlay-assessment-target-type-mismatch` | `relation_revision` target carries a Hypothesis ID | JSON Schema discriminated target typing |

## 5. MVP Boundaries

- **No GovernanceAttestation or governance standing in MVP**: Both are deferred to post-MVP.
- **No DeclassificationAttestation in MVP**: No access downgrade workflow is permitted.
