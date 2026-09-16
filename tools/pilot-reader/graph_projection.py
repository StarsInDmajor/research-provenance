"""Bounded, deterministic semantic projection; no IO and no inferred science edges.

Input dictionaries are already validated by rp. This is not schema validation.
Exact revision IDs survive every layer; layout never determines semantic role.
"""
from collections import Counter, defaultdict
import math
from routing import route_edges

NODE = 'rp/node-revision/v1'
RELATION = 'rp/scientific-relation-revision/v1'
BINDING = 'rp/thread-binding/v1'
KINDS = {'Question': '问题', 'Hypothesis': '思路提案', 'Observation': '文档观察',
         'Decision': '决定提案', 'Blocker': '阻塞', 'NextAction': '行动提案',
         'Interpretation': '解释', 'Method': '方法', 'Conclusion': '结论',
         'PaperClaim': '文献声明', 'Measurement': '测量'}
ROLES = {'primary': '主线', 'alternative': '备选', 'diagnostic': '诊断',
         'follow-up': '下一步', 'historical': '历史安排'}
# Static presentation allowlist. Unknown canonical types remain literal text with
# neutral styling; neither type colors nor selection imply confidence/verification.
EDGE_STYLES = {
    'supports': ('支持', '#287344', None),
    'weakens': ('削弱', '#b23838', '3 3'),
    'contradicts': ('反驳', '#b23838', '10 3 2 3'),
    'derived-from': ('推导自/依据', '#246ca6', '8 4'),
    'depends-on': ('依赖', '#956400', '12 3'),
    'blocked-by': ('被阻塞于', '#b23838', '7 4'),
    'motivates': ('促成', '#147d79', '2 4'),
    'revision': ('修订 · 非科研关系', '#80529b', '9 4 2 4'),
    'neutral': ('其他已记录关系', '#66717b', None),
}
TYPES = {k: v[0] for k, v in EDGE_STYLES.items() if k not in ('revision', 'neutral')} | {'requires': '需要'}


def edge_style(edge):
    key = 'revision' if edge['category'] == 'revision' else edge['type']
    # A scientific type named revision must not impersonate the revision layer.
    if key not in EDGE_STYLES or key == 'revision' and edge['category'] != 'revision':
        key = 'neutral'
    return key, EDGE_STYLES[key]


def wrap_text(value, size=15, width=238, lines=2):
    """Deterministic conservative glyph approximation, NOT font measurement.

    Break at codepoints (including long Latin words/IDs); no textLength squeezing.
    SVG clip paths are the final hard bound for wider/unusual browser fonts.
    """
    value = str(value).replace('\n', ' ').replace('\r', ' ')
    cursor = 0
    result = []
    def advance(c):
        return size * (.8 if ord(c) < 128 else 1.1)
    for index in range(lines):
        row, used = [], 0
        while cursor < len(value) and used + advance(value[cursor]) <= width:
            c = value[cursor]; cursor += 1; row.append(c); used += advance(c)
        if cursor < len(value) and index == lines-1:
            while row and used + advance('…') > width:
                used -= advance(row.pop())
            row.append('…')
        result.append(''.join(row))
        if cursor == len(value):
            break
    return result


def presentation_labels(records, names, case_id):
    """One display map for SVG/search/lookup; never edit name-map or raw records.

    The exact admitted I01 source records past test passes AND no browser proof.
    '历史' keeps this from reading as a new execution result. This is not a revision.
    """
    nodes = [r for r in records.values() if r['schema'] == NODE]
    current = heads(nodes)
    multi = case_id == 'csp-writing-r2'
    labels = {}
    for r in nodes:
        named = next((n for n in names if n['id'] == r['id']), None)
        if named is None and multi:
            named = next((n for n in names if records[n['id']]['logical_id'] == r['logical_id']), None)
        name = named['name'] if named and named['id'] == r['id'] else r['title']
        if (case_id in ('csp-startup-v1', 'csp-writing-r2')
                and r['id'] == 'obs_00000000000000000000009104'
                and r['logical_id'] == 'csp-i01' and r['title'] == 'I01 有实现不等于可用'
                and named and named['alias'] == 'I01'):
            name = '历史交互测试通过，浏览器执行未验证'
        label = named['alias']+' '+name if named else name
        if multi and sum(n['logical_id'] == r['logical_id'] for n in nodes) > 1:
            label += ' · '+('当前' if r['id'] in current else '旧修订')+' · '+r['id'][-5:]
        labels[r['id']] = label
    return labels


