"""Admitted CSP snapshots; deliberately NOT a generic RP loader.

Original and writing-r2 manifest digests are separately reviewed in source.
Private bytes live outside this repository. Never follow artifact/reference URIs. Same observed copies are used
for verification and display; repeated inventories are not atomic snapshots.
"""
from collections import Counter
from datetime import datetime
import json
import os
from pathlib import Path
import re
import stat
import subprocess

import build
from graph_projection import project, NODE, heads, presentation_labels

ADMISSION = Path(__file__).with_name('csp-case-admission.json')
ADMISSION_R2 = Path(__file__).with_name('csp-writing-r2-admission.json')
EXTRA_SCHEMA = 'rp/external-reference/v1'


def admission_path(case_id):
    build.require(case_id in ('csp-startup-v1', 'csp-writing-r2'), 'Unknown case admission')
    return ADMISSION if case_id == 'csp-startup-v1' else ADMISSION_R2


def read_bounded(path):
    path = Path(path)
    build.no_symlink_chain(path)
    build.require(path.is_file() and path.stat().st_size <= build.MAX_FILE, 'Manifest size/type rejected')
    with path.open('rb') as f:
        data = f.read(build.MAX_FILE+1)
    build.require(len(data) <= build.MAX_FILE, 'Manifest exceeds bound')
    return data


def strict_json(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            build.require(key not in result, 'Duplicate JSON key')
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=unique)


def local_copy(observed, path):
    build.require(isinstance(path, str) and bool(path), 'Invalid local path')
    p = Path(path)
    build.require(not p.is_absolute() and '..' not in p.parts and p.as_posix() == path
                  and path in observed, 'Unsafe or unmapped local path')
    return observed[path]


MAX_TOTAL_OBSERVED = 2_512_000  # 2MB .research files + 512KB local source excerpts
MAX_OBSERVED_FILES = 512


def observe_contained(root, extra_paths=None):
    root = build.check_lexical_path(root)
    build.require(root.is_dir(), 'Input directory missing')
    res_dir = build.check_lexical_path(root / '.research')
    build.require(res_dir.is_dir(), '.research directory missing')
    observed, total, entries = {}, 0, 0
    for current, dirs, files in os.walk(res_dir, followlinks=False):
        for name in sorted(dirs + files):
            p = Path(current) / name
            entries += 1
            build.require(entries <= MAX_OBSERVED_FILES * 2, 'Too many input entries in .research')
            mode = p.lstat().st_mode
            build.require(not stat.S_ISLNK(mode), 'Input symlink rejected')
            build.require(stat.S_ISDIR(mode) or stat.S_ISREG(mode), 'Special input file rejected')
            if stat.S_ISDIR(mode):
                continue
            build.require(len(observed) < MAX_OBSERVED_FILES and p.stat().st_size <= build.MAX_FILE,
                          'Input file count/size exceeds private reader bound')
            with p.open('rb') as f:
                content = f.read(build.MAX_FILE + 1)
            total += len(content)
            build.require(len(content) <= build.MAX_FILE and total <= MAX_TOTAL_OBSERVED,
                          'Input bytes exceed bound')
            observed[p.relative_to(root).as_posix()] = content

    if extra_paths:
        for rel_str in sorted(set(extra_paths)):
            if rel_str in observed:
                continue
            build.require(isinstance(rel_str, str) and bool(rel_str), 'Invalid extra path')
            p = Path(rel_str)
            build.require(not p.is_absolute() and '..' not in p.parts and p.as_posix() == rel_str,
                          'Extra consulted path escapes root or is non-canonical')
            target = root / p
            build.no_symlink_chain(target)
            build.require(target.is_file(), f'Consulted file {rel_str} missing or not regular file')
            st = target.stat()
            build.require(st.st_size <= build.MAX_FILE, f'Consulted file {rel_str} exceeds bound')
            build.require(len(observed) < MAX_OBSERVED_FILES, 'Total observed files count exceeds 512')
            with target.open('rb') as f:
                content = f.read(build.MAX_FILE + 1)
            build.require(len(content) <= build.MAX_FILE, f'Consulted file {rel_str} exceeds bound')
            total += len(content)
            build.require(total <= MAX_TOTAL_OBSERVED, 'Total observed input bytes exceed 2.5MB budget')
            observed[rel_str] = content

    return dict(sorted(observed.items()))


