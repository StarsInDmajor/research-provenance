"""Synthetic multi-revision reader regressions, not data-authoring TDD."""
import copy
import json
import unittest
from unittest.mock import patch

import test_case

import build
import case_io
import lookup
from graph_projection import heads, NODE, RELATION


class WritingR2Tests(unittest.TestCase):
    # Reuse only fixture helpers, not single-revision tests.
    put = test_case.CaseTests.put
    seal = test_case.CaseTests.seal
    load = test_case.CaseTests.load

    def setUp(self):
        test_case.CaseTests.setUp(self)
        self.new = copy.deepcopy(self.a)
        self.new.update(id='obs_'+'0'*24+'13', title=self.a['title'])
        self.new['revision'] = {'parents':[{'id':self.a['id'],'change_type':'clarification'}], 'summary':'clarify'}
        self.retired = copy.deepcopy(self.e)
        self.retired.update(id='rel_'+'0'*24+'11', relation_state='invalidated')
        self.retired['revision'] = {'parents':[{'id':self.e['id'],'change_type':'clarification'}]}
        self.records += [self.new,self.retired]
        self.put('.research/new.yaml',self.new); self.put('.research/retired.yaml',self.retired)
        self.put('name-map.json',[{'alias':'I03','id':self.new['id'],'name':'当前观察'},
                                  {'alias':'I04','id':self.b['id'],'name':'禁用机制'}])
        self.m['canonical_count']=8
        self.m['schema_counts'][NODE]=3; self.m['schema_counts'][RELATION]=2
        self.m['case_id']='csp-writing-r2'
        self.seal()
        self.admission.write_text(json.dumps({'case_id':'csp-writing-r2','manifest_sha256':build.sha(self.manifest.read_bytes())}))
        p=patch.object(case_io,'ADMISSION_R2',self.admission,create=True)
        p.start(); self.addCleanup(p.stop)

    def runner(self,args,**kw):
        out=test_case.CaseTests.runner(self,args,**kw)
        dto=json.loads(out.stdout)
        if args[1]=='validate': dto['data']['canonical_object_count']=self.m['canonical_count']
        if args[1]=='history': dto['data']['heads']=sorted(heads(dto['data']['revisions']))
        out.stdout=json.dumps(dto).encode()
        return out

    def test_r2_head_alias_old_exact_without_alias_and_ambiguous_title(self):
        try:
            data=self.load()
        except ValueError as exc:
            self.fail('Reviewed r2 manifest and head-only aliases must load: '+str(exc))
        self.assertEqual(lookup.select(data,'I03')['selected_id'],self.new['id'])
        old=lookup.select(data,self.a['id'])
        self.assertEqual(old['selected_id'],self.a['id'])
        self.assertIsNone(old['candidates'][0]['alias'])
        self.assertFalse(old['candidates'][0]['is_head'])
        self.assertEqual(lookup.select(data,self.a['title'])['status'],'ambiguous')
        labels={n['id']:n['label'] for n in data['graph']['nodes']}
        self.assertIn('I03',labels[self.a['id']]); self.assertIn('旧修订',labels[self.a['id']])
        self.assertIn('当前',labels[self.new['id']])

    def test_exact_history_lookup_labels_retired_relations_without_mutating_them(self):
        try:
            result=lookup.query(self.root,self.manifest,self.rp,self.a['id'],runner=self.runner)
        except (ValueError,StopIteration) as exc:
            self.fail('Historical exact lookup must work without alias: '+str(exc))
        self.assertIsNone(result['mapping'])
        self.assertEqual(result['incoming'],[self.e,self.retired])
        self.assertEqual(result['relation_status'][self.e['id']]['layer'],'historical')
        self.assertEqual(result['relation_status'][self.retired['id']]['layer'],'historical')
        self.assertTrue(result['relation_status'][self.retired['id']]['is_head'])
        self.assertEqual(result['history']['heads'],[self.new['id']])

    def test_active_current_relation_status_is_explicit(self):
        self.retired['relation_state']='active'
        self.put('.research/retired.yaml',self.retired)
        self.seal()
        self.admission.write_text(json.dumps({'case_id':'csp-writing-r2','manifest_sha256':build.sha(self.manifest.read_bytes())}))
        result=lookup.query(self.root,self.manifest,self.rp,'I04',runner=self.runner)
        self.assertEqual(result['relation_status'][self.e['id']]['layer'],'historical')
        self.assertEqual(result['relation_status'][self.retired['id']]['layer'],'active-current')

    def test_r2_cannot_use_original_admission_and_unknown_case_rejected(self):
        with patch.object(case_io,'ADMISSION_R2',self.base/'missing.json'):
            with self.assertRaisesRegex(ValueError,'Manifest size/type'): self.load()
        self.m['case_id']='unreviewed'; self.seal()
        with self.assertRaises(ValueError): self.load()

    def test_explicit_historical_alias_and_multiple_names_per_exact_id(self):
        names=json.loads((self.root/'name-map.json').read_text())
        names += [{'alias':'I03@r1','id':self.a['id'],'name':'旧观察'},
                  {'alias':'I13','id':self.new['id'],'name':'别名观察'}]
        self.put('name-map.json',names); self.seal()
        self.admission.write_text(json.dumps({'case_id':'csp-writing-r2','manifest_sha256':build.sha(self.manifest.read_bytes())}))
        try: data=self.load()
        except ValueError as exc: self.fail('Reviewed historical/alternate aliases must load: '+str(exc))
        self.assertEqual(lookup.select(data,'I03@r1')['selected_id'],self.a['id'])
        self.assertEqual(lookup.select(data,'I13')['selected_id'],self.new['id'])

    def test_r2_builder_counts_and_header(self):
        try:
            report=build.rebuild(self.root,None,self.rp,self.base/'private/csp-reader.html',
                                 self.m['as_of'],self.m['as_of'],runner=self.runner,case_manifest=self.manifest)
        except ValueError as exc: self.fail('Explicit r2 build must be admitted: '+str(exc))
        self.assertEqual(report['graph_counts']['historical_nodes'],3)
        self.assertEqual(report['graph_counts']['current_science_edges'],0)
        self.assertIn('撰写修订候选', (self.base/'private/csp-reader.html').read_text())
