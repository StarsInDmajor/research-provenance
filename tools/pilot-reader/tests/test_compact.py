"""Compact delivery regression; optional private accepted snapshot stays outside repo."""
import copy
from html import escape
import json
import os
from pathlib import Path
import sys
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build
from graph_projection import project
from test_projection import node, edge, binding
from entry_fixture import Tree

STAMP = dict(as_of='2026-09-12T00:00:00Z', generated_at='2026-09-12T00:00:00Z')

def elements(tree):
    yield tree
    for child in tree['children']:
        yield from elements(child)

class CompactTests(unittest.TestCase):
    def test_single_complete_text_map_and_empty_svg_shell(self):
        a = node('a')
        attack = '</pre></script>& 😀 中文 \u2028\u2029 @@JS@@ @@FALLBACK@@ "quoted"'
        a['statement'] = attack
        records = [a, node('b'), edge('e', 'a', 'b'), binding('ba', 'a', 'primary')]
        data = dict(graph=project(records, 't'), records={r['id']:r for r in records}, sources={}, notes={})
        before = copy.deepcopy(data)
        html = build.render(data, STAMP)
        parser = Tree(); parser.feed(html)
        els = list(elements(parser.root))
        canonical = [e for e in els if e['attrs'].get('id') == 'canonical-records']
        self.assertEqual(len(canonical), 1, 'complete native raw-map fallback required')
        self.assertEqual(json.loads(canonical[0]['text']), data['records'])
        self.assertIn(escape(build.compact(data['records']), quote=False), html)
        metadata = json.loads(next(e['text'] for e in els if e['attrs'].get('id') == 'graph-data'))
        self.assertNotIn('records', metadata)
        self.assertTrue(all('raw' not in n for n in metadata['graph']['nodes'] + metadata['graph']['edges']))
        svg = next(e for e in els if e['attrs'].get('id') == 'graph')
        self.assertFalse(svg['children'], 'no repeated static diagram in compact delivery')
        self.assertFalse(any('data-key' in e['attrs'] for e in els))
        workspace = next(e for e in els if e['attrs'].get('id') == 'workspace')
        self.assertNotIn(canonical[0], list(elements(workspace)))
        self.assertEqual(data, before)
        self.assertEqual(len(parser.scripts), 3)

    def test_new_svg_resource_mutation_aborts_and_preserves_previous_output(self):
        import tempfile
        import shutil
        from unittest.mock import patch
        from test_reusable_reader import create_project_alpha, RP_BIN
        with tempfile.TemporaryDirectory(prefix='rp-compact-render-mutation-') as tmp:
            root = Path(tmp); project_root = root/'project'; project_root.mkdir()
            create_project_alpha(project_root)
            output = root/'graph.html'
            build.rebuild_generic(project_root, RP_BIN, output)
            before = output.read_bytes()
            copied = root/'reader'; shutil.copytree(build.HERE, copied)
            original_render = build.render
            def mutate(data, evidence):
                html = original_render(data, evidence)
                (copied/'svg.js').write_text('changed source after render')
                return html
            with patch.object(build, 'HERE', copied), patch.object(build, 'render', mutate):
                with self.assertRaisesRegex(ValueError, 'Reader source modified'):
                    build.rebuild_generic(project_root, RP_BIN, output)
            self.assertEqual(output.read_bytes(), before)

    @unittest.skipUnless(os.environ.get('RP_COMPACT_SNAPSHOT'), 'explicit private copied snapshot only')
    def test_accepted_whole_project_fits_with_exact_records(self):
        import case_io
        import subprocess
        snapshot = Path(os.environ['RP_COMPACT_SNAPSHOT']).read_bytes()
        snap = json.loads(snapshot)['data']
        self.assertEqual(len(snap['objects']), 334)
        # Exercise the real generic loader, with its Core response fixed to the
        # accepted copy rather than a concurrently changing private candidate.
        import tempfile
        with tempfile.TemporaryDirectory(prefix='rp-compact-render-snapshot-') as tmp:
            root = Path(tmp); (root/'.research').mkdir()
            rp = Path(os.environ.get('RP_BIN', Path(__file__).resolve().parents[3]/'target/debug/rp'))
            def accepted(cmd, **kwargs):
                self.assertIn('snapshot', cmd)
                return subprocess.CompletedProcess(cmd, 0, snapshot, b'')
            data = case_io.load_generic_project(root, rp, runner=accepted)
            self.assertEqual((len(data['graph']['nodes']), len(data['graph']['edges'])), (78,128))
            html = build.render(data, STAMP | dict(mode='generic-snapshot', canonical_count=334))
            parser = Tree(); parser.feed(html)
            raw = next(e['text'] for e in elements(parser.root) if e['attrs'].get('id') == 'canonical-records')
            self.assertEqual(build.compact(json.loads(raw)).encode(), build.compact(snap['objects']).encode())
            self.assertLess(len(html.encode()), 500000)

if __name__ == '__main__': unittest.main()
