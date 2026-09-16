# Research Provenance Fixture: claim-chain-evidential-v1

> Status: **G7-approved Phase 0 fixture baseline; frozen for authorized Phase 1 conformance**
>
> Nature: **deliberately synthetic, modular, and domain-neutral**

## 1. Purpose

The `claim-chain-evidential-v1` fixture module tests the `evidential-v1` ClaimChain profile. It extends `minimum-v1` by requiring explicit evidential pathways between observations/measurements and target conclusions.

It verifies that:
1. Target conclusions are supported by explicit evaluative or derivational relation paths (`supports`, `weakens`, `contradicts`, `consistent-with`, `derived-from`, `generated`);
2. When multiple evidence inputs (≥ 2) contribute to a conclusion, an explicit `Synthesis` node is mandatory, connected via `input-to` relations;
3. Single-input inferences are permitted to connect directly without a Synthesis node;
4. Temporal predeclaration is not enforced in this profile (unlike `confirmatory-v1`).

## 2. Materialized Positive Coverage

| Entity | Materialized Count | Phase 0 Baseline Role |
|---|---:|---|
| Project root (`project.yaml`) | 1 | Project boundary declaration (`slug: claim-chain-evidential-v1`) |
| ExternalReference | 1 | Synthetic public comparison protocol |
| ArtifactManifest | 1 | Self-contained deterministic CSV evidence table |
| Question NodeRevision | 1 | Root comparative evidence question |
| Hypothesis NodeRevision | 1 | Scoped standard-envelope hypothesis |
| Measurement NodeRevision | 2 | Baseline and candidate evidence inputs |
| Synthesis NodeRevision | 1 | Explicit two-input comparison |
| Conclusion NodeRevision | 2 | Separate multi-input and single-input targets |
| ScientificRelationRevision | 12 | 7 used across positive snapshots plus 5 valid unselected overlay controls |
| ClaimChainSnapshot | 2 | One multi-input Synthesis chain and one direct single-input chain |

The multi-input snapshot requires both Measurements to enter the same Synthesis
through `input-to`, followed by `generated` to its Conclusion. The second
snapshot demonstrates that one Measurement may directly `support` a distinct
Conclusion without a Synthesis.

`expected-claim-chains.yaml` is the noncanonical machine-readable oracle for
both selections, validation outcomes, and exact source-closure entries/digests.

## 3. Claim Chain Policy

Defined in `claim-chain-policy.yaml`:
- `profile: evidential-v1`
- `extends: minimum-v1`
- `evidential_v1.require_target_evaluation_path: true`
- `evidential_v1.multi_input_inference.minimum_inputs_requiring_synthesis: 2`
- `evidential_v1.multi_input_inference.require_input_to_relations: true`
- `evidential_v1.single_input_inference.allow_direct_scoped_relation: true`
- `evidential_v1.temporal_predeclaration_required: false`

## 4. Materialized Single-Mutation Overlays

The test harness applies single-mutation overlays over `valid/`:

| Overlay ID | Single Mutation | Expected Finding Family |
|---|---|---|
| `overlay-multi-input-missing-synthesis` | Multi-input Snapshot swaps both `input-to` edges for direct Measurement-to-Conclusion edges | Evidential Synthesis requirement |
| `overlay-missing-evaluative-path` | Single-input Snapshot changes its target to the root Question, which no evidence path reaches | Target evaluative-path completeness |
| `overlay-invalid-input-to-endpoint` | One unselected valid control relation is mutated so `input-to` targets Hypothesis instead of Synthesis | Relation compatibility / endpoint role |
| `overlay-unsupported-evaluative-relation` | Single-input Snapshot replaces final `supports` with compatible but non-evaluative `depends-on` | Evaluative relation whitelist |

## 5. MVP Boundaries

- **No GovernanceAttestation or governance standing in MVP**: Both are deferred to post-MVP.
- **No DeclassificationAttestation in MVP**: Access downgrades are unconditionally rejected.
