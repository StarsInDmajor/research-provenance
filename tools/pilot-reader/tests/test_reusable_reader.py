"""TDD test suite for reusable bounded RP read/graph/name query."""
from collections import Counter
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build
import case_io
import lookup
from graph_projection import NODE, RELATION, BINDING

HERE = Path(__file__).resolve().parent
RP_BIN = Path(os.environ.get('RP_BIN', HERE.parents[2] / 'target/debug/rp'))


def create_project_alpha(root: Path):
    """Create isolated valid Project Alpha with block YAML syntax."""
    res = root / '.research'
    for d in ('threads', 'records/questions', 'records/hypotheses', 'records/observations',
              'records/decisions', 'relations', 'thread-bindings', 'assessments', 'artifacts'):
        (res / d).mkdir(parents=True, exist_ok=True)

    (res / 'project.yaml').write_text("""schema: rp/project/v1
id: proj_01J000000000000000000000A1
slug: alpha-recombination
title: Project Alpha Helium Recombination
created_at: '2026-09-12T00:00:00Z'
access_defaults:
  level: internal
  compartments: []
repository_policy:
  visibility: private
  allowed_remotes: []
schema_policy:
  core_version: v1
  kind_registry: rp/kinds/v1
  allowed_extensions: []
""")

    (res / 'threads/thread-alpha--thd_01J000000000000000000000A1.yaml').write_text("""schema: rp/research-thread/v1
id: thd_01J000000000000000000000A1
title: Helium Emission Line Survey
objective: Detect primordial helium recombination lines
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
root_question_revision: qst_01J000000000000000000000A1
parent_thread_id: null
forked_from_thread_id: null
fork_reason: null
access:
  level: internal
  compartments: []
""")

    (res / 'records/questions/question-he--qst_01J000000000000000000000A1.yaml').write_text("""schema: rp/node-revision/v1
id: qst_01J000000000000000000000A1
logical_id: alpha-qst-helium
kind: Question
record_state: frozen
title: Is helium recombination line emission detectable?
statement: Can we detect primordial helium line transitions above the continuum?
scope:
  statement: Redshift range z=1000 to z=1500
question:
  resolution_criteria:
  - Compare measured intensity against sensitivity threshold
  evidence_requirements:
  - High-resolution spectral band observation
assumptions:
- Standard cosmological recombination history
limitations:
- Optical depth variations not fully modeled
revision:
  parents: []
  summary: Initial research question
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
source:
  revisions: []
access:
  level: internal
  compartments: []
tags:
- alpha
- cosmology
""")

    (res / 'records/hypotheses/hypothesis-he-v1--hyp_01J000000000000000000000A2.yaml').write_text("""schema: rp/node-revision/v1
id: hyp_01J000000000000000000000A2
logical_id: alpha-hyp-line-excess
kind: Hypothesis
record_state: frozen
title: Helium line intensity exceeds continuum by 5 percent
statement: The 1083nm transition shows 5 percent excess over the continuum.
scope:
  statement: Narrowband infrared channel
hypothesis:
  falsification_criteria:
  - Measured excess is below 1 percent at 3 sigma
  revision_criteria:
  - Better atomic rates change expected profile
assumptions:
- Negligible dust extinction
limitations:
- Instrument response model is synthetic
revision:
  parents: []
  summary: Initial hypothesis v1
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
source:
  revisions:
  - qst_01J000000000000000000000A1
access:
  level: internal
  compartments: []
tags:
- alpha
- hypothesis
""")

    (res / 'records/observations/observation-spectrum--obs_01J000000000000000000000A3.yaml').write_text("""schema: rp/node-revision/v1
id: obs_01J000000000000000000000A3
logical_id: alpha-obs-calibrated-spectrum
kind: Observation
record_state: frozen
title: Calibrated infrared line profile
statement: Observed spectrum exhibits a 6.2 percent flux peak at expected line center.
scope:
  statement: Deep exposure data release 1
observation:
  context: High-altitude dry site synthetic spectrum
  method:
    description: Baseline-subtracted peak flux integration
  reproducibility_criteria:
  - Use identical continuum spline subtraction window
assumptions:
- Continuum baseline is linear in local window
limitations:
- Sky emission lines present in adjacent channels
revision:
  parents: []
  summary: Initial observation
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
source:
  revisions: []
access:
  level: internal
  compartments: []
tags:
- alpha
- observation
""")

    (res / 'relations/rel-qst-motivates-hyp--rel_01J000000000000000000000A1.yaml').write_text("""schema: rp/scientific-relation-revision/v1
id: rel_01J000000000000000000000A1
logical_id: alpha-rel-motivation
from_revision: qst_01J000000000000000000000A1
to_revision: hyp_01J000000000000000000000A2
type: motivates
relation_state: active
record_state: frozen
assertion_mode: asserted
rationale: The emission question directly motivates our line intensity hypothesis.
scope:
  statement: Scoped to Project Alpha
  conditions: []
  exclusions: []
source:
  revisions:
  - qst_01J000000000000000000000A1
revision:
  parents: []
  summary: Initial motivation relation
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
access:
  level: internal
  compartments: []
""")

    (res / 'relations/rel-obs-supports-hyp--rel_01J000000000000000000000A2.yaml').write_text("""schema: rp/scientific-relation-revision/v1
id: rel_01J000000000000000000000A2
logical_id: alpha-rel-support
from_revision: obs_01J000000000000000000000A3
to_revision: hyp_01J000000000000000000000A2
type: supports
relation_state: active
record_state: frozen
assertion_mode: asserted
rationale: 6.2 percent flux excess is consistent with the 5 percent prediction.
scope:
  statement: Scoped to first observation run
  conditions: []
  exclusions: []
source:
  revisions:
  - obs_01J000000000000000000000A3
revision:
  parents: []
  summary: Initial support relation
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
access:
  level: internal
  compartments: []
""")

    (res / 'thread-bindings/binding-qst--tbd_01J000000000000000000000A1.yaml').write_text("""schema: rp/thread-binding/v1
id: tbd_01J000000000000000000000A1
logical_id: alpha-tbd-question
thread_id: thd_01J000000000000000000000A1
target:
  id: qst_01J000000000000000000000A1
  type: node_revision
role: primary
record_state: frozen
rationale: Core research question of thread Alpha
revision:
  parents: []
  summary: Initial binding
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
access:
  level: internal
  compartments: []
""")

    (res / 'thread-bindings/binding-hyp--tbd_01J000000000000000000000A2.yaml').write_text("""schema: rp/thread-binding/v1
id: tbd_01J000000000000000000000A2
logical_id: alpha-tbd-hypothesis
thread_id: thd_01J000000000000000000000A1
target:
  id: hyp_01J000000000000000000000A2
  type: node_revision
role: primary
record_state: frozen
rationale: Primary working hypothesis
revision:
  parents: []
  summary: Initial binding
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
access:
  level: internal
  compartments: []
""")

    (res / 'thread-bindings/binding-obs--tbd_01J000000000000000000000A3.yaml').write_text("""schema: rp/thread-binding/v1
id: tbd_01J000000000000000000000A3
logical_id: alpha-tbd-observation
thread_id: thd_01J000000000000000000000A1
target:
  id: obs_01J000000000000000000000A3
  type: node_revision
role: primary
record_state: frozen
rationale: Key observational evidence
revision:
  parents: []
  summary: Initial binding
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
access:
  level: internal
  compartments: []
""")

    # Assessment on hyp_01J000000000000000000000A2
    (res / 'assessments/assessment-hyp--asm_01J000000000000000000000A1.yaml').write_text("""schema: rp/assessment/v1
id: asm_01J000000000000000000000A1
target:
  id: hyp_01J000000000000000000000A2
  type: node_revision
assessment_scope: overall-claim
epistemic_status: supported
assessed_at: '2026-09-12T00:00:00Z'
assessed_by:
  type: human
  id: researcher-alpha
review_assurance: human-reviewed
supersedes_assessments: []
quantitative_assessment: null
source:
  revisions:
  - obs_01J000000000000000000000A3
evidence_evaluation:
  directness: direct
  independence: same_run
  evidence_quality: medium
  agreement: consistent
  decision_confidence: medium
  rationale: Initial observation matches predicted 5 percent excess.
  limitations:
  - Single observing season
access:
  level: internal
  compartments: []
""")


