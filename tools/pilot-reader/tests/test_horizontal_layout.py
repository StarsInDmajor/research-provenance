"""Crowded-rank packing, with optional private full-project regression evidence."""
import copy
from collections import Counter
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from graph_projection import layout, project, visible
from geometry_metrics import metrics, vertices
from routing import intersects, overlap
from test_projection import node, edge, binding
from wire_fixture import decode_html

PRIVATE = Path('/tmp/rp-horizontal-layout-private')


def assert_geometry(test, graph):
    nodes = graph['nodes']
    for i, a in enumerate(nodes):
        test.assertTrue(12 <= a['x'] <= graph['width']-282)
        test.assertTrue(12 <= a['y'] <= graph['height']-126)
        for b in nodes[i+1:]:
            test.assertTrue(abs(a['x']-b['x']) >= 294 or abs(a['y']-b['y']) >= 138,
                            (a['id'], b['id']))
    boxes = []
    rects = [(n['id'], n['x'], n['x']+270, n['y'], n['y']+114) for n in nodes]
    for e in graph['edges']+graph['revisionEdges']:
        path = vertices(e['path'])
        for a,b in zip(path, path[1:]):
            for rect in rects:
                if rect[0] not in (e['from'], e['to']):
                    test.assertFalse(intersects(a,b,rect), (e['id'], rect[0]))
        points = path + vertices(e['labelLeaderPath'])
        if e['labelStatus'] == 'placed':
            box = ('', e['labelX']-e['labelHalfWidth'], e['labelX']+e['labelHalfWidth'],
                   e['labelY']-20, e['labelY']+8)
            test.assertFalse(any(overlap(box, other) for other in boxes+rects))
            boxes.append(box)
            points += [(e['labelX']-e['labelHalfWidth'], e['labelY']-20),
                       (e['labelX']+e['labelHalfWidth'], e['labelY']+8)]
        for x, y in points:
            test.assertTrue(12 <= x <= graph['width']-12 and 12 <= y <= graph['height']-12,
                            (e['id'], x, y, graph['width'], graph['height']))


def footprint(graph):
    return dict(width=graph['width'], height=graph['height'],
                aspect=round(graph['width']/graph['height'], 4),
                longestColumn=max(Counter(n['x'] for n in graph['nodes']).values()),
                longestRow=max(Counter(n['y'] for n in graph['nodes']).values()),
                **{k:v for k,v in metrics(graph['edges']+graph['revisionEdges']).items() if k != 'pairs'})


