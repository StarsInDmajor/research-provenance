"""Opt-in private archive locators. No URI fetching or original-source reads.

Canonical bindings use reader-canonical-json-v1 (build.compact UTF-8), not JCS.
Only explicitly listed packet paths are read, after matching canonical artifacts.
The archived header and exact line-counted capture are verified; original file
hashes are capture claims, NOT a claim of current/live scientific verification.
"""
from datetime import datetime
from pathlib import Path
import json
import re
import stat
import build

MAX_SIDECAR = 192000
MAX_SOURCES = 400
FIELDS = ('path','start_line','end_line','section','sha256','excerpt_sha256','captured_at',
          'display_end_line','display_sha256','excerpt')
require = build.require


def read_sidecar(path):
    path = build.check_lexical_path(path)
    require(stat.S_ISREG(path.stat().st_mode) and path.stat().st_size <= MAX_SIDECAR,
            'Source locator sidecar size/type rejected')
    with path.open('rb') as f: raw = f.read(MAX_SIDECAR + 1)
    require(len(raw) <= MAX_SIDECAR, 'Source locator sidecar exceeds bound')
    return raw


def strict_json(raw):
    def unique(pairs):
        out = {}
        for k,v in pairs:
            require(k not in out, 'Duplicate source locator JSON key')
            out[k] = v
        return out
    def reject(_): raise ValueError('Non-finite JSON rejected')
    try:
        return json.loads(raw, object_pairs_hook=unique, parse_constant=reject)
    except RecursionError:
        raise ValueError('Source locator JSON nesting exceeds bound') from None


def exact(value, keys):
    require(isinstance(value, dict) and set(value) == set(keys), 'Source locator fields rejected')


def relative(value):
    require(isinstance(value, str) and 0 < len(value.encode('utf-8')) <= 512
            and not any(c in value for c in '\\:\x00\n\r')
            and all(p not in ('','.','..') for p in value.split('/'))
            and not Path(value).is_absolute(), 'Unsafe source locator path')
    return value


def digest(value):
    require(isinstance(value, str) and re.fullmatch(r'sha256:[0-9a-f]{64}',value),
            'Source locator digest rejected')


def bound_record(records, rid, value):
    require(isinstance(rid,str) and rid in records, 'Source locator record ID rejected')
    digest(value)
    require(value == build.sha(build.compact(records[rid]).encode()), 'Source locator canonical digest mismatch')


def validate_source(s):
    relative(s['path'])
    for k in ('start_line','end_line'):
        require(type(s[k]) is int and 1 <= s[k] <= 1000000, 'Source locator line rejected')
    require(s['start_line'] <= s['end_line'] and s['end_line']-s['start_line'] < 10000,
            'Source locator range rejected')
    require(isinstance(s['section'],str) and len(s['section'].encode()) <= 1024, 'Source section rejected')
    for k in ('sha256','excerpt_sha256'): digest(s[k])
    stamp=s['captured_at']
    require(isinstance(stamp,str) and len(stamp)<=40 and re.fullmatch(
        r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})',stamp), 'Source capture timestamp rejected')
    require(datetime.fromisoformat(stamp.replace('Z','+00:00')).tzinfo is not None, 'Source timezone required')


def captured_excerpt(packet, s):
    """Count ORIGINAL lines after a matched header, not packet lines/fences.

    Captures can contain nested Markdown fences. Never split on a closing fence.
    Verify the exact full excerpt hash including its final LF before selection.
    """
    validate_source(s)
    text=packet.decode('utf-8')
    header=(f"## {s['path']}:L{s['start_line']}-L{s['end_line']}\n"
            f"Original file {s['sha256']}; excerpt {s['excerpt_sha256']}; captured {s['captured_at']}\n\n```text\n")
    positions=[m.end() for m in re.finditer(re.escape(header),text) if m.start()==0 or text[m.start()-1]=='\n']
    require(len(positions)==1, 'Source capture header missing/ambiguous')
    tail=text[positions[0]:]; lines=tail.splitlines(keepends=True)
    count=s['end_line']-s['start_line']+1
    excerpt=''.join(lines[:count])
    require(len(lines)>=count and build.sha(excerpt.encode())==s['excerpt_sha256']
            and tail[len(excerpt):].startswith('\n```\n'), 'Source capture bytes/line count mismatch')
    return excerpt


