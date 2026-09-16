# Research Provenance Workbench (`rp`)

`rp` is a standalone CLI for offline validation and deterministic navigation of
Git-tracked `.research/` projects. It embeds the frozen v1 schemas. The package
also ships `rp-view` (static HTML generation), `rp-lookup` (read-only
title/ID lookup) and `rp-server` (a live graph server: `/api/wire`,
`/api/records`, SSE hot-reload on project changes — start manually when
needed). The canonical Rust CLI remains `rp`; the others are separate thin
launchers, not subcommands.

Migrated 2026-09-16 from nixos-config (`pkgs/misc/research-provenance`);
full history up to the split lives there.

## Initialization

`rp init --project <path> [--json]` initializes an ordinary existing directory,
including a nonempty Git repository (`.git` directory or worktree file), or creates
only the final missing root component beneath an existing parent. Git is neither
required nor invoked; no Git metadata or unrelated files are created or changed.
Existing bytes, symlinks and permission modes are preserved, including root modes.
Any existing `.research` entry (even a dangling symlink) is a conflict. Symlinked
roots or path components and `..` traversal are rejected without following them.

On Linux, init uses no-follow `openat2` containment, pinned directory descriptors,
a bounded private staging subtree, and `renameat2(RENAME_NOREPLACE)` publication.
New directories use `0700`, files `0600` (or stricter umask defaults). Concurrent
initializers cannot overwrite one another. Prepublication failures leave no partial
`.research`; cleanup visits only recorded owned entries through pinned parents,
not arbitrary directory trees. A root created before failure may remain empty and
private; uncertain/replaced staging entries are left rather than deleting foreign
data. Abrupt termination can leave private staging debris. A postpublication sync
failure leaves the complete published subtree in place, not a rollback.

Init rechecks the root pathname's identity before publishing and rejects detected
root/ancestor substitutions. If a rename happens after that check, publication
remains attached to the originally opened directory, possibly at its new name;
the replacement at the old path is not the destination. This is atomic publication
of **one init subtree**, not a general multi-file transaction, nor protection
against an adversary controlling the same account/repository. Linux `openat2` and
no-replace rename support are required; unsupported platforms fail closed.

## Cooperative execution budget

The CLI starts **one 60-second monotonic budget** before command dispatch and
retains it through validation, navigation and output. This is **not a hard 60s
wall-clock guarantee**. There is no new CLI override or OS signal handler.
Embedders can supply `ExecutionBudget` to `validate_project_with_budget` or
`initialize_project_with_budget`, retaining an `Arc<AtomicBool>` cancellation
token. Caller durations are clamped to 1ms–600s; clones share the original stop
latch and clock. Injected elapsed clocks support deterministic tests. The first
observed stop is sticky; cancellation wins if cancellation and expiry are both
observed at the same checkpoint.

Stopped validation is incomplete and withholds its index. Deadline produces
`RP_E_RESOURCE_DEADLINE_EXCEEDED` / `invalid` / exit 1; caller cancellation produces
`RP_E_INTERRUPTED` / `interrupted` / exit 130. CLI terminal results have `data:null`.
These are not findings-cap truncation warnings. Nested report helpers share the
state: a stopped subject cannot publish a body or matched-success binding.

Checkpoints cover scan entries, canonical read chunks, YAML preflight/scanner/
event construction, schema call boundaries, existing validation diagnostic gates,
content hash chunks, strict report JSON value nodes, access queues/edges, graph
cycle stacks, claim repeated traversal scans, subject reservations, navigation
boundaries/query rows/diff recursion and render writes. CLI uses typed checked
thread-list/access-explanation paths. **This is not universal coverage of every
legacy core API or inner allocation/helper scan**: Option/infallible getters are
not typed cancellation entrypoints, and some collections/sorts/JCS/DTO work is
checked only at surrounding boundaries. The original pre-DTO allocation/resource
limits are not thereby solved.

In particular, jsonschema **0.54 `Validator::iter_errors` eagerly fills a `Vec`
before returning an iterator**, with no public evaluation callback. Checkpoints
cannot interrupt in-progress schema compilation/evaluation, eager error
allocation, regex execution or blocking syscalls; schema branch-evaluation and
eager-allocation ceilings remain unimplemented. `NONBLOCK` on contained target
opens prevents FIFO-open stalls before regular-file checks, not arbitrary disk or
filesystem stalls.

JSON is serialized once into a checking byte-capped buffer (32 MiB including its
newline). On stop/overflow, no ordinary bytes have reached stdout: they are
discarded and one small terminal/error envelope is selected without replenishing
ordinary findings/output budgets. Human rendering also uses a bounded buffer.
Once selected, output is written to completion without mid-write cancellation;
blocking stdout remains an external limitation. Init checks before staging,
between steps and before publishing; a stop **after publication leaves the tree
in place**, even when no success report is returned. Existing safe ownership-based
staging cleanup remains unchanged.

## Nix package

From the repository root:

```bash
nix build .#research-provenance
nix run .#research-provenance -- --help
```

The production derivation uses the committed `Cargo.lock`, vendors crates through
`rustPlatform.buildRustPackage`, and performs Cargo operations offline. The installed commands are exactly
`bin/rp`, `bin/rp-view`, and `bin/rp-lookup`; `rp-fixture-test` and
`rp-scale-generate` remain workspace test tools. The reader uses Python stdlib
only and exactly ten allowlisted source/resources under
`share/research-provenance/reader`. No tests, bytecode caches, pilot pins, case
admissions or private source excerpts are installed.

```bash
# After building; or use these names from your chosen Nix profile/package set:
./result/bin/rp-view --project /path/to/project --output /private/graph.html
./result/bin/rp-lookup --project /path/to/project 'Exact full title'
# Optional shared selectors: --thread <exact-thread-id> --as-of <RFC3339>
# rp-view only: --include-local-sources --force
```

