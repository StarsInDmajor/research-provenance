import copy
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from graph_projection import project, visible


def node(rid, parents=(), logical=None, kind='Question'):
    return dict(schema='rp/node-revision/v1', id=rid, logical_id=logical or rid,
                kind=kind, title='原始 title '+rid, statement='Original statement',
                revision={'parents': [{'id': p} for p in parents]})


def edge(rid, a, b, parents=(), state='active', logical=None):
    return dict(schema='rp/scientific-relation-revision/v1', id=rid,
                logical_id=logical or rid, type='depends-on', from_revision=a,
                to_revision=b, relation_state=state, rationale='Exact rationale',
                scope={'statement': 'Exact scope'},
                revision={'parents': [{'id': p} for p in parents]})


def binding(rid, target, role, parents=(), thread='t'):
    return dict(schema='rp/thread-binding/v1', id=rid, logical_id=rid,
                thread_id=thread, target={'id': target}, role=role,
                revision={'parents': [{'id': p} for p in parents]})


class ProjectionTests(unittest.TestCase):
    def test_heads_exact_endpoints_and_old_roles(self):
        records = [node('old'), node('new', ['old'], 'old'), node('other'),
                   edge('e', 'other', 'old'), binding('b0', 'old', 'follow-up'),
                   binding('b1', 'old', 'historical', ['b0']),
                   binding('b2', 'new', 'follow-up')]
        original = copy.deepcopy(records)
        g = project(records, 't')
        by = {n['id']: n for n in g['nodes']}
        self.assertEqual(by['old']['roles'], ['historical'])
        self.assertFalse(by['old']['current'])
        self.assertTrue(by['old']['ghost'])
        self.assertEqual(by['new']['roles'], ['follow-up'])
        self.assertEqual(g['edges'][0]['to'], 'old')
        self.assertEqual(len(visible(g)['nodes']), 3)
        self.assertEqual(records, original)
        self.assertEqual(g['revisionEdges'][0]['from'], 'old')
        self.assertEqual(g['revisionEdges'][0]['to'], 'new')

    def test_multiheads_conflicts_thread_isolation_and_time_irrelevance(self):
        records = [node('a'), node('b', ['a'], 'a'), node('c', ['a'], 'a'),
                   binding('x', 'b', 'primary'), binding('y', 'b', 'alternative'),
                   binding('z', 'c', 'follow-up', thread='not-t')]
        records[0]['created_at'] = '2099-01-01'
        g = project(records, 't')
        b = next(n for n in g['nodes'] if n['id'] == 'b')
        self.assertTrue(b['fork'])
        self.assertTrue(b['roleConflict'])
        self.assertEqual(b['roles'], ['alternative', 'primary'])
        self.assertEqual({n['id'] for n in visible(g)['nodes']}, {'b', 'c'})
        self.assertEqual({n['id'] for n in visible(g, mode='alternative')['nodes']}, {'b'})

    def test_active_relation_heads_history_and_hidden_counts(self):
        g = project([node('a'), node('b'), edge('e0', 'a', 'b'),
                     edge('e1', 'a', 'b', ['e0'], 'invalidated', 'e0'),
                     edge('e2', 'b', 'a', ['e0'], logical='e0'),
                     binding('b1', 'a', 'primary')], 't')
        self.assertEqual([e['id'] for e in visible(g)['edges']], ['e2'])
        self.assertEqual(len(visible(g, history=True)['edges']), 3)
        v = visible(g, mode='mainline')
        self.assertEqual([e['id'] for e in v['edges']], ['e2'])
        self.assertEqual(v['hiddenEdges'], 2)
        self.assertEqual(v['hiddenNodes'], 0)
        self.assertEqual(v['focusedNodes'], 1)
        self.assertEqual(v['contextNodes'], 1)
        self.assertEqual(visible(g, mode='alternative')['nodes'], [])

    def test_parallel_reverse_cycle_isolate_determinism(self):
        records = [node(x) for x in 'abcd'] + [edge('e1', 'a', 'b'),
                    edge('e2', 'a', 'b'), edge('e3', 'b', 'a'),
                    edge('e4', 'b', 'c'), edge('e5', 'c', 'a')]
        g = project(records, 't')
        self.assertEqual(g, project(list(reversed(records)), 't'))
        self.assertEqual(len({e['path'] for e in g['edges'][:3]}), 3)
        self.assertTrue(next(n for n in g['nodes'] if n['id'] == 'd')['isolated'])
        self.assertEqual(len({(n['x'], n['y']) for n in g['nodes']}), 4)
        self.assertEqual([(e['from'], e['to']) for e in g['edges'][:3]],
                         [('a', 'b'), ('a', 'b'), ('b', 'a')])

    def test_overscale_explicit_reject(self):
        # Beta-1 ceilings: layout bound 2,000 nodes; relation ceiling 50,000.
        with self.assertRaisesRegex(ValueError, 'Layout bound exceeded'):
            project([node(str(i)) for i in range(2_001)], 't')
        with self.assertRaisesRegex(ValueError, '50000'):
            project([node('a'), node('b')] + [edge(str(i), 'a', 'b') for i in range(50_001)], 't')

    def test_unknown_endpoint_rejected_not_retargeted(self):
        with self.assertRaisesRegex(ValueError, 'endpoint'):
            project([node('a'), edge('e', 'a', 'missing')], 't')


if __name__ == '__main__':
    unittest.main()