class HorizontalLayoutTests(unittest.TestCase):
    def test_eighty_same_rank_nodes_use_landscape_not_a_vertical_stack(self):
        records = [node(f'n{i:02}') for i in range(80)]
        before = copy.deepcopy(records)
        g = project(records, 't')
        self.assertLess(g['height'], 3000)
        self.assertTrue(1.5 <= g['width']/g['height'] <= 2.5)
        self.assertLessEqual(max(Counter(n['x'] for n in g['nodes']).values()), 14)
        self.assertGreater(len({n['x'] for n in g['nodes']}), 1)
        self.assertEqual(len(visible(g)['nodes']), 80)
        self.assertEqual(g['edges'], [])
        assert_geometry(self, g)
        self.assertEqual(g, project(list(reversed(records)), 't'))
        self.assertEqual(records, before)

    def test_overloaded_related_rank_retains_normalized_reading_order(self):
        records = [node('root'), binding('b', 'root', 'primary'), node('result')]
        for i in range(80):
            rid = f'n{i:02}'
            records += [node(rid), edge(f'e{i:02}', rid, 'root')]
        records += [dict(edge('last', 'n79', 'result'), type='supports')]
        g = project(records, 't'); by = {n['id']:n for n in g['nodes']}
        self.assertLess(g['height'], 3200)
        self.assertTrue(1.5 <= g['width']/g['height'] <= 2.5)
        self.assertEqual(by['root']['x'], min(n['x'] for n in g['nodes']))
        self.assertEqual(by['root']['y'], min(n['y'] for n in g['nodes']))
        for e in g['edges']:
            a,b = (e['to'],e['from']) if e['type'] == 'depends-on' else (e['from'],e['to'])
            self.assertLess(by[a]['x'], by[b]['x'])
        self.assertEqual(len(g['nodes']), 82); self.assertEqual(len(g['edges']), 81)
        assert_geometry(self, g)

    def test_crowded_cycles_history_multihead_disconnected_remain_exact(self):
        records = [node(f'n{i:02}') for i in range(60)]
        records += [dict(edge(f'e{i:02}', f'n{i:02}', f'n{(i+1)%30:02}'), type='supports') for i in range(30)]
        records += [node('old'), node('new1', ['old'], 'old'), node('new2', ['old'], 'old')]
        records += [edge('ghost-edge', 'n00', 'old')]
        before = copy.deepcopy(records)
        g = project(records, 't'); by = {n['id']:n for n in g['nodes']}
        self.assertTrue(1.5 <= g['width']/g['height'] <= 2.5)
        self.assertEqual(len(g['nodes']), 63)
        self.assertEqual(len(g['edges']), 31)
        self.assertEqual(len(g['revisionEdges']), 2)
        self.assertTrue(by['old']['ghost'])
        self.assertTrue(by['new1']['fork'] and by['new2']['fork'])
        self.assertEqual(len({by[f'n{i:02}']['component'] for i in range(30)}), 1)
        self.assertGreater(len({by[f'n{i:02}']['x'] for i in range(30)}), 1,
                           'SCC fallback grid must also pack horizontally, without collapsing members')
        assert_geometry(self, g)
        self.assertEqual(g, project(list(reversed(records)), 't'))
        self.assertEqual(records, before)

    def test_crowded_hidden_history_relocation_uses_free_packed_slots(self):
        records = [node(f'n{i:02}') for i in range(60)]
        records += [node('old'), node('new1', ['old'], 'old'), node('new2', ['old'], 'old')]
        g = project(records, 't'); by = {n['id']:n for n in g['nodes']}
        self.assertFalse(by['old']['current'] or by['old']['ghost'])
        self.assertEqual(len(visible(g)['nodes']), 62)
        self.assertEqual(len(visible(g, history=True)['nodes']), 63)
        assert_geometry(self, g)
        self.assertTrue(1.5 <= g['width']/g['height'] <= 2.5)
        self.assertLessEqual(min(abs(by['old']['y']-by[k]['y']) for k in ('new1','new2')), 600)

    def test_small_rank_geometry_is_unchanged(self):
        nodes = [dict(id=str(i), current=True, ghost=False, raw={}) for i in range(8)]
        self.assertEqual(layout(nodes, []), (370, 1584))
        self.assertEqual([(n['x'], n['y']) for n in nodes], [(60, 100+190*i) for i in range(8)])

    @unittest.skipUnless((PRIVATE/'before.html').exists() and (PRIVATE/'snapshot.json').exists(),
                         'explicit private actual snapshot/archive absent')
    def test_actual_86_nodes_landscape_canonical_identity_bounds_and_determinism(self):
        old_data = decode_html((PRIVATE/'before.html').read_text())
        snap = json.loads((PRIVATE/'snapshot.json').read_text())
        self.assertEqual(snap['findings'], [])
        records = snap['data']['objects']
        self.assertEqual(records, old_data['records'])
        old = old_data['graph']
        g = project(list(records.values()), old['thread'], {n['id']:n['label'] for n in old['nodes']})
        self.assertEqual((len(g['nodes']), len(g['edges']), len(records)), (86,77,299))
        self.assertLess(g['height'], 3500)
        self.assertTrue(1.5 <= g['width']/g['height'] <= 2.5)
        self.assertEqual(len(visible(g)['nodes']), 86)
        self.assertEqual(metrics(g['edges'])['routed'], 77,
                         'all scientific paths must survive packing, not just a count of nodes')
        after_metrics, before_metrics = metrics(g['edges']), metrics(old['edges'])
        self.assertLess(after_metrics['sharedLength'], before_metrics['sharedLength'])
        self.assertLessEqual(after_metrics['crossings'], before_metrics['crossings']*1.2)
        self.assertGreaterEqual(after_metrics['labels'], before_metrics['labels'])
        self.assertEqual(g['startIds'], old['startIds'])
        starts = [n for n in g['nodes'] if n['id'] in g['startIds']]
        self.assertEqual(min(n['x'] for n in starts), min(n['x'] for n in g['nodes']))
        self.assertEqual(min(n['y'] for n in starts), min(n['y'] for n in g['nodes']))
        for key in ('nodes', 'edges', 'revisionEdges'):
            by = {n['id']:n for n in old[key]}
            for item in g[key]:
                for field in ('raw','label','current','from','to','roles','ghost','fork','isolated','bindingIds'):
                    self.assertEqual(item.get(field), by[item['id']].get(field))
        assert_geometry(self, g)
        by = {n['id']:n for n in g['nodes']}
        for e in g['edges']:
            a,b = (e['to'],e['from']) if e['type'] in ('depends-on','derived-from') else (e['from'],e['to'])
            if by[a]['component'] != by[b]['component']:
                self.assertLess(by[a]['x'], by[b]['x'])
        self.assertEqual(g, project(list(reversed(list(records.values()))), old['thread'],
                                    {n['id']:n['label'] for n in old['nodes']}))
        (PRIVATE/'metrics.json').write_text(json.dumps(dict(before=footprint(old),after=footprint(g)), indent=2))


if __name__ == '__main__':
    unittest.main()