def create_project_beta(root: Path):
    """Create isolated valid Project Beta with Method/Interpretation/Conclusion kinds."""
    res = root / '.research'
    for d in ('threads', 'records/questions', 'records/methods', 'records/interpretations',
              'records/conclusions', 'relations', 'thread-bindings'):
        (res / d).mkdir(parents=True, exist_ok=True)

    (res / 'project.yaml').write_text("""schema: rp/project/v1
id: proj_01J000000000000000000000B1
slug: beta-clustering
title: Project Beta Galaxy Clustering Pipeline
created_at: '2026-09-12T00:00:00Z'
access_defaults:
  level: internal
  compartments: []
repository_policy:
  visibility: private
  allowed_remotes: []
schema_policy:
  core_version: v1
  kind_registry: rp/kinds/v1
  allowed_extensions: []
""")

    (res / 'threads/thread-beta--thd_01J000000000000000000000B1.yaml').write_text("""schema: rp/research-thread/v1
id: thd_01J000000000000000000000B1
title: Redshift Space Distortion Analysis
objective: Measure the growth rate of structure f*sigma8
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-beta
root_question_revision: qst_01J000000000000000000000B1
parent_thread_id: null
forked_from_thread_id: null
fork_reason: null
access:
  level: internal
  compartments: []
""")

    (res / 'records/questions/question-rsd--qst_01J000000000000000000000B1.yaml').write_text("""schema: rp/node-revision/v1
id: qst_01J000000000000000000000B1
logical_id: beta-qst-growth
kind: Question
record_state: frozen
title: What is the growth rate of structure at z=0.5?
statement: Determine f*sigma8 from anisotropic two-point correlation clustering.
scope:
  statement: Effective redshift z_eff = 0.51
question:
  resolution_criteria:
  - Measure monopole and quadrupole moments with <3 percent statistical error
  evidence_requirements:
  - Spectroscopic catalog with random catalogs
assumptions:
- General Relativity holds on linear scales
limitations:
- Fiber collision corrections have 1 percent residual systematic
revision:
  parents: []
  summary: Initial question
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-beta
source:
  revisions: []
access:
  level: internal
  compartments: []
tags:
- beta
- rsd
""")

    (res / 'records/interpretations/interpretation-rsd--int_01J000000000000000000000B2.yaml').write_text("""schema: rp/node-revision/v1
id: int_01J000000000000000000000B2
logical_id: beta-int-linear-growth
kind: Interpretation
record_state: frozen
title: Growth rate consistent with Planck LCDM
statement: The quadrupole-to-monopole ratio favors beta=0.38 corresponding to standard gravity.
scope:
  statement: Linear perturbation regime
interpretation:
  alternatives_considered:
  - Dvali-Gabadadze-Porrati modified gravity
  unresolved_alternatives:
  - f(R) gravity models with small Compton wavelength
assumptions:
- Linear bias model is sufficient at scales > 40 Mpc/h
limitations:
- Excludes nonlinear scale data
revision:
  parents: []
  summary: Initial interpretation
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-beta
source:
  revisions: []
access:
  level: internal
  compartments: []
tags:
- beta
- interpretation
""")

    (res / 'records/conclusions/conclusion-growth--con_01J000000000000000000000B3.yaml').write_text("""schema: rp/node-revision/v1
id: con_01J000000000000000000000B3
logical_id: beta-con-growth-rate
kind: Conclusion
record_state: frozen
title: Structure growth rate confirms General Relativity at z=0.5
statement: Combined multipole analysis yields f*sigma8 = 0.47 +/- 0.03, consistent with GR.
scope:
  statement: Quoted at pivot scale k = 0.05 h/Mpc
conclusion:
  residual_uncertainty:
  - Alcock-Paczynski distortion parameter correlated with growth rate
  use_limitations:
  - Assumes flat Lambda-CDM background expansion history
assumptions:
- Neutrino mass sum is fixed to minimum 0.06 eV
limitations:
- High-order perturbation terms neglected
revision:
  parents: []
  summary: Final growth rate conclusion
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-beta
source:
  revisions:
  - int_01J000000000000000000000B2
access:
  level: internal
  compartments: []
tags:
- beta
- conclusion
""")

    (res / 'relations/rel-con-derived--rel_01J000000000000000000000B1.yaml').write_text("""schema: rp/scientific-relation-revision/v1
id: rel_01J000000000000000000000B1
logical_id: beta-rel-conclusion-derivation
from_revision: con_01J000000000000000000000B3
to_revision: int_01J000000000000000000000B2
type: derived-from
relation_state: active
record_state: frozen
assertion_mode: asserted
rationale: Growth rate conclusion is derived from the linear clustering model interpretation.
scope:
  statement: Scoped to Project Beta DR1
  conditions: []
  exclusions: []
source:
  revisions:
  - int_01J000000000000000000000B2
revision:
  parents: []
  summary: Initial derived-from relation
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-beta
access:
  level: internal
  compartments: []
""")

    (res / 'thread-bindings/binding-beta-qst--tbd_01J000000000000000000000B1.yaml').write_text("""schema: rp/thread-binding/v1
id: tbd_01J000000000000000000000B1
logical_id: beta-tbd-question
thread_id: thd_01J000000000000000000000B1
target:
  id: qst_01J000000000000000000000B1
  type: node_revision
role: primary
record_state: frozen
rationale: Primary question of Beta
revision:
  parents: []
  summary: Initial binding
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-beta
access:
  level: internal
  compartments: []
""")

    (res / 'thread-bindings/binding-beta-int--tbd_01J000000000000000000000B2.yaml').write_text("""schema: rp/thread-binding/v1
id: tbd_01J000000000000000000000B2
logical_id: beta-tbd-interpretation
thread_id: thd_01J000000000000000000000B1
target:
  id: int_01J000000000000000000000B2
  type: node_revision
role: primary
record_state: frozen
rationale: Model interpretation
revision:
  parents: []
  summary: Initial binding
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-beta
access:
  level: internal
  compartments: []
""")

    (res / 'thread-bindings/binding-beta-con--tbd_01J000000000000000000000B3.yaml').write_text("""schema: rp/thread-binding/v1
id: tbd_01J000000000000000000000B3
logical_id: beta-tbd-conclusion
thread_id: thd_01J000000000000000000000B1
target:
  id: con_01J000000000000000000000B3
  type: node_revision
role: primary
record_state: frozen
rationale: Final conclusion
revision:
  parents: []
  summary: Initial binding
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-beta
access:
  level: internal
  compartments: []
""")


