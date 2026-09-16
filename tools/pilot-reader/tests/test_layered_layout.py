"""Layout semantics and actual private r2 regressions; no private text fixtures."""
import copy
import json
from pathlib import Path
import re
import subprocess
import sys
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from graph_projection import project, visible, case_reading, layout
from routing import route_edges
from test_projection import node, edge, binding
from geometry_metrics import metrics, vertices

ARCHIVE=Path('/tmp/rp-layered-layout-before/r2.html')
NODE='/nix/store/2bslrww4ch7my47xxwabj1qy4acq4720-nodejs-slim-24.14.1/bin/node'
ROOT=Path(__file__).resolve().parents[1]


def data_at(path):
    from wire_fixture import decode_html
    return decode_html(path.read_text())


def scene(g,history=False):
    v=copy.deepcopy(visible(g,history=history));es=v['edges']+v['revisionEdges']
    route_edges(v['nodes'],es,g['edges']+g['revisionEdges'])
    return v,metrics(es)


class LayeredLayoutTests(unittest.TestCase):
    def test_independent_metrics_crossings_shared_length_and_endpoint_trim(self):
        def e(rid,a,b,path):return dict(id=rid,**{'from':a,'to':b},path=path,routeStatus='routed',labelStatus='hidden')
        a=e('a','a','b','M 0 0 L 100 0')
        b=e('b','c','d','M 50 -50 L 50 50')
        self.assertEqual(metrics([a,b])['crossings'],1)
        b=e('b','c','d','M 20 0 L 80 0')
        self.assertEqual(metrics([a,b])['sharedLength'],60)
        b=e('b','a','d','M 0 0 L 20 0 L 20 100')
        self.assertEqual(metrics([a,b])['sharedLength'],0)
        b=e('b','c','d','M 0 0.01 L 100 0.01')
        self.assertEqual(metrics([a,b])['sharedLength'],0)

    def test_revision_budget_rejects_before_layout(self):
        rs=[node('old'+str(i)) for i in range(50)]
        rs += [node('new'+str(i),['old'+str(j) for j in range(50)]) for i in range(7)]
        with self.assertRaisesRegex(ValueError,'300 revision'):
            project(rs,'t')

    def test_basis_reading_not_arrow_direction_or_kind_grid(self):
        records=[node('a-dependent'),node('z-basis'),node('m-result')]
        records += [edge('e','a-dependent','z-basis'),dict(edge('s','a-dependent','m-result'),type='supports')]
        before=copy.deepcopy(records);g=project(records,'t');by={n['id']:n for n in g['nodes']}
        self.assertLess(by['z-basis']['x'],by['a-dependent']['x'])
        self.assertLess(by['a-dependent']['x'],by['m-result']['x'])
        self.assertEqual(records,before)
        self.assertEqual(g,project(list(reversed(records)),'t'))

    def test_cycles_self_disjoint_multihead_history_bounded_no_collapsing(self):
        rs=[node('a'),node('b'),node('alone'),node('old'),node('new1',['old'],'old'),node('new2',['old'],'old')]
        rs += [edge('ab','a','b'),edge('ba','b','a'),edge('loop','a','a')]
        g=project(rs,'t');by={n['id']:n for n in g['nodes']}
        self.assertEqual(len(by),6);self.assertTrue(by['new1']['fork'] and by['new2']['fork'])
        self.assertLessEqual(abs(by['old']['x']-by['new1']['x']),400)
        self.assertLessEqual(abs(by['old']['y']-by['new1']['y']),600)
        self.assertEqual(len({(n['x'],n['y']) for n in g['nodes']}),6)
        for a in g['nodes']:
            for b in g['nodes']:
                if a['id']!=b['id']: self.assertTrue(abs(a['x']-b['x'])>=294 or abs(a['y']-b['y'])>=138)
        self.assertEqual(len(visible(g)['nodes']),5)
        self.assertEqual(len(visible(g,history=True)['revisionEdges']),2)

    def test_unknown_type_uses_named_raw_direction_fallback(self):
        g=project([node('z'),node('a'),dict(edge('e','z','a'),type='unmapped-type')],'t')
        by={n['id']:n for n in g['nodes']};self.assertLess(by['z']['x'],by['a']['x'])

    def test_parallel_edges_have_distinct_boundary_ports(self):
        ns=[dict(id='a',x=0,y=100),dict(id='b',x=780,y=100)]
        es=[dict(id=str(i),label='依赖',current=True,**{'from':'a','to':'b'}) for i in range(3)]
        route_edges(ns,es)
        self.assertEqual(len({vertices(e['path'])[0] for e in es}),3)
        self.assertEqual(len({vertices(e['path'])[-1] for e in es}),3)
        for e in es:self.assertEqual(vertices(e['path'])[0][0],270)

    def test_global_routing_reduces_shared_corridors_not_just_labels(self):
        nodes=[dict(id=i,x=x,y=y) for i,x,y in [('a',0,200),('b',780,200),('c',390,200),('d',0,600),('e',780,600),('f',390,600)]]
        edges=[dict(id='e'+str(i),label='依赖',category='science',current=True,**{'from':a,'to':b}) for i,(a,b) in enumerate([('a','b'),('a','e'),('d','b'),('d','e')])]
        route_edges(nodes,edges)
        self.assertLess(metrics(edges)['sharedLength'],50)
        self.assertEqual(metrics(edges)['routed'],4)

    def test_initial_frame_contains_full_placed_labels_and_paths(self):
        g=project([node('a')]+[edge('loop'+str(i),'a','a') for i in range(3)],'t')
        for e in g['edges']:
            ps=vertices(e['path'])
            if e['labelStatus']=='placed':ps += [(e['labelX']-e['labelHalfWidth'],e['labelY']-20),(e['labelX']+e['labelHalfWidth'],e['labelY']+8)]
            for x,y in ps:self.assertTrue(12<=x<=g['width']-12 and 12<=y<=g['height']-12,(x,y,g['width'],g['height']))

    @unittest.skipUnless(ARCHIVE.exists(),'private pre-task archive absent')
    def test_actual_r2_ranks_metrics_parity_and_preserved_semantics(self):
        data=data_at(ARCHIVE);old=data['graph'];records=list(data['records'].values())
        # Use the existing presentation labels and admitted navigation metadata.
        g=project(records,old['thread'],{n['id']:n['label'] for n in old['nodes']},data['names'],'csp-writing-r2')
        self.assertEqual(g['startIds'],old['startIds']);self.assertEqual(g['readingSources'],old['readingSources'])
        for key in ('nodes','edges','revisionEdges'):
            before={n['id']:n for n in old[key]}
            for item in g[key]:
                for field in ('raw','label','current','from','to','roles','ghost'):
                    self.assertEqual(item.get(field),before[item['id']].get(field))
        static={e['id']:e for e in g['edges'] if e['current']}
        default,_=scene(g)
        for e in default['edges']:
            for key in ('path','labelX','labelY','labelStatus','labelLeaderPath'):
                self.assertEqual(e[key],static[e['id']][key],'final coordinate-frame parity')
        by={n['id']:n for n in g['nodes']};q=by[g['startIds'][0]];fault=by[next(iter(g['readingSources'][q['id']]))]
        self.assertEqual(q['x'],min(n['x'] for n in g['nodes']));self.assertEqual(q['y'],min(n['y'] for n in g['nodes']))
        self.assertLess(q['x'],fault['x']);self.assertFalse(any(q['id'] in (e['from'],e['to']) for e in g['edges']))
        for e in g['edges']:
            if e['current']:
                a,b=(e['to'],e['from']) if e['type'] in ('derived-from','depends-on') else (e['from'],e['to'])
                self.assertLess(by[a]['x'],by[b]['x'],e['id'])
        for e in g['revisionEdges']:
            self.assertLessEqual(abs(by[e['from']]['x']-by[e['to']]['x']),400)
            self.assertLessEqual(abs(by[e['from']]['y']-by[e['to']]['y']),600)
        # Execute archived router, not new router on archived coordinates.
        js=ARCHIVE.read_text().split('<script>')[1].split('</script>')[0].split('const edgeRouting')[0]
        runner=js+"\nconsole.log(JSON.stringify([false,true].map(h=>{const g="+json.dumps(old)+";const ns=g.nodes.filter(n=>h||n.current||n.ghost),ids=new Set(ns.map(n=>n.id));const es=[...g.edges.filter(e=>(h||e.current)&&ids.has(e.from)&&ids.has(e.to)),...(h?g.revisionEdges:[])];const r=Routing.route(ns,es,[...g.edges,...g.revisionEdges]);return es.map(e=>({...e,...r[e.id]}));})));"
        baseline=json.loads(subprocess.check_output([NODE],input=runner,text=True,cwd=ROOT))
        report={}
        for history in (False,True):
            v,after=scene(g,history);before=metrics(baseline[int(history)])
            self.assertLess(after['sharedLength'],before['sharedLength']*.6)
            self.assertLess(after['crossings'],before['crossings'])
            self.assertEqual(after['routed'],before['routed']);self.assertEqual(after['edges'],13 if not history else 27)
            self.assertEqual(len(v['nodes']),15 if not history else 19)
            self.assertLess(after['totalLength'],before['totalLength']*1.5)
            for e in v['edges']+v['revisionEdges']:
                ps=vertices(e['path'])
                if e['labelStatus']=='placed':ps += [(e['labelX']-e['labelHalfWidth'],e['labelY']-20),(e['labelX']+e['labelHalfWidth'],e['labelY']+8)]
                for x,y in ps:self.assertTrue(12<=x<=g['width']-12 and 12<=y<=g['height']-12,(e['id'],x,y))
            report['history' if history else 'default']={'before':before,'after':after}
            js="const R=require('./routing.js');const c="+json.dumps(dict(nodes=v['nodes'],edges=v['edges']+v['revisionEdges'],all=g['edges']+g['revisionEdges']))+";console.log(JSON.stringify(R.route(c.nodes,c.edges,c.all)));"
            other=json.loads(subprocess.check_output([NODE],input=js,cwd=ROOT,text=True))
            for e in v['edges']+v['revisionEdges']:
                for k in ('path','routeStatus','labelStatus','labelX','labelY','labelLeaderPath'): self.assertEqual(e[k],other[e['id']][k])
        Path('/tmp/rp-layered-layout-metrics.json').write_text(json.dumps(report,indent=2))

if __name__=='__main__':unittest.main()
