# Private alpha test delivery tooling

This is release glue, not Core/reader runtime. It copies only a caller-authorized
candidate `.research`, declared already-captured `file:sources/` packets, and
explicit reference/sidecar files. No live research discovery, build, network,
activation, GC-root registration, skills export, or `current` pointer is performed.
`freeze.py` requires a nonexistent `alpha-1` root and an owned 0700 parent (or
creates that parent); interrupted creation is left in place for inspection, never
silently replaced. Run from the source repository so Git observation has meaning.

```sh
python3 -B freeze.py NEW_ROOT CANDIDATE PRESENTATION_ROOT EXACT_STORE_PACKAGE
```

`freeze.py` records the actually installed package tree/NAR hash/deriver and
repository HEAD + dirty flag separately. HEAD is not the release identity. It
uses paired store launchers for validation, snapshot and HTML generation. It
creates an unsealed provisional manifest; add private README, feedback guidance
and evidence before the initial seal. Ordinary files are 0600, directories 0700;
`bin/rp-node` is the sole regular executable (0700). Store aliases retain store
target modes; they do not retain the closure against garbage collection.

`rp-node.py` is substituted with the paired launcher's isolated store Python. It
resolves a unique entry in the copied logical-ID-keyed node map to an exact
revision, then calls the release's paired `rp-lookup` with the fixed project/time.
Only a single query or help is accepted. Unknown short codes fail, duplicate map
matches fail, other exact strings delegate without shell interpretation. This is
convenience, not a security sandbox around raw Core commands.

```sh
python3 -B verify-release.py ROOT ORIGINAL_PRESENTATION_ROOT
python3 -B inventory.py seal ROOT
python3 -B inventory.py verify ROOT
```

Verification does not write the release. Capture its stdout **after** it exits if
adding evidence before sealing. It checks R79/R72 in four forms, hostile scratch
CWD/PATH/PYTHONPATH/PYTHONHOME, query/override rejection, original JSON preservation,
duplicate-map resolution, original graph wire and canonical equality, installed
snapshot equality, archived locator/packet bindings, and input preservation.
Original presentation root is the authorized copied pilot, never live science.
Generated HTML strict-DOM suites remain in `../pilot-reader/tests/`; check installed
resource bytes match the checkout harness dependencies before using them. Those
tests are not native-browser or scientific acceptance.

## Parent-owned skills attachment

Leave `skills/` absent until the sibling/parent provides the bundle. Only add files
under `skills/`, with private modes, without modifying runtime/source/graph lock
components. Then explicitly reseal:

```sh
# Use the Python recorded in references/release-lock.json (absolute store path).
PAIRED_PYTHON -I ROOT/references/inventory.py attach-skills ROOT
PAIRED_PYTHON -I ROOT/references/inventory.py verify ROOT
```

The attach operation verifies every prior inventoried file and package tree, rejects
additions elsewhere, and adds the skills tree to the inventory. It marks skills
attached **pending parent final verification**, not runtime discovery/activation.
Parent records actual final review/load evidence separately before updating final
status. The initial manifest excludes only itself and its checksum to avoid a
recursive hash; `manifest.sha256` covers its bytes. Tree hashes cover the sorted
JSON inventory, including permissions and symlink target hashes. These are unsigned
integrity records, not an authenticity or approval system. If intentionally adding
final evidence/status, parent must review the resulting manifest/inventory and
reseal explicitly; do not use `seal` to overwrite an existing seal.

No prior stable alpha is implied. Store restoration/build or GC retention requires
separate authorization; the manifest deriver is provenance, not a promise that a
future rebuild will recover identical bytes. Do not commit private candidate
metadata or scientific content to this repository.