def case_reading(graph, names, case_id):
    """Known case aliases are navigation metadata, not kind codes or science edges."""
    if case_id not in ('csp-startup-v1', 'csp-writing-r2'):
        return
    by = {n['id']: n for n in graph['nodes']}
    aliases = {n['alias']: n['id'] for n in names}
    question = aliases.get('Q00')
    if question in graph['startIds']:
        graph['startIds'] = [question]
    meanings = {'Q': 'question / 问题', 'G': 'goal / 目标', 'P': 'pilot / 试点',
                'I': 'investigation / 调查', 'N': 'next action / 下一行动', 'B': 'blocker / 阻塞'}
    graph['aliasLegend'] = [letter+' = '+meaning for letter, meaning in meanings.items()
                            if any(a.startswith(letter) for a in aliases)]
    examples = [a+' = '+by[aliases[a]]['kind'] for a in ('I03','I04','I06','I09') if a in aliases]
    if examples:
        graph['aliasLegend'].append('实际 kind 示例：'+'；'.join(examples))
    fault = aliases.get('I02')
    if question in by and fault in by[question]['raw'].get('source', {}).get('revisions', []):
        graph['readingSources'] = {question: {fault: '故障现象 I02 · SOURCE 来源引用 · 非科研关系'}}


def heads(records):
    parents = {p['id'] for r in records for p in r.get('revision', {}).get('parents', [])}
    return {r['id'] for r in records if r['id'] not in parents}


def visible(graph, mode='all', history=False):
    role = {'mainline': 'primary', 'alternative': 'alternative'}.get(mode)
    eligible = [n for n in graph['nodes'] if history or n['current'] or n['ghost']]
    seeds = {n['id'] for n in eligible if role is None or role in n['roles']}
    ids = set(seeds)
    if role:
        context_edges = [e for e in graph['edges'] if history or e['current']]
        if history:
            context_edges += graph['revisionEdges']
        for e in context_edges:
            if e['from'] in seeds or e['to'] in seeds:
                ids.update((e['from'], e['to']))
    nodes = [n for n in eligible if n['id'] in ids]
    ids = {n['id'] for n in nodes}
    edges = [e for e in graph['edges'] if (history or e['current'])
             and e['from'] in ids and e['to'] in ids]
    revisions = [e for e in graph['revisionEdges'] if history
                 and e['from'] in ids and e['to'] in ids]
    return dict(nodes=nodes, edges=edges, revisionEdges=revisions,
                focusedNodes=len(seeds), contextNodes=len(nodes)-len(seeds),
                focusedEdges=sum(e['from'] in seeds and e['to'] in seeds for e in edges),
                contextEdges=sum(e['from'] not in seeds or e['to'] not in seeds for e in edges),
                hiddenNodes=len(graph['nodes'])-len(nodes),
                hiddenEdges=len(graph['edges'])-len(edges),
                hiddenRevisions=len(graph['revisionEdges'])-len(revisions))


