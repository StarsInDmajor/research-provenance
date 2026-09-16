# Research Provenance Phase 0 Security and Threat-Model Review

> Status: **G7-approved Phase 0 review with frozen Phase 1 Task 0 amendments**
>
> Scope: Layer B restricted YAML, Draft 2020-12 schemas, standalone Phase 1A
> validation/query/export contracts, fixtures, and benchmark-generator design.
>
> Verdict: **PASS at design level with mandatory implementation requirements**

## 1. Security Claim Boundary

The MVP is a local safety and integrity tool. It aims to prevent accidental
leakage, malformed-input hazards, provenance loss, unsafe export, and silent data
corruption. It does **not** claim confidentiality, non-repudiation, or
adversarial tamper resistance against a user or process that already controls
the repository, working tree, shell account, validator binary, or signing
configuration.

Git commits, YAML files, local SQLite projections, and unsigned Ledger seals are
not a security boundary against that attacker. Strong publication assurance
requires a separately approved server/CI policy gate and protected external
signing keys; it is post-MVP.

## 2. Assets

- canonical semantic objects and immutable revision history;
- source and Artifact identity, size, and digests;
- access labels, compartments, and export decisions;
- provenance links and ClaimChain source-closure digests;
- researcher notes and potentially restricted Artifact paths;
- generated projections and temporary validation materializations;
- later Layer A normalized event facts.

Canonical facts must never directly contain secrets, full raw prompts or
conversations, hidden reasoning, base64 media, unrestricted tool payloads, or
secret-bearing command output.

## 3. Trust Boundaries

| Boundary | Trust position |
|---|---|
| Repository bytes → restricted YAML parser | Untrusted input |
| Parsed object → Draft 2020-12 validator | Structurally untrusted |
| Schema-valid objects → project graph | Semantically untrusted |
| Artifact URI/path → filesystem | Untrusted path and mutable bytes |
| Project extension schema → schema engine | Untrusted local policy input |
| Canonical graph → human/JSON/HTML output | Untrusted display content |
| Graph → export bundle | Confidentiality boundary enforced by effective access |
| Adapter/native session → future Layer A ingest | Untrusted producer input |
| Local repository owner/shell | Outside adversarial protection claim |

## 4. Required Controls

### 4.1 Restricted YAML Parser

The parser must:

- accept strict UTF-8 only, with at most an optional UTF-8 BOM;
- accept exactly one YAML document and one non-null top-level mapping per file;
- reject extra `---`/`...` document boundaries and scalar/sequence roots;
- reject duplicate keys before construction;
- reject anchors, aliases, merge keys, complex keys, and custom tags;
- use a YAML 1.2 Core scalar model without language-specific object creation;
- bound file bytes, scalar length, collection length, nesting depth, and total
  parsed bytes before or during construction;
- avoid implicit timestamp or arbitrary precision behavior that changes the
  canonical parsed value across runtimes;
- never execute constructors or deserialize application objects.

These controls prevent alias expansion, duplicate-key shadowing, constructor
execution, and parser memory/CPU exhaustion.

### 4.2 JSON Schema Engine

The validator must:

- support Draft 2020-12 and enable format assertion;
- load the built-in schema catalog offline from trusted package resources;
- disable network retrieval for `$ref` and metaschemas;
- permit project extension schemas only from declared project-contained files;
- treat Project `schema_policy.allowed_extensions` as an allowlist, not as a
  demand that every allowed namespace have a local schema file;
- require and validate a local extension schema only when an object contains an
  `extensions[namespace]` payload or a NodeRevision uses an extension kind from
  that namespace/version;
- reject extension-schema `$ref` values that escape the project schema root or
  use remote schemes;
- bound schema bytes, reference depth, evaluated branches, regex input length,
  errors, and wall-clock time;
- map schema findings to stable v1 error codes without exposing host paths.

The candidate built-in patterns are static, anchored, and avoid nested
catastrophic quantifiers. Runtime selection must still test the chosen regex and
schema engine under adversarial-length strings.

### 4.3 Paths, Artifacts, and Filesystem Races

Project discovery must reject symlinked record files and directories under
`.research/`, avoid following directory symlinks, and maintain visited filesystem
identities where the platform permits so scanning cannot escape the Project root
or loop.

For `file:` Artifacts and policy/narrative/schema paths, Core must:

1. reject absolute paths, empty paths, NUL, `.`/`..` segments, and encoded forms
   that become traversal after percent decoding;