def load_node_statuses(path, data):
    """Caller-supplied presentation only. Never follow source locators/URIs.

    v1 digests bind the complete parsed canonical record via build.compact UTF-8
    (reader-canonical-json-v1, NOT Core's RFC8785 digest or a file-byte hash).
    Source record bindings prevent changed evidence metadata being reused silently.
    """
    raw = read_bounded(build.check_lexical_path(path))
    value = strict_json(raw)
    require = build.require
    require(isinstance(value, dict) and set(value) == {'version', 'project_id', 'entries'},
            'Node status envelope fields rejected')
    require(type(value['version']) is int and value['version'] == 1
            and value['project_id'] == data['project']['id'], 'Node status project/version mismatch')
    entries = value['entries']
    require(isinstance(entries, dict) and len(entries) <= 100, 'Node status entry bound rejected')
    nodes = {n['id']: n for n in data['graph']['nodes']}
    records = data['records']
    fields = {'revision_id', 'canonical_digest', 'status', 'label', 'reason', 'assessed_at', 'source_refs'}
    labels = {'historical': {'历史方案', '历史记录'}, 'superseded': {'已替代'}, 'current': {'材料声明现用'}}

    def bounded_text(s, limit):
        return isinstance(s, str) and bool(s.strip()) and len(s) <= limit

    def bound_record(rid, digest):
        require(isinstance(rid, str) and rid in records, 'Node status referenced ID missing')
        require(isinstance(digest, str) and re.fullmatch(r'sha256:[0-9a-f]{64}', digest)
                and digest == build.sha(build.compact(records[rid]).encode('utf-8')),
                'Node status canonical digest mismatch')

    for rid, entry in entries.items():
        require(rid in nodes and isinstance(entry, dict) and set(entry) == fields,
                'Node status ID/fields rejected')
        require(entry['revision_id'] == rid, 'Node status exact revision mismatch')
        bound_record(rid, entry['canonical_digest'])
        status = entry['status']
        require(isinstance(status, str) and status in labels
                and isinstance(entry['label'], str) and entry['label'] in labels[status],
                'Node status enum/label rejected')
        require(bounded_text(entry['reason'], 2000), 'Node status reason required')
        stamp = entry['assessed_at']
        require(isinstance(stamp, str) and len(stamp) <= 40
                and re.fullmatch(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})', stamp),
                'Node status timestamp rejected')
        require(datetime.fromisoformat(stamp.replace('Z', '+00:00')).tzinfo is not None,
                'Node status assessment timezone required')
        refs = entry['source_refs']
        require(isinstance(refs, list) and 1 <= len(refs) <= 8, 'Node status evidence required/bounded')
        seen = set()
        for ref in refs:
            require(isinstance(ref, dict) and set(ref) == {'id', 'canonical_digest', 'locator'},
                    'Node status source fields rejected')
            bound_record(ref['id'], ref['canonical_digest'])
            require(records[ref['id']]['schema'] in ('rp/artifact-manifest/v1', 'rp/external-reference/v1')
                    and ref['id'] not in seen and bounded_text(ref['locator'], 1000),
                    'Node status source reference rejected')
            seen.add(ref['id'])
    # Validate the WHOLE mapping before projecting anything; raw records unchanged.
    for rid, entry in entries.items():
        nodes[rid]['presentationStatus'] = entry
    return raw