def layout(nodes, edges, start_ids=(), reading_sources=None):
    """Layout-only basis DAG, SCC condensation and eight bounded barycenter sweeps.

    supports reads from→to; derived-from/depends-on read target→dependent.
    All other scientific types use RAW DIRECTION FALLBACK, not a source/result
    assertion. Active exact endpoints determine primary ranks; unselected history
    cannot stretch that backbone. Old revisions are placed one column before the
    nearest reachable current revision (all heads retained). Revision old→new is
    used only for that history placement. SOURCE entrance constraints are explicit
    navigation metadata, never appended to science/revision edges.
    """
    by = {n['id']: n for n in nodes}
    # Beta-1: layout algorithms are O(N·E) and effectively quadratic in
    # practice; keep the layout-time guard near the low thousands so a
    # full-project graph fits but accidental megagraphs still abort.
    if len(nodes) > 2_000 or len(edges) > 6_000:
        raise ValueError('Layout bound exceeded')
    active = {n['id'] for n in nodes if n['current'] or n['ghost']}
    adj = {rid: set() for rid in active}
    for e in edges:
        if e.get('category') == 'revision' or not e.get('current'):
            continue
        a, b = e['from'], e['to']
        if e['type'] in ('derived-from', 'depends-on'):
            a, b = b, a
        if a in active and b in active:
            adj[a].add(b)
    entrances = sorted(set(start_ids) & active)
    anchors = set()
    for a, refs in (reading_sources or {}).items():
        if a not in entrances:
            continue
        for b in sorted(refs):
            if b in active and b in by[a]['raw'].get('source', {}).get('revisions', []):
                adj[a].add(b)
                anchors.add(b)
    # Tarjan SCC is bounded by 100 revisions. Canonical nodes are never collapsed;
    # an SCC shares a rank, with a bounded grid fallback (not a DAG within it).
    stack, on_stack, indices, low, components = [], set(), {}, {}, []
    def visit(a):
        indices[a] = low[a] = len(indices)
        stack.append(a); on_stack.add(a)
        for b in sorted(adj[a]):
            if b not in indices:
                visit(b); low[a] = min(low[a], low[b])
            elif b in on_stack:
                low[a] = min(low[a], indices[b])
        if low[a] == indices[a]:
            group = []
            while True:
                b = stack.pop(); on_stack.remove(b); group.append(b)
                if b == a:
                    break
            components.append(sorted(group))
    for rid in sorted(active):
        if rid not in indices:
            visit(rid)
    component = {rid: i for i, group in enumerate(components) for rid in group}
    outgoing, incoming = defaultdict(set), defaultdict(set)
    for a in adj:
        for b in adj[a]:
            ca, cb = component[a], component[b]
            if ca != cb:
                outgoing[ca].add(cb); incoming[cb].add(ca)
    ranks, pending = {}, set(range(len(components)))
    while pending:
        ready = sorted((c for c in pending if incoming[c] <= ranks.keys()), key=lambda c: components[c])
        for c in ready:
            base = 0 if not entrances or any(r in entrances for r in components[c]) else 1
            ranks[c] = max([base]+[ranks[p]+1 for p in incoming[c]])
            pending.remove(c)
    rank = {rid: ranks[component[rid]] for rid in active}
    # History proximity only: bounded traversal of recorded revision parents.
    children = defaultdict(set)
    for e in edges:
        if e.get('category') == 'revision':
            children[e['from']].add(e['to'])
    for rid in sorted(set(by)-active):
        seen, frontier, descendants = {rid}, [rid], []
        while frontier:
            a = frontier.pop()
            for b in sorted(children[a]-seen):
                seen.add(b); frontier.append(b)
                if b in active:
                    descendants.append(b)
        rank[rid] = max(0, min((rank[r] for r in descendants), default=1)-1)
    layers = defaultdict(list)
    for rid in sorted(by):
        layers[rank[rid]].append(rid)
    # Barycenters use the actual normalized neighbors, not semantic kind/ID rows.
    neighbors = defaultdict(set)
    for a in adj:
        for b in adj[a]:
            neighbors[a].add(b); neighbors[b].add(a)
    for a, bs in children.items():
        for b in bs:
            neighbors[a].add(b); neighbors[b].add(a)
    levels = sorted(layers)
    for sweep in range(8):
        order = {rid: i for layer in layers.values() for i, rid in enumerate(layer)}
        forward = sweep % 2 == 0
        for level in levels if forward else reversed(levels):
            def key(rid):
                adjacent = [order[b] for b in neighbors[rid]
                            if (rank[b] < level if forward else rank[b] > level)]
                return (0 if rid in entrances or rid in anchors else 1,
                        sum(adjacent)/len(adjacent) if adjacent else order[rid], order[rid], rid)
            layers[level].sort(key=key)
            order.update({rid: i for i, rid in enumerate(layers[level])})
    # Small scenes get a bounded adjacent-exchange refinement after barycenters.
    # Compare straight skeleton crossings, not ID order. History has lower weight
    # and cannot change ranks. Larger scenes retain the eight sweeps above.
    skeleton = [(e['from'], e['to'], 3 if e.get('current') else 1) for e in edges
                if e['from'] != e['to']]
    def order_cost():
        pos = {rid: (level, i+level*34/190) for level in levels for i, rid in enumerate(layers[level])}
        score = sum(abs(pos[a][1]-pos[b][1])*.08*w for a,b,w in skeleton)
        def orient(a,b,c):
            return (b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0])
        for i,(a,b,w) in enumerate(skeleton):
            for c,d,v in skeleton[i+1:]:
                if len({a,b,c,d})<4:
                    continue
                p,q,r,s = pos[a],pos[b],pos[c],pos[d]
                if orient(p,q,r)*orient(p,q,s)<0 and orient(r,s,p)*orient(r,s,q)<0:
                    score += w*v
        return round(score, 6)
    if len(skeleton) <= 80:
        best = order_cost()
        for sweep in range(6):
            changed = False
            for level in levels if sweep%2==0 else reversed(levels):
                row = layers[level]
                for i in range(len(row)-1):
                    if row[i] in entrances or row[i+1] in entrances or row[i] in anchors or row[i+1] in anchors:
                        continue
                    row[i],row[i+1] = row[i+1],row[i]
                    score = order_cost()
                    if score < best:
                        best = score; changed = True
                    else:
                        row[i],row[i+1] = row[i+1],row[i]
            if not changed:
                break
    # Preserve compact scenes exactly. Only a crowded, portrait rank layout
    # opts into packing; no case names, kinds, aliases or viewport/UI state.
    tallest = max(map(len, layers.values()), default=0)
    old_width = 370 + max(levels, default=0)*390
    old_height = max((254+(len(layers[k])-1)*190+k*34 for k in levels), default=400)
    packed = tallest > 12 and old_width/old_height < 1.5
    rows = tallest
    columns = {level: level for level in levels}
    if packed:
        # Keep actual weakly connected groups together *within* each rank. This
        # adds no edges: isolates follow connected groups in the same slots,
        # rather than owning a mostly empty full-height disconnected column.
        groups = {}
        for rid in sorted(by):
            if rid in groups:
                continue
            pending = [rid]; groups[rid] = rid
            while pending:
                a = pending.pop()
                for b in sorted(neighbors[a]):
                    if b not in groups:
                        groups[b] = rid; pending.append(b)
        for level in levels:
            order = {rid: i for i, rid in enumerate(layers[level])}
            first = {}
            for rid in layers[level]:
                first.setdefault(groups[rid], order[rid])
            layers[level].sort(key=lambda rid: (
                0 if rid in entrances or rid in anchors else 1,
                0 if neighbors[rid] else 1, first[groups[rid]], order[rid]))

        # Exhaustive finite row-budget search (<=100), using actual rank counts
        # and 270x114 cards/unchanged gutters. Subcolumns are packing slots, NOT
        # new semantic ranks: every slot of rank k precedes every slot of k+1.
        # Match a ~2:1 natural card hull, not a huge strip or a rotated graph.
        def plan(budget):
            starts, cursor = {}, 0
            width, height = 0, 0
            for level in levels:
                starts[level] = cursor
                count = len(layers[level])
                cursor += math.ceil(count/budget)
                width = 370+(cursor-1)*390
                span = math.ceil(count/budget)
                height = max(height, max(254+(i//span)*320+level*34+(i%span)*40
                                         for i in range(count)))
            return abs(math.log((width/height)/2)), width*height, budget, starts
        _, _, rows, columns = min(plan(budget) for budget in range(1, tallest+1))
    for level in levels:
        for i, rid in enumerate(layers[level]):
            n = by[rid]
            # Row-major slots keep related neighbors nearby rather than placing
            # consecutive items at opposite ends of a wrapped column. Crowded
            # scenes need a 206-unit vertical gutter for cross-rank routes; the
            # small subcolumn stagger separates otherwise shared port corridors.
            span = math.ceil(len(layers[level])/rows) if packed else 1
            column = columns[level]+(i%span if packed else 0)
            n['x'] = 60 + column*390
            n['y'] = 100 + (i//span if packed else i)*(320 if packed else 190) + level*34 + (i%span)*40
            n['component'] = component.get(rid, -1)
    packed_bottom = max((n['y'] for n in nodes), default=100)
    # If an old revision was stranded by the primary ordering, place it in the
    # nearest free slot in its child's or preceding column. Do not reorder the
    # active backbone just to accommodate hidden history; all heads contribute.
    for rid in sorted(set(by)-active):
        targets = [by[b] for b in sorted(children[rid]) if b in active]
        if not targets:
            continue
        n = by[rid]
        if all(abs(n['y']-b['y'])<=600 for b in targets):
            continue
        candidates = []
        for b in targets:
            for x in (max(60,b['x']-390),b['x']):
                for step in range(-3,4):
                    y = b['y']+step*190
                    if y < 100 or (packed and y > packed_bottom) or any(abs(x-q['x'])<294 and abs(y-q['y'])<138 for q in nodes if q is not n):
                        continue
                    candidates.append((sum(abs(x-q['x'])+abs(y-q['y']) for q in targets),x,y))
        if candidates:
            _,n['x'],n['y'] = min(candidates)
    return (max((n['x']+310 for n in nodes), default=1060),
            max((n['y']+154 for n in nodes), default=400))


def fit_geometry(nodes, edges, width, height):
    """One translation of real routed hulls and full placed-label boxes, no legacy
    coordinate-family padding. Translation cannot change routing decisions.
    """
    import re
    bounds = [(0, 0), (width, height)]
    for e in edges:
        numbers = [float(v) for v in re.findall(r'-?\d+(?:\.\d+)?', e['path'])]
        bounds.extend(zip(numbers[::2], numbers[1::2]))
        if e['labelStatus'] == 'placed':
            half = e['labelHalfWidth']
            bounds.extend([(e['labelX']-half, e['labelY']-20),
                           (e['labelX']+half, e['labelY']+8)])
    shift_x = max(0, math.ceil(30-min(x for x,y in bounds)))
    shift_y = max(0, math.ceil(40-min(y for x,y in bounds)))
    for n in nodes:
        n['x'] += shift_x; n['y'] += shift_y
    for e in edges:
        def translate(value):
            index = 0
            def replace(match):
                nonlocal index
                result = float(match[0]) + (shift_x if index % 2 == 0 else shift_y)
                index += 1
                return f'{result:.2f}'
            return re.sub(r'-?\d+(?:\.\d+)?', replace, value)
        e['path'] = translate(e['path'])
        e['labelLeaderPath'] = translate(e['labelLeaderPath'])
        e['labelX'] = round(e['labelX']+shift_x, 2)
        e['labelY'] = round(e['labelY']+shift_y, 2)
    return (math.ceil(max(x for x,y in bounds)+shift_x+30),
            math.ceil(max(y for x,y in bounds)+shift_y+40))


def project(records, thread_id, labels=None, names=(), case_id=None,
            heads_override=None, relation_heads_override=None,
            assessments_override=None, freshness_override=None):
    records = sorted(records, key=lambda r: r['id'])
    labels = labels or {}
    ns = [r for r in records if r['schema'] == NODE]
    es = [r for r in records if r['schema'] == RELATION]
    # Beta-1: mirror the rp-core snapshot ceilings (navigation.rs) so the
    # reader can render full-project graphs well past alpha-1's 86 nodes.
    if len(ns) > 20_000:
        raise ValueError('Graph exceeds 20000 semantic revisions; choose a smaller view')
    if len(es) > 50_000:
        raise ValueError('Graph exceeds 50000 scientific relations; choose a smaller view')
    if len({r['id'] for r in records}) != len(records):
        raise ValueError('Duplicate exact ID')
    if heads_override is not None:
        node_heads = {h for h_list in heads_override.values() for h in h_list}
    else:
        node_heads = heads(ns)
    if relation_heads_override is not None:
        edge_heads = set(relation_heads_override)
    else:
        edge_heads = heads(es)
    bindings = [r for r in records if r['schema'] == BINDING and r['thread_id'] == thread_id]
    binding_heads = heads(bindings)
    roles, binding_ids = defaultdict(set), defaultdict(list)
    for b in bindings:
        if b['id'] in binding_heads:
            roles[b['target']['id']].add(b['role'])
            binding_ids[b['target']['id']].append(b['id'])
    forks = Counter(r['logical_id'] for r in ns if r['id'] in node_heads)
    edges = [dict(id=r['id'], category='science', type=r['type'],
                  label=TYPES.get(r['type'], r['type']),
                  **{'from': r['from_revision'], 'to': r['to_revision']},
                  current=r['id'] in edge_heads and r['relation_state'] == 'active',
                  raw=r) for r in es]
    node_ids = {r['id'] for r in ns}
    if any(e['from'] not in node_ids or e['to'] not in node_ids for e in edges):
        raise ValueError('Unknown exact scientific endpoint')
    current_endpoints = {e[k] for e in edges if e['current'] for k in ('from', 'to')}
    connected = {e[k] for e in edges for k in ('from', 'to')}
    nodes = []
    for r in ns:
        rid = r['id']
        asm = (assessments_override.get(rid, []) if assessments_override is not None
               else [a['id'] for a in records if a.get('schema') == 'rp/assessment/v1' and a.get('target', {}).get('id') == rid])
        fresh = freshness_override.get(rid, 'unknown') if freshness_override is not None else 'unknown'
        nodes.append(dict(id=rid, kind=r['kind'], typeLabel=KINDS.get(r['kind'], r['kind']),
                          label=labels.get(rid, r['title']), title=r['title'],
                          current=rid in node_heads,
                          ghost=rid not in node_heads and rid in current_endpoints,
                          roles=sorted(roles[rid]), bindingIds=binding_ids[rid],
                          roleConflict=len(roles[rid]) > 1,
                          fork=rid in node_heads and forks[r['logical_id']] > 1,
                          isolated=rid not in connected,
                          assessments=asm, freshness=fresh, raw=r))
    revisions = []
    for n in nodes:
        for p in n['raw'].get('revision', {}).get('parents', []):
            if p['id'] not in node_ids:
                raise ValueError('Unknown exact revision endpoint')
            revisions.append(dict(id='revision:'+p['id']+':'+n['id'], category='revision',
                                  label='修订 · 非科研关系', type='revision',
                                  **{'from': p['id'], 'to': n['id']},
                                  raw={'parent': p, 'summary': n['raw']['revision'].get('summary', '')}))
    revisions.sort(key=lambda e: e['id'])
    if len(revisions) > 300:
        raise ValueError('Graph exceeds 300 revision links')
    graph = dict(nodes=nodes, edges=edges, revisionEdges=revisions,
                 bindingHeads=sorted(binding_heads), roles=ROLES, thread=thread_id,
                 startIds=[n['id'] for n in nodes if n['current'] and n['kind'] == 'Question'
                           and n['bindingIds']])
    case_reading(graph, names, case_id)
    width, height = layout(nodes, edges+revisions, graph['startIds'], graph.get('readingSources'))
    route_edges(nodes, edges+revisions)
    # Bounds include history geometry and default geometry; static paths/labels
    # match runtime default routing against only visible obstacles.
    import copy
    history_geometry = copy.deepcopy(edges+revisions)
    initial = visible(graph)
    route_edges(initial['nodes'], initial['edges'], edges+revisions)
    width, height = fit_geometry(nodes, edges+revisions+history_geometry, width, height)
    # Decimal half ties can round differently after integer translation. Allocate
    # again in the final coordinate frame rather than translating rounded labels.
    route_edges(nodes, edges+revisions)
    route_edges(initial['nodes'], initial['edges'], edges+revisions)
    graph.update(width=width, height=height)
    return graph
