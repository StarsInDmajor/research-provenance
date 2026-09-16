"""Independent polyline evaluator for archived/new scenes (not router cost code).

Pairwise collinear overlap length and distinct transverse intersection points per
edge pair, including bends/T touches. Shared exact endpoint approaches are trimmed
24 units on each incident path. No label leaders are scientific paths. Coordinates
are the rendered two-decimal SVG values; tolerance 1e-6. Not a visual legibility score.
"""
import math
import re
from itertools import combinations


def vertices(path):
    ns = list(map(float, re.findall(r'-?\d+(?:\.\d+)?', path)))
    return list(zip(ns[::2], ns[1::2]))


def distance(a, b):
    return math.hypot(b[0]-a[0], b[1]-a[1])


def trim(ps, amount):
    ps = list(ps)
    while len(ps) > 1 and amount:
        d = distance(ps[0], ps[1])
        if d <= amount:
            amount -= d
            ps.pop(0)
        else:
            ps[0] = tuple(a+(b-a)*amount/d for a,b in zip(ps[0], ps[1]))
            break
    return ps


def metrics(edges):
    shared, crossings = 0, 0
    details = []
    cross = lambda a,b: a[0]*b[1]-a[1]*b[0]
    sub = lambda a,b: (a[0]-b[0], a[1]-b[1])
    for e,f in combinations(edges, 2):
        ep, fp = vertices(e['path']), vertices(f['path'])
        common = {e['from'],e['to']} & {f['from'],f['to']}
        if e['from'] in common: ep = trim(ep,24)
        if e['to'] in common: ep = trim(ep[::-1],24)[::-1]
        if f['from'] in common: fp = trim(fp,24)
        if f['to'] in common: fp = trim(fp[::-1],24)[::-1]
        hits, overlap = set(), 0
        for a,b in zip(ep,ep[1:]):
            u = sub(b,a); size = distance(a,b)
            if size < 1e-6: continue
            for c,d in zip(fp,fp[1:]):
                v, w = sub(d,c), sub(c,a)
                denom = cross(u,v)
                if abs(denom) < 1e-6:
                    if abs(cross(w,u)) > 1e-6: continue
                    axis = 0 if abs(u[0]) >= abs(u[1]) else 1
                    low, high = sorted(((c[axis]-a[axis])/u[axis],(d[axis]-a[axis])/u[axis]))
                    overlap += max(0,min(1,high)-max(0,low))*size
                else:
                    t, s = cross(w,v)/denom, cross(w,u)/denom
                    if -1e-9 <= t <= 1+1e-9 and -1e-9 <= s <= 1+1e-9:
                        hits.add(tuple(round(a[i]+t*u[i],5) for i in (0,1)))
        shared += overlap; crossings += len(hits)
        if overlap or hits: details.append(dict(pair=[e['id'],f['id']],shared=round(overlap,2),crossings=len(hits)))
    lengths = [sum(distance(a,b) for a,b in zip(vertices(e['path']),vertices(e['path'])[1:])) for e in edges]
    return dict(edges=len(edges),routed=sum(e['routeStatus']=='routed' for e in edges),
                labels=sum(e['labelStatus']=='placed' for e in edges),sharedLength=round(shared,2),
                crossings=crossings,totalLength=round(sum(lengths),2),maxLength=round(max(lengths,default=0),2),pairs=details)