2. resolve relative to an already opened Project root, not process CWD;
3. prevent symlink escape using descriptor-relative/openat-style traversal or an
   equivalent no-follow strategy;
4. verify the final opened descriptor remains below the Project root;
5. stream digest computation with byte and time limits;
6. compare declared and observed size before accepting the digest;
7. detect replacement/truncation during validation by validating one open file
   descriptor and checking stable metadata where supported;
8. never shell-expand, glob, or execute Artifact URIs.

`https:` Artifact and ExternalReference URIs are identifiers only during normal
Phase 1A validation. Core must not fetch them implicitly. Any future explicit
network fetcher requires a separate SSRF policy: HTTPS only, DNS/IP re-check,
private/loopback/link-local denial by default, redirect limits, response-size
limits, and explicit user action.

Policy files must exist inside `.research/policies/`. Narrative references must
exist inside `.research/notes/` and match their declared SHA-256 digest. Project
extension schemas use the deterministic path
`.research/schemas/<namespace-path>/vN.schema.json`, where namespace dots become
path segments—for example `astronomy.example.org/v1` resolves to
`.research/schemas/astronomy/example/org/v1.schema.json`. An extension kind's
namespace must exactly match one key under `extensions`; an extension payload or
extension-kind use with an undeclared, missing, invalid, remote-referring, or
escaping schema fails validation. Merely allowlisting an otherwise unused
namespace does not require the schema file to exist.

### 4.4 Reference and Graph Integrity

Whole-project validation must reject:

- duplicate object IDs and filename/ID mismatches;
- dangling or wrong-type exact IDs;
- NodeRevision parents outside the same `logical_id` lineage or with a different
  built-in/extension kind;
- ScientificRelationRevision parents outside the same `logical_id` lineage or
  with a different relation `type`;
- ThreadBinding parents outside the same `logical_id`, `thread_id`, and exact
  target context;
- Assessment supersession outside the lane keyed by exact target,
  `assessment_scope`, assessor type/id, and `review_assurance`, or any
  timestamp-based implicit winner selection;
- mutable logical IDs in frozen endpoint or ClaimChain selections;
- revision, relation, thread, or ClaimChain cycles where prohibited;
- invalidated relations selected as active evidence;
- relation endpoint kind incompatibility;
- unresolved extension-kind compatibility bases;
- source-closure digest mismatches, using every parsed field in each canonical
  object digest with no implicit presentation-field exclusion;
- Artifact digest/size mismatches;
- canonical `.research/` objects with `record_state: draft`.

Every traversal requires explicit visited sets plus depth, row, and deadline
bounds. A malformed graph must produce bounded Findings rather than recurse
unboundedly.

### 4.5 Access and Export

- Access is computed from every exact semantic reference that can reveal another
  object, including but not limited to the frozen fixture dependency classes.
  Extension schemas declare extension-specific semantic IDs through the
  `x-rp-semantic-references` keyword (`pointer_template`, `target_schema`); every
  extension scalar matching a canonical exact-ID pattern must be covered by a
  valid declaration or the payload is rejected fail-closed.
- The required dependency floor excludes the object; effective access joins the
  declaration with that floor. `dependency_count` is the unique transitive
  dependency count excluding the target.
- Dependency explanations and invalid-object witnesses use shortest path, then
  frozen policy edge order, then ID. Emit one primary access Finding per invalid
  object; its message may summarize all level and compartment deficits.
- Declared access must dominate the floor. Every downgrade or inherited
  compartment removal is rejected without exception in MVP.
- `DeclassificationAttestation` and governance bypass fields are closed-schema
  errors in v1.
- Export eligibility requires both an adequate level ceiling and authorization
  for every effective compartment.
- Export assembly must re-check every emitted object's effective access and the
  transitive bundle closure; it must fail closed rather than silently omit a
  dependency and produce a misleading bundle.
- Derived validation, overview, and `access explain` output intended for a less
  privileged audience must not leak restricted titles, paths, counts, IDs, or
  the existence of hidden targets.

The local CLI does not define user authentication. Its access request is a
release/export policy decision, not a claim that local filesystem readers are
cryptographically isolated from repository contents.

### 4.6 Output and Projection Safety

Human-readable terminal output must escape or replace control characters,
including ANSI/OSC sequences and bidi-control characters where they could spoof
labels. JSON mode emits valid JSON escaping and exactly one result object.

