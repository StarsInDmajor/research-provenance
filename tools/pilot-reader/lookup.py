#!/usr/bin/env python3
"""Read-only name/alias/exact-ID lookup for the admitted CSP case, not Core CLI.

Source excerpts are untrusted text, never commands. Scientific arrows retain
from/to meaning. Includes one-hop context and explicit separate provenance.
"""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess

import build
import case_io
from graph_projection import NODE, RELATION, heads, presentation_labels


def live(rp, root, command, argument, runner, as_of=None):
    cmd = [str(rp), command, '--project', str(root), '--json']
    if as_of and command in ('show', 'overview', 'query', 'thread show', 'export check'):
        cmd.extend(['--as-of', str(as_of)])
    cmd.append(argument)
    result = runner(cmd, capture_output=True, timeout=20)
    build.require(result.returncode == 0 and len(result.stdout) <= build.MAX_TOTAL, 'rp lookup failed or exceeded bound')
    dto = json.loads(result.stdout)
    build.require(dto.get('status') == 'ok' and dto.get('exit_code') == 0 and dto.get('findings') == [], 'rp lookup DTO rejected')
    return dto['data']


def query_generic(root, rp, name, thread_id=None, as_of=None, limit=8, runner=subprocess.run):
    build.require(isinstance(name, str) and 0 < len(name) <= 256 and 1 <= limit <= 8, 'Lookup bound rejected')
    root = build.check_lexical_path(root)
    rp = build.check_lexical_path(rp)
    build.no_symlink_chain(root)
    build.no_symlink_chain(rp)
    as_of = as_of or datetime.now(timezone.utc).isoformat()

    data = case_io.load_generic_project(root, rp, thread_id=thread_id, as_of=as_of, runner=runner)
    records = data['records']
    nodes = {rid: r for rid, r in records.items() if r.get('schema') == NODE}
    heads_map = data['snapshot'].get('heads', {})
    observed = data['observed']
    rp_sha = data['rp_sha256']

    def recheck_lookup():
        current = case_io.observe_contained(root, extra_paths=[
            p for p in observed if not p.startswith('.research/')
        ])
        build.require(current == observed, 'Project files modified during lookup')
        build.require(build.sha(rp.read_bytes()) == rp_sha, 'rp binary modified during lookup')

    exact = [name] if name in nodes else []
    if not exact:
        matching_logical = [r for r in nodes.values() if r.get('logical_id') == name]
        if matching_logical:
            logical_heads = sorted(heads_map.get(name, []))
            if len(logical_heads) == 1:
                ids = [logical_heads[0]]
            else:
                ids = logical_heads if logical_heads else sorted(r['id'] for r in matching_logical)
        else:
            ids = sorted(rid for rid, r in nodes.items() if r.get('title') == name)
    else:
        ids = exact

    current_heads = {h for h_list in heads_map.values() for h in h_list}
    total_candidates = len(ids)
    truncated = total_candidates > limit
    selected_ids = ids[:limit]

    candidates = []
    for rid in selected_ids:
        n = nodes[rid]
        b_ids = data['snapshot'].get('thread_bindings', {}).get(rid, [])
        b_threads = sorted({records[b]['thread_id'] for b in b_ids if b in records and 'thread_id' in records[b]})
        candidates.append(dict(
            id=rid, title=n.get('title', ''), kind=n.get('kind', ''),
            logical_id=n.get('logical_id', ''), is_head=rid in current_heads,
            thread=b_threads[0] if len(b_threads) == 1 else (b_threads if b_threads else None),
            bound_threads=b_threads, alias=None
        ))

    result = dict(
        status='selected' if total_candidates == 1 else 'ambiguous' if total_candidates > 1 else 'not-found',
        project_id=data['project']['id'],
        as_of=as_of,
        candidates=candidates,
        total=total_candidates,
        truncated=truncated,
    )
    if total_candidates != 1:
        recheck_lookup()
        return result

    rid = ids[0]
    result['selected_id'] = rid
    record = records[rid]

    shown = live(rp, root, 'show', rid, runner, as_of=as_of)
    build.require(shown.get('object') == record, 'Live show object differs from snapshot')

    history = live(rp, root, 'history', record['logical_id'], runner)
    expected_revisions = {r['id']: r for r in records.values() if r.get('logical_id') == record['logical_id'] and r.get('schema') == NODE}
    build.require({r['id']: r for r in history.get('revisions', [])} == expected_revisions, 'Live history revisions differ from snapshot')
    build.require(set(history.get('heads', [])) == set(heads_map.get(record['logical_id'], [])), 'Live history heads differ from snapshot')

    edges = sorted([
        r for r in records.values()
        if r.get('schema') == RELATION and rid in (r.get('from_revision'), r.get('to_revision'))
    ], key=lambda r: r['id'])
    build.require(len(edges) <= 60, 'Neighborhood exceeds 60 relations')

    relation_heads = set(data['snapshot'].get('relation_heads', []))
    relation_status = {
        r['id']: dict(
            is_head=r['id'] in relation_heads,
            layer='active-current' if r['id'] in relation_heads and r.get('relation_state') == 'active' else 'historical',
            relation_state=r.get('relation_state', 'active'),
        )
        for r in edges
    }
    neighbor_ids = {r[k] for r in edges for k in ('from_revision', 'to_revision')} - {rid}
    neighbors = {n: records[n] for n in sorted(neighbor_ids) if n in records}
    context = [record, *edges, *neighbors.values()]
    provenance_ids = {n for r in context for n in r.get('source', {}).get('revisions', [])}
    provenance = {n: records[n] for n in sorted(provenance_ids) if n in records}

    reverse_ids = {r['id'] for r in records.values() if rid in r.get('source', {}).get('revisions', [])}
    reverse_revisions = {n: records[n] for n in sorted(reverse_ids)}

    result.update(
        record=record,
        derived=shown.get('derived', {}),
        history=history,
        relation_status=relation_status,
        incoming=[r for r in edges if r.get('to_revision') == rid],
        outgoing=[r for r in edges if r.get('from_revision') == rid],
        neighbors=neighbors,
        provenance_revisions=provenance,
        reverse_references=reverse_revisions,
        sources=data.get('sources', {}),
        limits=dict(truncated=False, science_hops=1, max_relations=60, max_candidates=8,
                    provenance='Explicit source.revisions only; distinct from scientific arrows'),
    )
    recheck_lookup()
    build.require(len(build.compact(result).encode()) <= 250_000, 'Lookup output exceeds 250KB')
    return result