def load_generic_project(root, rp, thread_id=None, as_of=None, include_local_sources=False, runner=subprocess.run):
    root = build.check_lexical_path(root)
    rp = build.check_lexical_path(rp)
    rp_sha = build.sha(rp.read_bytes())
    observed = observe_contained(root)

    # Core's `snapshot` command already runs `load_valid_index`, which validates the
    # entire project with full budget, schemas, access closure, and graph semantics.
    # Running a duplicate `rp validate` subprocess on the same unchanged input is omitted.
    snap_cmd = [str(rp), 'snapshot', '--project', str(root), '--json']
    if as_of:
        snap_cmd.extend(['--as-of', str(as_of)])
    res_snap = runner(snap_cmd, capture_output=True, timeout=20)
    build.require(res_snap.returncode == 0, 'rp snapshot failed')
    build.require(len(res_snap.stdout) <= build.MAX_TOTAL, 'rp snapshot output exceeds bound')
    dto_snap = json.loads(res_snap.stdout)
    build.require(dto_snap.get('status') == 'ok' and dto_snap.get('exit_code') == 0
                  and dto_snap.get('findings') == [], 'rp snapshot rejected project')

    snap = dto_snap['data']
    records = snap['objects']
    threads = snap['threads']
    project_rec = snap['project']

    if thread_id is not None:
        build.require(any(t['id'] == thread_id for t in threads), f'Specified thread {thread_id} not in project')
        chosen_thread = thread_id
    else:
        build.require(len(threads) == 1,
                      f'Project has {len(threads)} threads; --thread is required to disambiguate')
        chosen_thread = threads[0]['id']

    node_count = sum(r.get('schema') == NODE for r in records.values())
    rel_count = sum(r.get('schema') == 'rp/scientific-relation-revision/v1' for r in records.values())
    build.require(node_count <= 100, f'Project exceeds 100 semantic nodes ({node_count})')
    build.require(rel_count <= 300, f'Project exceeds 300 scientific relations ({rel_count})')

    sources = {}
    consulted_extra = {}
    cumulative_excerpt_bytes = 0

    for aid, r in records.items():
        if r.get('schema') == 'rp/artifact-manifest/v1':
            uri = r.get('uri', '')
            if not include_local_sources:
                sources[aid] = dict(artifact_id=aid, title=r.get('title', aid),
                                    sha256=r.get('sha256', ''), size_bytes=r.get('size_bytes', 0),
                                    original_path=uri, sections=[],
                                    excerpt='[元数据引用；未包含本地全文摘录]',
                                    verified=False, source_type='metadata-only')
            elif uri.startswith(('http://', 'https://')):
                sources[aid] = dict(artifact_id=aid, title=r.get('title', aid),
                                    sha256=r.get('sha256', ''), size_bytes=r.get('size_bytes', 0),
                                    original_path=uri, sections=[],
                                    excerpt='[外部 URI 引用，未执行网络检索]',
                                    verified=False, source_type='external-uri')
            elif uri.startswith('file:'):
                rel_str = uri[5:]
                build.require(bool(rel_str) and not rel_str.startswith('/') and '\\' not in rel_str and ':' not in rel_str,
                              'Artifact file: URI must use contained relative canonical spelling without leading slash or backslash')
                p = Path(rel_str)
                build.require(not p.is_absolute() and '..' not in p.parts and p.as_posix() == rel_str
                              and not any(part in ('.', '..', '') for part in p.parts),
                              'Artifact path escapes project root or is non-canonical')
                target_file = root / p
                build.no_symlink_chain(target_file)
                build.require(target_file.is_file(), f'Artifact file {rel_str} missing or not regular file')
                declared_size = r.get('size_bytes')
                build.require(type(declared_size) is int and 0 <= declared_size <= build.MAX_FILE,
                              'Artifact declared size invalid')
                build.require(cumulative_excerpt_bytes + declared_size <= 512_000,
                              'Cumulative local sources excerpt budget (512KB) exceeded before read')
                st = target_file.stat()
                build.require(st.st_size <= build.MAX_FILE, f'Artifact file {rel_str} exceeds 64KB bound')
                build.require(st.st_size == declared_size, f'Artifact file {rel_str} size mismatch')
                build.require(cumulative_excerpt_bytes + st.st_size <= 512_000,
                              'Cumulative local sources excerpt budget (512KB) exceeded before read')
                with target_file.open('rb') as f:
                    raw_bytes = f.read(build.MAX_FILE + 1)
                build.require(len(raw_bytes) <= build.MAX_FILE, f'Artifact file {rel_str} exceeds 64KB bound')
                build.require(len(raw_bytes) == declared_size and len(raw_bytes) == st.st_size,
                              f'Artifact file {rel_str} read length mismatch')
                build.require(build.sha(raw_bytes) == r.get('sha256'), f'Artifact file {rel_str} digest mismatch')
                cumulative_excerpt_bytes += len(raw_bytes)
                try:
                    decoded_text = raw_bytes.decode('utf-8')
                    excerpt = build.excerpt_text(decoded_text)
                    is_text = True
                except UnicodeDecodeError:
                    excerpt = '[二进制或非UTF-8文件，省略文本摘录]'
                    is_text = False
                consulted_extra[p.as_posix()] = raw_bytes
                sources[aid] = dict(artifact_id=aid, title=r.get('title', aid),
                                    sha256=r.get('sha256', ''), size_bytes=r.get('size_bytes', 0),
                                    original_path=uri, sections=[],
                                    excerpt=excerpt, verified=True, is_text=is_text,
                                    source_type='local-file')
            else:
                sources[aid] = dict(artifact_id=aid, title=r.get('title', aid),
                                    sha256=r.get('sha256', ''), size_bytes=r.get('size_bytes', 0),
                                    original_path=uri, sections=[],
                                    excerpt='[未识别或不支持的本地 URI 协议]',
                                    verified=False, source_type='unsupported-uri')

    observed.update(consulted_extra)
    observed = dict(sorted(observed.items()))

    # Verify input unchanged before projection
    current_observed = observe_contained(root, extra_paths=list(consulted_extra.keys()))
    build.require(current_observed == observed, 'Input files changed before projection')

    labels = {rid: r.get('title', rid) for rid, r in records.items() if r.get('schema') == NODE}
    graph = project(list(records.values()), chosen_thread, labels=labels, names=[], case_id=None,
                    heads_override=snap.get('heads'),
                    relation_heads_override=snap.get('relation_heads'),
                    assessments_override=snap.get('assessments'),
                    freshness_override=snap.get('freshness'))

    # Final check of inputs and rp binary
    current_observed = observe_contained(root, extra_paths=list(consulted_extra.keys()))
    build.require(current_observed == observed, 'Input files changed during projection')
    build.require(build.sha(rp.read_bytes()) == rp_sha, 'rp binary modified during loading')

    result = dict(records=records, graph=graph, sources=sources, notes={}, names=[],
                  project=project_rec, thread=chosen_thread,
                  snapshot=snap, observed=observed, rp_sha256=rp_sha)
    return result



