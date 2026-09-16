# Private RP pilot reader v5 + isolated CSP candidate spike

## Installed generic commands (current packaging)

The `research-provenance` Nix package now installs `rp`, `rp-view`, and
`rp-lookup`. Use `rp-view --project /path/to/project --output /private/graph.html`
or `rp-lookup --project /path/to/project 'Exact full title'`. Both accept
`--thread` and `--as-of`; view alone accepts `--include-local-sources`,
`--node-statuses`, `--source-locators` and `--force`. These generic-only entry points reject `--rp`, `--before` and
`--case-manifest`; they select their paired installed `rp` automatically. They
build HTML / return JSON only, without starting a browser or server.

See the [package README](../../README.md#nix-package) for installation, exit
codes, privacy/bounds, output protections and installed checks. Python runs with
`-I` and explicit installed-resource imports; no checkout, debug binary, caller
CWD/PATH or Python environment is needed. Only the twelve generic runtime files
are installed, not tests, pins, admissions or private source excerpts. Existing
`build.py` and `lookup.py` developer interfaces below remain unchanged, including
explicit pinned-case regression modes. Historical no-install statements below
refer to those earlier deliveries, not current packaging. Browser acceptance is
still a separate task.

## Optional domain/tag navigation

The toolbar's **领域** button opens a nonmodal **领域/标签** list inside the
fullscreen workspace, separate from node details. Membership is derived at
runtime from the existing canonical node `tags`; no per-node wire fields,
scientific edges, roles or layout coordinates are added. A small display-only
Chinese vocabulary labels recognized tags; all other tags (including workflow
tags) remain verbatim under **其他标签**, and nodes without tags appear under
**未标记**. Each group counts unique exact revision IDs; multi-tag membership
is intentional and group totals must not be summed.

The list defaults to current revisions and shows explicit current/history
counts. Its history checkbox is independent of the graph history filter;
historical-content presentation labels remain distinct from revision status.
Browsing groups does not navigate or reroute. Choosing a full-title node uses
ordinary exact-ID reveal (not forced one-hop focus), closes auxiliary panels,
and lets Back restore the group, list scroll and exact domain/node focus key.
Escape exits fullscreen first, then closes the list without clearing graph
selection. These are automated structure/state contracts, not browser or
keyboard visual acceptance.

## Explicit historical-content presentation (optional, generic view only)

`rp-view --project /local/project --output /private/graph.html --node-statuses /private/statuses.json`
opts in to a caller-authored **presentation mapping**, not a Core schema,
Assessment, approval registry or automatic science-lifecycle classifier. No file
is auto-discovered in the project. Absence leaves content status pending; even a
frozen latest revision with unknown freshness is not automatically historical.
The original project and canonical raw JSON remain unchanged.

Protocol v1 is a strict JSON object, <=65536 UTF-8 bytes and <=100 entries:

```json
{
  "version": 1,
  "project_id": "proj_EXACT",
  "entries": {
    "obs_EXACT": {
      "revision_id": "obs_EXACT",
      "canonical_digest": "sha256:<64 lower-case hex digits>",
      "status": "historical",
      "label": "历史记录",
      "reason": "Dated documentary evidence and its limited scope; not a validity judgement.",
      "assessed_at": "2026-09-12T15:00:00Z",
      "source_refs": [{
        "id": "art_EXISTING",
        "canonical_digest": "sha256:<64 lower-case hex digits>",
        "locator": "source-index entry; relative packet path; original path/lines/digest"
      }]
    }
  }
}
```

Digest format is **reader-canonical-json-v1**: SHA-256 of the entire parsed
canonical record serialized with `build.compact(record).encode('utf-8')`
(`json.dumps(ensure_ascii=False, sort_keys=True, separators=(',', ':'))`). It is
explicitly **not Core RFC8785/JCS**, and not the YAML/packet file-byte hash. This
separate presentation identity handles any JSON number using the reader's
serialization; do not substitute a Core report digest. Source refs bind the
complete existing ArtifactManifest/ExternalReference record, including its
source digest/URI metadata. Locator text documents the audited packet/path/lines;
the loader **never follows it**, nor claims to reverify external originals or
packet bodies in metadata-only mode. Use original artifact SHA plus source-index
locators and a separately recorded manual byte audit when authoring the mapping.

Allowed pairs: `historical` → `历史方案` or `历史记录`; `superseded` → `已替代`;
`current` → `材料声明现用` (dated evidence only, not global current acceptance).
No `superseded_by` support in v1: relationships must not be invented. Reason is
1–2000 characters, source refs 1–8 distinct IDs with 1–1000 character locators,
`assessed_at` a timezone-aware ISO timestamp of the documentary check, **not a
scientific event date**; `--as-of` remains the independent freshness evaluation.
Unknown fields, duplicates, wrong project/revision/digest/source IDs, oversized
input, lexical `..`, symlinks and special files are rejected. The whole mapping
validates before projection and its bytes are rechecked before and through atomic
HTML publication. These are repeated observations, not adversarial TOCTOU immunity.

Border rule: dashed for a non-head node revision/ghost, or explicitly documented
historical/superseded content. Solid means **no explicit historical mark**, NOT
confirmed validity. `最新记录修订·描述历史方案` differs from `旧修订` and
`历史端点(当前引用)`; candidates, deferrals and plans do not imply history.
Compact node labels and details show the evidence label, reason, check time and
plain-text source references. All selection, history, back, fullscreen and reset
paths retain the class; no coordinates, counts, filters or raw records change.
Browser validation checks bounded status fields/IDs; producer validation checks
digest equality. There is no JS canonicalizer or URI execution.

Historical-status support itself adds no runtime resource or dependency.
Tests: `test_node_statuses.py`, `history.test.js`, and installed launcher contract.
Parent package rebuild and independent content review remain release gates.

## Archived per-node source locators (optional Stage 2, generic view only)

`rp-view --project /local/project --output /private/graph.html --node-statuses /private/statuses.json --source-locators /private/locators.json`
keeps artifact cards metadata-only while adding **依据出处** to exact node details.
Nothing is auto-discovered. No browser/server, filesystem fetch from HTML, URI
execution, network, scientific imports, live-source refresh or new dependency.
The extra packaged module is `source_locators.py`; stage 3 domain navigation is
not included. Installed/Nix release checks remain a separate parent gate.

The private capture adapter generates a **new** sidecar from existing capture
source-index, exact node-map, packet inventory and canonical artifact references;
users need not hand-author a protocol. The adapter is private because the capture
index/packet convention is not a universal RP schema. It verifies archived packet
bytes equal the project copies, then binds the whole canonical node and artifact
records using reader-canonical-json-v1 (defined above). It never modifies source
packets, canonical records or the capture index. No private index is copied here.

Runtime input is closed JSON: envelope `version:1`, `project_id`, `entries` keyed
by **every exact node revision**. Each entry has only `revision_id`,
`canonical_digest`, `sources`. An empty `sources` array explicitly means exact
locators unavailable. Each source has only `artifact_id`, `artifact_digest`,
`packet_path`, `path`, `start_line`, `end_line`, `section`, `sha256`,
`excerpt_sha256`, `captured_at`. The artifact must be in that node's actual
`source.artifacts`; its canonical `file:` URI must match the explicit contained
packet path, and its digest/size must match bounded packet bytes. `path` is a
relative original-file **display locator**, never opened. Absolute/escaping or
noncanonical relative paths, symlinks, unknown keys, duplicate entries/keys,
wrong identities/hashes, invalid line ranges and malformed UTF-8 are rejected.
Only `.research` and explicitly opted-in packet paths are observed; metadata-only
artifacts without locator entries do not trigger body reads. Limits: 192000-byte
sidecar, 100 nodes, 8 sources/node, 400 links, 64KiB/packet, 512000 packet bytes.

The archived packet convention is a `## relative/path:Lstart-Lend` header, followed
by `Original file sha256:…; excerpt sha256:…; captured <timestamp>`, a blank line,
then an opening text fence. The loader counts **original source lines**, verifies
the complete captured excerpt hash and closing wrapper, and handles nested
Markdown fences without confusing packet line numbers with original file lines.
A matching source hash is the archived capture's claim, not live-file verification.

All locators remain available. At most the first source chunk per node gets a
whole-line prefix (up to 3 lines / **160 UTF-8 bytes**), deduplicated across nodes.
Long first lines result in locator-only entries, never partial-line ellipsis.
The full cited range and captured full-excerpt digest are preserved separately
from the displayed range and displayed-excerpt digest. The UI says
**节选，完整原文未嵌入**, **捕获于 … 的来源节选**, and explicitly explains omitted
excerpts. It uses textContent, with stable exact-node/source keys for disclosure,
focus and Back; artifact metadata cards remain separate. No scientific acceptance
is implied. There is no duplicated static full source-index/body in the HTML.

The compact optional wire uses one shared string table, one artifact-ID table,
and source tuples `[path,start,end,section,sourceHash,captureHash,capturedAt,
displayEnd,displayHash,excerpt]` (text fields are string indices; -1 means empty).
Each node has `[chunkIndex,artifactIndex]` pairs; canonical records remain stored
once, unchanged. BOOT validates optional shape, links, bounds and effective lines;
producer verifies digests. Packet, canonical, renderer and sidecar observations
are repeated before/inside atomic publication. This detects observed changes,
not adversarial TOCTOU immunity. Builds write only the chosen HTML and retain the
strict **<500000-byte whole-graph cap**; no science or graph scope truncation.

Tests: `test_source_locators.py`, `review-stage2.test.js`, installed launcher
positive/negative contract, and explicit private-input `review-stage2-actual.py`.
No real scientific content is checked into generic test fixtures.

## Compact wire delivery (current renderer)

All newly generated artifacts use `compact-wire-v1`, including pinned-case builds.
Old artifacts remain read-only and execute their own embedded scripts. No record,
relation, source scope, per-view arrays or layout algorithm is reduced to fit.
The exclusive **500000-byte HTML cap**, 512 canonical records and 2MB input bounds
remain unchanged; genuinely oversized output is still rejected before replacement.

- One complete canonical map is serialized as escaped **text** in native
  `<details><pre id="canonical-records">` outside the workspace. It retains
  non-graph records (bindings, assessments, artifacts, references, histories).
  JSON metadata contains no second records map or node/science-edge `raw` copy;
  title/kind and ordinary labels hydrate from exact canonical records. Intentional
  presentation label overrides, head/role/freshness/assessment metadata and
  synthetic revision links are retained.
- Boot bounds/parses both channels, checks exact IDs/schema/endpoint correspondence
  and required references with own-property lookups, then hydrates the shared map.
  `svg.js` builds the SVG using namespace DOM APIs, fixed tag/attribute names and
  `textContent`, before DOM validation and the existing mount/readiness gate.
  Python layout/routing and JS view routing remain unchanged. Static SVG output
  remains a test oracle, not a second diagram embedded in current deliveries.
- **No JavaScript / failed boot:** the native full-record JSON disclosure remains
  available, but no usable graph is promised. This intentionally replaces the old
  static diagram/heading fallback; it does **not** claim equivalent accessibility.
  Source metadata is distinguished from included local excerpts. Sources remain
  bounded/opt-in, no local-file AJAX, networking, compression or new dependencies.
- Exact final script hashes include `routing.js`, `graph.js`, then `svg.js` in the
  renderer script, followed by bootstrap. Script CSP is not relaxed. `svg.js` is
  included in every source inventory and the installed exact-resource allowlist.
- Tests use shared old/compact decode fixtures and execute actual generated boot
  with a namespace-aware strict DOM double. Attribute/text/geometry parity is
  compared with the static oracle; this is **not** browser font, memory, hit-test,
  visual or interaction-performance benchmarking.

Earlier sections describing server SVG apply only to archived versions.

## Reusable bounded reader and generic name/ID lookup

The generic path supports bounded, trusted-local RP projects within the supported
read/projection features, without per-case code edits or admission SHA repins. It is
not unrestricted arbitrary-project support or an audience-filtered export. Pinned
engineering cases remain explicit regression modes. Python consumes validated Core
snapshot JSON; there is no second Python restricted-YAML parser.

Parent verification: 327 Rust workspace tests, 76 Python tests, all JS suites,
fmt/Clippy, Nix package and installed-smoke/CLI-conformance/resource-determinism
checks passed. This does not establish browser usability on new projects or change
the historical performance failure. Build current rp before Python integration tests;
`target/debug/rp` must not silently refer to an older binary.

### Generic build command

```sh
python3 pkgs/misc/research-provenance/tools/pilot-reader/build.py \
  --project /path/to/project \
  --rp pkgs/misc/research-provenance/target/debug/rp \
  --output ~/rp-pilot/my-reader.html \
  [--thread <THREAD_ID>] [--as-of <RFC3339>] [--force] [--include-local-sources]
```

- `--thread`: Optional if the project has exactly one thread; required when ambiguous (>1 threads).
- `--as-of`: Snapshot evaluation timestamp; defaults to current UTC time.
- `--force`: Force overwrite of existing files. If omitted, only files identified as verified reader
  artifacts can be replaced.
- `--include-local-sources`: Opt-in to embed local file source excerpts (normalized relative paths,
  bounded to 64KB). Default embeds artifact metadata only without full file bodies.

### Generic name and ID lookup

```sh
python3 pkgs/misc/research-provenance/tools/pilot-reader/lookup.py \
  --project /path/to/project \
  --rp pkgs/misc/research-provenance/target/debug/rp \
  [--thread <THREAD_ID>] [--as-of <RFC3339>] \
  <EXACT_ID_OR_NAME>
```

- **Exact ID**: Returns exact revision show and history DTO from Core without jumping to head.
- **Logical ID**: If single head, selects it; multiple concurrent heads return
  `ambiguous`, at most eight displayed candidates plus total/truncated metadata.
- **Title**: Exact full title match; returns `ambiguous` with candidate list if multiple match.
- Output returns direct incoming/outgoing scientific relations, directional semantics, forward
  provenance (`source.revisions`), reverse references, and honest status (`selected`, `ambiguous`,
  `not-found`, `rejected`).

### Bounded safety contracts
- **Contained read scope**: Reads only `.research/` directory and explicitly referenced local files.
  Never recursively scans arbitrary repositories, `.git`, or secrets.
- **Restricted YAML**: Parsed and validated by the existing Rust Core; Python reads
  the resulting JSON, not YAML through SafeLoader.
- **Core snapshot**: `rp snapshot` exports after full validation; admission limits
  are 100 semantic revisions, 300 scientific relation revisions, 512 total canonical
  records and 2,000,000 serialized canonical bytes. Counting precedes DTO cloning.
  Reader input/output limits can reject projects below these individual ceilings.
- **Consistency**: Repeated file/binary observations and live show/history identity
  checks detect observed changes; they do not prove an atomic working-tree snapshot.
  Subprocess capture size is checked after capture; explicit rp executable is trusted,
  not sandboxed against malicious output. Historical raw URI metadata remains private.
- **Assessment transparency**: When an Assessment exists, the UI never claims "未评价". It visibly
  renders the assessment count and "评价展示暂不支持".

## Crowded-rank horizontal packing (current generic renderer)

Crowded portrait graphs now use deterministic landscape-biased packing, rebuilt
once into the artifact. Only the current `reion3-project/graph.html` is republished;
old CSP/v1–v5 artifacts remain unchanged. Cards stay **270×114**, with unchanged
text, zoom, controls, `说明` kinds guide, compact wire and routing algorithms.

- Opt in only when a rank has **more than 12 nodes** and the original card hull
  has aspect **<1.5**. Small scenes keep their prior coordinates exactly; no case
  alias/ID checks select the algorithm.
- Search at most 100 row budgets against actual rank populations, minimizing
  distance from a **2:1** card hull. Each semantic rank occupies adjacent packing
  subcolumns, filled row-first. Every earlier reading rank still precedes every
  later one; subcolumns are not new ranks or scientific connections. SCC members
  remain separate cards in a grid, without promising LTR arrows inside cycles.
- Existing normalized basis direction, roots, history and concurrent heads stay
  intact. Weakly connected groups are kept together within ranks; isolates share
  the same packing space after connected groups, not a separate tall column.
  Packed rows use a 320-unit pitch and 40-unit subcolumn stagger to leave route
  corridors; compact scenes retain 190-unit rows and 34-unit rank offsets.
  History relocation checks real occupied rectangles and cannot extend the packed
  bottom. Filters, local exploration, Back and fullscreen never relayout nodes.
- This is finite heuristic geometry, not globally optimal routing. The full actual
  86-node / 77-edge scene is tested, including all canonical identities and bounds;
  geometric crossing/shared-length/label tradeoffs are reported, not hidden by
  dropping nodes or reducing font size. Browser font/hit-testing remains unverified.

Run `python3 -m unittest discover -s pkgs/misc/research-provenance/tools/pilot-reader/tests`
and all `tests/*.test.js`. `test_horizontal_layout.py` is self-contained except
one optional actual-corpus test using an explicitly copied private snapshot and
pre-task HTML under `/tmp/rp-horizontal-layout-private/`. Final-HTML compact/SVG,
startup, layout-help and pan/fullscreen checks use the actual published path.
TDD evidence and parent review handoff: `/tmp/rp-horizontal-layout-summary.md`.
Source build uses an explicitly paired installed `rp`; the installed reader package
has not yet been rebuilt with this layout. No Core/schema/Nix changes or deployment.

## Layered reading layout / reserved routes / discoverable help (current r2)

This section supersedes older coordinate-freeze/type-grid/shortest-independent-route
statements below. Only private `csp-case-r2/csp-reader.html` is republished. Node
positions change once at rebuild, **not** on filter/history/local focus/Back or
fullscreen. No arrange button, physics, network, dependencies or browser used.
Parent independent review and actual browser acceptance remain pending.

- Layout uses an ephemeral basis-reading constraint graph: supports from→to;
  derived-from/depends-on target→dependent. Other types use **raw-direction
  fallback**, not a universal source→result claim. Stored arrows/types/exact IDs
  and SOURCE meanings never change. Q00 is an upper-left suggested entrance;
  its actual SOURCE reference places I02 next, without rendering a science edge.
- Active endpoints determine longest-path ranks after bounded Tarjan SCC
  condensation. All canonical SCC members remain separate in a within-rank grid;
  multiple roots, isolates and concurrent heads survive. Eight barycenter sweeps
  plus six adjacent-exchange passes (only ≤80 skeleton links) reduce skeleton
  crossings with deterministic ties. A small 34-unit rank offset suggests diagonal
  reading; it is not chronology/causality or a strict total order. Old revisions
  use recorded old→new links for proximity, never to stretch the primary ranks;
  stranded history gets a nearest free local slot when available, not a hidden
  node or selected winning head. Bounds: 100 nodes / 300 science / 300 revision.
- Removed obsolete `_coordinate_frame_routes`. Fit uses real route hulls and full
  placed labels; default/reset use default-visible geometry even with negative
  bounds. Build re-routes in the final translated coordinate frame to avoid decimal
  half-tie drift between static labels and runtime. No arbitrary global padding.
- Global routing is stable exact-ID greedy reservation, not a universal optimizer.
  Up to151 local candidates (12 nearby obstacle lane proposals plus ±10 variants),
  all node rectangles validated first. Score up to24 shortest valid alternatives,
  length + 2×shared collinear length + 70×proper segment crossings; candidate
  detour capped at 1.5×shortest +180. Parallel canonical pairs have bounded distinct
  ports/lanes; reverse arrows retain direction. Visible-set changes may choose new
  routes, but do not renumber canonical pair lanes or move nodes. The existing
  16-entry visible-ID cache and no per-frame computation are retained. Global
  label allocation still defers unsafe text without dropping paths/relations.
- Toolbar **说明** reopens the right detail pane and native reading guide, focuses
  its summary and scrolls to the guide. It preserves detail DOM, selection, search,
  viewport and Back stack, including fullscreen; no modal. Letters remain case
  aliases, not kind codes: Q问题 / G目标 / P试点 / I调查 / B阻塞 / N下一行动.
  Existing actual I kind examples remain. New required help/guide IDs are checked
  before readiness and the button uses native keyboard activation.

Independent polyline metrics are in `tests/geometry_metrics.py`; reports compare
archived **embedded old router** against actual new coordinates/routes in default
and all-history views, counting all13 and27 paths respectively. Pairwise shared
collinear length and distinct transverse intersections (including T/bend touches)
ignore only 24 units at shared exact endpoints. Labels/leaders are not science
paths. This is geometric evidence, not a visual/font/hit-testing score. Individual
route lengths and deferred labels, including regressions, are reported honestly
in `/tmp/rp-layered-layout-summary.md` and `/tmp/rp-layered-layout-metrics.json`.
Actual private-corpus tests are skipped if the explicit pre-task `/tmp` archive is
absent; synthetic regressions and all existing suites remain repository-contained.

Run Python discovery and all `tests/*.test.js`, plus final-HTML `layout-help`,
`labels-case`, `routing-dom`, `clarity`, `follow-relations`, `fullscreen-trackpad`
and `startup`. Old original-CSP and legacy32/35 builds use scratch outputs only.

## Follow relations / reverse SOURCE / complete supported diffs (review top 3)

Approved first three priorities from
`docs/plans/2026-09-11-research-provenance-multi-model-canvas-review.md` are
implemented in the frontend. **Automated checks complete; parent independent
review/test and browser acceptance remain pending.** No dashboard, layout,
initial/reset bounds, zoom, routing, style or fullscreen changes in this batch.
Only private `csp-case-r2/csp-reader.html` is republished; older cases are tested
through scratch builds, never replaced.

- Incident scientific relations identify the opposite node by its existing
  alias/title and explain the original typed direction. Separate **前往节点**
  and **查看关系** actions permit direct node→node reading without a mandatory
  relation-detail stop. All supported relation types have directional wording;
  unknown types retain neutral raw type/direction. Current-only lists explicitly
  count excluded historical relations; history lists show exact current/historical
  and invalidated states. No canonical edge is rewritten.
- Action focus keys use selected ID + action + exact relation/target IDs, not
  duplicate labels or list positions. Disclosures/compare selectors have stable
  context keys. Click origin is explicitly focused before navigation capture
  because pointer/assistive clicks need not focus buttons in every browser.
  Back restores the exact second same-type action, scroll, open disclosures and
  prior view; same-selection filter refresh retains this state. Graph keyboard
  targets have exact identity keys. General search→Back focus polish is deferred.
- **被这些记录引用 · SOURCE · 非科研关系** is a read-only reverse index built
  once at mount from `records` and graph copies, deduplicated by exact referrer ID
  per target. Only `source.revisions` is indexed; scientific edges and counts are
  untouched. It always includes historical uses, independently of view filters.
  Projection current flags, not raw `relation_state: active`, determine state.
  NodeRevision and scientific-relation referrers navigate exactly; relation labels
  include both endpoint names. Other typed records show raw text and an explicit
  “不在画布 / 当前历史未判定”, never a fake locate action.
- Index corpus maximum is 512 records under the existing startup byte/depth/entry
  bounds; each target lists at most 100 unique referrers. Overflow reports shown,
  total and omitted counts, with all matching raw records in a text disclosure.
  Duplicate references/projection copies do not multiply counts. Bootstrap now
  checks the additional consumed `records`/SOURCE shapes before handlers register.
- Diff whitelist adds `conclusion`, `interpretation`, `method`, with readable
  payload/field labels. Existing 40-change / 4000-character-per-side comparison
  bounds remain; nested array/object/text truncation is explicit and supplies
  **对比双方完整原始字段**. Selected full raw fields remain available too.
  Final-HTML tests read actual I09 old/new uncertainty and use limitations at
  runtime; private payload text is not copied into repository fixtures.

Actual-corpus checks distinguish I02's incoming derived-from I04/I09, Q00→I02
SOURCE, and current N01→I04 SOURCE. Old N01 does **not** cite I04: it cites old
I09/B02/G05 and is tested at its actual source. Q00 remains scientifically isolated.

Verification: 47 Python tests, all 11 JS suites, strict final-HTML
`follow-relations`, clarity, routing DOM, label-case, fullscreen and startup suites;
actual pinned-rp original/r2 lookup, original CSP scratch build, legacy 32/35
integration with invalid/changing-input preservation, r2 build **78 objects / zero
Findings**. Existing source/CSP hash generation is unchanged. Entire embedded
r2 graph/data (including canonical records, aliases, sources and geometry) equals
the pre-edit artifact. Old published outputs and all input hashes are preserved.
No Rust/Nix, dependencies, network, GUI/browser, commit or deploy.

```sh
NODE=/nix/store/2bslrww4ch7my47xxwabj1qy4acq4720-nodejs-slim-24.14.1/bin/node
"$NODE" pkgs/misc/research-provenance/tools/pilot-reader/tests/follow-relations.test.js
"$NODE" pkgs/misc/research-provenance/tools/pilot-reader/tests/follow-relations.test.js \
  ~/rp-pilot/csp-case-r2/csp-reader.html
```

Durable evidence and review handoff: `/tmp/rp-follow-relations-summary.md`.
This completes only review priorities 1–3 at the implementation/automated-test
level. Remaining canvas/entry/browser-UX priorities are not accepted by these tests.

## Fullscreen canvas and fine wheel zoom (current r2, presentation only)

Only `~/rp-pilot/csp-case-r2/csp-reader.html` is rebuilt.
**全屏画布** first fills the browser viewport with CSS; only that explicit click
requests native Fullscreen API on the workspace containing toolbar, graph and
right floating details. Denial, synchronous exceptions, disabled or unavailable
API retain the **窗口填满模式（非浏览器原生全屏）** fallback, not a startup failure.
**浏览器原生全屏** is reported only when `fullscreenElement` confirms it.
No automatic request/prompt, browser/server, dependency or UA/device detection.

- Toolbar stays above a flex graph using remaining viewport height (`100vh` /
  `100dvh` fallback). Expanded details overlay the right, at most 380px, with their
  own constrained scroll area; narrow screens retain an exposed portion of graph.
  There is no modal backdrop or split detail column in expanded mode.
- **收起详情 / 展开详情** has `aria-expanded`; fullscreen has `aria-pressed` and
  explicit exit text. Both are readiness-gated. Collapse works independently of
  selection/Back; actual disclosure DOM and scroll survive hide/reopen and mode
  changes. Enter/exit never renders details, fits, reroutes, moves nodes or changes
  model box/navigation. Changed SVG rectangles use existing letterbox mapping.
- Esc exits expanded mode first (including from search), without clearing search
  or selection; outside it, prior search/selection Esc semantics apply. Native
  `fullscreenchange` exits CSS mode too. Prior focus and page scroll are restored;
  inline styles are never overwritten. A pending request blocks another entry
  until settled; late resolution is exited even after interaction failure. Native
  exit denial keeps a truthful native UI, with browser Esc still available.
- Print CSS restores normal document flow, shows even collapsed details inline,
  removes floating overlays and viewport sizing; this is structural, not visual
  print acceptance.
- `wheelFactor(deltaY, deltaMode, pageHeight)` uses **0.0012/pixel**, **16px/line**,
  and viewport page height clamped to **100–1000px** (800px when unavailable).
  Exponential zoom is capped to reciprocal **×1.1 / ÷1.1 per event**. 1px gives
  roughly 0.12%; 50px gives about 6%; 100px reaches the 10% cap. Zero/nonfinite or
  invalid modes are no-ops; Ctrl+wheel pinch uses the same fine mapping, no large
  multiplier. +/- buttons also use ×1.1 / ÷1.1; existing limits/reset/pan remain.
  Below the per-event cap, equal total deltas produce equal zoom independent of
  event count. Only SVG wheel is prevented; pane scroll is not hijacked. Updates
  set viewBox/zoom text only, without routing computation or animation batching.

Tuning evidence is numerical and strict-DOM handler tests, **not physical touchpad
or browser native fullscreen experience**. Run all existing Python/JS suites plus
`tests/fullscreen-trackpad.test.js` (optional final HTML path); startup tests
conditionally check the three new required DOM fields only in new deliveries.
The build's existing exact-byte CSP hashes and source inventory cover these edits.
Evidence/review handoff: `/tmp/rp-fullscreen-trackpad-summary.md`. Parent independent
review and browser fullscreen/window fallback, narrow screen, keyboard, physical
trackpad and print acceptance remain pending. Canonical records, copied sources,
manifest, aliases, coordinates, routing/type semantics and old HTML stay unchanged.

## Visible-obstacle edge routing (current r2, presentation only)

Only `~/rp-pilot/csp-case-r2/csp-reader.html` is republished.
Nodes keep their published absolute coordinates; no automatic relayout or arrange
button. `routing.js` is a pure Node-testable router; `routing.py` implements the
same finite candidate contract for static rendering (shared numerical fixtures).
The old projection route calculation is retained **only to freeze the existing
coordinate-frame padding**; none of those legacy routes reach the renderer.

- Route only visible science/revision edges against visible 270×114 rectangles,
  with 12px nonendpoint clearance. Preserve exact endpoint identities, boundary
  ports, arrow markers, type styles and canonical data. Labels use conservative
  text bounds (including historical suffix), not actual font measurement.
- Try direct/parallel bent and nearby orthogonal port routes; twelve nearest
  corridor rectangles propose at most 51 candidates. All visible rectangles still
  validate each candidate; choose the shortest valid candidate with stable ties.
  This is **not global optimization or a universal router**. No all-global-top
  rule: a nearby detour is used only when the shorter candidate fails validation.
  Route costs and segment lengths use 1e-6 tie quantization for Python/JS parity.
- Canonical pair membership fixes lane ranks even when other members are hidden;
  parallel/reverse pairs and self loops remain separately routed. Active/history,
  revision, role-context and local expansion still use the existing view semantics.
- Path validity is independent of label room. Only a failed path search returns
  `unroutable`; a routed edge always retains line/hit/arrow/type, even when its
  separate `labelStatus` is `hidden`. Never replace a valid relation with fake
  missing geometry simply because its label cannot fit.
- Labels allocate globally per visible set in exact-ID order, without changing
  canonical data or the cache key. At most250 candidates (five segments × five
  anchors × ten positions) per edge; no iterative global optimization. Text uses
  conservative ASCII/wide-Latin/Unicode widths and the full historical/invalidated
  suffix, with padded 28-unit line height. Labels wider than360 units are deferred.
  Placed boxes clear every placed box and visible node card. Compact type-colored
  borders/backgrounds cannot cover route centerlines (2-unit extra clearance) or
  prior leaders. Short arrowless leaders (maximum36 units) attach to their own
  route; these are label associations, not scientific relations. Shared route
  corridors can remain: this is not a wholesale lane/path rewrite.
- Unavailable labels are explicitly counted separately from hidden/unroutable
  relations; full text/type/direction remain in title, accessible name, search
  and details. Selecting/focusing a deferred label never overlays other text.
  Fit includes only placed label geometry. Static guide also discloses fallback.
  No browser font measurement or visual acceptance is claimed.
- Model caches 16 visible-set results (sorted node+edge IDs, deterministic FIFO
  eviction). `routeStats` exposes computations/hits/current key/entry count.
  Pan, zoom, hover and selection do not recompute geometry. View copies carry
  current geometry; `items` and canonical projection are never mutated.
- Both SVG line/hit paths and label coordinates update together. Fit/reveal uses
  current route hull/label bounds, not old empty arches. Back derives routes from
  restored visibility without route history or automatic fitting, preserving the
  actual saved viewport. Startup recomputes the default visible scene.
- Build embeds routing before graph in the **same renderer script**; the existing
  three independent data/renderer/bootstrap CSP hashes cover the exact bytes.
  Both routing source files join the bounded source inventory/recheck. Every
  runtime source stays below 64KiB, no dependencies/network/browser added.

Tests: Python suite; `tests/{labels,labels-case,routing,routing-dom,state,exploration,dom,clarity,startup}.test.js`
with the installed Node below. `routing-dom.test.js <html>` exercises real boot and
handlers, including actual r2 B01→G05 local focus. Scratch original-CSP and legacy
builds never replace old outputs. Evidence, limitations, preservation hashes and
independent parent-review handoff: `/tmp/rp-edge-label-summary.md` (label repair),
`/tmp/rp-visible-routing-summary.md` (prior route work).
**No browser visual acceptance claimed; parent review remains separate.**

## Current r2 reading clarity (user4, display-only)

Primary output: `~/rp-pilot/csp-case-r2/csp-reader.html`.
Delivery marker: `reading-clarity-user4`; the technical disclosure contains its
explicit generation timestamp. **This is a presentation update, not a frozen
NodeRevision, source event, new feedback record or acceptance result.** Canonical,
name-map, manifest, admissions, sources and all older case/v1–v5 outputs stay intact.
Only the current r2 HTML is replaced; new build/test reports stay under `/tmp`.

- I01's exact known CSP revision displays **I01 历史交互测试通过，浏览器执行未验证**.
  Its original title **I01 有实现不等于可用** and original statement remain in
  details. The admitted copied startup-review excerpt records historical test
  passes and explicitly says browser execution remains unverified; this title
  does not report a new test. `graph_projection.presentation_labels` is the one
  presentation map used by case rendering and lookup (with or without `I01 `).
  Stable aliases, exact IDs, original names and ambiguity handling still work.
- **从问题开始** reveals CSP Q00 and its recorded scientific one-hop neighborhood.
  Q00 is scientifically isolated; the immediate **故障现象 I02 · SOURCE** button
  comes only from its real `source.revisions`. It is not a scientific arrow.
  This is a suggested entry, not a chronological/hierarchical root of a complete
  project graph. Legacy entry uses current Questions actually bound to the chosen
  thread; several Questions produce choices, none produces an explanatory prompt.
  Initial selection remains empty; entry and subsequent navigation are reversible
  with Back. NextAction sources retain “编写依据，不等于行动目标 · 非科研关系”.
- Nodes remain **270×114**. Labels wrap deterministically within 238 nominal units,
  at most two lines (baselines 50/70) plus ellipsis; type / roles / status use
  separate baselines 26/88/104. Full label, original title, roles and exact ID stay
  in SVG title/accessible name and details. Each current/history node has a unique
  static-index `clipPath`, containing every text row even with unusual fonts.
  Width is a conservative Chinese/Latin/codepoint approximation, **not actual
  browser font measurement**; no `textLength` squeezing. Long roles also ellipsize.
- Native **阅读说明 · 字母与关系图例** above details is open without JS, collapsed
  once at mount, and independently toggleable without replacing the selection.
  Its letters are case aliases, **not kind codes**: Q question, G goal, P pilot,
  I investigation, N next action, B blocker. Examples come from actual case kinds:
  I03 Observation, I04 Interpretation, I06 Method, I09 Conclusion. Legacy pages
  make no CSP alias-legend claims.
- Relation colors/patterns are a static server-side allowlist: supports green
  solid; weakens red short-dash; contradicts red long/short-dash; derived-from blue
  dashed (**推导自/依据**, target is basis); depends-on amber long-dash; blocked-by
  red dashed; motivates teal dotted; revisions purple dot-dash, old→new. Unknown
  types remain escaped literal labels with neutral styling. Each type's arrowhead
  matches its line color. Selection/neighbor/focus changes width/opacity, never
  type colors or dash. Invalidated edges retain type but are separately faded and
  labeled **已撤销（历史）**. SOURCE cards are gray dotted, not science edges.
  Legend lists relations actually present (including history), plus revision/source
  meanings. Colors never mean confidence, verified execution or acceptance.

Verification: Python projection/geometry/security/lookup tests; real mount state,
exploration and strict generated/final HTML fake-DOM tests; exact CSP hashes;
clipping-ID uniqueness; start/choice/source/Back/native-disclosure isolation;
current/history selected type attributes; installed pinned-rp original CSP/r2 and
32/35 legacy scratch pipelines with failure preservation. New optional projection
metadata is shape/endpoint checked at bootstrap; producer tests check its semantics.
New-template startup checks do not impose new required DOM on immutable old HTML.

```sh
python3 -m unittest discover -s pkgs/misc/research-provenance/tools/pilot-reader/tests
# Use the installed Node path documented below; no downloads.
"$NODE" pkgs/misc/research-provenance/tools/pilot-reader/tests/clarity.test.js
"$NODE" pkgs/misc/research-provenance/tools/pilot-reader/tests/clarity.test.js \
  ~/rp-pilot/csp-case-r2/csp-reader.html
"$NODE" pkgs/misc/research-provenance/tools/pilot-reader/tests/startup.test.js \
  ~/rp-pilot/csp-case-r2/csp-reader.html
```

Durable review/evidence handoff: `/tmp/rp-csp-reading-clarity-summary.md`.
**Actual browser fonts/layout/hit-testing/CSP/opening context, narrow-screen/print
and researcher usability remain pending.** No browser GUI/headless, network,
Core/schema changes, commit or deploy. Parent independent review must not be
inferred from implementation self-review or these tests.

## Explicit second case and Agent lookup (not a generic RP loader)

The original `pilot-pins.json` contract, 32/35 corpora and archived v1–v5 HTML
remain unchanged. A second named **CSP startup candidate** is explicitly admitted
by `csp-case-admission.json` (manifest digest only; no private excerpts in repo).
Its private project, manifest, source index, stable name map, constructor and
usage instructions live under `~/rp-pilot/csp-case/`.
New reader: `csp-case/csp-reader.html`; parent/data review and usefulness pending.

One command an Agent can execute from this repository:

```sh
python3 pkgs/misc/research-provenance/tools/pilot-reader/lookup.py \
  --project ~/rp-pilot/csp-case/project \
  --case-manifest ~/rp-pilot/csp-case/case-manifest.json \
  --rp /nix/store/rrblc2vg0h7q915mprqq864zl7322hlj-research-provenance-0.1.0/bin/rp \
  I03
```

Replace `I03` with `I08`, a reviewed local name, canonical title or exact ID.
No handoff package required. Short aliases are stable explicit case-local mappings,
never layout indexes. Exact alias/ID takes precedence; title/name ambiguity returns
up to eight candidates without selecting the first (exit 2). Unknown also exits 2;
validation/admission/live mismatch rejects with exit 1. Source text is not executable.

`case_io.py` validates the admitted manifest, pinned executable and full inventory,
actually runs `rp validate` with zero Findings, reobserves all files, then parses
only observed JSON-compatible canonical bytes. Counts/types come from the reviewed
manifest, not a changed legacy pin contract. Reviewed local source paths, artifact
size/digest and name coverage are checked; artifact/reference URIs are never followed.
This second case allows the existing Core external-reference schema for Method's
protocol reference; it does not extend Core/schema. Lookup verifies selected bytes
with actual `rp show` and its lineage with `rp history`, then returns original directed
one-hop relations, neighboring records, distinct explicit provenance and bounded
source excerpts/locators. Relation limit 60, output limit 250KB; oversize rejects
rather than silently omitting evidence. The original overall file/byte bounds apply.

Builder `--case-manifest` opts into **single-snapshot** mode: no `--before`, no fake
changes. Only `csp-reader.html` can be published by this branch, with the same private
atomic publication guards. It checks manifest/admission/input/binary and all consumed
reader sources again before replacement. It never learns new admission automatically.
Rebuild commands and the absent-root case-local constructor are documented privately;
this is deliberately a two-case fixture spike, not a generic project loader/writer.

The projection retains full display labels and canonical titles; SVG text alone
now uses bounded two-line wrapping/ellipsis plus full tooltips (see user4 above). Existing Core
Interpretation/Method/Conclusion kinds have Chinese display labels. Node details
expose payload/limitations and navigable `source.revisions` separately from science
relations; no provenance or layout is converted into a synthetic scientific edge.
New tests: `tests/test_case.py`; retained Python/JS/55-startup and legacy rp integration.
Run legacy integration only with a **scratch output**, not archived v5.

The sections below retain historical v5/v4/v3 evidence. Their former single-case and
all-six-arguments statements apply to the legacy branch, not this explicit override.

## User-confirmed basic interaction

After v5 delivery, the user explicitly reported “交互正常了” (interaction works).
The basic startup/disabled-control blocker is closed on this user evidence, not a
claimed agent browser test. Detailed exploration usability, console audit, browser
matrix, mobile and print remain unverified. Earlier pending notes below record the
pre-feedback verification boundary; they do not override this new feedback.

## Graph-first entry and startup boundary (v5)

Current private output: `~/rp-pilot/reader-v5.html`.
V1/v2 (in the private case) and v3/v4 remain byte-identical historical artifacts.
The current goal is **node-by-node content → origins/uses → project logic** in
an interactive browser graph, with updates through Agent proposals and researcher
confirmation. A glanceable whole-project overview or summary is not required.

- Minimal header and graph/search + fixed detail panel are the default entry.
  The old briefing is folded at the bottom as **项目背景**, explicitly historical,
  not a current change assertion. Initial selection stays empty; startup does not
  steal search focus. The pinned initial 10 nodes / 5 science relations are unchanged;
  this is recorded material, not a promise of complete project coverage.
- `bootstrap.js` owns loading/ready/failed, strict required DOM IDs/tags/attributes,
  bounded projection checks (100 nodes / 300 science edges / 300 revision edges,
  exact unique keys, node endpoints, geometry and consumed field shapes), and the
  safe localized failure status. This is a presentation consistency gate, **not**
  generic RP schema validation. `graph.js` owns model/mount/render, not auto-start.
- Toolbar remains disabled until validation, handler registration and first render
  complete. Mount and boot are one-shot per document. Failed partial handlers are
  guarded; failed scene/detail/search are inert plus CSS pointer-disabled. Local
  event errors enter failed state; there is no global error swallowing listener.
  A blocked bootstrap leaves the static startup warning and no-JS originals useful.
- Failure messages never echo exception/JSON details. Retry means the user's explicit
  **browser reload**, not data refresh or silent automatic retries. No network,
  storage, eval, dependency, server, GUI/headless tooling or Core/schema changes.
- Final HTML authorizes JSON, graph and bootstrap independently with exact standard
  CSP `sha256-` hashes; artifact evidence still uses `sha256:`. Seven runtime source
  files, including bootstrap, are inventoried and rechecked before publication.

Internal evidence: 16 Python tests; prior state/exploration/real-handler suites;
55 strict HTML-derived DOM tests execute **actual generated script entry points**,
including the final private file, missing/duplicate DOM, malformed/duplicate data,
pre-render disabled controls, init/update exceptions and one-shot behavior. The
independent CSP validator rejects literal malformed-colon and wrong-digest fixtures.
Selective regression mutations and real pinned-rp integration are recorded in
`/tmp/rp-graph-first-startup-summary.md` and `/tmp/rp-graph-first-*.log`.
**This is not browser execution proof. P1 actual browser/opening-context/CSP,
SVG hit testing, keyboard, desktop/narrow-screen and print acceptance remain
PENDING / unaccepted. Parent review is required.** No readiness acceptance claim.
Final generation: `2026-09-10T03:16:14Z`, **210,918 bytes**, file 0600 / directory
0700, SHA-256 `c5cb71da56d78f6939e24952b493086f1c10324758bdff366d07f1779e69fb1c`.

Additional current test commands (installed Node path is below; no new deps):
```sh
"$NODE" pkgs/misc/research-provenance/tools/pilot-reader/tests/startup.test.js
"$NODE" pkgs/misc/research-provenance/tools/pilot-reader/tests/startup.test.js \
  ~/rp-pilot/reader-v5.html
```

Remaining restrictions: pinned single engineering case, fixed binary/input pins,
JSON-compatible YAML only, historical source excerpts, static non-live snapshot;
no arbitrary-project loader, generic schema support, graph editing or reusable
proposal/publish loop. Revision comparisons still cover the explicitly listed
content fields (40 rows, bounded depth/text); other fields remain in originals.
Scientific, artifact-source and revision relations remain distinct. Depends-on
outgoing means **dependency source**, incoming means **used by the dependent**;
other relation types are not relabeled as generic evidence/causality.

## Historical v4 evidence (not the current entry requirements)

## Startup defect correction

User reported all controls inactive. CSP hash tokens incorrectly used artifact
notation `sha256:` instead of standard CSP `sha256-`; inline JS never received
valid authorization. Generator/test now independently check CSP token grammar and
exact hashes, without allowing unsafe-inline scripts. V4 was rebuilt; older v3
is retained as historical evidence and still has the defective CSP. Opening-context
sandbox rules may independently block scripts. Real-browser acceptance is pending;
fake-DOM tests never prove CSP execution. See the project startup-review document
under docs/plans for evidence and frontend delivery/acceptance refactoring scope.

## Exploration update (v4)

Historical output: `~/rp-pilot/reader-v4.html`; existing v3 stayed
byte-identical. Current rebuild/integration commands below target **v5 only**;
do not rebuild v3/v4 when checking this slice. This remains a private pinned-case prototype, not a general editor.

- Role filters now emphasize seeds and retain muted one-hop context. Local focus
  and cumulative incoming/outgoing expansion traverse recorded arrow direction,
  not inferred causality. Cycles terminate through visited sets.
- Back restores selection, role/history/local view, viewport, search and detail
  scroll/disclosures. The stack is capped at 50; pan/zoom alone add no frame, but
  their current viewport is captured before navigating away. Reset is reversible.
- Search supports arrows/Enter/Escape and zooms to target bounds; hidden objects
  are revealed without rewriting the scientific graph.
- Details prioritize readable text, source excerpts and parent-ordered revision
  history with bounded old/new field comparison. Technical originals remain.
- Same-selection view changes refresh context counts while restoring detail state;
  captured drag clicks cannot suppress the next assistive click.

Parent verification: 15 Python tests; JS state, exploration and real-handler
fake-DOM suites; actual-rp integration and final build all PASS. Added stale-detail
regression fails against the prior implementation and passes with the repair.
Generation 2026-09-09T13:06:27Z, output 202040 bytes, mode0600; v3 SHA preserved.
Browser visual/hit-testing/mobile/print acceptance remains PENDING. No GUI used.

Additional test command:
```sh
"$NODE" pkgs/misc/research-provenance/tools/pilot-reader/tests/exploration.test.js
```

The remainder records the v3 baseline; its strict role-filter behavior is superseded
by the contextual v4 behavior above, and v3-specific output metadata is historical.


A **visible, server-rendered SVG** immediately below a short Chinese work briefing,
with an adjacent, independently scrolling/sticky details panel (below the graph on
narrow screens). Native JS provides selection and navigation. No server, network,
browser tooling, install integration, Core changes or writing of research records.

**G1 + G2 implemented; browser visual/interaction acceptance is pending.** Tests
below do not establish SVG hit testing, actual browser CSP enforcement, layout
legibility, mobile usability or print acceptance. This is not Phase 1A acceptance.

## Bounded, pinned input — not a general RP reader

The reviewed temporary pilot has **32 before / 35 after canonical objects**:
11 NodeRevision objects, 10 current heads, 5 ScientificRelationRevision objects,
12 bindings / 11 binding heads, 5 Artifacts, 1 Thread and 1 Project in after.
Default graph: **10 semantic nodes + 5 actual scientific relations**. History:
**11 semantic nodes + 5 scientific relations + 1 separate old→new revision edge**.
No fake science edge is derived from a narrative, an action dependency, an artifact
reference, the position of a card or an evidence-confidence guess.

`pilot-pins.json` stores the independently inspected pilot inventory identities,
reviewed source-copy mappings/locators, fixed `as_of`, and exact installed rp
executable path plus SHA-256. It contains metadata, not source excerpts. The
inventories identify **47 before and 51 after files**. Before bytes must all survive
in after. New data requires a separate review and pin update; do not automatically
rewrite the pins to make a validation failure disappear.

1. Require the exact installed rp path and digest from the pin file.
2. Observe all files in both inputs with bounded reads, rejecting symlinks and
   special files. Bounds: 512 files, 1,024 entries, 64 KiB/file, 2 MB/corpus.
3. Actually run `rp validate --project <root> --json` on **both** corpora (20-second
   per-command timeout); require exit 0, status ok, zero findings, valid=true and
   32/35 counts. No cached query DTO substitutes for this validation. `validate`
   does not accept as_of, so the builder does not pass it.
4. Recheck complete inventories. Only then JSON-parse the observed canonical bytes.
   The pilot `.yaml` files are **JSON-compatible YAML**. Python's `json.loads` is
   deliberately **not an arbitrary YAML loader**; schema/duplicate-key validation
   belongs to rp. Only the pinned case's six schemas are accepted. Pure projection
   fixtures exercise graph edge cases, not generic schema support.
5. Compare pinned inventories, verify artifact size+SHA against both the record and
   reviewed mapping, and verify narrative SHA. Verify and display from the **same
   observed byte copy**; never resolve `artifact.uri` or unreviewed source paths.
6. Project, render, then reobserve both inventories and binary/pin identity before
   replacement (also after the temporary output is written). The seven runtime source
   files are also inventoried before validation and rechecked after validation and
   before publication; their digests are included in the build evidence.

These repeated observations detect changes visible at the check points. They are
**not an atomic working-tree snapshot, immutable Git baseline, secure public
export or immunity to adversarial TOCTOU**. rp itself reads disk between observations;
transient changes restored between checks cannot be ruled out. Trusted local pilot
scope remains essential. Source-copy hashes describe original bytes; displayed
excerpts only redact private machine path prefixes. Canonical node titles/statements
are unchanged in details, even when Chinese short presentation labels differ.

## Rebuild (no install / no network)

From nixos-config, with the existing installed binary:

```sh
python3 pkgs/misc/research-provenance/tools/pilot-reader/build.py \
  --project /tmp/rp-private-pilot-20260908-case/after \
  --before /tmp/rp-private-pilot-20260908-case/before \
  --rp /nix/store/rrblc2vg0h7q915mprqq864zl7322hlj-research-provenance-0.1.0/bin/rp \
  --output ~/rp-pilot/reader-v5.html \
  --as_of 2026-09-08T23:59:59Z \
  --generated_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
```

All six arguments are required; hyphenated timestamp aliases also work. For
byte-identical regeneration, reuse the exact `generated_at` from the previous build
report rather than invoking date again. This explicit generation stamp is distinct
from the frozen snapshot as_of and later, untimed presentation feedback. Output is
archived, never silently refreshed; there is no fake refresh button.

Builder prints a JSON evidence report to stdout; callers may save it privately
under `/tmp/rp-node-reader*.log`. Reports contain inventory identities, rp evidence,
counts, generated time, output SHA/size and pending visual acceptance, not secrets.

The new dedicated output directory is created 0700; the HTML is 0600. An existing
immediate output directory must already be owned 0700; **existing shared ancestors
such as `~/output` are never chmodded**. Reject input/source-tree overlap, any
symlink component, unexpected filename, nonprivate/nonregular/hardlinked existing
output. Write a 0600 temporary file in the output directory, fsync, recheck inputs,
then `os.replace`. Prepublication validation/render/check failures preserve the old
HTML. v1/v2 and canonical data are never overwritten. No crash-durability or
adversarial concurrent filesystem mutation guarantee is asserted.

## Graph semantics and interaction

- Exact NodeRevision IDs are the node keys. Parents, **not timestamps**, determine
  heads. Parallel heads survive; forks and conflicting head-binding role sets are
  visibly marked. Bindings are scoped to the selected thread. Old NextAction has
  historical role, not a current follow-up.
- Current layer takes active relation lineage heads only. A current edge to an old
  revision keeps its original endpoint, shown as a dashed historical-endpoint node.
  Historical/invalidated relation revisions remain in the history layer. No edge is
  retargeted to a newer revision. Revision edges have generated auxiliary keys,
  explicitly separate from canonical science relation IDs.
- Layout uses undirected components, then stable type/role/ID rows. Cycles have no
  special failure mode; no topological/tree assumption or continuous physics loop.
  Parallel and reverse-direction edges use distinct paths; long same-row edges
  use column gutters and exterior top lanes independent of ID/direction. Self
  edges have per-member loops and label positions. Geometry/control-point and
  label bounds expand/translate the viewBox with margins.
  Isolates remain visible as “未记录连接”. Hard reject **>100 semantic revisions or
  >300 science relation revisions**, including history; never silently truncate.
- Click or Tab+Enter/Space a node: select/highlight its actual one-hop neighborhood
  and open original title/statement, exact ID, roles, bindings, sources and history.
  Click an edge: show exact from→to, type, rationale, scope and source revisions.
- Chinese labels, original titles and exact IDs are searchable, including hidden
  nodes/edges. Results explicitly offer “隐藏，展开并定位”; choosing one resets
  filters/reveals the required historical layer and explains the change. History
  buttons do the same rather than linking into a closed/dead anchor.
- All/mainline/alternative filters derive only from actual roles (`primary` and
  `alternative`); diagnostic/follow-up roles are not silently called mainline.
  Counts show visible/total nodes, science edges and revision edges, plus hidden
  counts. Filtered neighboring relations are explicitly noted in details.
- Drag pans the canvas (never a node). Click suppression starts after 5 CSS pixels;
  pointer capture is delayed until a real drag so ordinary SVG clicks aren't
  retargeted away from nodes. Buttons and pointer-centered wheel zoom have bounded
  scale/translation; letterboxed SVG coordinates are accounted for. Fit, Reset,
  show-all/history and Esc clear are actually wired.
- Source expansion uses dashed provenance cards, not fake supports edges. Artifact
  URIs are text only, never executable links. Details preserve exact scientific
  rationale/scope and point to verified source excerpts (historical measurements,
  not new performance tests). No Assessment/confidence is invented.
- Server-rendered SVG stays visible with JS disabled; disabled controls plus a
  no-script notice describe the lack of interaction. Bottom disclosures retain
  semantic revisions/relations and readable source excerpts without JS. It is not
  necessary to expand a large record table before seeing the graph.

## Self-contained security contract

Under 500,000 UTF-8 bytes, no dependencies, CDN, fetch, storage, eval, external
images or runtime network. CSP defaults to none; both exact inline JavaScript and
JSON data script receive SHA-256 CSP hashes. The data script escapes `<`, `>`, `&`,
U+2028 and U+2029. Generated SVG/HTML text and attributes are HTML-escaped; all
runtime untrusted text uses `textContent` / `createElement`, never `innerHTML`.
No event-handler attributes or active source/external URI. Only static internal
SVG markers and a skip anchor use URI references. Inline CSS is allowed by CSP.

## Verification commands and evidence

Node was discovered installed, not obtained from a guessed environment variable:

```sh
NODE=/nix/store/2bslrww4ch7my47xxwabj1qy4acq4720-nodejs-slim-24.14.1/bin/node
python3 -m unittest discover -s pkgs/misc/research-provenance/tools/pilot-reader/tests
"$NODE" pkgs/misc/research-provenance/tools/pilot-reader/tests/state.test.js
"$NODE" pkgs/misc/research-provenance/tools/pilot-reader/tests/dom.test.js
python3 pkgs/misc/research-provenance/tools/pilot-reader/tests/integration.py \
  --project /tmp/rp-private-pilot-20260908-case/after \
  --before /tmp/rp-private-pilot-20260908-case/before \
  --rp /nix/store/rrblc2vg0h7q915mprqq864zl7322hlj-research-provenance-0.1.0/bin/rp \
  --output ~/rp-pilot/reader-v5.html
```

Integration is explicit/opt-in because it uses the pinned temporary corpus and
regenerates v5 with a fixed test generation stamp. Run the normal build afterward
to stamp the actual final generation time. Invalid and changing inputs are tested
only on disposable copies under `/tmp/rp-node-reader-integration-*`.

Behavioral RED was captured before implementation (projection/state stubs returned
empty graphs; output safety stubs failed assertions), not just missing-module
errors. Evidence logs: `/tmp/rp-node-reader-python-red.log`,
`/tmp/rp-node-reader-js-red.log`, `/tmp/rp-node-reader-safety-red.log`; current GREEN,
DOM, integration, build evidence and durable handoff live under the same prefix.

Coverage: exact heads, multihead, role conflicts/thread isolation, old endpoints,
active/historical/invalidated edges, cycles, parallel directions, isolates, hidden
counts, overscale, deterministic rendering, malicious script/title/URI text, CSP
hashes, DOM IDs/endpoint mappings, bounded source byte copies, symlink/private-path
output protection, invalid copied corpus actual rp rejection, observed changes
after validation and during render, preserved old output and unchanged originals.

JS tests exercise pure state and **mount's real handlers through a small fake DOM**:
selection/keyboard, search and reveal, filter/history, details exact rationale/scope,
zoom/pan/reset, drag suppression and ordinary-click capture behavior. This is not a
headless browser and is explicitly not visual/hit-testing evidence.

No full Rust/Nix rebuild is needed or performed: no Core, schema, Nix integration or
canonical changes. No network, server, browser GUI/headless tools, commit or deploy.

## Recorded local verification (2026-09-09)

- 14 Python tests GREEN; pure JS state and real mount-handler fake-DOM suites GREEN.
- Explicit actual-rp integration GREEN, including invalid copied corpus and changes
  after validation/during rendering, with prior output retained.
- Final normal build generated at **2026-09-09T12:24:21Z**, snapshot as_of unchanged.
- HTML **186,257 bytes**; SHA-256
  `523bc1b683b46d8229298ad0b6a3ff36609d8635cdb99dc0294090220933e6c5`.
- Dedicated directory 0700, HTML 0600; existing shared `~/output` remained 0755.
- Existing old preservation manifests still match: 173 v1-declared files and 182
  pre-v2-declared files; both source corpus inventories remain unchanged.
- `node --check` and scoped `git diff --check` passed. New files remain uncommitted.

## Manual acceptance still needed

1. In the actual browser/opening context with CSP enforced, open v5: verify loading
   becomes ready, controls respond, and graph/search/detail are the primary entry.
   No upfront summary or glanceable whole-project overview is required.
2. Search/select a detail, read its contents, follow origins and uses with actual
   relation semantics; distinguish hidden/absent connections and return to context.
3. Select the memory blocker and its depends-on arrow; inspect exact direction and
   observation. Expand its source and recognize the historical 515.13 / 512 MiB FAIL.
4. Navigate old/new action in details; confirm historical versus follow-up role.
5. Search a hidden revision, filter and reset; zoom, pan, fit; use keyboard selection.
6. Check desktop, narrow screen and print legibility; verify real browser CSP works.

This slice has no runtime installation to roll back; retain older artifacts for comparison. There is no
migration, service, persisted interaction state or database to restore.

### Independent geometry review corrections

The parent found a real question→alternative edge crossing the unrelated mainline
card and coincident parallel self loops. Numerical regressions now sample path
segments/Bezier curves (1,000 steps each), test 12px unrelated-card clearance,
reverse direction and ID-order controls, distinct loop midpoints/label targets and
viewBox margins. Integration additionally checks actual pinned rel61.
`/tmp/rp-node-reader-geometry-red.log` reproduces three assertion failures against
the prior routing branches reconstructed only in a scratch module; the first
attempt had an import-path error, corrected before collecting this behavioral RED.
`/tmp/rp-node-reader-geometry-green.log` passes against current source. This is
geometric evidence, not browser visual acceptance.
