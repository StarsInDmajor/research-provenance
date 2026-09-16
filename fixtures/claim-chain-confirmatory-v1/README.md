# Research Provenance Fixture: claim-chain-confirmatory-v1

> Status: **G7-approved Phase 0 fixture baseline; frozen for authorized Phase 1 conformance**
>
> Nature: **deliberately synthetic, modular, and domain-neutral**

## 1. Purpose

The `claim-chain-confirmatory-v1` fixture module tests the full `confirmatory-v1` ClaimChain profile. It extends `evidential-v1` with strict confirmatory protocol guarantees:
1. Strict confirmatory backbone topology:
   `Question` --`motivates`--> predeclared `Hypothesis` --`predicts`--> `Prediction` --`tested-by`--> `Test` <--`result-of`-- `Measurement` / `Observation`
2. Temporal predeclaration ordering:
   Prediction and Test `created_at` timestamps must strictly precede the confirmatory analysis/result-computation timestamp (`temporal.recorded_at`, a linked execution event, or finally the result Revision `created_at`), not necessarily the source data's historical observation window;
3. Artifact-backed execution provenance for all selected empirical result nodes;
4. Multi-evidence synthesis evaluating both consistent candidate findings and contradictory stress findings into a scope-limited conclusion;
5. Explicit positive and contradictory outcomes without converting profile validity into a scientific truth judgment.

## 2. Materialized Positive Coverage

| Entity | Materialized Count | Phase 0 Baseline Role |
|---|---:|---|
| Project root (`project.yaml`) | 1 | Project boundary declaration (`slug: claim-chain-confirmatory-v1`) |
| ResearchRun | 1 | Synthetic execution container without Layer A coupling |
| ExternalReference | 2 | Versioned protocol and archival source |
| ArtifactManifest | 3 | Standard, stress, and archival self-contained CSVs |
| Dataset NodeRevision | 3 | Standard, stress, and archival source selections |
| Question NodeRevision | 1 | Exact confirmatory root |
| Hypothesis NodeRevision | 1 | Frozen scoped hypothesis |
| Prediction NodeRevision | 1 | Frozen quantitative criteria |
| Method NodeRevision | 1 | Frozen analysis procedure |
| Test NodeRevision | 2 | Standard/retrospective and stress Tests |
| Measurement NodeRevision | 4 | Baseline, candidate, stress, and archival retrospective results |
| Synthesis NodeRevision | 1 | Explicit four-input mixed-outcome synthesis |
| Conclusion NodeRevision | 1 | Scope-restricted target |
| ScientificRelationRevision | 19 | Complete backbone, result mapping, outcomes, Synthesis, and Method edges |
| ClaimChainSnapshot | 1 | `confirmatory-v1` with artifact-backed execution provenance |

The materialized Snapshot selects 12 NodeRevisions and 19 active Relations.
Its exact 39-entry source closure and deterministic digest are recorded in the
noncanonical `expected-claim-chain.yaml` oracle.

## 3. Synthetic Chronology & Predeclaration

```text
2026-01-01  Question and Hypothesis frozen
2026-01-02  Prediction, Method, standard Test, and stress Test frozen
2026-01-10  baseline, candidate, and archival results computed after predeclaration
2026-02-01  stress result computed after predeclaration
2026-02-04  mixed-outcome Synthesis and scope-limited Conclusion frozen
```

`confirmatory-v1` requires each result's applicable Prediction and Test creation
to precede the result's analysis/computation time. The baseline result exercises
the final `NodeRevision.created_at` fallback; candidate and stress results use
`temporal.recorded_at`. The archival result was observed in November 2025 but
analyzed in January 2026 after predeclaration, demonstrating the valid
retrospective case. Profile validity verifies structure and chronology only; it
does not declare the synthetic Conclusion scientifically true.

## 4. Materialized Single-Mutation Overlays

The test harness applies single-mutation overlays over `valid/`:

| Overlay ID | Single Mutation | Expected Finding Family |
|---|---|---|
| `overlay-temporal-predeclaration-inverted` | Move the standard Test `created_at` after its applicable result-computation times | Temporal predeclaration integrity |
| `overlay-missing-confirmatory-backbone` | Remove both selected `tested-by` relations while preserving minimum-profile connectivity | Confirmatory backbone completeness |
| `overlay-provenance-gap` | Candidate Measurement omits its direct ArtifactManifest source while retaining Dataset provenance | Artifact-backed result policy |
| `overlay-result-without-test` | Remove only the stress result's `result-of` relation | Per-result Test mapping |
| `overlay-invalid-confirmatory-root` | Replace the root Question with a selected Measurement | Confirmatory root-kind policy |

## 5. MVP Boundaries

- **No GovernanceAttestation or governance standing in MVP**: Both are deferred to post-MVP.
- **No DeclassificationAttestation in MVP**: This profile fixture remains `internal`; downgrade behavior is tested separately by `access-closure-v1` and is rejected unconditionally.