def admission_path(case_id):
    build.require(case_id in ('csp-startup-v1', 'csp-writing-r2'), 'Unknown case admission')
    return ADMISSION if case_id == 'csp-startup-v1' else ADMISSION_R2


def read_bounded(path):
    path = Path(path)
    build.no_symlink_chain(path)
    build.require(path.is_file() and path.stat().st_size <= build.MAX_FILE, 'Manifest size/type rejected')
    with path.open('rb') as f:
        data = f.read(build.MAX_FILE+1)
    build.require(len(data) <= build.MAX_FILE, 'Manifest exceeds bound')
    return data


def strict_json(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            build.require(key not in result, 'Duplicate JSON key')
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=unique)


def local_copy(observed, path):
    build.require(isinstance(path, str) and bool(path), 'Invalid local path')
    p = Path(path)
    build.require(not p.is_absolute() and '..' not in p.parts and p.as_posix() == path
                  and path in observed, 'Unsafe or unmapped local path')
    return observed[path]


def manifest(path):
    raw = read_bounded(path)
    m = strict_json(raw)
    build.require(isinstance(m, dict), 'Case manifest object required')
    admission_raw = read_bounded(admission_path(m.get('case_id')))
    admission = strict_json(admission_raw)
    build.require(admission.get('case_id') == m['case_id']
                  and build.sha(raw) == admission.get('manifest_sha256'), 'Case admission digest mismatch')
    fields = {'schema','case_id','mode','project_id','thread','as_of','rp_path','rp_sha256',
              'inventory','canonical_count','schema_counts','name_map','sources'}
    build.require(isinstance(m, dict) and set(m) == fields, 'Case manifest schema fields rejected')
    build.require(m['schema'] == 'rp/pilot-case-manifest/v1' and m['case_id'] in ('csp-startup-v1', 'csp-writing-r2')
                  and m['mode'] == 'single-snapshot', 'Only admitted CSP single-snapshot case supported')
    for field,prefix in [('project_id','proj'),('thread','thd')]:
        build.require(isinstance(m[field],str) and re.fullmatch(prefix+r'_[0-9A-HJKMNP-TV-Z]{26}',m[field]),
                      'Invalid manifest identity')
    for field in ('rp_sha256','inventory'):
        build.require(isinstance(m[field],str) and re.fullmatch(r'sha256:[0-9a-f]{64}',m[field]), 'Invalid manifest digest')
    build.require(type(m['canonical_count']) is int and 1 <= m['canonical_count'] <= build.MAX_FILES,
                  'Manifest count rejected')
    build.require(isinstance(m['schema_counts'],dict) and bool(m['schema_counts'])
                  and set(m['schema_counts']) <= build.SCHEMAS | {EXTRA_SCHEMA}
                  and all(type(n) is int and n > 0 for n in m['schema_counts'].values())
                  and sum(m['schema_counts'].values()) == m['canonical_count'], 'Manifest schema counts rejected')
    build.require(isinstance(m['sources'],dict) and len(m['sources']) <= 32, 'Source mappings exceed bound')
    build.require(datetime.fromisoformat(m['as_of'].replace('Z','+00:00')).tzinfo is not None,
                  'Explicit snapshot timezone required')
    return m, raw, admission_raw


