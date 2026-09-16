"""Visible routing contract shared with the pure Node implementation."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from graph_projection import route_edges
import build
from test_geometry import points, intersects

NODE='/nix/store/2bslrww4ch7my47xxwabj1qy4acq4720-nodejs-slim-24.14.1/bin/node'
ROOT=Path(__file__).resolve().parents[1]

class VisibleRoutingTests(unittest.TestCase):
    def test_hide_obstruction_shortens_without_coordinate_changes(self):
        nodes=[dict(id=i,x=x,y=400) for i,x in [('A',0),('B',320),('C',640)]]
        e=dict(id='ac',label='依赖',**{'from':'A','to':'C'})
        before=copy.deepcopy(nodes)
        route_edges(nodes,[e]); blocked=e['path']
        self.assertFalse(any(intersects(p,nodes[1],12) for p in points(blocked)))
        route_edges([nodes[0],nodes[2]],[e])
        self.assertGreaterEqual(min(y for x,y in points(e['path'])),440,'hidden nodes must not force top arch')
        self.assertNotEqual(e['path'],blocked)
        self.assertEqual(nodes,before)

    def test_routing_sources_inline_before_consumer_and_static_failure_disclosed(self):
        from entry_fixture import generated, Tree
        from test_projection import node, edge
        from graph_projection import project
        html=generated()
        scripts=Tree();scripts.feed(html)
        renderer=scripts.scripts[1]['text']
        self.assertEqual(renderer,'\n'.join((ROOT/name).read_text() for name in ('routing.js','graph.js','svg.js')))
        self.assertLess(renderer.index('const Routing'),renderer.index('class Model'))
        graph=project([node('a'),node('b'),edge('e','a','b')],'t')
        graph['edges'][0]['routeStatus']='unroutable'
        markup=build.render_svg(graph)
        self.assertIn('data-route-status="unroutable"',markup)
        self.assertIn('visibility="hidden"',markup)
        self.assertIn('未找到安全路线',build.reading_guide(graph))

    def test_python_and_js_share_cases_and_geometry(self):
        cases=[]
        for xy in [(640,400),(0,900),(640,900)]:
            for reverse in [False,True]:
                for obstacle in [False,True]:
                    nodes=[dict(id='z',x=0,y=400),dict(id='a',x=xy[0],y=xy[1])]
                    if obstacle:nodes.append(dict(id='B',x=320,y=400))
                    edges=[dict(id='e',label='依赖',**{'from':'a' if reverse else 'z','to':'z' if reverse else 'a'})]
                    cases.append(dict(nodes=nodes,edges=edges))
        cases.append(dict(nodes=[dict(id='a',x=100,y=200)],edges=[dict(id='l'+str(i),label='loop',**{'from':'a','to':'a'}) for i in range(3)]))
        cases.append(dict(nodes=[dict(id=i,x=x,y=y) for i,x,y in [('a',0,400),('b',640,400),('c',320,400),('d',320,230),('f',320,590)]],edges=[dict(id=str(i),label='依赖',category='science',current=i%2==0,raw={'relation_state':'invalidated'},**{'from':'a' if i%2==0 else 'b','to':'b' if i%2==0 else 'a'}) for i in range(4)]))
        cases.append(dict(nodes=[dict(id='a',x=0,y=200),dict(id='b',x=640,y=200),dict(id='cover',x=260,y=140)],edges=[dict(id='e',label='blocked',**{'from':'a','to':'b'})]))
        # Cross-endpoint same midpoint, long Unicode/unknown text, dense fallback.
        cross_nodes = [dict(id=i, x=x, y=y) for i, x, y in [('a',0,0),('b',800,600),('c',800,0),('d',0,600),('e',0,300),('f',800,300)]]
        cross_edges = [dict(id=str(i), label=label, category='science', current=True, **{'from':a,'to':b}) for i,a,b,label in [(1,'a','b','支持'),(2,'c','d','推导自/依据'),(3,'e','f','依赖')]]
        cases.append(dict(nodes=cross_nodes,edges=cross_edges))
        cases.append(dict(nodes=cross_nodes,edges=[dict(e,id='dense'+str(i),current=False,raw={'relation_state':'invalidated'}) for i in range(60) for e in [cross_edges[i%3]]]))
        cases.append(dict(nodes=cross_nodes,edges=[dict(cross_edges[0],label='W😀中'*500)]))
        js="const R=require('./routing.js');let s='';process.stdin.on('data',d=>s+=d);process.stdin.on('end',()=>console.log(JSON.stringify(JSON.parse(s).map(c=>R.route(c.nodes,c.edges,c.edges)))));"
        result=subprocess.run([NODE,'-e',js],cwd=ROOT,input=json.dumps(cases),text=True,capture_output=True,check=True)
        actual=json.loads(result.stdout)
        for case,got in zip(cases,actual):
            route_edges(case['nodes'],case['edges'])
            for e in case['edges']:
                other=got[e['id']]
                for key in ('routeStatus','labelStatus','labelLeaderPath','labelHalfWidth'):
                    self.assertEqual(e[key],other[key], (e['id'], key, e['path'], other['path']))
                self.assertEqual(points(e['path']),points(other['path']))
                self.assertAlmostEqual(e['labelX'],other['labelX'],places=2)
                self.assertAlmostEqual(e['labelY'],other['labelY'],places=2)

    def test_cross_edge_labels_and_dense_fallback(self):
        nodes = [dict(id=i,x=x,y=y) for i,x,y in [('a',0,0),('b',800,600),('c',800,0),('d',0,600),('e',0,300),('f',800,300)]]
        edges = [dict(id=str(i),label=label,category='science',current=True,**{'from':a,'to':b}) for i,a,b,label in [(1,'a','b','支持'),(2,'c','d','推导自/依据'),(3,'e','f','依赖')]]
        route_edges(nodes,edges)
        def box(e):return (e['labelX']-e['labelHalfWidth'],e['labelX']+e['labelHalfWidth'],e['labelY']-20,e['labelY']+8)
        def overlap(a,b):return a[0]<b[1] and a[1]>b[0] and a[2]<b[3] and a[3]>b[2]
        for i,e in enumerate(edges):
            for other in edges[i+1:]:self.assertFalse(overlap(box(e),box(other)), 'independent midpoint label collision')
        self.assertTrue(all(e['labelStatus']=='placed' for e in edges))
        reverse=copy.deepcopy(list(reversed(edges)));route_edges(list(reversed(nodes)),reverse)
        self.assertEqual(edges,list(reversed(reverse)))
        long=dict(edges[0],label='W😀'*500);route_edges(nodes,[long])
        self.assertEqual(long['routeStatus'],'routed');self.assertEqual(long['labelStatus'],'hidden')
        self.assertEqual(long['labelLeaderPath'],'');self.assertIn(' L ',long['path'])

if __name__=='__main__':unittest.main()