Every HTML/SVG/Markdown projection treats titles, statements, tags, URI labels,
and extension values as untrusted text. Renderers must use context-appropriate
escaping and must not insert canonical strings as raw HTML, script, CSS, URL, or
shell fragments. External links require scheme allowlisting and safe browser
attributes.

SQLite, HTML, graph, and summary projections remain disposable and never become
a second editable authority.

### 4.7 Temporary Files and Atomicity

- Draft and fixture materialization roots use user-private directories (`0700`)
  and files (`0600`) unless a stricter platform default applies.
- Writes use same-filesystem temporary files, flush as required, validate, then
  atomic rename.
- No canonical file is modified in place.
- Temporary paths are unpredictable, not followed through symlinks, and cleaned
  on normal exit and interruption.
- `rp-fixture-test` may mutate only its temporary copy and may execute only the
  descriptor-declared integrity maintenance operation.

### 4.8 Resource Exhaustion

Phase 1A must enforce limits for:

- scanned files and directories;
- YAML bytes per object and total bytes;
- nesting depth, scalar length, arrays, and mapping keys;
- graph nodes, edges, traversal depth, rows, and Findings;
- Artifact bytes and hashing duration;
- query result rows and rendering size;
- schema evaluation and regular-expression time;
- whole-command wall time and cancellation.

Concrete defaults and hard ceilings are frozen in `resource-limits.yaml`; the
acceptance and conditional Phase 1B benchmark thresholds are frozen in
`benchmark-budget.yaml`. Absence of enforcement or an unbounded override is not
acceptable.

### 4.9 Phase 1A Provenance-Mode Boundary

Layer A Event objects and event-byte verification do not exist in Phase 1A.
Therefore `execution_provenance: event-backed` and `artifact-and-event`, and any
non-empty `source.events`, fail with `RP_E_EVENT_PROVENANCE_UNSUPPORTED`; they
must not be silently treated as absent or artifact-backed.

For `execution_provenance: artifact-backed`, every selected Observation and
Measurement result must directly reference at least one `ArtifactManifest` whose
URI uses `file:` and whose local bytes, size, and digest were verified during the
same bounded validation. An `https:` Artifact remains identifier-only and cannot
satisfy this requirement.

## 5. Deferred Attack Surfaces

The following are explicitly outside Phase 1A and require a fresh security
review before implementation:

- Layer A Event/WriterInstance/Segment/seal persistence;
- Pi, DSH, Cordis, or other Harness adapters;
- native session import and Compaction reconciliation;
- archive/bundle extraction;
- remote schema or Artifact retrieval;
- browser mutation UI;
- SQLite as anything other than a disposable projection;
- signatures, governance approval, and declassification;
- multi-user service or SaaS exposure.

Future adapters must treat Pi tree JSONL and DSH/Cordis data as untrusted native
formats. Core Schema remains independent; adapters normalize facts without
persisting hidden reasoning or secret-bearing raw payloads.

## 6. Residual Risks

| Risk | Residual status |
|---|---|
| Repository owner maliciously rewrites facts and digests | Accepted and explicitly out of scope for local MVP |
| Local user can read files above their export ceiling | Access labels are release policy, not local OS isolation |
| Implementation-specific parser/schema differences | Mitigated by cross-runtime fixtures and Runtime ADR benchmarks |
| Very large valid repositories consume resources | Mitigated by frozen mandatory bounds and explicit bounded overrides; acceptance remains benchmark-verified |
| Human text visually spoofs terminal/UI output | Requires renderer escaping and control-character policy |
| External Artifact changes after validation | Digest detects later mismatch; no perpetual freshness guarantee |
| Git merge creates semantically plausible but incorrect science | Validator detects structure, not scientific truth |

## 7. Review Result

At the Phase 0 design level:

- no secret or credential material is required by the schema or fixtures;
- no service, network listener, authentication surface, privileged process, or
  deployment change is introduced;
- unsafe YAML features and closed-schema bypasses are explicitly rejected;
- filesystem, schema-resolution, output-injection, resource-exhaustion, and SSRF
  controls are mandatory implementation acceptance requirements;
- access downgrade and declassification bypass are fail-closed;
- adversarial tamper-resistance claims remain explicitly excluded.

Security review passed at G7. These controls are part of the approved Phase 0
baseline, and implementation code must be reviewed again before separate Phase
1 authorization or any deployment.