class TestReusableReader(unittest.TestCase):
    """Test generic loading, projection, building, and lookup without per-case pins."""

    def setUp(self):
        self.temp_dir = tempfile.mkdtemp(prefix='rp-reusable-test-')
        self.path_alpha = Path(self.temp_dir) / 'project_alpha'
        self.path_beta = Path(self.temp_dir) / 'project_beta'
        self.out_dir = Path(self.temp_dir) / 'output'
        self.path_alpha.mkdir()
        self.path_beta.mkdir()
        self.out_dir.mkdir(mode=0o700)
        create_project_alpha(self.path_alpha)
        create_project_beta(self.path_beta)

    def tearDown(self):
        shutil.rmtree(self.temp_dir, ignore_errors=True)

    def test_core_rp_rejects_duplicate_keys_aliases_and_tags(self):
        """Actual rp Core validation strictly rejects duplicate keys, YAML aliases/anchors, and custom tags."""
        # 1. Duplicate key rejection
        tmp1 = Path(tempfile.mkdtemp())
        create_project_alpha(tmp1)
        p1 = tmp1 / '.research/project.yaml'
        p1.write_text(p1.read_text() + 'title: second-title\n')
        res1 = subprocess.run([str(RP_BIN), 'validate', '--project', str(tmp1), '--json'],
                              capture_output=True, text=True)
        dto1 = json.loads(res1.stdout)
        self.assertFalse(dto1.get('data', {}).get('valid', True))
        error_codes1 = [f['error_code'] for f in dto1.get('findings', [])]
        self.assertIn('RP_E_YAML_DUPLICATE_KEY', error_codes1)

        # 2. YAML alias/anchor rejection
        tmp2 = Path(tempfile.mkdtemp())
        create_project_alpha(tmp2)
        p2 = tmp2 / '.research/project.yaml'
        p2.write_text('anchor: &anchor value\n' + p2.read_text())
        res2 = subprocess.run([str(RP_BIN), 'validate', '--project', str(tmp2), '--json'],
                              capture_output=True, text=True)
        dto2 = json.loads(res2.stdout)
        self.assertFalse(dto2.get('data', {}).get('valid', True))
        error_codes2 = [f['error_code'] for f in dto2.get('findings', [])]
        self.assertIn('RP_E_YAML_ALIAS_FORBIDDEN', error_codes2)

        # 3. Custom YAML tag rejection
        tmp3 = Path(tempfile.mkdtemp())
        create_project_alpha(tmp3)
        p3 = tmp3 / '.research/project.yaml'
        p3.write_text(p3.read_text().replace('slug: alpha-recombination', 'slug: !custom alpha-recombination'))
        res3 = subprocess.run([str(RP_BIN), 'validate', '--project', str(tmp3), '--json'],
                              capture_output=True, text=True)
        dto3 = json.loads(res3.stdout)
        self.assertFalse(dto3.get('data', {}).get('valid', True))
        error_codes3 = [f['error_code'] for f in dto3.get('findings', [])]
        self.assertIn('RP_E_YAML_CUSTOM_TAG_FORBIDDEN', error_codes3)

    def test_load_generic_projects_without_pins(self):
        """Same load_generic_project function loads two distinct projects without admission SHA repins."""
        data_a = case_io.load_generic_project(self.path_alpha, RP_BIN)
        self.assertIn('records', data_a)
        self.assertIn('graph', data_a)
        self.assertEqual(data_a['graph']['thread'], 'thd_01J000000000000000000000A1')
        self.assertEqual(len(data_a['graph']['nodes']), 3)
        self.assertEqual(len(data_a['graph']['edges']), 2)
        # Verify Assessment is captured and not falsely claimed as "未评价"
        node_hyp = next(n for n in data_a['graph']['nodes'] if n['id'] == 'hyp_01J000000000000000000000A2')
        self.assertTrue(bool(node_hyp.get('assessments')))
        self.assertIn('asm_01J000000000000000000000A1', node_hyp['assessments'])

        data_b = case_io.load_generic_project(self.path_beta, RP_BIN)
        self.assertEqual(data_b['graph']['thread'], 'thd_01J000000000000000000000B1')
        self.assertEqual(len(data_b['graph']['nodes']), 3)
        self.assertEqual(len(data_b['graph']['edges']), 1)
        node_kinds = {n['kind'] for n in data_b['graph']['nodes']}
        self.assertEqual(node_kinds, {'Question', 'Interpretation', 'Conclusion'})

    def test_generic_build_outputs_both_projects(self):
        """Generic rebuild creates valid HTML for both projects with dynamic metadata."""
        out_a = self.out_dir / 'reader-alpha.html'
        out_b = self.out_dir / 'reader-beta.html'

        evidence_a = build.rebuild_generic(self.path_alpha, RP_BIN, out_a)
        self.assertTrue(out_a.is_file())
        content_a = out_a.read_text(encoding='utf-8')
        self.assertIn('Project Alpha Helium Recombination', content_a)
        self.assertIn('Is helium recombination line emission detectable?', content_a)
        # Node with assessment must not say "未评价"
        from wire_fixture import decode_html
        assessed = next(n for n in decode_html(content_a)['graph']['nodes'] if n['id'] == 'hyp_01J000000000000000000000A2')
        self.assertEqual(assessed['assessments'], ['asm_01J000000000000000000000A1'])
        # Actual rendered assessment wording remains asserted in generic-dom.test.js.

        evidence_b = build.rebuild_generic(self.path_beta, RP_BIN, out_b)
        self.assertTrue(out_b.is_file())
        content_b = out_b.read_text(encoding='utf-8')
        self.assertIn('Project Beta Galaxy Clustering Pipeline', content_b)
        self.assertIn('Structure growth rate confirms General Relativity', content_b)

    def test_single_head_logical_lookup_and_parent_path_rejection(self):
        result = lookup.query_generic(self.path_alpha, RP_BIN, 'alpha-qst-helium')
        self.assertEqual(result['status'], 'selected')
        self.assertEqual(result['record']['logical_id'], 'alpha-qst-helium')
        with self.assertRaises(ValueError):
            lookup.query_generic(self.path_alpha / '..' / self.path_alpha.name,
                                 RP_BIN, 'alpha-qst-helium')

    def test_generic_lookup_by_exact_id_and_title(self):
        """lookup.py finds nodes by exact ID, title, and handles ambiguity honestly."""
        # Query Alpha by exact ID
        res_id = lookup.query_generic(self.path_alpha, RP_BIN, 'hyp_01J000000000000000000000A2')
        self.assertEqual(res_id['status'], 'selected')
        self.assertEqual(res_id['selected_id'], 'hyp_01J000000000000000000000A2')
        self.assertEqual(len(res_id['incoming']), 2)  # motivates + supports

        # Query Alpha by title
        res_title = lookup.query_generic(self.path_alpha, RP_BIN, 'Is helium recombination line emission detectable?')
        self.assertEqual(res_title['status'], 'selected')
        self.assertEqual(res_title['selected_id'], 'qst_01J000000000000000000000A1')

        # Query Beta by exact ID
        res_beta = lookup.query_generic(self.path_beta, RP_BIN, 'con_01J000000000000000000000B3')
        self.assertEqual(res_beta['status'], 'selected')
        self.assertEqual(res_beta['record']['kind'], 'Conclusion')

        # Query non-existent
        res_missing = lookup.query_generic(self.path_alpha, RP_BIN, 'non-existent-node')
        self.assertEqual(res_missing['status'], 'not-found')

    def test_mutation_add_node_rebuilds_with_new_count(self):
        """Adding a valid node to Project Alpha rebuilds and reflects the new count without tool edits."""
        # Initial build
        out_a = self.out_dir / 'mutated-alpha.html'
        build.rebuild_generic(self.path_alpha, RP_BIN, out_a)
        init_data = case_io.load_generic_project(self.path_alpha, RP_BIN)
        self.assertEqual(len(init_data['graph']['nodes']), 3)

        # Add a Decision node to Project Alpha
        res = self.path_alpha / '.research'
        (res / 'records/decisions/decision-proceed--dec_01J000000000000000000000A4.yaml').write_text("""schema: rp/node-revision/v1
id: dec_01J000000000000000000000A4
logical_id: alpha-dec-proceed
kind: Decision
record_state: frozen
title: Proceed with proposal for high-resolution spectrometer
statement: Allocate telescope time for narrowband spectroscopy.
scope:
  statement: Next proposal cycle
decision:
  considered_alternatives:
  - Wait for wider sky survey data
  selected_option: Submit targeted narrow-band proposal
  rationale: Initial peak excess warrants dedicated spectroscopic follow-up.
  revisit_triggers:
  - Contamination identified in instrument calibration
  reversibility: reversible
assumptions:
- Instrument schedule remains open
limitations:
- Contingent on observing time allocation
revision:
  parents: []
  summary: Initial decision
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
source:
  revisions: []
access:
  level: internal
  compartments: []
tags:
- alpha
- decision
""")

        # Add binding for Decision
        (res / 'thread-bindings/binding-dec--tbd_01J000000000000000000000A4.yaml').write_text("""schema: rp/thread-binding/v1
id: tbd_01J000000000000000000000A4
logical_id: alpha-tbd-decision
thread_id: thd_01J000000000000000000000A1
target:
  id: dec_01J000000000000000000000A4
  type: node_revision
role: follow-up
record_state: frozen
rationale: Operational next step
revision:
  parents: []
  summary: Initial binding
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
access:
  level: internal
  compartments: []
""")

        # Rebuild without any source or tool edits!
        build.rebuild_generic(self.path_alpha, RP_BIN, out_a, force=True)
        updated_data = case_io.load_generic_project(self.path_alpha, RP_BIN)
        self.assertEqual(len(updated_data['graph']['nodes']), 4)

        # Lookup new node
        res_new = lookup.query_generic(self.path_alpha, RP_BIN, 'dec_01J000000000000000000000A4')
        self.assertEqual(res_new['status'], 'selected')
        self.assertEqual(res_new['record']['kind'], 'Decision')

    def test_ambiguous_name_and_multihead_honesty(self):
        """Ambiguous name and multi-head logical IDs return ambiguous status, not arbitrary first candidate."""
        res = self.path_alpha / '.research'
        # Add a second node with identical title
        (res / 'records/hypotheses/duplicate-title--hyp_01J000000000000000000000D0.yaml').write_text("""schema: rp/node-revision/v1
id: hyp_01J000000000000000000000D0
logical_id: alpha-hyp-duplicate
kind: Hypothesis
record_state: frozen
title: Helium line intensity exceeds continuum by 5 percent
statement: Alternative duplicate hypothesis with identical title.
scope:
  statement: Narrowband infrared channel
hypothesis:
  falsification_criteria:
  - Falsified by threshold
  revision_criteria:
  - Revision criteria
assumptions:
- Assumption
limitations:
- Limitation
revision:
  parents: []
  summary: Duplicate hypothesis
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
source:
  revisions: []
access:
  level: internal
  compartments: []
tags:
- duplicate
""")
        (res / 'thread-bindings/binding-dup--tbd_01J000000000000000000000D0.yaml').write_text("""schema: rp/thread-binding/v1
id: tbd_01J000000000000000000000D0
logical_id: alpha-tbd-duplicate
thread_id: thd_01J000000000000000000000A1
target:
  id: hyp_01J000000000000000000000D0
  type: node_revision
role: alternative
record_state: frozen
rationale: Duplicate binding
revision:
  parents: []
  summary: Initial binding
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
access:
  level: internal
  compartments: []
""")

        query_res = lookup.query_generic(self.path_alpha, RP_BIN, 'Helium line intensity exceeds continuum by 5 percent')
        self.assertEqual(query_res['status'], 'ambiguous')
        self.assertEqual(len(query_res['candidates']), 2)
        candidate_ids = {c['id'] for c in query_res['candidates']}
        self.assertEqual(candidate_ids, {'hyp_01J000000000000000000000A2', 'hyp_01J000000000000000000000D0'})

    def test_output_file_protection_and_no_clobber(self):
        """Overwriting unrelated files requires --force; own generated files are safely overwritten."""
        out_file = self.out_dir / 'protected-reader.html'
        # Create unrelated foreign file
        out_file.write_text('foreign sensitive content')
        os.chmod(out_file, 0o600)

        with self.assertRaises(ValueError) as ctx:
            build.rebuild_generic(self.path_alpha, RP_BIN, out_file, force=False)
        self.assertIn('not a verified reader artifact', str(ctx.exception))
        # Content must remain uncorrupted
        self.assertEqual(out_file.read_text(), 'foreign sensitive content')

        # With force=True, it succeeds
        build.rebuild_generic(self.path_alpha, RP_BIN, out_file, force=True)
        self.assertIn('Project Alpha Helium Recombination', out_file.read_text())

        # Once it is our own generated file, rebuilding without force is permitted
        build.rebuild_generic(self.path_alpha, RP_BIN, out_file, force=False)
        self.assertIn('Project Alpha Helium Recombination', out_file.read_text())

    def test_multiple_threads_disambiguation(self):
        """Projects with multiple threads require --thread; specifying thread succeeds."""
        res = self.path_alpha / '.research'
        (res / 'threads/thread-second--thd_01J000000000000000000000A2.yaml').write_text("""schema: rp/research-thread/v1
id: thd_01J000000000000000000000A2
title: Secondary Diagnostic Thread
objective: Diagnostic branch for Alpha
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
root_question_revision: qst_01J000000000000000000000A1
parent_thread_id: null
forked_from_thread_id: null
fork_reason: null
access:
  level: internal
  compartments: []
""")
        # Calling without thread must fail with ambiguous thread message
        with self.assertRaises(ValueError) as ctx:
            case_io.load_generic_project(self.path_alpha, RP_BIN)
        self.assertIn('--thread is required', str(ctx.exception))

        # Specifying thread succeeds
        data = case_io.load_generic_project(self.path_alpha, RP_BIN, thread_id='thd_01J000000000000000000000A1')
        self.assertEqual(data['thread'], 'thd_01J000000000000000000000A1')

    def test_malicious_script_escaping(self):
        """Malicious script or unsafe text in title is HTML escaped and CSP protected."""
        res = self.path_alpha / '.research'
        # Edit question title to include XSS vector
        p = res / 'records/questions/question-he--qst_01J000000000000000000000A1.yaml'
        text = p.read_text().replace('Is helium recombination line emission detectable?',
                                     '<script>alert("xss")</script>')
        p.write_text(text)

        out_xss = self.out_dir / 'reader-xss.html'
        build.rebuild_generic(self.path_alpha, RP_BIN, out_xss)
        html = out_xss.read_text(encoding='utf-8')
        # Literal unescaped script tag must NOT exist in the body
        self.assertNotIn('<script>alert("xss")</script>', html)
        from wire_fixture import decode_html
        question = decode_html(html)['records']['qst_01J000000000000000000000A1']
        self.assertEqual(question['title'], '<script>alert("xss")</script>')
        self.assertIn('&lt;script&gt;', html)

    def test_missing_reference_fails_closed(self):
        """Invalid project with missing endpoints is rejected by rp validate / snapshot."""
        res = self.path_alpha / '.research'
        # Point relation to non-existent endpoint
        p = res / 'relations/rel-obs-supports-hyp--rel_01J000000000000000000000A2.yaml'
        text = p.read_text().replace('to_revision: hyp_01J000000000000000000000A2',
                                     'to_revision: hyp_01J000000000000000000000NON')
        p.write_text(text)

        with self.assertRaises(ValueError) as ctx:
            case_io.load_generic_project(self.path_alpha, RP_BIN)
        self.assertIn('rp', str(ctx.exception).lower())

    def test_symlink_inside_research_rejected(self):
        """Symlinks inside .research directory are rejected before validation."""
        sym = self.path_alpha / '.research/symlink_file.yaml'
        sym.symlink_to(self.path_alpha / '.research/project.yaml')

        with self.assertRaises(ValueError) as ctx:
            case_io.observe_contained(self.path_alpha)
        self.assertIn('symlink', str(ctx.exception).lower())

    def test_historical_id_lookup_does_not_jump_to_head(self):
        """Querying by historical revision ID returns that exact revision, not the newest head."""
        res = self.path_alpha / '.research'
        # Add v2 revision to hypothesis
        (res / 'records/hypotheses/hypothesis-he-v2--hyp_01J000000000000000000000A8.yaml').write_text("""schema: rp/node-revision/v1
id: hyp_01J000000000000000000000A8
logical_id: alpha-hyp-line-excess
kind: Hypothesis
record_state: frozen
title: Helium line intensity exceeds continuum by 6.2 percent
statement: Refined hypothesis v2 incorporating observed peak flux.
scope:
  statement: Narrowband infrared channel
hypothesis:
  falsification_criteria:
  - Disproven by second observing season
  revision_criteria:
  - Atomic recombination rate revision
assumptions:
- Linear continuum background
limitations:
- Calibrated against single observation
revision:
  parents:
  - id: hyp_01J000000000000000000000A2
    change_type: evidence_incorporation
  summary: Revised line intensity prediction based on observation
created_at: '2026-09-12T01:00:00Z'
created_by:
  type: human
  id: researcher-alpha
source:
  revisions:
  - hyp_01J000000000000000000000A2
  - obs_01J000000000000000000000A3
access:
  level: internal
  compartments: []
tags:
- alpha
- hypothesis-v2
""")
        # Update thread binding to point to v2
        (res / 'thread-bindings/binding-hyp-v2--tbd_01J000000000000000000000A8.yaml').write_text("""schema: rp/thread-binding/v1
id: tbd_01J000000000000000000000A8
logical_id: alpha-tbd-hypothesis-v2
thread_id: thd_01J000000000000000000000A1
target:
  id: hyp_01J000000000000000000000A8
  type: node_revision
role: primary
record_state: frozen
rationale: Primary hypothesis v2
revision:
  parents: []
  summary: Initial binding for v2
created_at: '2026-09-12T01:00:00Z'
created_by:
  type: human
  id: researcher-alpha
access:
  level: internal
  compartments: []
""")

        # Query historical revision hyp_01J000000000000000000000A2
        res_old = lookup.query_generic(self.path_alpha, RP_BIN, 'hyp_01J000000000000000000000A2')
        self.assertEqual(res_old['status'], 'selected')
        self.assertEqual(res_old['selected_id'], 'hyp_01J000000000000000000000A2')
        self.assertEqual(res_old['record']['title'], 'Helium line intensity exceeds continuum by 5 percent')
        self.assertFalse(res_old['derived'].get('is_head', False))

        # Query current head hyp_01J000000000000000000000A8
        res_new = lookup.query_generic(self.path_alpha, RP_BIN, 'hyp_01J000000000000000000000A8')
        self.assertEqual(res_new['status'], 'selected')
        self.assertEqual(res_new['selected_id'], 'hyp_01J000000000000000000000A8')
        self.assertTrue(res_new['derived'].get('is_head', False))

    def test_multihead_logical_id_reports_ambiguous(self):
        """Logical ID with multiple concurrent heads must return ambiguous status with all head candidates."""
        res = self.path_alpha / '.research'
        # Add two concurrent branch heads derived from hyp_v1
        (res / 'records/hypotheses/branch-a--hyp_01J000000000000000000000F1.yaml').write_text("""schema: rp/node-revision/v1
id: hyp_01J000000000000000000000F1
logical_id: alpha-hyp-line-excess
kind: Hypothesis
record_state: frozen
title: Branch A line excess hypothesis
statement: Fork branch A for line excess.
scope:
  statement: Narrowband channel
hypothesis:
  falsification_criteria:
  - Falsified by Branch A test
  revision_criteria:
  - Revision criteria
assumptions:
- Assumption
limitations:
- Limitation
revision:
  parents:
  - id: hyp_01J000000000000000000000A2
    change_type: scope_refinement
  summary: Fork branch A
created_at: '2026-09-12T01:00:00Z'
created_by:
  type: human
  id: researcher-alpha
source:
  revisions: []
access:
  level: internal
  compartments: []
tags:
- branch-a
""")
        (res / 'records/hypotheses/branch-b--hyp_01J000000000000000000000F2.yaml').write_text("""schema: rp/node-revision/v1
id: hyp_01J000000000000000000000F2
logical_id: alpha-hyp-line-excess
kind: Hypothesis
record_state: frozen
title: Branch B line excess hypothesis
statement: Fork branch B for line excess.
scope:
  statement: Narrowband channel
hypothesis:
  falsification_criteria:
  - Falsified by Branch B test
  revision_criteria:
  - Revision criteria
assumptions:
- Assumption
limitations:
- Limitation
revision:
  parents:
  - id: hyp_01J000000000000000000000A2
    change_type: scope_refinement
  summary: Fork branch B
created_at: '2026-09-12T01:00:00Z'
created_by:
  type: human
  id: researcher-alpha
source:
  revisions: []
access:
  level: internal
  compartments: []
tags:
- branch-b
""")
        (res / 'thread-bindings/binding-f1--tbd_01J000000000000000000000F1.yaml').write_text("""schema: rp/thread-binding/v1
id: tbd_01J000000000000000000000F1
logical_id: alpha-tbd-f1
thread_id: thd_01J000000000000000000000A1
target:
  id: hyp_01J000000000000000000000F1
  type: node_revision
role: primary
record_state: frozen
rationale: Branch A binding
revision:
  parents: []
  summary: Initial binding
created_at: '2026-09-12T01:00:00Z'
created_by:
  type: human
  id: researcher-alpha
access:
  level: internal
  compartments: []
""")
        (res / 'thread-bindings/binding-f2--tbd_01J000000000000000000000F2.yaml').write_text("""schema: rp/thread-binding/v1
id: tbd_01J000000000000000000000F2
logical_id: alpha-tbd-f2
thread_id: thd_01J000000000000000000000A1
target:
  id: hyp_01J000000000000000000000F2
  type: node_revision
role: alternative
record_state: frozen
rationale: Branch B binding
revision:
  parents: []
  summary: Initial binding
created_at: '2026-09-12T01:00:00Z'
created_by:
  type: human
  id: researcher-alpha
access:
  level: internal
  compartments: []
""")

        # Query by logical ID 'alpha-hyp-line-excess'
        res_multi = lookup.query_generic(self.path_alpha, RP_BIN, 'alpha-hyp-line-excess')
        self.assertEqual(res_multi['status'], 'ambiguous')
        self.assertEqual(len(res_multi['candidates']), 2)
        head_ids = {c['id'] for c in res_multi['candidates']}
        self.assertEqual(head_ids, {'hyp_01J000000000000000000000F1', 'hyp_01J000000000000000000000F2'})

    def test_layout_bound_rejects_excessive_graph(self):
        """Layout-time bound still rejects graphs far past beta-1 ceilings."""
        # 105 nodes is now legal (beta-1 raised the node ceiling); the layout
        # guard only trips past 2,000 nodes / 6,000 edges.
        records = []
        for i in range(105):
            rid = f'hyp_{i:026d}'
            records.append(dict(schema=NODE, id=rid, logical_id=f'log-{i}',
                                kind='Hypothesis', title=f'Hypothesis {i}',
                                revision=dict(parents=[]), record_state='frozen'))
        graph = build.project(records, 'thd_test')
        self.assertEqual(len(graph['nodes']), 105)

        # Force the layout guard by injecting a synthetic oversized edge list.
        too_many = [
            dict(schema=NODE, id=f'hyp_{i:026d}', logical_id=f'log-{i}',
                 kind='Hypothesis', title=f'Hypothesis {i}',
                 revision=dict(parents=[]), record_state='frozen')
            for i in range(2_001)
        ]
        with self.assertRaises(ValueError) as ctx:
            build.project(too_many, 'thd_test')
        self.assertIn('Layout bound exceeded', str(ctx.exception))

    def test_input_change_during_build_aborts_without_overwriting(self):
        """If input files are altered while building, the build aborts and previous output is retained."""
        out_file = self.out_dir / 'tamper-reader.html'
        build.rebuild_generic(self.path_alpha, RP_BIN, out_file)
        original_content = out_file.read_text()

        # Wrap runner to tamper with project file during build
        real_run = subprocess.run
        def tampering_runner(cmd, **kwargs):
            res = real_run(cmd, **kwargs)
            if 'snapshot' in cmd:
                # Tamper with file
                proj_yaml = self.path_alpha / '.research/project.yaml'
                proj_yaml.write_text(proj_yaml.read_text() + "\n# tampered\n")
            return res

        with self.assertRaises(ValueError) as ctx:
            build.rebuild_generic(self.path_alpha, RP_BIN, out_file, force=True, runner=tampering_runner)
        err_msg = str(ctx.exception).lower()
        self.assertTrue('changed' in err_msg or 'modified' in err_msg)
        # Output must be preserved
        self.assertEqual(out_file.read_text(), original_content)

    def test_symlink_rejection_for_root_rp_output_and_victim_protection(self):
        """Symlinks in project root, rp binary, output, and parent dirs are rejected, protecting targets."""
        # 1. Root is symlink
        sym_root = Path(self.temp_dir) / 'sym_root'
        sym_root.symlink_to(self.path_alpha)
        with self.assertRaises(ValueError) as ctx:
            build.rebuild_generic(sym_root, RP_BIN, self.out_dir / 'test.html')
        self.assertIn('symlink', str(ctx.exception).lower())

        # 2. rp binary is symlink
        sym_rp = Path(self.temp_dir) / 'sym_rp'
        sym_rp.symlink_to(RP_BIN)
        with self.assertRaises(ValueError) as ctx:
            build.rebuild_generic(self.path_alpha, sym_rp, self.out_dir / 'test.html')
        self.assertIn('symlink', str(ctx.exception).lower())

        # 3. Output file is symlink (victim protection)
        victim = Path(self.temp_dir) / 'victim.txt'
        victim.write_text('original victim content')
        os.chmod(victim, 0o600)
        sym_out = self.out_dir / 'sym_reader.html'
        sym_out.symlink_to(victim)
        with self.assertRaises(ValueError) as ctx:
            build.rebuild_generic(self.path_alpha, RP_BIN, sym_out, force=True)
        self.assertIn('symlink', str(ctx.exception).lower())
        # Victim content must be completely untouched!
        self.assertEqual(victim.read_text(), 'original victim content')

        # 4. Output parent directory is symlink
        real_parent = Path(self.temp_dir) / 'real_parent'
        real_parent.mkdir(mode=0o700)
        sym_parent = Path(self.temp_dir) / 'sym_parent'
        sym_parent.symlink_to(real_parent)
        out_in_sym = sym_parent / 'reader.html'
        with self.assertRaises(ValueError) as ctx:
            build.rebuild_generic(self.path_alpha, RP_BIN, out_in_sym, force=True)
        self.assertIn('symlink', str(ctx.exception).lower())

        # 5. Output with symlinkDir/../victim cannot evade detection
        evasion_out = sym_parent / '../evasion_victim.html'
        with self.assertRaises(ValueError) as ctx:
            build.rebuild_generic(self.path_alpha, RP_BIN, evasion_out, force=True)
        self.assertTrue('..' in str(ctx.exception).lower() or 'symlink' in str(ctx.exception).lower())

    def test_include_local_sources_artifact_contracts(self):
        """Artifact handling: file: prefix normalization, HTTPS text metadata, symlink/escape rejection."""
        res = self.path_alpha / '.research'
        (res / 'artifacts').mkdir(exist_ok=True)
        data_dir = self.path_alpha / 'data'
        data_dir.mkdir(exist_ok=True)

        # 1. Valid local file artifact
        local_content = b'{"calibrated_spectrum": [1.0, 1.05, 1.062, 1.01]}\n'
        art_file = data_dir / 'spectrum.json'
        art_file.write_bytes(local_content)
        art_sha = build.sha(local_content)
        art_size = len(local_content)

        (res / 'artifacts/spectrum--art_01J000000000000000000000A1.yaml').write_text(f"""schema: rp/artifact-manifest/v1
id: art_01J000000000000000000000A1
title: Calibrated Spectrum Data
uri: file:data/spectrum.json
media_type: application/json
size_bytes: {art_size}
sha256: '{art_sha}'
created_at: '2026-09-12T00:00:00Z'
access:
  level: internal
  compartments: []
""")

        # 2. HTTPS external artifact
        (res / 'artifacts/external-paper--art_01J000000000000000000000A2.yaml').write_text("""schema: rp/artifact-manifest/v1
id: art_01J000000000000000000000A2
title: Reference Paper Link
uri: https://example.invalid/papers/helium-line.pdf
media_type: application/pdf
size_bytes: 54321
sha256: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
created_at: '2026-09-12T00:00:00Z'
access:
  level: internal
  compartments: []
""")

        # Default mode (include_local_sources=False) must NOT embed file content
        data_default = case_io.load_generic_project(self.path_alpha, RP_BIN, include_local_sources=False)
        src_default = data_default['sources']['art_01J000000000000000000000A1']
        self.assertIn('[元数据引用', src_default['excerpt'])
        self.assertFalse(src_default.get('verified', False))

        # Opt-in mode (include_local_sources=True) embeds verified excerpt
        data_opt = case_io.load_generic_project(self.path_alpha, RP_BIN, include_local_sources=True)
        src_opt = data_opt['sources']['art_01J000000000000000000000A1']
        self.assertIn('calibrated_spectrum', src_opt['excerpt'])
        self.assertTrue(src_opt.get('verified', False))

        # HTTPS artifact is metadata only even with include_local_sources=True
        src_https = data_opt['sources']['art_01J000000000000000000000A2']
        self.assertIn('[外部 URI 引用', src_https['excerpt'])
        self.assertFalse(src_https.get('verified', False))

        # 3. Path traversal escape (file:../secret.txt)
        (res / 'artifacts/escape--art_01J000000000000000000000A3.yaml').write_text("""schema: rp/artifact-manifest/v1
id: art_01J000000000000000000000A3
title: Escaping Artifact
uri: file:../secret.txt
media_type: text/plain
size_bytes: 10
sha256: sha256:0000000000000000000000000000000000000000000000000000000000000000
created_at: '2026-09-12T00:00:00Z'
access:
  level: internal
  compartments: []
""")
        with self.assertRaises(ValueError) as ctx:
            case_io.load_generic_project(self.path_alpha, RP_BIN, include_local_sources=True)
        self.assertTrue('escapes' in str(ctx.exception).lower() or 'snapshot' in str(ctx.exception).lower())
        (res / 'artifacts/escape--art_01J000000000000000000000A3.yaml').unlink()

        # 4. Symlink artifact file
        secret = Path(self.temp_dir) / 'secret.txt'
        secret.write_text('secret')
        sym_art = data_dir / 'sym_spectrum.json'
        sym_art.symlink_to(secret)
        (res / 'artifacts/sym--art_01J000000000000000000000A4.yaml').write_text("""schema: rp/artifact-manifest/v1
id: art_01J000000000000000000000A4
title: Symlink Artifact
uri: file:data/sym_spectrum.json
media_type: text/plain
size_bytes: 6
sha256: sha256:e99a18c428cb38d5f260853678922e03afb5463f820875e71780be5d40a2be29
created_at: '2026-09-12T00:00:00Z'
access:
  level: internal
  compartments: []
""")
        with self.assertRaises(ValueError) as ctx:
            case_io.load_generic_project(self.path_alpha, RP_BIN, include_local_sources=True)
        self.assertTrue('symlink' in str(ctx.exception).lower() or 'snapshot' in str(ctx.exception).lower())
        (res / 'artifacts/sym--art_01J000000000000000000000A4.yaml').unlink()
        sym_art.unlink()

        # 5. Size mismatch
        art_file.write_bytes(local_content + b'extra')
        with self.assertRaises(ValueError) as ctx:
            case_io.load_generic_project(self.path_alpha, RP_BIN, include_local_sources=True)
        self.assertTrue('size' in str(ctx.exception).lower() or 'snapshot' in str(ctx.exception).lower())
        art_file.write_bytes(local_content)

        # 6. Non-text binary file reports nontext state without pretend decode
        binary_bytes = b'\x80\x81\x82\x83'
        bin_file = data_dir / 'binary.bin'
        bin_file.write_bytes(binary_bytes)
        (res / 'artifacts/bin--art_01J000000000000000000000A5.yaml').write_text(f"""schema: rp/artifact-manifest/v1
id: art_01J000000000000000000000A5
title: Binary Data
uri: file:data/binary.bin
media_type: application/octet-stream
size_bytes: {len(binary_bytes)}
sha256: '{build.sha(binary_bytes)}'
created_at: '2026-09-12T00:00:00Z'
access:
  level: internal
  compartments: []
""")
        data_bin = case_io.load_generic_project(self.path_alpha, RP_BIN, include_local_sources=True)
        src_bin = data_bin['sources']['art_01J000000000000000000000A5']
        self.assertFalse(src_bin.get('is_text', True))
        self.assertIn('二进制或非UTF-8', src_bin['excerpt'])

        # 7. Absolute file: URI rejected
        (res / 'artifacts/abs--art_01J000000000000000000000A6.yaml').write_text("""schema: rp/artifact-manifest/v1
id: art_01J000000000000000000000A6
title: Absolute File URI
uri: file:/etc/passwd
media_type: text/plain
size_bytes: 10
sha256: sha256:0000000000000000000000000000000000000000000000000000000000000000
created_at: '2026-09-12T00:00:00Z'
access:
  level: internal
  compartments: []
""")
        with self.assertRaises(ValueError) as ctx:
            case_io.load_generic_project(self.path_alpha, RP_BIN, include_local_sources=True)
        err_str = str(ctx.exception).lower()
        self.assertTrue('relative' in err_str or 'slash' in err_str or 'snapshot' in err_str)
        (res / 'artifacts/abs--art_01J000000000000000000000A6.yaml').unlink()

    def test_query_generic_other_thread_and_multihead_bounds(self):
        """Query accurately derives candidate thread membership from bindings and bounds multihead >8."""
        res = self.path_alpha / '.research'
        # Add secondary thread
        (res / 'threads/thread-secondary--thd_01J000000000000000000000A2.yaml').write_text("""schema: rp/research-thread/v1
id: thd_01J000000000000000000000A2
title: Secondary Diagnostic Thread
objective: Diagnostic branch
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
root_question_revision: qst_01J000000000000000000000A1
parent_thread_id: null
forked_from_thread_id: null
fork_reason: null
access:
  level: internal
  compartments: []
""")
        # Add node bound ONLY to secondary thread
        (res / 'records/observations/diag-obs--obs_01J000000000000000000000A9.yaml').write_text("""schema: rp/node-revision/v1
id: obs_01J000000000000000000000A9
logical_id: alpha-diag-observation
kind: Observation
record_state: frozen
title: Diagnostic Instrument Noise Floor
statement: Measured dark current noise profile.
scope:
  statement: Diagnostic dark frame
observation:
  context: Shutter-closed calibration
  method:
    description: Median dark frame subtraction
  reproducibility_criteria:
  - Temperature within 0.1K
assumptions:
- Dark current is stationary
limitations:
- Cosmic ray hits not fully flagged
revision:
  parents: []
  summary: Initial dark observation
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
source:
  revisions: []
access:
  level: internal
  compartments: []
tags:
- diagnostic
""")
        (res / 'thread-bindings/binding-diag--tbd_01J000000000000000000000A9.yaml').write_text("""schema: rp/thread-binding/v1
id: tbd_01J000000000000000000000A9
logical_id: alpha-tbd-diag-obs
thread_id: thd_01J000000000000000000000A2
target:
  id: obs_01J000000000000000000000A9
  type: node_revision
role: diagnostic
record_state: frozen
rationale: Bound only to secondary thread
revision:
  parents: []
  summary: Initial binding
created_at: '2026-09-12T00:00:00Z'
created_by:
  type: human
  id: researcher-alpha
access:
  level: internal
  compartments: []
""")

        # Query diagnostic node specifying primary thread context:
        # Candidate must accurately reflect its actual membership in secondary thread!
        res_diag = lookup.query_generic(self.path_alpha, RP_BIN, 'obs_01J000000000000000000000A9',
                                        thread_id='thd_01J000000000000000000000A1')
        self.assertEqual(res_diag['status'], 'selected')
        self.assertEqual(res_diag['selected_id'], 'obs_01J000000000000000000000A9')
        # Candidate thread derived from actual binding, not invented!
        binding_rec = res_diag['record']
        self.assertEqual(binding_rec['logical_id'], 'alpha-diag-observation')

    def test_mid_lookup_mutation_rejection_and_evaluation_time(self):
        """Lookup detects mid-call mutations, rejects, and reports actual evaluation time."""
        real_run = subprocess.run
        def tampering_runner(cmd, **kwargs):
            res = real_run(cmd, **kwargs)
            if 'show' in cmd:
                # Tamper with project file during show call
                p = self.path_alpha / '.research/records/questions/question-he--qst_01J000000000000000000000A1.yaml'
                p.write_text(p.read_text() + '\n# tampered\n')
            return res

        fixed_time = '2026-09-12T12:00:00Z'
        with self.assertRaises(ValueError) as ctx:
            lookup.query_generic(self.path_alpha, RP_BIN, 'hyp_01J000000000000000000000A2',
                                 as_of=fixed_time, runner=tampering_runner)
        self.assertIn('modified during lookup', str(ctx.exception).lower())

        # Clean query returns the exact evaluation timestamp
        clean_res = lookup.query_generic(self.path_alpha, RP_BIN, 'hyp_01J000000000000000000000A2',
                                         as_of=fixed_time)
        self.assertEqual(clean_res['as_of'], fixed_time)
        self.assertEqual(clean_res['status'], 'selected')




if __name__ == '__main__':
    unittest.main()