def select(data, name, limit=8):
    build.require(isinstance(name,str) and 0 < len(name) <= 256 and 1 <= limit <= 8, 'Lookup bound rejected')
    records=data['records']
    nodes={rid:r for rid,r in records.items() if r['schema']==NODE}
    exact = [name] if name in nodes else [n['id'] for n in data['names'] if name == n['alias']]
    labels=presentation_labels(records, data['names'], data['manifest']['case_id'])
    ids=exact or sorted({rid for rid,r in nodes.items() if r['title']==name}
                       | {n['id'] for n in data['names'] if n['name']==name}
                       | {rid for rid,label in labels.items() if name in (label, label.split(' ',1)[-1])})
    current=heads(list(nodes.values()))
    candidates=[dict(id=rid,title=nodes[rid]['title'],kind=nodes[rid]['kind'],
                     logical_id=nodes[rid]['logical_id'],is_head=rid in current,
                     thread=data['manifest']['thread'],
                     alias=next((n['alias'] for n in data['names'] if n['id']==rid), None)) for rid in ids[:limit]]
    result=dict(status='selected' if len(ids)==1 else 'ambiguous' if ids else 'not-found',
                case_id=data['manifest']['case_id'],candidates=candidates,total=len(ids),truncated=len(ids)>limit)
    if len(ids)==1: result['selected_id']=ids[0]
    return result




