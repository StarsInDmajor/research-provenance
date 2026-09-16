"""Stage 2: archive-bound optional locators; no live scientific input."""
import copy
import importlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build
from test_projection import node, project

STAMP = '2026-09-12T09:19:56Z'

def digest(r): return build.sha(build.compact(r).encode())

class SourceLocators(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name); (self.root/'sources').mkdir()
        self.body='标题 </pre></script>\n中文😀 exact\nthird line\n'
        self.source=dict(path='src/config.py',start_line=10,end_line=12,section='# Config',
                         sha256=build.sha(b'captured full source'),excerpt_sha256=build.sha(self.body.encode()),captured_at=STAMP)
        s=self.source
        self.packet=(f"# Archive\n\n## {s['path']}:L10-L12\nOriginal file {s['sha256']}; excerpt {s['excerpt_sha256']}; captured {STAMP}\n\n```text\n"+self.body+'\n```\n').encode()
        (self.root/'sources/p.md').write_bytes(self.packet)
        self.art=dict(id='art_a',schema='rp/artifact-manifest/v1',uri='file:sources/p.md',sha256=build.sha(self.packet),size_bytes=len(self.packet))
        n=node('a'); n['logical_id']='config'; n['source']={'artifacts':['art_a']}
        self.data=dict(project={'id':'proj_a'},graph=project([n], 't'),records={'a':n,'art_a':self.art},observed={})
        self.value=dict(version=1,project_id='proj_a',entries={'a':dict(revision_id='a',canonical_digest=digest(n),sources=[dict(artifact_id='art_a',artifact_digest=digest(self.art),packet_path='sources/p.md',**s)])})
        self.path=self.root/'locators.json'

    def module(self):
        self.assertTrue((Path(build.__file__).parent/'source_locators.py').exists(), 'optional source locator loader missing')
        return importlib.import_module('source_locators')

    def load(self, value=None):
        self.path.write_text(json.dumps(self.value if value is None else value))
        return self.module().load(self.path,self.data,self.root)

    def test_verified_prefix_exact_original_lines_and_different_display_digest(self):
        before=copy.deepcopy(self.data['records']); self.load()
        self.assertEqual(before,self.data['records'])
        loc=self.data['sourceLocators']; self.assertEqual(len(loc['chunks']),1)
        chunk=self.module().unpack(loc)[0]
        self.assertEqual(chunk['path'],'src/config.py'); self.assertEqual(chunk['end_line'],12)
        self.assertEqual(chunk['display_start_line'],10)
        self.assertEqual(chunk['display_sha256'],build.sha(chunk['excerpt'].encode()))
        self.assertEqual(chunk['excerpt_sha256'],build.sha(self.body.encode()))
        self.assertTrue(self.body.startswith(chunk['excerpt'])); self.assertLessEqual(len(chunk['excerpt'].encode()),240)
        self.assertEqual(self.data['graph']['nodes'][0]['sourceLocators'],[[0,0]])
        self.assertEqual(self.data['observed']['sources/p.md'],self.packet)

    def test_strict_sidecar_rejections_atomic_projection(self):
        mutations=[lambda v:v.update(project_id='wrong'),lambda v:v.update(extra=1),lambda v:v.update(entries=[]),
            lambda v:v['entries']['a'].update(revision_id='b'),lambda v:v['entries']['a'].update(canonical_digest=build.sha(b'bad')),
            lambda v:v['entries']['a'].update(sources={}),lambda v:v['entries']['a'].update(sources=[v['entries']['a']['sources'][0]]*9),
            lambda v:v['entries']['a']['sources'][0].update(artifact_id='no'),lambda v:v['entries']['a']['sources'][0].update(artifact_id=[]),
            lambda v:v['entries']['a']['sources'][0].update(artifact_digest=build.sha(b'bad')),
            lambda v:v['entries']['a']['sources'][0].update(section=[]),lambda v:v['entries']['a']['sources'][0].update(extra='bad'),
            lambda v:v['entries']['a']['sources'][0].update(start_line=True),lambda v:v['entries']['a']['sources'][0].update(end_line=9),
            lambda v:v['entries']['a']['sources'][0].update(end_line=13),lambda v:v['entries']['a']['sources'][0].update(sha256=build.sha(b'bad')),
            lambda v:v['entries']['a']['sources'][0].update(excerpt_sha256=build.sha(b'bad')),
            lambda v:v['entries']['a']['sources'][0].update(captured_at='yesterday'),
            lambda v:v['entries']['a'].update(sources=v['entries']['a']['sources']*2)]
        for field in ('path','packet_path'):
            for path in ('/tmp/source','../source','a/../b','a//b','./a','a\\b','https:x'):
                mutations.append(lambda v,f=field,p=path:v['entries']['a']['sources'][0].update(**{f:p}))
        for change in mutations:
            with self.subTest(change=change):
                v=copy.deepcopy(self.value); change(v)
                with self.assertRaises(ValueError): self.load(v)
                self.assertNotIn('sourceLocators',self.data)
        self.data['records']['a']['source']['artifacts']=[]
        self.value['entries']['a']['canonical_digest']=digest(self.data['records']['a'])
        with self.assertRaises(ValueError): self.load()

    def test_packet_and_sidecar_rechecked_at_both_publication_boundaries(self):
        from unittest.mock import patch
        import case_io
        from test_reusable_reader import RP_BIN
        data=self.data
        (self.root/'project/sources').mkdir(parents=True)
        packet_path=self.root/'project/sources/p.md'
        packet_path.write_bytes(self.packet)
        data.update(sources={},notes={},names=[],thread='t')
        output=self.root/'graph.html'
        self.path.write_text(json.dumps(self.value))
        output.write_text('previous output'); output.chmod(0o600)
        for boundary in ('render','atomic_write'):
            for target in (self.path,packet_path):
                self.path.write_text(json.dumps(self.value)); packet_path.write_bytes(self.packet)
                data['observed']={}
                original=getattr(build,boundary)
                def mutate(*args):
                    if boundary=='render':
                        result=original(*args);target.write_bytes(b'tampered');return result
                    target.write_bytes(b'tampered');return original(*args)
                def observed(*args,**kwargs):return {'sources/p.md':packet_path.read_bytes()}
                with patch.object(case_io,'load_generic_project',return_value=data), patch.object(case_io,'observe_contained',observed), patch.object(build,boundary,mutate):
                    with self.assertRaisesRegex(ValueError,'modified|changed'):
                        build.rebuild_generic(self.root/'project',RP_BIN,output,as_of=STAMP,source_locators=self.path,force=True)
                self.assertEqual(output.read_text(),'previous output')
        # The real-loader sidecar case below also verifies existing destination preservation.

    def test_duplicate_oversize_symlink_missing_and_packet_tamper(self):
        mod=self.module()
        with self.assertRaises(ValueError):mod.strict_json('['*20000+']'*20000)
        for raw in ('{"version":1,"version":1}', ' '*192001, '{"x":NaN}', '[]', '['*1500+']'*1500):
            self.path.write_text(raw)
            with self.assertRaises(ValueError): mod.load(self.path,self.data,self.root)
        with self.assertRaises((OSError,ValueError)): mod.load(self.root/'absent',self.data,self.root)
        self.path.write_text(json.dumps(self.value)); link=self.root/'link'; link.symlink_to(self.path)
        with self.assertRaises(ValueError): mod.load(link,self.data,self.root)
        (self.root/'sources/p.md').write_bytes(self.packet+b'tamper')
        with self.assertRaises(ValueError): self.load()
        (self.root/'sources/p.md').unlink(); (self.root/'sources/p.md').symlink_to(self.path)
        with self.assertRaises(ValueError): self.load()

    def test_corrupt_capture_even_when_packet_manifest_is_rebound(self):
        for packet in (self.packet.replace('中文'.encode(),b'wrong'),self.packet[:-6],self.packet+b'\xff'):
            (self.root/'sources/p.md').write_bytes(packet)
            self.art.update(sha256=build.sha(packet),size_bytes=len(packet))
            self.value['entries']['a']['sources'][0]['artifact_digest']=digest(self.art)
            with self.assertRaises(ValueError):self.load()
            self.assertNotIn('sourceLocators',self.data)

    def test_dedup_and_explicit_unavailable(self):
        n=copy.deepcopy(self.data['records']['a']); n['id']='b'; self.data['records']['b']=n
        self.data['graph']=project([self.data['records']['a'],n], 't')
        self.value['entries']['b']=dict(revision_id='b',canonical_digest=digest(n),sources=[])
        self.load(); self.assertEqual(self.data['graph']['nodes'][1]['sourceLocators'],[])
        self.value['entries']['b']['sources']=copy.deepcopy(self.value['entries']['a']['sources'])
        self.load(); self.assertEqual(len(self.data['sourceLocators']['chunks']),1)

    def test_packet_nested_fences_and_utf8_line_budget_no_partial_lines(self):
        mod=self.module(); s=self.source
        for body in ('```text\nnested\n```\n', '中'*100+'\nsmall\nlast\n'):
            s=dict(s,excerpt_sha256=build.sha(body.encode()))
            packet=(f"## {s['path']}:L10-L12\nOriginal file {s['sha256']}; excerpt {s['excerpt_sha256']}; captured {STAMP}\n\n```text\n"+body+'\n```\n').encode()
            self.assertEqual(mod.captured_excerpt(packet,s),body)
            text,end=mod.prefix(body,10)
            self.assertLessEqual(len(text.encode()),240)
            self.assertTrue(body.startswith(text))
            self.assertEqual(end,9+len(text.splitlines()))
            if body.startswith('中'): self.assertEqual(text,'')

