"""Explicit presentation history, never inferred scientific validity (stdlib only)."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build
import case_io
from test_projection import node, project
from test_reusable_reader import create_project_alpha, RP_BIN

STAMP = '2026-09-12T00:00:00Z'

def entry(raw, source):
    return dict(revision_id=raw['id'], canonical_digest=build.sha(build.compact(raw).encode()),
                status='historical', label='历史记录', reason='Documented earlier configuration; not false science.',
                assessed_at=STAMP, source_refs=[dict(id=source['id'], canonical_digest=build.sha(build.compact(source).encode()),
                                                   locator='source-index.json: earlier scope, lines 1–4')])

class NodeStatuses(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='rp-history-'); self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        a = node('a'); source = dict(id='ref_a', schema='rp/external-reference/v1', title='Dated source')
        self.data = dict(project={'id':'proj_a'}, graph=project([a], 't'), records={'a':a,'ref_a':source})
        self.value = dict(version=1, project_id='proj_a', entries={'a':entry(a, source)})
        self.path = self.root/'statuses.json'

    def load(self, value=None, path=None):
        self.path.write_text(json.dumps(self.value if value is None else value))
        self.assertTrue(callable(getattr(case_io, 'load_node_statuses', None)), 'explicit loader missing')
        return case_io.load_node_statuses(path or self.path, self.data)

    def test_projection_only_and_no_automatic_ingestion(self):
        before = copy.deepcopy(self.data)
        observed = self.load()
        self.assertEqual(self.data['records'], before['records'])
        self.assertEqual(self.data['graph']['nodes'][0]['presentationStatus'], self.value['entries']['a'])
        self.assertEqual(observed, self.path.read_bytes())
        self.assertNotIn('presentationStatus', before['graph']['nodes'][0])

    def test_strict_protocol_rejections(self):
        mutations = [
            lambda v:v.update(project_id='wrong'), lambda v:v.update(extra=True),
            lambda v:v['entries']['a'].update(revision_id='missing'),
            lambda v:v['entries']['a'].update(canonical_digest='sha256:'+'0'*64),
            lambda v:v['entries']['a'].update(status='fossil'),
            lambda v:v['entries']['a'].update(label='invalid'),
            lambda v:v['entries']['a'].update(reason=''),
            lambda v:v['entries']['a'].update(reason='x'+' '*2000),
            lambda v:v['entries']['a'].update(assessed_at='2026-09-12X00:00:00+00:00'),
            lambda v:v['entries']['a'].update(assessed_at='yesterday'),
            lambda v:v['entries']['a'].update(source_refs=[]),
            lambda v:v['entries']['a']['source_refs'][0].update(id='absent'),
            lambda v:v['entries']['a']['source_refs'][0].update(canonical_digest='sha256:'+'0'*64),
            lambda v:v['entries']['a'].update(superseded_by=['absent']),
            lambda v:v['entries'].update(missing=v['entries']['a']),
            lambda v:v['entries']['a'].update(extra=True),
            lambda v:v.update(entries={str(i):v['entries']['a'] for i in range(101)}),
        ]
        for change in mutations:
            with self.subTest(change=change):
                v=copy.deepcopy(self.value); change(v)
                with self.assertRaises(ValueError): self.load(v)

    def test_duplicate_oversize_and_unsafe_paths(self):
        self.load()
        for content in ('{"version":1,"version":1}', ' '*65537):
            self.path.write_text(content)
            with self.assertRaises(ValueError): case_io.load_node_statuses(self.path, self.data)
        link=self.root/'link.json'; link.symlink_to(self.path)
        for path in (link, self.root/'missing/../statuses.json'):
            with self.assertRaises(ValueError): self.load(path=path)

    def test_mutated_source_record_does_not_attach_status(self):
        self.data['records']['ref_a']['title']='Changed source'
        with self.assertRaises(ValueError): self.load()

    def test_border_semantics_and_labels(self):
        n=self.data['graph']['nodes'][0]
        for fresh in ('unknown','fresh','stale','review-due'):
            n['freshness']=fresh
            self.assertNotIn('history-mark', build.render_svg(self.data['graph']))
        n['current']=False
        self.assertIn('history-mark', build.render_svg(self.data['graph']))
        self.assertIn('旧修订', build.render_svg(self.data['graph']))
        n['ghost']=True
        self.assertIn('历史端点(当前引用)', build.render_svg(self.data['graph']))
        n['current']=True; n['ghost']=False
        self.load()
        self.assertIn('history-mark', build.render_svg(self.data['graph']))
        self.assertIn('最新记录修订·描述历史记录', build.render_svg(self.data['graph']))
        self.assertIn('实线仅表示没有显式历史标记', build.reading_guide(self.data['graph']))
        n['presentationStatus'].update(status='superseded',label='已替代')
        self.assertIn('history-mark', build.render_svg(self.data['graph']))
        self.assertIn('描述已替代', build.render_svg(self.data['graph']))
        n['presentationStatus'].update(status='current',label='材料声明现用')
        self.assertNotIn('history-mark', build.render_svg(self.data['graph']))

    def test_rebuild_rechecks_sidecar_through_atomic_publication(self):
        from installed_reader import fixture
        project_root=self.root/'project'; project_root.mkdir(); fixture(project_root, 'A1')
        data=case_io.load_generic_project(project_root, RP_BIN, as_of=STAMP)
        # Neutral canonical source reference in the packaging fixture.
        sources=[r for r in data['records'].values() if r['schema'] in ('rp/artifact-manifest/v1','rp/external-reference/v1')]
        self.assertTrue(sources)
        raw=data['graph']['nodes'][0]['raw']
        self.value=dict(version=1,project_id=data['project']['id'],entries={raw['id']:entry(raw,sources[0])})
        self.path.write_text(json.dumps(self.value))
        output=self.root/'graph.html'; build.rebuild_generic(project_root,RP_BIN,output,as_of=STAMP)
        old=output.read_bytes(); original=build.render
        def mutate(*args):
            html=original(*args); self.path.write_text('{}'); return html
        self.assertIn('node_statuses', __import__('inspect').signature(build.rebuild_generic).parameters)
        with patch.object(build,'render',mutate), self.assertRaisesRegex(ValueError,'status.*changed'):
            build.rebuild_generic(project_root,RP_BIN,output,as_of=STAMP,node_statuses=self.path)
        self.assertEqual(old,output.read_bytes())
        # The second check must run inside atomic_write, not only before temp creation.
        self.path.write_text(json.dumps(self.value))
        original_atomic=build.atomic_write
        def mutate_before_publication(output, html, final_check):
            self.path.write_text('{}')
            return original_atomic(output, html, final_check)
        with patch.object(build, 'atomic_write', mutate_before_publication), self.assertRaisesRegex(ValueError,'status.*changed'):
            build.rebuild_generic(project_root,RP_BIN,output,as_of=STAMP,node_statuses=self.path)
        self.assertEqual(old,output.read_bytes())
        # Wrong project/digest/ID etc. cannot touch an already delivered graph.
        for key, value in [('project_id','wrong'), ('version',2)]:
            invalid=copy.deepcopy(self.value); invalid[key]=value
            self.path.write_text(json.dumps(invalid))
            with self.assertRaises(ValueError):
                build.rebuild_generic(project_root,RP_BIN,output,as_of=STAMP,node_statuses=self.path)
            self.assertEqual(old,output.read_bytes())

if __name__=='__main__': unittest.main()