def load_case(root, manifest_path, rp, runner=subprocess.run):
    root, manifest_path, rp = map(Path, (root,manifest_path,rp))
    m, raw, admission_raw = manifest(manifest_path)
    build.no_symlink_chain(rp)
    build.require(str(rp) == m['rp_path'] and build.sha(rp.read_bytes()) == m['rp_sha256'],
                  'Pinned rp executable path/digest mismatch')
    observed = build.observe(root)
    build.require(build.identity(observed) == m['inventory'], 'Admitted case inventory mismatch')
    validation = build.validate(rp,root,m['canonical_count'],runner)
    build.unchanged(root,observed)
    # Parse canonical copies ONLY after full rp validation and complete reobservation.
    records = build.canonical(observed, schemas=build.SCHEMAS | {EXTRA_SCHEMA})
    build.require(len(records) == m['canonical_count']
                  and dict(Counter(r['schema'] for r in records.values())) == m['schema_counts'],
                  'Canonical schema counts mismatch')
    build.require([r['id'] for r in records.values() if r['schema']=='rp/project/v1'] == [m['project_id']],
                  'Wrong project identity')
    build.require([r['id'] for r in records.values() if r['schema']=='rp/research-thread/v1'] == [m['thread']],
                  'Expected one matching thread')
    names = strict_json(local_copy(observed,m['name_map']))
    build.require(isinstance(names,list) and len(names) <= 100, 'Name map bound rejected')
    aliases, ids = set(), set()
    multi = m['case_id'] == 'csp-writing-r2'
    nodes = [r for r in records.values() if r['schema'] == NODE]
    node_heads = heads(nodes)
    for n in names:
        build.require(isinstance(n,dict) and set(n)=={'alias','id','name'}, 'Name map entry rejected')
        build.require(isinstance(n['alias'],str) and re.fullmatch(r'[A-Z][0-9]{2}(?:@r[1-9][0-9]*)?' if multi else r'[A-Z][0-9]{2}',n['alias'])
                      and n['alias'] not in aliases, 'Duplicate or invalid alias')
        build.require(n['id'] in records and records[n['id']]['schema']==NODE and (multi or n['id'] not in ids),
                      'Name map exact ID rejected')
        build.require(isinstance(n['name'],str) and 0 < len(n['name']) <= 160, 'Name bound rejected')
        aliases.add(n['alias']); ids.add(n['id'])
    build.require(node_heads <= ids if multi else ids == {r['id'] for r in nodes},
                  'Incomplete node name map')
    sources, notes = {}, {}
    for s in m['sources'].values():
        aid=s['artifact_id']
        build.require(aid not in sources and aid in records, 'Duplicate/unknown mapped artifact')
        artifact=records[aid]
        build.require(artifact['schema']=='rp/artifact-manifest/v1' and artifact['sha256']==s['sha256']
                      and artifact['size_bytes']==s['size_bytes'], 'Artifact declaration mismatch')
        local_copy(observed,s['copied_path'])
        excerpt=build.verify_copy(observed,s['copied_path'],s['sha256'],s['size_bytes'])
        build.require(isinstance(s['original_path'],str) and isinstance(s['sections'],list)
                      and bool(s['sections']) and len(s['sections'])<=12, 'Source locators rejected')
        for section in s['sections']:
            build.require(isinstance(section.get('heading'),str) and type(section.get('start_line')) is int
                          and type(section.get('end_line')) is int
                          and 1 <= section['start_line'] <= section['end_line'], 'Source line locator rejected')
        sources[aid]=dict(s,title=artifact['title'],excerpt=excerpt)
    build.require(set(sources)=={r['id'] for r in records.values() if r['schema']=='rp/artifact-manifest/v1'},
                  'Unmapped artifact')
    for rid,r in records.items():
        build.require(all(a in sources for a in r.get('source',{}).get('artifacts',[])), 'Unmapped artifact source')
        if 'narrative' in r:
            n=r['narrative']; local_copy(observed,n['path'])
            notes[rid]=build.verify_copy(observed,n['path'],n['sha256'])
    labels = presentation_labels(records, names, m['case_id'])
    graph=project(list(records.values()),m['thread'],labels,names,m['case_id'])
    result=dict(records=records,graph=graph,sources=sources,notes=notes,names=names,
                manifest=m,validation=validation,observed=observed,
                manifest_bytes=raw,admission_bytes=admission_raw)
    recheck(result,root,manifest_path,rp)
    return result


def recheck(data,root,manifest_path,rp):
    build.unchanged(root,data['observed'])
    build.require(read_bounded(manifest_path)==data['manifest_bytes']
                  and read_bounded(admission_path(data['manifest']['case_id']))==data['admission_bytes'], 'Case admission/manifest changed')
    build.require(build.sha(Path(rp).read_bytes())==data['manifest']['rp_sha256'], 'rp binary changed')
