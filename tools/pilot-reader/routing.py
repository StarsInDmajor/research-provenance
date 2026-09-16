"""Finite local candidate routing; same contract/cases as routing.js, no IO."""
import math


def intersects(a, b, r):
    l, right, t, bottom = r[1:]
    if max(a[0], b[0]) <= l or min(a[0], b[0]) >= right or max(a[1], b[1]) <= t or min(a[1], b[1]) >= bottom:
        return False
    lo, hi = 0, 1
    for p, q, low, high in [(a[0], b[0], l, right), (a[1], b[1], t, bottom)]:
        if p == q:
            if p <= low or p >= high:
                return False
            continue
        x, y = (low-p)/(q-p), (high-p)/(q-p)
        lo, hi = max(lo, min(x, y)), min(hi, max(x, y))
    return lo < hi-1e-9 and hi > 1e-9 and lo < 1-1e-9


def length(ps):
    return sum(math.hypot(b[0]-a[0], b[1]-a[1]) for a, b in zip(ps, ps[1:]))


def path(ps):
    return ' '.join(('M ' if i == 0 else 'L ')+f'{p[0]:.2f} {p[1]:.2f}' for i, p in enumerate(ps))


def points(d):
    ns = [float(n) for n in d.split() if n not in ('M', 'L')]
    return list(zip(ns[::2], ns[1::2]))


def overlap(a, b):
    return a[1] < b[2] and a[2] > b[1] and a[3] < b[4] and a[4] > b[3]


def label_text(e):
    suffix = ''
    if e.get('category') == 'science' and not e.get('current'):
        suffix = ' · 已撤销（历史）' if e.get('raw', {}).get('relation_state') == 'invalidated' else ' · 历史'
    return e.get('label', '')+suffix


def allocate(edges, rects):
    occupied, leaders = [], []
    lines = [s for e in edges for s in zip(points(e['path']), points(e['path'])[1:])]
    for e in sorted(edges, key=lambda e: e['id']):
        half = max(24, sum((12 if c in 'MWmw@%&' else 8) if ord(c) < 128 else 14 for c in label_text(e))/2+6)
        e.update(labelX=0, labelY=0, labelHalfWidth=half, labelStatus='hidden', labelLeaderPath='')
        if e['routeStatus'] != 'routed' or half > 180:
            continue
        ps = points(e['path'])
        segments = sorted(enumerate(zip(ps, ps[1:])), key=lambda v: (-round(length(v[1]), 6), v[0]))
        # Same <=250 candidates, <=36-unit own-segment leader as routing.js.
        def place():
            for _, (a, b) in segments:
                for ratio in (.5, .25, .75, .125, .875):
                    ax, ay = (math.floor((a[i]+(b[i]-a[i])*ratio)*100+.5)/100 for i in (0, 1))
                    for x, y, lx, ly in [(ax, ay-12, ax, ay-4), (ax, ay+32, ax, ay+12),
                                         (ax-half-6, ay+6, ax-6, ay), (ax+half+6, ay+6, ax+6, ay),
                                         (ax, ay-44, ax, ay-36), (ax, ay+56, ax, ay+36),
                                         (ax-half, ay-12, ax, ay-4), (ax+half, ay-12, ax, ay-4),
                                         (ax-half, ay+32, ax, ay+12), (ax+half, ay+32, ax, ay+12)]:
                        box, leader = ('', x-half, x+half, y-20, y+8), [(ax, ay), (lx, ly)]
                        if any(overlap(box, q) or intersects(*leader, q) for q in rects+occupied):
                            continue
                        padded = ('', box[1]-2, box[2]+2, box[3]-2, box[4]+2)
                        if any(intersects(*s, padded) for s in lines+leaders):
                            continue
                        e.update(labelX=round(x, 2), labelY=round(y, 2), labelStatus='placed', labelLeaderPath=path(leader))
                        occupied.append(box)
                        leaders.append(leader)
                        return
        place()


def congestion(ps, reserved):
    """Finite soft cost on previously reserved segments; node clearance wins.

    Collinear length costs 2/unit, transverse crossings 70. Ignore segment-end
    touches here (independent report counts them). This is greedy, not optimal.
    """
    cost = 0
    for a, b in zip(ps, ps[1:]):
        ux, uy = b[0]-a[0], b[1]-a[1]
        size = math.hypot(ux, uy)
        if size < 1e-6:
            continue
        for c, d in reserved:
            vx, vy, wx, wy = d[0]-c[0], d[1]-c[1], c[0]-a[0], c[1]-a[1]
            den = ux*vy-uy*vx
            if abs(den) < 1e-6:
                if abs(wx*uy-wy*ux) > 1e-6:
                    continue
                low, high = sorted(((c[0]-a[0])/ux, (d[0]-a[0])/ux) if abs(ux)>=abs(uy)
                                   else ((c[1]-a[1])/uy, (d[1]-a[1])/uy))
                cost += 2*max(0, min(1, high)-max(0, low))*size
            else:
                t, s = (wx*vy-wy*vx)/den, (wx*uy-wy*ux)/den
                if 1e-6 < t < 1-1e-6 and 1e-6 < s < 1-1e-6:
                    cost += 70
    return cost