`rp-view` writes self-contained HTML and prints a JSON generation report; it does
not open anything. `rp-lookup` prints JSON for exact full titles, exact revision
IDs or logical IDs (single head selected, concurrent heads ambiguous). It exits
0 for selected, 2 for ambiguous/not-found or usage errors, 1 for rejection.
Neither launcher accepts `--rp`, `--case-manifest`, `--before` or abbreviated
options. Developer `build.py`/`lookup.py` interfaces and pinned regression modes
are unchanged in the source tree.

Launchers resolve their real installed location (including profile symlinks) to
use the paired `bin/rp` and reader resources. Their Nix-store Python interpreter
runs isolated (`-I`), ignoring `PYTHONPATH`, `PYTHONHOME`, user site and CWD import
paths; only the installed resource directory is added explicitly. No source
checkout, debug binary or caller PATH lookup is needed. Copying the whole package
works on the same supported Linux system while its Nix-store dependencies remain
available; this is not standalone distribution or cross-OS portability.

### Reader boundaries

- Trusted-local bounded projects only, **not audience-filtered/public export**.
  Artifact bodies are metadata-only by default; canonical text and raw URI
  metadata can still be private. Opt-in local excerpts are verified and bounded
  to 64 KiB/file and 512,000 bytes total; external URIs are not fetched.
- Core snapshots admit at most 100 semantic revisions, 300 scientific relation
  revisions, 512 canonical records and 2,000,000 canonical bytes. Reader limits
  can reject smaller projects: 64 KiB/input file, HTML below 500,000 bytes;
  lookup 256-character query, eight displayed candidates, 60 one-hop relations,
  250 KB output. These are not unrestricted arbitrary-project support.
- The current reader requires one selected thread: omit `--thread` only when
  exactly one exists. `--as-of` defaults to current UTC. Titles and canonical IDs
  are generic; short case aliases are not universal.
- Use a private `.html` destination outside the project/installed resources.
  Newly created parent directories are 0700; an existing immediate parent must
  already be owned 0700. HTML is 0600. Symlinks, hardlinked/nonprivate existing
  files and input overlap are rejected. `--force` permits unrelated private
  output replacement, not bypassing those protections. Existing reader
  recognition uses markup/legacy-name heuristics, not cryptographic provenance.
- Repeated observations detect observed changes, not atomic project snapshots
  or adversarial TOCTOU immunity. Sources remain untrusted text. Automated tests
  do not establish browser visual/usability acceptance.

See [reader developer/history documentation](tools/pilot-reader/README.md).

The package derivation is also reused by the `research-provenance-package`,
`research-provenance-workspace`, and `research-provenance-quality` flake checks,
so the release build, formatting, Clippy, dependency policy, and workspace tests
do not create duplicate large derivations. Additional cheap checks exercise the
installed binary against positive fixtures, CLI/resource limits, and a scan-order
permutation. `installCheckPhase` also executes both installed reader commands
from scratch directories with two neutral synthetic block-YAML fixtures, hostile
CWD/Python environment, metadata/excerpt checks, title/exact/logical queries,
relocated and profile-symlink launchers, and failure/file-preservation checks.
The standalone smoke harness needs only stdlib Python:

```bash
python3 -I tools/pilot-reader/tests/installed_reader.py \
  --package "$(readlink -f result)" -v
```

## Development checks

```bash
M=Cargo.toml
cargo fmt --manifest-path "$M" --all --check
cargo clippy --manifest-path "$M" --workspace --all-targets -- -D warnings
cargo test --manifest-path "$M" --workspace
cargo deny \
  --manifest-path crates/rp-cli/Cargo.toml \
  --exclude-dev --offline --locked \
  check --config "$(pwd)/deny.toml" bans sources
```

The frozen fixtures and source schema snapshots live under
`docs/plans/research-provenance-{fixtures,schemas}/`. Embedded schema parity and
all 38 declared negative overlays are covered by workspace tests.

## Production dependency boundary

The production graph is rooted at `rp-cli` and follows normal dependencies into
`rp-core`; workspace test tools and dev-dependencies are excluded. `deny.toml`
rejects:

- network clients and transports;
- TLS implementations and platform TLS adapters;
- SQLite/database projection crates;
- async executors and runtimes;
- `jsonschema` network/TLS resolver features;
- non-crates.io registry and Git dependencies.

`jsonschema` is built with default features disabled. Consequently its HTTP,
async retrieval, and TLS features are not enabled. Nix sandboxing plus the
committed lock file and crate checksums provide offline reproducibility without a
RustSec flake input or a `flake.lock` change.

Security advisory review is intentionally not represented as an offline advisory
database snapshot in Task 12. It can be run separately with nixpkgs
`cargo-audit`/`cargo-deny` when a current advisory database is available, without
adding a flake input.

## Scope status

Phase 1A is implemented but **not fully accepted**. The 2026-09-07 run passed
93 workspace tests and focused Nix checks; workstation p95 was 3.81–3.97 seconds,
but 515.13 MiB peak RSS exceeded the frozen 512 MiB gate. Stress is not yet
supported by the acceptance runner. A subsequent holistic review reproduced a
canonical-dispatch validation gap and identified additional contract coverage
work: passing fixtures do not establish complete correctness/security.

See the [current review and proposed roadmap](../../../docs/plans/2026-09-07-research-provenance-revised-roadmap.md).
Authoring, UI, cache and adapter proposals there await confirmation; they are not
implemented features. Current CLI validation checks the working tree, not Git
history immutability, and trusted local inspection is not an audience-filtered
export interface.
