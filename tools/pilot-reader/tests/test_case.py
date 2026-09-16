"""Two-case spike contracts; synthetic bytes are not empirical CSP evidence."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build
import case_io
import lookup
from graph_projection import project
from test_projection import node, edge


class CaseTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='rp-csp-case-test-')
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.root = self.base/'project'; self.root.mkdir()
        self.rp = self.base/'rp'; self.rp.write_bytes(b'synthetic executable identity')
        self.manifest = self.base/'case-manifest.json'
        self.admission = self.base/'admission.json'
        self.calls = []
        self.a = node('obs_'+'0'*24+'03', kind='Observation')
        self.b = node('int_'+'0'*24+'04', kind='Interpretation')
        self.a['title'] = '历史格式'; self.b['title'] = '禁用机制'
        self.a['source'] = {'artifacts':['art_'+'0'*25+'1']}
        self.b['source'] = {'revisions':[self.a['id']]}
        self.e = edge('rel_'+'0'*25+'1',self.b['id'],self.a['id'])
        self.e['source'] = {'revisions':[self.a['id'],self.b['id']]}
        self.art = dict(schema='rp/artifact-manifest/v1', id='art_'+'0'*25+'1',
                        title='literal source', uri='file:sources/excerpt.txt',
                        sha256=build.sha(b'literal CSP excerpt'), size_bytes=19)
        self.proj = dict(schema='rp/project/v1',id='proj_'+'0'*25+'1')
        self.thread = dict(schema='rp/research-thread/v1',id='thd_'+'0'*25+'1')
        self.records = [self.a,self.b,self.e,self.art,self.proj,self.thread]
        for i,r in enumerate(self.records): self.put(f'.research/{i}.yaml',r)
        self.put('name-map.json',[{'alias':'I03','id':self.a['id'],'name':'历史格式'},
                                  {'alias':'I04','id':self.b['id'],'name':'禁用机制'}])
        (self.root/'sources').mkdir(); (self.root/'sources/excerpt.txt').write_bytes(b'literal CSP excerpt')
        self.m = dict(schema='rp/pilot-case-manifest/v1',case_id='csp-startup-v1',
                      mode='single-snapshot',project_id=self.proj['id'],thread=self.thread['id'],
                      as_of='2026-09-10T23:59:59Z',rp_path=str(self.rp),rp_sha256=build.sha(self.rp.read_bytes()),
                      inventory=build.identity(build.observe(self.root)),canonical_count=6,
                      schema_counts={build.NODE:2,build.RELATION:1,'rp/artifact-manifest/v1':1,
                                     'rp/project/v1':1,'rp/research-thread/v1':1},
                      name_map='name-map.json',sources={'excerpt':dict(artifact_id=self.art['id'],
                      copied_path='sources/excerpt.txt',original_path='synthetic.txt',
                      original_sha256=build.sha(b'literal CSP excerpt'),sections=[dict(heading='literal',start_line=1,end_line=1)],
                      sha256=self.art['sha256'],size_bytes=19,extraction='literal')})
        self.seal()
        self.patch = patch.object(case_io,'ADMISSION',self.admission); self.patch.start(); self.addCleanup(self.patch.stop)

    def put(self,name,value):
        p=self.root/name; p.parent.mkdir(parents=True,exist_ok=True)
        p.write_text(json.dumps(value,ensure_ascii=False))

    def seal(self):
        self.m['inventory']=build.identity(build.observe(self.root))
        self.manifest.write_text(json.dumps(self.m))
        self.admission.write_text(json.dumps({'case_id':'csp-startup-v1','manifest_sha256':build.sha(self.manifest.read_bytes())}))

    def runner(self,args,**kwargs):
        self.calls.append(args[1])
        data={'valid':True,'canonical_object_count':6}
        if args[1]=='show':
            rid=args[-1]; data={'object':next(r for r in self.records if r['id']==rid),'derived':{'is_head':True}}
        if args[1]=='history':
            logical=args[-1]; data={'logical_id':logical,'revisions':[r for r in self.records if r.get('logical_id')==logical],
                                   'heads':[r['id'] for r in self.records if r.get('logical_id')==logical]}
        return subprocess.CompletedProcess(args,0,json.dumps(dict(status='ok',exit_code=0,findings=[],data=data)).encode(),b'')

    def load(self): return case_io.load_case(self.root,self.manifest,self.rp,runner=self.runner)

    def test_single_snapshot_loads_validated_observed_bytes(self):
        result=self.load()
        self.assertEqual(result.get('records'),{r['id']:r for r in self.records})
        self.assertEqual(self.calls,['validate'])
        self.assertEqual(result['graph']['nodes'][0]['title'], '禁用机制')

    def test_case_builder_publishes_single_snapshot_no_fake_update(self):
        output=self.base/'private/csp-reader.html'
        report=build.rebuild(self.root,None,self.rp,output,self.m['as_of'],self.m['as_of'],
                             runner=self.runner,case_manifest=self.manifest)
        self.assertEqual(report['graph_counts']['current_nodes'],2)
        html=output.read_text()
        self.assertIn('单快照',html)
        self.assertNotIn('32 条增至 35 条',html)
        self.assertNotIn('10 / 11 节点',html)
        self.assertNotIn('没有独立记录的精确发生时间',html)
        self.assertEqual(self.calls,['validate'])
        old=output.read_bytes()
        (self.root/'sources/excerpt.txt').write_bytes(b'mutated')
        with self.assertRaises(ValueError):
            build.rebuild(self.root,None,self.rp,output,self.m['as_of'],self.m['as_of'],
                          runner=self.runner,case_manifest=self.manifest)
        self.assertEqual(output.read_bytes(),old)

    def test_exact_alias_title_and_id_lookup(self):
        data=self.load()
        for name in ['I03','历史格式',self.a['id']]:
            result=lookup.select(data,name)
            self.assertEqual(result.get('selected_id'),self.a['id'])
        self.assertEqual(lookup.select(data,'unknown')['status'],'not-found')
        self.assertEqual(lookup.select(data,'obs_'+'9'*26)['status'],'not-found')

    def test_ambiguous_title_never_selects_first_and_is_bounded(self):
        self.b['title']=self.a['title']; self.put('.research/1.yaml',self.b); self.seal()
        result=lookup.select(self.load(),self.a['title'],limit=1)
        self.assertEqual(result.get('status'),'ambiguous')
        self.assertNotIn('selected_id',result)
        self.assertEqual(len(result['candidates']),1)
        self.assertEqual(result['total'],2)
        self.assertTrue(result['truncated'])

    def test_live_lookup_returns_direction_sources_and_history(self):
        result=lookup.query(self.root,self.manifest,self.rp,'I03',runner=self.runner)
        self.assertEqual(result.get('record'),self.a)
        self.assertEqual(result['incoming'],[self.e]); self.assertEqual(result['outgoing'],[])
        self.assertEqual(result['neighbors'][self.b['id']],self.b)
        self.assertEqual(result['sources'][self.art['id']]['excerpt'],'literal CSP excerpt')
        self.assertIn('show',self.calls); self.assertIn('history',self.calls)
        self.assertFalse(result['limits']['truncated'])

    def test_duplicate_alias_rejected(self):
        self.put('name-map.json',[{'alias':'I03','id':self.a['id'],'name':'one'},
                                  {'alias':'I03','id':self.b['id'],'name':'two'}]); self.seal()
        with self.assertRaisesRegex(ValueError,'alias'): self.load()

    def test_unsafe_map_and_source_paths_rejected(self):
        for field in ('name_map','source'):
            with self.subTest(field=field):
                saved=copy.deepcopy(self.m)
                if field=='name_map': self.m[field]='../outside.json'
                else: self.m['sources']['excerpt']['copied_path']='../outside.txt'
                self.seal()
                with self.assertRaises(ValueError): self.load()
                self.m=saved

    def test_wrongproject_unknown_manifest_and_counts_rejected(self):
        for change in [{'project_id':'proj_'+'9'*26},{'case_id':'arbitrary'},
                       {'canonical_count':32},{'schema_counts':{build.NODE:6}}]:
            with self.subTest(change=change):
                saved=copy.deepcopy(self.m); self.m.update(change); self.seal()
                with self.assertRaises(ValueError): self.load()
                self.m=saved

    def test_unadmitted_or_mutated_manifest_rejected(self):
        self.manifest.write_text(self.manifest.read_text()+' ')
        with self.assertRaisesRegex(ValueError,'admission'): self.load()

    def test_mutated_input_and_changes_during_validation_rejected(self):
        (self.root/'sources/excerpt.txt').write_bytes(b'mutated')
        with self.assertRaises(ValueError): self.load()
        (self.root/'sources/excerpt.txt').write_bytes(b'literal CSP excerpt')
        def mutating(args,**kw):
            result=self.runner(args,**kw); (self.root/'new.txt').write_text('change'); return result
        with self.assertRaisesRegex(ValueError,'changed'):
            case_io.load_case(self.root,self.manifest,self.rp,runner=mutating)

    def test_failed_validation_precedes_canonical_parse(self):
        def reject(args,**kw): return subprocess.CompletedProcess(args,1,b'{}',b'')
        with patch.object(build,'canonical',side_effect=AssertionError('must not parse')):
            with self.assertRaisesRegex(ValueError,'validation'):
                case_io.load_case(self.root,self.manifest,self.rp,runner=reject)

    def test_live_show_mismatch_rejected(self):
        def mismatch(args,**kw):
            out=self.runner(args,**kw)
            if args[1]=='show':
                dto=json.loads(out.stdout); dto['data']['object']['title']='different'; out.stdout=json.dumps(dto).encode()
            return out
        with self.assertRaises(ValueError): lookup.query(self.root,self.manifest,self.rp,'I03',runner=mismatch)

    def test_new_existing_kinds_and_full_title_display(self):
        for kind,label in [('Interpretation','解释'),('Method','方法'),('Conclusion','结论')]:
            n=node('a',kind=kind); n['title']='I03 一个不应该被任意截断的完整中文节点名称'
            g=project([n],'t')
            self.assertEqual(g['nodes'][0]['typeLabel'],label)
            self.assertEqual(g['nodes'][0]['label'],n['title'])

if __name__=='__main__': unittest.main()
