# Research Provenance Fixture Suite

> Status: **G7-approved Phase 0 conformance baseline plus additive Phase 1 freshness fixture**
>
> Nature: **deliberately synthetic, modular, and domain-neutral**

## 1. Overview

This directory contains the modular test and conformance fixtures for the Research Provenance Workbench (Schema v1). Following the confirmed modular fixture suite architecture (Gate G6-Q5, Decision R3), the previously monolithic `conformance-v1` prototype has been retired and decomposed into targeted, single-responsibility fixture modules with single-mutation test overlays. Parsed canonical objects are checked against the Draft 2020-12 candidate bundle at [`../research-provenance-schemas/v1/`](../research-provenance-schemas/v1/).

These fixtures exist exclusively to validate ontology semantics, DAG relations, multi-head resolution, assessment independence, research thread indexing, ClaimChain profiles, and access closure calculations. They do not represent real experiments and must never be cited as scientific results.

## 2. Modular Fixture Suite Catalog

| Module | Primary Focus & Responsibility | Key Schema Capabilities Verified |
|---|---|---|
| [`overview-v1`](./overview-v1/) | Project-wide structural navigation & overview | Multi-head DAG branches/merges, concurrent assessments on unchanged revisions, research thread indexing, blocker/next-action tracking |
| [`claim-chain-minimum-v1`](./claim-chain-minimum-v1/) | Baseline ClaimChain profile (`minimum-v1`) | Exact frozen revision references, weak connectivity (single component), active relations required, cycle rejection, source closure calculation |
| [`claim-chain-evidential-v1`](./claim-chain-evidential-v1/) | Evidential ClaimChain profile (`evidential-v1`) | Evaluative/derivational paths (supports, weakens, contradicts, consistent-with), multi-input explicit `Synthesis` with `input-to`, direct single-input |
| [`claim-chain-confirmatory-v1`](./claim-chain-confirmatory-v1/) | Full Confirmatory ClaimChain profile (`confirmatory-v1`) | Confirmatory backbone, predeclaration before analysis/result computation, artifact-backed provenance |
| [`access-closure-v1`](./access-closure-v1/) | Access classification, structural closure & export guard | Required dependency floor versus effective access, 4-level lattice, compartment union, revision/relation/Assessment/Thread/Binding/ClaimChain traversal, exclusive branch, and unconditional downgrade rejection |
| [`freshness-v1`](./freshness-v1/) | Additive Phase 1 derived freshness | Hand-audited positive/negative `fresh`, `review-due`, `stale`, and `unknown` cases with deterministic `as_of` and precedence boundaries |

## 3. Structural Design Principles

1. **Modular Scope Separation**: Each fixture module contains only the semantic nodes and relations required to verify its specific profile or domain.
2. **Self-contained project roots**: Each module's `valid/` directory simulates an independent Git Project root. Every referenced `file:` Artifact is contained below that root; fixtures do not use `../` traversal or shared external bytes. The five G7 projects that use `fixture.synthetic/v1` contain the matching local extension schema, while allowlist-only `legacy-import/v1` intentionally has no schema file.
3. **Single-Mutation Overlays**: Negative testing uses targeted mutation descriptors over the valid baseline rather than copying complete repository trees for every negative case.
4. **No Governance / Declassification in MVP**:
   - `GovernanceAttestation`, `DeclassificationAttestation`, and governance standing are deferred to post-MVP (confirmed G5-Q3, G5-Q4).
   - All access downgrade attempts are rejected unconditionally in MVP (no declassification bypass).
5. **Frozen G7 baseline plus additive fixture**: The original five modules and
   all 176 canonical G7 objects remain byte-for-byte frozen. No aggregate
   `prototype-skeleton.yaml` remains. `overview-v1` supplies explicit
   Observation, Decision, PaperClaim, and relation-target Assessment examples,
   giving positive schema coverage to all 15 built-in kinds and both Assessment
   target variants. `freshness-v1` is additive Phase 1 test data and does not
   rewrite the G7 fixture history.

## 4. Overlay Materialization Contract

1. Each `overlay-*.yaml` descriptor introduces exactly one intentional semantic
   mutation over a temporary copy of its module's `valid/` baseline.
2. If that mutation changes a selected object or ClaimChain selection, the
   descriptor may request deterministic integrity maintenance through
   `maintenance.recompute_source_closure_sha256`. Recalculating the persisted
   closure digest is not a second semantic mutation.
3. If the intentional mutation makes closure resolution impossible—for example,
   replacing an exact revision ID with a logical ID—the descriptor must declare
   the unavoidable secondary integrity Finding instead of fabricating a digest.
4. `rp-fixture-test` applies the semantic mutation, performs only declared
   maintenance, then compares the validator's structured primary and
   allowed-secondary Findings. Overlay descriptors are never canonical facts.
5. Overlay descriptors validate against `test-overlay.schema.json`; derived
   oracles validate against `fixture-expectations.schema.json`. These test
   schemas are indexed outside canonical Layer B dispatch.
