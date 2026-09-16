# Research Provenance v1 JSON Schema Bundle

> Status: **G7-approved Phase 0 canonical baseline with frozen Phase 1 Task 0 executable contracts**
>
> Dialect: **JSON Schema Draft 2020-12**

This directory contains the machine-readable single-document validation layer
for the Research Provenance Workbench. It is planning baseline data, not an
installed package or validator implementation.

## Entry Points

| File | Responsibility |
|---|---|
| `layer-b.schema.json` | Union of the nine standalone Layer B canonical object schemas |
| `fixture-object.schema.json` | Layer B plus the temporary `ResearchRun` bridge used by Phase 0 fixtures |
| `schema-catalog.json` | Canonical Layer B/fixture dispatch plus explicit noncanonical contract index |
| `presentation-order.yaml` | Canonical writer field order; parser and JCS remain order-independent |
| `validation-stages.yaml` | Parser → Schema → gated whole-project semantics and frozen semantic clarifications |
| `finding-registry.yaml` | Complete stable v1 Finding code, stage, severity, and family registry |
| `resource-limits.yaml` | Default and hard/test ceilings for scans, parsing, schemas, graphs, Artifacts, queries, output, and deadlines |
| `benchmark-budget.yaml` | Phase 1A acceptance thresholds and conditional Phase 1B trigger |
| `cli-contract.yaml` | Standalone Phase 1A commands, streams, result envelope, query bounds, and exit codes |
| `scale-generator-contract.yaml` | Deterministic smoke/workstation/stress generation algorithms and manifest rules |
| `security-review.md` | Threat boundaries and mandatory parser, schema, path, export, output, temp-file, and resource controls |
| `common.schema.json` | Shared IDs, timestamps, actors, access, sources, revisions, narrative, and extension definitions |

Layer B object schemas:

1. `project.schema.json`
2. `node-revision.schema.json`
3. `assessment.schema.json`
4. `scientific-relation-revision.schema.json`
5. `research-thread.schema.json`
6. `thread-binding.schema.json`
7. `claim-chain-snapshot.schema.json`
8. `external-reference.schema.json`
9. `artifact-manifest.schema.json`

`research-run.schema.json` is explicitly a Layer A bridge schema. It exists
because the Phase 0 overview and confirmatory fixtures contain logical Run
metadata, but it does not introduce WriterInstance, Segment, Event, ingest,
adapter, or live-capture contracts into Layer B.

## Noncanonical Test and Command Contracts

The following Draft 2020-12 schemas are versioned executable contracts but are
**not** canonical Layer B objects and are absent from `layer-b.schema.json` and
`fixture-object.schema.json` dispatch:

- `finding.schema.json` — `rp/finding/v1` wire shape;
- `cli-result.schema.json` and `cli-data.schema.json` — exactly-one command
  result envelope and command-specific data DTOs;
- `test-overlay.schema.json` — one-mutation fixture descriptors;
- `fixture-expectations.schema.json` and
  `access-closure-expectations.schema.json` — derived fixture/oracle documents;
- `freshness-policy.schema.json` — project-contained freshness policy files;
- `scale-manifest.schema.json` — generated benchmark corpus manifest.

Their paths are indexed under `contracts` in `schema-catalog.json`; they are not
added to the canonical `schemas` dispatch map.

## Closed-Schema Rule

Every concrete object and nested structured payload is closed with
`additionalProperties: false`. The only general extension point is:

```yaml
extensions:
  namespace.example/v1:
    extension_field: value
```

The namespace must also be declared in the Project's
`schema_policy.allowed_extensions`. The allowlist entry alone does not require a
schema file. JSON Schema checks namespace syntax; whole-project Core validation
resolves and validates the local schema when an extension payload or extension
kind is actually used, through the deterministic project-contained mapping
`.research/schemas/<dot-separated-namespace-as-path>/vN.schema.json` (for
example `astronomy.example.org/v1` maps to
`.research/schemas/astronomy/example/org/v1.schema.json`). Remote or escaping
`$ref` targets are forbidden.