def one(a, b, e, index, count, rects, reserved=()):
    ax, ay, bx, by = a['x']+135, a['y']+57, b['x']+135, b['y']+57
    dx, dy = bx-ax, by-ay
    d = math.hypot(dx, dy)
    if not d:
        start, end = [ax-65, a['y']], [ax+65, a['y']]
        sa, sb = [start[0], a['y']-16], [end[0], a['y']-16]
    else:
        ux, uy = dx/d, dy/d
        radius = min(135/abs(ux) if ux else 1e9, 57/abs(uy) if uy else 1e9)
        start, end = [ax+ux*radius, ay+uy*radius], [bx-ux*(radius+5), by-uy*(radius+5)]
        # Spread canonical pair ports along the same boundary, not through cards.
        # Reverse arrows use the same world-space lane sign. Dense pairs compress
        # within a finite 48-unit band rather than escaping the endpoint side.
        port = (index-(count-1)/2)*min(8, 48/max(1,count-1))
        if count > 1 and abs(ux)*57 >= abs(uy)*135:
            start[1] = max(a['y']+12, min(a['y']+102, start[1]+port))
            end[1] = max(b['y']+12, min(b['y']+102, end[1]+port))
        elif count > 1:
            start[0] = max(a['x']+12, min(a['x']+258, start[0]+port))
            end[0] = max(b['x']+12, min(b['x']+258, end[0]+port))
        sa, sb = [start[0]+ux*16, start[1]+uy*16], [end[0]-ux*16, end[1]-uy*16]
    lane = (index-(count-1)/2)*12
    sign = 1 if e['from'] < e['to'] else -1
    offset = [-dy/d*lane*sign, dx/d*lane*sign] if d else [0, 0]
    candidates = []
    def add(ps):
        candidates.append([p for i, p in enumerate(ps) if not i or p != ps[i-1]])
    if d:
        if count == 1:
            add([start, end])
        else:
            add([start, sa, [(sa[0]+sb[0])/2+offset[0], (sa[1]+sb[1])/2+offset[1]], sb, end])
    l, r, t, bot = min(sa[0], sb[0]), max(sa[0], sb[0]), min(sa[1], sb[1]), max(sa[1], sb[1])
    def distance(q):
        return max(l-q[2], q[1]-r, 0)+max(t-q[4], q[3]-bot, 0)
    nearby = sorted(rects, key=lambda q: (distance(q), q[0]))[:12]
    xs, ys = set(), set()
    gap = 2+index*6
    for _, left, right, top, bottom in nearby:
        xs.update((left-gap, right+gap))
        ys.update((top-gap, bottom+gap))
    if d:
        xs.add((sa[0]+sb[0])/2+lane)
        ys.add((sa[1]+sb[1])/2+lane)
    # Nearby alternatives allow reserved corridors to separate without global
    # padding. <=151 candidates; no search beyond these finite local lanes.
    xs = {v+delta for v in xs for delta in (-10, 0, 10)}
    ys = {v+delta for v in ys for delta in (-10, 0, 10)}
    for y in sorted(ys):
        add([start, sa, [sa[0], y], [sb[0], y], sb, end])
    for x in sorted(xs):
        add([start, sa, [x, sa[1]], [x, sb[1]], sb, end])
    collisions = [(id, l+12, r-12, t+12, bot-12) if id in (a['id'], b['id']) else (id, l, r, t, bot) for id, l, r, t, bot in rects]
    # Quantized cost avoids cross-language floating-point tie drift.
    valid = []
    for ps in sorted(candidates, key=lambda ps: round(length(ps), 6)):
        # Collinear extra vertices must not masquerade as distinct lanes.
        if d and count > 1 and lane != 0 and all(abs((p[0]-start[0])*dy-(p[1]-start[1])*dx) < 1e-7 for p in ps):
            continue
        if len(ps) < 2 or any(intersects(a, b, q) for a, b in zip(ps, ps[1:]) for q in collisions):
            continue
        size = length(ps)
        if valid and size > valid[0][0]*1.5+180:
            break
        valid.append((size, ps))
        if len(valid) == 24:
            break
    if valid:
        _, ps = min(valid, key=lambda pair: round(pair[0]+congestion(pair[1], reserved), 6))
        return dict(path=path(ps), routeStatus='routed')
    return dict(path=path([start]), routeStatus='unroutable')


def route_edges(nodes, edges, all_edges=None):
    all_edges = edges if all_edges is None else all_edges
    # Beta-1: keep the routing-time guard in step with the layout bound so a
    # full-project graph routes; the real ceilings live in graph_projection.
    if len(nodes) > 2_000 or len(edges) > 6_000 or len(all_edges) > 6_000:
        raise ValueError('Routing bound exceeded')
    by = {n['id']: n for n in nodes}
    rects = [(n['id'], n['x']-12, n['x']+282, n['y']-12, n['y']+126) for n in nodes]
    groups = {}
    for e in all_edges:
        groups.setdefault(tuple(sorted((e['from'], e['to']))), []).append(e['id'])
    for ids in groups.values():
        ids.sort()
    reserved = []
    for e in sorted(edges, key=lambda e: e['id']):
        ids = groups[tuple(sorted((e['from'], e['to'])))]
        e.update(one(by[e['from']], by[e['to']], e, ids.index(e['id']), len(ids), rects, reserved))
        ps = points(e['path'])
        reserved.extend(zip(ps, ps[1:]))
    allocate(edges, rects)