def prefix(body, start):
    # Whole-line prefix only. A long first line means locator-only, not ellipsis.
    selected=[]; size=0
    for line in body.splitlines(keepends=True):
        size += len(line.encode())
        if size > 160 or len(selected)==3: break
        selected.append(line)
    return ''.join(selected), start+len(selected)-1


def unpack(loc):
    return [{k:(row[i] if i in (1,2,7) else loc['strings'][row[i]] if row[i]>=0 else '')
             for i,k in enumerate(FIELDS)} | {'display_start_line':row[1]} for row in loc['chunks']]


def load(path, data, root):
    import case_io
    raw=read_sidecar(path); value=strict_json(raw)
    exact(value, ('version','project_id','entries'))
    require(type(value['version']) is int and value['version']==1 and value['project_id']==data['project']['id'],
            'Source locator project/version mismatch')
    entries=value['entries']; nodes={n['id']:n for n in data['graph']['nodes']}; records=data['records']
    require(isinstance(entries,dict) and len(entries)<=100 and set(entries)==set(nodes),
            'Source locator entries must cover exact nodes (empty sources = unavailable)')
    packets={}; chunks=[]; chunk_ids={}; artifacts=[]; bindings={}; total=0; primary=set()
    for rid,e in entries.items():
        exact(e, ('revision_id','canonical_digest','sources'))
        require(e['revision_id']==rid, 'Source locator exact revision mismatch')
        bound_record(records,rid,e['canonical_digest'])
        refs=e['sources']; require(isinstance(refs,list) and len(refs)<=8, 'Source locator array bound rejected')
        total+=len(refs); require(total<=MAX_SOURCES, 'Source locator count exceeds bound')
        links=[]; seen=set()
        for s in refs:
            exact(s, ('artifact_id','artifact_digest','packet_path',*FIELDS[:7]))
            validate_source(s); rel=relative(s['packet_path']); aid=s['artifact_id']
            bound_record(records,aid,s['artifact_digest']); a=records[aid]
            require(aid in records[rid].get('source',{}).get('artifacts',[]) and
                    a['schema']=='rp/artifact-manifest/v1' and a.get('uri')=='file:'+rel,
                    'Source locator must reference actual node artifact and contained copy')
            if rel not in packets:
                packet=case_io.read_bounded(build.check_lexical_path(Path(root)/rel))
                require(build.sha(packet)==a.get('sha256') and len(packet)==a.get('size_bytes'), 'Source packet digest/size mismatch')
                require(sum(map(len,packets.values()))+len(packet)<=512000, 'Source packets exceed total bound')
                packets[rel]=packet
            require(build.sha(packets[rel])==a.get('sha256') and len(packets[rel])==a.get('size_bytes'), 'Source artifact packet mismatch')
            body=captured_excerpt(packets[rel],s)
            key=build.compact({k:s[k] for k in FIELDS[:7]})
            require(key not in seen, 'Duplicate node source locator'); seen.add(key)
            if key not in chunk_ids:
                chunk_ids[key]=len(chunks); chunks.append({k:s[k] for k in FIELDS[:7]} | {'body':body})
            ci=chunk_ids[key]
            if not links: primary.add(ci)
            if aid not in artifacts: artifacts.append(aid)
            links.append([ci,artifacts.index(aid)])
        bindings[rid]=links
    strings=[]; string_ids={}
    def intern(s):
        if s not in string_ids: string_ids[s]=len(strings); strings.append(s)
        return string_ids[s]
    wire=[]
    for i,s in enumerate(chunks):
        excerpt,end=prefix(s['body'],s['start_line']) if i in primary else ('',s['start_line']-1)
        s.update(excerpt=excerpt,display_end_line=end,display_sha256=build.sha(excerpt.encode()) if excerpt else '')
        wire.append([s[k] if j in (1,2,7) else intern(s[k]) if s[k] else -1 for j,k in enumerate(FIELDS)])
    # Publish only after the entire mapping passed. No canonical mutation.
    data['sourceLocators']=dict(strings=strings,chunks=wire,artifacts=artifacts)
    for rid,links in bindings.items(): nodes[rid]['sourceLocators']=links
    data['observed'].update(packets)
    return raw