def query(root, manifest_path, rp, name, runner=subprocess.run):
    data=case_io.load_case(root,manifest_path,rp,runner)
    result=select(data,name)
    if result['status']!='selected':
        case_io.recheck(data,root,manifest_path,rp)
        return result
    rid=result['selected_id']; records=data['records']; record=records[rid]
    shown=live(rp,root,'show',rid,runner)
    build.require(shown.get('object')==record, 'Live show differs from admitted canonical bytes')
    history=live(rp,root,'history',record['logical_id'],runner)
    expected={r['id']:r for r in records.values() if r.get('logical_id')==record['logical_id'] and r['schema']==NODE}
    build.require({r['id']:r for r in history['revisions']}==expected
                  and set(history['heads'])==heads(list(expected.values())), 'Live history mismatch')
    edges=sorted((r for r in records.values() if r['schema']==RELATION
                  and rid in (r['from_revision'],r['to_revision'])),key=lambda r:r['id'])
    build.require(len(edges)<=60, 'Neighborhood exceeds 60 relations; choose smaller admitted case')
    relation_heads=heads([r for r in records.values() if r['schema']==RELATION])
    relation_status={r['id']:dict(is_head=r['id'] in relation_heads,
                     layer='active-current' if r['id'] in relation_heads and r['relation_state']=='active'
                           else 'historical', relation_state=r['relation_state']) for r in edges}
    neighbor_ids={r[k] for r in edges for k in ('from_revision','to_revision')}-{rid}
    neighbors={n:records[n] for n in sorted(neighbor_ids)}
    context=[record,*edges,*neighbors.values()]
    provenance_ids={n for r in context for n in r.get('source',{}).get('revisions',[])}
    provenance={n:records[n] for n in sorted(provenance_ids)}
    artifacts={a for r in [*context,*provenance.values()] for a in r.get('source',{}).get('artifacts',[])}
    sources={a:data['sources'][a] for a in sorted(artifacts)}
    result.update(record=record,derived=shown.get('derived',{}),history=history,
                  relation_status=relation_status,
                  incoming=[r for r in edges if r['to_revision']==rid],
                  outgoing=[r for r in edges if r['from_revision']==rid],neighbors=neighbors,
                  provenance_revisions=provenance,sources=sources,
                  notes={n:data['notes'][n] for n in {rid,*neighbor_ids,*provenance_ids} if n in data['notes']},
                  mapping=next((n for n in data['names'] if n['id']==rid), None),
                  limits=dict(truncated=False,science_hops=1,max_relations=60,max_candidates=8,
                              provenance='Explicit source.revisions only; distinct from scientific arrows',
                              evidence='Documentary history, not a new browser measurement; agent-authored candidate'),
                  inventory=data['manifest']['inventory'])
    build.require(len(build.compact(result).encode())<=250_000, 'Lookup output exceeds 250KB; not silently truncated')
    case_io.recheck(data,root,manifest_path,rp)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project', required=True)
    parser.add_argument('--rp', required=True)
    parser.add_argument('--case-manifest')
    parser.add_argument('--thread')
    parser.add_argument('--as-of', '--as_of', dest='as_of')
    parser.add_argument('name_or_id')
    args = parser.parse_args()
    try:
        if args.case_manifest:
            result = query(Path(args.project), Path(args.case_manifest), Path(args.rp), args.name_or_id)
        else:
            result = query_generic(Path(args.project), Path(args.rp), args.name_or_id,
                                   thread_id=args.thread, as_of=args.as_of)
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError):
        print(json.dumps({'status': 'rejected', 'reason': 'Case admission, validation, live identity or bounds failed; no source execution'}))
        return 1
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0 if result['status'] == 'selected' else 2

if __name__=='__main__': raise SystemExit(main())