`node-revision.schema.json` includes all 15 domain-neutral built-in payloads in
`rp/kinds/v1` and uses a discriminated `oneOf` to require exactly the payload and
ID prefix corresponding to `kind`. Namespaced extension kinds use `node_` IDs
and store their structured payload under `extensions`.

## Draft 2020-12 Format Contract

Validators MUST enable format assertion, not annotation-only handling. Critical
formats also carry explicit patterns where practical:

- typed Crockford Base32 ULIDs;
- RFC 3339 date-time strings with explicit `Z` or numeric offset;
- lowercase SHA-256 digests;
- lowercase-slug logical IDs and compartments;
- MIME media types;
- project-relative policy and Artifact URI shapes.

Restricted YAML parsing happens before JSON Schema validation. Duplicate keys,
anchors, aliases, merge keys, complex keys, and custom tags therefore remain
parser errors, not JSON Schema findings.

## JSON Schema / Semantic Core Boundary

Draft 2020-12 validates one parsed object:

- required and unknown fields;
- scalar types and patterns;
- ID prefix and kind-payload discrimination;
- enum vocabularies;
- nullable fields;
- non-empty payload criteria;
- unique arrays;
- closed nested mappings.

Standalone Core still performs whole-project semantics that JSON Schema cannot
establish:

- duplicate IDs and dangling-reference resolution;
- extension allowlist and extension-schema resolution;
- canonical `.research/` frozen-only enforcement;
- Revision and Relation DAG checks;
- relation endpoint kind compatibility;
- complete access dependency closure across every exact semantic reference,
  unique dependency counts, canonical witnesses, and sorted compartment order;
- Thread and ThreadBinding integrity;
- ClaimChain connectivity, chronology, profile, and Synthesis rules;
- Artifact path containment, byte size, and digest verification;
- full-field RFC 8785/JCS source-closure recomputation;
- export eligibility against effective access.

## Fixture Coverage

All 176 G7 canonical candidates remain byte-for-byte unchanged and validate
against this bundle. The additive `freshness-v1` fixture is separate and does
not rewrite G7 history. The original
172-object fixture suite was extended in `overview-v1` with explicit positive
NodeRevision examples for `Observation`, `Decision`, and `PaperClaim`, plus an Assessment
whose target is a `relation_revision`. This gives positive payload coverage for
all 15 built-in kinds and both Assessment target variants.

The related negative overlays distinguish validation stages:

- restricted-YAML parser: duplicate key;
- Draft 2020-12: closed field, missing Observation field, invalid Decision enum,
  empty PaperClaim locator, mismatched Assessment target type/ID, unknown access
  level, unsupported declassification field, and logical ID used where an exact
  revision ID is required;
- semantic Core: references, graph topology, relation compatibility, access
  closure, profile, chronology, provenance, and compartment ordering.

## Candidate Decisions Frozen by This Bundle

- Method IDs use `mth_`, not the earlier example typo `meth_`.
- `scope` is a closed generic mapping supporting `statement`, `target`,
  `research_run_id`, `dataset_revisions`, `conditions`, and `exclusions`.
- Relation scope requires `conditions` and `exclusions`, plus at least one of
  `statement` or `aspect`.
- Revision parent `change_type` is optional metadata; parent `id` is required.
- Canonical decimal measurements and quantitative Assessment values are strings.
- `Assessment.target.type` supports `node_revision` and `relation_revision` with
  discriminated ID prefixes.
- `ResearchThread` parent/fork fields may be omitted or explicitly null.
- `ExternalReference.source_version` is required; release time and digest remain
  optional/nullable.
- MVP Artifact manifests admit project-relative `file:` and `https:` URIs;
  containment and allowlisting remain semantic checks.
- Portable object schemas permit `draft` where the authoring model supports it,
  while canonical `.research/` project validation requires `frozen`.