class SourcePublication(unittest.TestCase):
    def test_optional_build_rechecks_sidecar_and_packet_before_atomic_replace(self):
        from unittest.mock import patch
        import inspect
        import case_io
        from installed_reader import fixture
        from test_reusable_reader import RP_BIN
        self.assertIn('source_locators',inspect.signature(build.rebuild_generic).parameters,
                      'generic opt-in missing')
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); pr=root/'project'; rid,_,_=fixture(pr,'A1')
            data=case_io.load_generic_project(pr,RP_BIN,as_of=STAMP)
            value=dict(version=1,project_id=data['project']['id'],entries={rid:dict(
                revision_id=rid,canonical_digest=digest(data['records'][rid]),sources=[])})
            path=root/'locators.json'; path.write_text(json.dumps(value)); output=root/'graph.html'
            build.rebuild_generic(pr,RP_BIN,output,as_of=STAMP,source_locators=path)
            old=output.read_bytes(); self.assertIn(b'sourceLocators',old)
            for boundary in ('render','atomic_write'):
                original=getattr(build,boundary)
                def mutate(*args):
                    if boundary=='render':
                        result=original(*args); path.write_text('{}'); return result
                    path.write_text('{}'); return original(*args)
                path.write_text(json.dumps(value))
                with patch.object(build,boundary,mutate), self.assertRaisesRegex(ValueError,'locator.*changed'):
                    build.rebuild_generic(pr,RP_BIN,output,as_of=STAMP,source_locators=path)
                self.assertEqual(old,output.read_bytes())
            with self.assertRaises((OSError,ValueError)):
                build.rebuild_generic(pr,RP_BIN,output,as_of=STAMP,source_locators=root/'missing')
            self.assertEqual(old,output.read_bytes())

if __name__=='__main__': unittest.main()
