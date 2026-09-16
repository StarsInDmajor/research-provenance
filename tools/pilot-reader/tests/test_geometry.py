"""Numerical geometry regressions from independent inspection (not visual proof)."""
import copy
import re
import unittest
import sys
from pathlib import Path
sys.path.append(str(Path(__file__).resolve().parents[1]))
from graph_projection import project, route_edges
from build import render_svg
from test_projection import node, edge


def points(path):
    tokens = re.findall(r'[MLQC]|-?\d+(?:\.\d+)?', path)
    result = []; current = None
    while tokens:
        command = tokens.pop(0)
        count = {'M': 2, 'L': 2, 'Q': 4, 'C': 6}[command]
        values = [float(tokens.pop(0)) for _ in range(count)]
        controls = list(zip(values[::2], values[1::2]))
        if command == 'M':
            current = controls[0]; result.append(current); continue
        controls.insert(0, current)
        for step in range(1, 1001):
            t = step/1000
            level = controls
            while len(level) > 1:
                level = [((1-t)*a[0]+t*b[0], (1-t)*a[1]+t*b[1]) for a,b in zip(level,level[1:])]
            result.append(level[0])
        current = controls[-1]
    return result


def intersects(point, card, margin=0):
    x,y=point
    return card['x']-margin < x < card['x']+270+margin and card['y']-margin < y < card['y']+114+margin


class GeometryTests(unittest.TestCase):
    def test_actual_pilot_long_edge_and_reverse_clear_mainline(self):
        q='qst_00000000000000000000000010'
        m='hyp_00000000000000000000000011'
        a='hyp_00000000000000000000000012'
        cards=[dict(id=q,x=35,y=100),dict(id=m,x=355,y=100),dict(id=a,x=675,y=100)]
        # This exact old path's t=.15 sample was inside the unrelated mainline.
        self.assertTrue(intersects((359.75,205.45),cards[1]))
        for source,target in [(q,a),(a,q),('a','z'),('z','a')]:
            fixture=copy.deepcopy(cards)
            fixture[0]['id']=source;fixture[2]['id']=target
            e=dict(id='rel_00000000000000000000000061',label='促成',**{'from':source,'to':target})
            route_edges(fixture,[e])
            samples=points(e['path'])
            self.assertFalse(any(intersects(p,fixture[1],12) for p in samples),e['path'])
            self.assertFalse(intersects((e['labelX'],e['labelY']),fixture[1],12))
            self.assertEqual((e['from'],e['to']),(source,target))

    def test_parallel_self_edges_have_distinct_selectable_label_geometry(self):
        records=[node('a')]+[edge('loop'+str(i),'a','a') for i in range(3)]
        g=project(records,'t')
        samples=[points(e['path']) for e in g['edges']]
        for a,b in zip(samples,samples[1:]):
            self.assertNotEqual(a[len(a)//2],b[len(b)//2], 'self edges coincide at actual midpoint')
        labels=[(e['labelX'],e['labelY']) for e in g['edges']]
        self.assertEqual(len(set(labels)),3)
        for e in g['edges']:
            self.assertEqual((e['from'],e['to'],e['label']),('a','a','依赖'))
        for p in [p for sample in samples for p in sample]+labels:
            self.assertTrue(12 <= p[0] <= g['width']-12 and 12 <= p[1] <= g['height']-12, p)
        markup = render_svg(g)
        for e in g['edges']:
            self.assertIn('data-key="'+e['id']+'" data-from="a" data-to="a" tabindex="0" role="button"', markup)
            self.assertIn(f'<text x="{e["labelX"]}" y="{e["labelY"]}" class="edge-label" visibility="visible">依赖</text>', markup)
            self.assertGreaterEqual(e['labelY']-20,12)
            self.assertGreaterEqual(e['labelX']-75,12)
            self.assertLessEqual(e['labelX']+75,g['width']-12)
        self.assertEqual(g,project(list(reversed(records)),'t'))

    def test_long_edges_clear_all_other_rows_and_labels_stay_in_view(self):
        records=[node(str(i)) for i in range(8)]
        records += [edge('e'+str(i),'0','3') for i in range(4)]
        records += [edge('connect'+str(i),str(i),str(i+1)) for i in range(7)]
        g=project(records,'t')
        for e in g['edges']:
            if not e['id'].startswith('e'): continue
            unrelated=[n for n in g['nodes'] if n['id'] not in (e['from'],e['to'])]
            self.assertFalse(any(intersects(p,n,12) for p in points(e['path']) for n in unrelated))
            self.assertGreaterEqual(e['labelY']-20,12)
            self.assertLessEqual(e['labelX']+75,g['width']-12)


if __name__=='__main__': unittest.main()
