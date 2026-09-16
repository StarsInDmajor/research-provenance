#!/usr/bin/env python3
"""Private pinned pilot SVG reader. Standard library; no general YAML support.

rp validates the on-disk corpora first. Only this reviewed JSON-compatible YAML
case is then JSON-parsed from bounded observed byte copies. Repeated complete
inventories detect observed changes, not atomic snapshots or adversarial TOCTOU.
"""
import argparse
import base64
from datetime import datetime
import hashlib
from html import escape
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile

from graph_projection import project, visible, NODE, RELATION, BINDING, EDGE_STYLES, edge_style, wrap_text

HERE = Path(__file__).resolve().parent
MAX_FILE = 65536
MAX_TOTAL = 2_000_000
MAX_FILES = 512
FRESHNESS_LABELS = {'unknown': '时效未判定', 'fresh': '时效正常', 'review-due': '待复核', 'stale': '已过保'}
SCHEMAS = {NODE, RELATION, BINDING, 'rp/project/v1',
           'rp/research-thread/v1', 'rp/artifact-manifest/v1'}
LABELS = dict(zip([
    'qst_00000000000000000000000010', 'hyp_00000000000000000000000011',
    'hyp_00000000000000000000000012', 'obs_00000000000000000000000013',
    'obs_00000000000000000000000014', 'obs_00000000000000000000000015',
    'dec_00000000000000000000000016', 'blk_00000000000000000000000017',
    'blk_00000000000000000000000018', 'nxt_00000000000000000000000019',
    'nxt_00000000000000000000000020'], [
    '检查点能否帮助恢复工作？', '先小范围试用现有工具', '先补保护，再收集反馈',
    '历史内存峰值超上限', '校验与报告绑定已有修复', '校验库内部资源边界未齐',
    '有限试用，不降低验收要求', '历史内存门槛未通过', '内部资源保证尚不完整',
    '准备一个带来源的检查点', '阅读提案，追溯并收集纠正']))


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return 'sha256:' + hashlib.sha256(data).hexdigest()


def compact(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'))


def script_json(value):
    return compact(value).replace('&', '\\u0026').replace('<', '\\u003c').replace('>', '\\u003e').replace('\u2028', '\\u2028').replace('\u2029', '\\u2029')


def text(value):
    return escape(str(value), quote=True)


def excerpt_text(value):
    # Display-only path redaction. Hashes always describe the unmodified bytes.
    value = value.replace('/home/pulcerto/nixos-config/', '')
    return re.sub(r'/(?:home|tmp|root|Users)/[^\s`"<>]+', '[私有路径省略]', value)


def no_symlink_chain(path):
    path = Path(path)
    require('..' not in path.parts, 'Path with .. component rejected')
    require(path.is_absolute(), 'Absolute path required')
    for part in [*reversed(path.parents), path]:
        require(not part.is_symlink(), 'Unsafe symlink path')


def check_lexical_path(raw):
    p = Path(raw)
    require('..' not in p.parts, 'Path with .. component rejected before normalization')
    if not p.is_absolute():
        p = Path(os.getcwd()) / p
    require('..' not in p.parts, 'Path with .. component rejected')
    no_symlink_chain(p)
    return p


def observe(root):
    root = Path(root)
    no_symlink_chain(root)
    require(root.is_dir(), 'Input directory missing')
    observed, total, entries = {}, 0, 0
    for current, dirs, files in os.walk(root, followlinks=False):
        for name in sorted(dirs+files):
            p = Path(current)/name
            entries += 1
            require(entries <= MAX_FILES*2, 'Too many input entries')
            mode = p.lstat().st_mode
            require(not stat.S_ISLNK(mode), 'Input symlink rejected')
            require(stat.S_ISDIR(mode) or stat.S_ISREG(mode), 'Special input file rejected')
            if stat.S_ISDIR(mode):
                continue
            require(len(observed) < MAX_FILES and p.stat().st_size <= MAX_FILE,
                    'Input file count/size exceeds private reader bound')
            with p.open('rb') as f:
                content = f.read(MAX_FILE+1)
            total += len(content)
            require(len(content) <= MAX_FILE and total <= MAX_TOTAL, 'Input bytes exceed bound')
            observed[p.relative_to(root).as_posix()] = content
    return dict(sorted(observed.items()))


def inventory(observed):
    return {name: sha(data) for name, data in sorted(observed.items())}


def identity(observed):
    return sha(compact(inventory(observed)).encode())


def unchanged(root, observed):
    require(observe(root) == observed, 'Input inventory changed during build')


def verify_copy(observed, name, digest, size=None):
    p = Path(name)
    require(not p.is_absolute() and '..' not in p.parts and name in observed,
            'Unreviewed or unsafe source path')
    content = observed[name]
    require(len(content) <= MAX_FILE and sha(content) == digest, 'Source digest mismatch')
    if size is not None:
        require(len(content) == size, 'Source size mismatch')
    return content.decode('utf-8')


def canonical(observed, schemas=None):
    records = {}
    for name, data in observed.items():
        if not (name.startswith('.research/') and name.endswith('.yaml')):
            continue
        # rp already checked duplicate keys and schema. Deliberately not a YAML loader.
        r = json.loads(data)
        require(r.get('schema') in (SCHEMAS if schemas is None else schemas), 'Not the supported pinned pilot schemas')
        rid = r['id']
        require(re.fullmatch(r'[a-z]+_[A-Za-z0-9]+', rid) and rid not in records,
                'Invalid/duplicate exact canonical ID')
        records[rid] = r
    return records


def validate(exe, root, count, runner=subprocess.run):
    result = runner([str(exe), 'validate', '--project', str(root), '--json'],
                    capture_output=True, timeout=20)
    require(result.returncode == 0, 'rp validation rejected input; diagnostic body withheld')
    dto = json.loads(result.stdout)
    require(dto.get('status') == 'ok' and dto.get('exit_code') == 0
            and dto.get('findings') == [] and dto.get('data', {}).get('valid') is True
            and dto['data'].get('canonical_object_count') == count,
            'rp validation DTO rejected')
    return dict(canonical_objects=count, findings=0, returncode=0,
                stdout_sha256=sha(result.stdout), stderr_sha256=sha(result.stderr))


def load_data(before, after, pins):
    for name, observed in [('before', before), ('after', after)]:
        require(identity(observed) == pins['inventories'][name], 'Pinned pilot inventory mismatch')
    require(all(after.get(k) == v for k, v in before.items()), 'Before files not preserved')
    old, records = canonical(before), canonical(after)
    require(len(old) == 32 and len(records) == 35, 'Wrong pinned canonical counts')
    require(sum(r['schema'] == 'rp/research-thread/v1' for r in records.values()) == 1,
            'Pinned pilot requires one thread')
    sources, notes = {}, {}
    for source in pins['sources'].values():
        aid = source['artifact_id']
        artifact = records[aid]
        require(artifact['sha256'] == source['sha256'] and artifact['size_bytes'] == source['size_bytes'],
                'Artifact declaration mismatch')
        # Only reviewed pin mapping, never follow artifact.uri.
        excerpt = verify_copy(after, source['copied_path'], source['sha256'], source['size_bytes'])
        sources[aid] = dict(source, title=artifact['title'], excerpt=excerpt_text(excerpt))
    for rid, r in records.items():
        require(all(aid in sources for aid in r.get('source', {}).get('artifacts', [])),
                'Unmapped artifact source')
        if 'narrative' in r:
            n = r['narrative']
            notes[rid] = excerpt_text(verify_copy(after, n['path'], n['sha256']))
    graph = project(list(records.values()), pins['thread'], LABELS)
    view = visible(graph)
    require(len(view['nodes']) == 10 and len(view['edges']) == 5 and len(graph['nodes']) == 11,
            'Pinned graph count mismatch')
    require(len(graph['bindingHeads']) == 11, 'Pinned binding heads mismatch')
    return dict(graph=graph, sources=sources, notes=notes, records=records)


def is_own_artifact(path):
    path = Path(path)
    if not path.is_file():
        return False
    try:
        with path.open('r', encoding='utf-8', errors='ignore') as f:
            prefix = f.read(32768)
        return 'data-app-state="loading"' in prefix and 'id="workspace"' in prefix
    except OSError:
        return False


def check_output(output, inputs, force=False, legacy=True):
    output = check_lexical_path(output)
    if legacy:
        require(output.name in {'reader-v3.html', 'reader-v4.html', 'reader-v5.html', 'csp-reader.html'},
                'Only explicit legacy reader names or csp-reader.html may be replaced')
    else:
        require(output.suffix == '.html' and len(output.name) >= 6 and not output.name.startswith('.'),
                'Sensible HTML output filename required')
    for source in inputs:
        source = check_lexical_path(source)
        require(not output.resolve().is_relative_to(source.resolve()) and not source.resolve().is_relative_to(output.resolve()),
                'Output overlaps input or source directory')
    missing, parent = [], output.parent
    while not parent.exists():
        missing.append(parent)
        parent = parent.parent
    for p in reversed(missing):
        p.mkdir(mode=0o700)
    # Existing shared ancestors (e.g. ~/output) are never chmodded. Only the
    # dedicated immediate output directory must already be private or be new.
    require(output.parent.is_dir() and stat.S_IMODE(output.parent.stat().st_mode) == 0o700
            and output.parent.stat().st_uid == os.getuid(), 'Output directory must be owned private 0700')
    if output.exists():
        s = output.stat()
        require(stat.S_ISREG(s.st_mode) and s.st_nlink == 1 and s.st_uid == os.getuid()
                and stat.S_IMODE(s.st_mode) == 0o600, 'Existing output must be private regular 0600')
        if not force:
            require(is_own_artifact(output) or output.name in {'reader-v3.html', 'reader-v4.html', 'reader-v5.html', 'csp-reader.html'},
                    'Existing output file is not a verified reader artifact; pass --force to overwrite')
    return output


def atomic_write(output, html, final_check):
    temp = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', dir=output.parent,
                                         prefix='.'+output.stem+'-', suffix='.tmp', delete=False) as f:
            temp = Path(f.name)
            os.fchmod(f.fileno(), 0o600)
            f.write(html)
            f.flush()
            os.fsync(f.fileno())
        final_check()
        no_symlink_chain(output)
        os.replace(temp, output)
        temp = None
    finally:
        if temp is not None:
            temp.unlink(missing_ok=True)


def node_history(n):
    """Three independent axes: revision head, explicit content status, freshness."""
    status = n.get('presentationStatus')
    historical = status is not None and status['status'] in ('historical', 'superseded')
    revision = '历史端点(当前引用)' if n['ghost'] else '旧修订' if not n['current'] else '最新记录修订'
    content = ('描述' + status['label']) if historical else status['label'] if status else '内容状态待核实'
    return (not n['current'] or n['ghost'] or historical), revision + '·' + content


def render_svg(graph):
    current = visible(graph)
    ids = {n['id'] for n in current['nodes']}
    eids = {e['id'] for e in current['edges']}
    out = [f'<svg id="graph" viewBox="0 0 {graph["width"]} {graph["height"]}" '
           'preserveAspectRatio="xMidYMid meet" role="group" aria-label="研究节点图，可用 Tab 与 Enter 选中记录">',
           '<title>精确修订节点与已记录关系</title><defs>']
    for key, (_, color, _) in EDGE_STYLES.items():
        out.append(f'<marker id="arrow-{key}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M 0 0 L 10 5 L 0 10 z" fill="{color}"/></marker>')
    for i, _ in enumerate(graph['nodes']):
        out.append(f'<clipPath id="node-clip-{i}" clipPathUnits="userSpaceOnUse"><rect x="12" y="10" width="246" height="98"/></clipPath>')
    out.append('</defs>')
    for e in graph['edges']+graph['revisionEdges']:
        revision = e['category'] == 'revision'
        hidden = ' is-hidden' if revision or e['id'] not in eids else ''
        key, (_, color, dash) = edge_style(e)
        pattern = f' stroke-dasharray="{dash}"' if dash else ''
        invalidated = not revision and e['raw'].get('relation_state') == 'invalidated'
        label = e['label'] + (' · 已撤销（历史）' if invalidated else ' · 历史' if not revision and not e['current'] else '')
        label_status = 'hidden' if e.get('routeStatus') == 'unroutable' else e.get('labelStatus', 'hidden')
        label_visibility = f' visibility="{"visible" if label_status == "placed" else "hidden"}"'
        label_note = ' · 标签暂隐，请查看详情' if label_status == 'hidden' else ''
        route_status = text(e.get('routeStatus', 'routed'))
        out.append(f'<g class="edge {"revision" if revision else "science"} type-{key}{hidden}{" invalidated" if invalidated else ""}" data-key="{text(e["id"])}" data-from="{text(e["from"])}" data-to="{text(e["to"])}" tabindex="{ -1 if hidden else 0}" role="button" aria-label="{text(label+": "+e["from"]+" → "+e["to"]+label_note)}" data-route-status="{route_status}" data-label-status="{label_status}">'
                   f'<title>{text(label+" · "+e["id"]+label_note)}</title><path class="edge-hit" d="{text(e["path"])}"/>'
                   f'<path class="edge-line" d="{text(e["path"])}" stroke="{color}"{pattern} marker-end="url(#arrow-{key})"/>'
                   f'<path class="edge-label-leader" d="{text(e.get("labelLeaderPath", ""))}" stroke="{color}"{label_visibility}/>'
                   f'<rect class="edge-label-bg" x="{e["labelX"]-e.get("labelHalfWidth", 0)}" y="{e["labelY"]-20}" width="{2*e.get("labelHalfWidth", 0)}" height="28" rx="3" fill="#fffdf5" stroke="{color}"{label_visibility}/>'
                   f'<text x="{e["labelX"]}" y="{e["labelY"]}" class="edge-label"{label_visibility}>{text(label)}</text></g>')
    for i, n in enumerate(graph['nodes']):
        hidden = ' is-hidden' if n['id'] not in ids else ''
        role = ' / '.join(graph['roles'].get(r, r) for r in n['roles']) or '未绑定角色'
        flags = (' · 分叉' if n['fork'] else '') + (' · 角色冲突' if n['roleConflict'] else '')
        dashed, history_label = node_history(n)
        flags += ' · ' + history_label
        full = n['label']+' · 原始标题：'+n['title']+' · '+role+flags+' · '+n['id']
        out.append(f'<g class="node {text(n["kind"])}{hidden}{" ghost" if n["ghost"] else ""}{" history-mark" if dashed else ""}" data-key="{text(n["id"])}" transform="translate({n["x"]} {n["y"]})" tabindex="{-1 if hidden else 0}" role="button" aria-label="{text(n["typeLabel"]+"："+full)}">'
                   f'<title>{text(full)}</title><rect class="node-card" width="270" height="114" rx="10"/>'
                   f'<g clip-path="url(#node-clip-{i})">')
        rows = [('node-type', n['typeLabel'] + (' · ' + n['presentationStatus']['label'] if n.get('presentationStatus') else ''), 12, 26, 1),
                ('node-label', n['label'], 15, 50, 2),
                ('node-role', role+flags, 10, 88, 1)]
        if n.get('assessments'):
            iso_tag = ' · 未记录连接' if n['isolated'] else ''
            status_desc = f"评价记录 ({len(n['assessments'])}){iso_tag} · {FRESHNESS_LABELS.get(n.get('freshness'), '时效未判定')}"
        elif n['isolated']:
            status_desc = '未记录连接'
        else:
            status_desc = f"未评价 · {FRESHNESS_LABELS.get(n.get('freshness'), '时效未判定')}"
        rows.append(('node-status', status_desc, 9, 104, 1))
        for cls, value, size, y, count in rows:
            for line, fragment in enumerate(wrap_text(value, size=size, lines=count)):
                out.append(f'<text class="{cls}" x="16" y="{y+line*20}">{text(fragment)}</text>')
        out.append('</g></g>')
    out.append('</svg>')
    return ''.join(out)


def reading_guide(graph):
    out = ['<p>从问题开始是建议入口，不是完整图的时间根或层级根；多个问题请自行选择。未记录科研连接不表示没有来源。</p>',
           '<p>阅读布局大致从左上到右下：依据 / 依赖对象在先，使用它的记录在后。位置不是时间或因果；原始箭头仍表示其已记录类型与方向。其他类型采用原方向布局后备，不统一解释为来源→结果。循环与并存修订全部保留；筛选不移动节点。</p>']
    out.append('<h3>边框 · 修订 / 内容历史 / 时效分别阅读</h3><p>虚线：旧节点修订（含历史端点(当前引用)），或有明确材料依据的历史方案 / 历史记录 / 已替代内容。最新记录修订也可能描述历史方案，不表示当前采用；历史记录不是科学结果为假或无效。</p><p>实线仅表示没有显式历史标记，不表示确认有效。未映射内容状态待核实；unknown 时效、frozen 冻结、候选未晋升、延期或计划均不自动判为过时。材料声明现用也只是注明日期的材料判断，不是全项目当前采用握手或人的批准。</p><p>详情中的展示判断 / 原因 / 来源 / assessed_at 属于调用者显式提供的私有展示映射，不是 Core Assessment 或科学生命周期 schema。assessed_at 是材料核对时间；--as-of 单独计算时效，二者均不是科学事件日期。来源定位文字不执行 URI。历史内容标记不增删节点、边或筛选集合。</p>')
    meanings = {
        'Question': ('问题', '希望弄清楚什么'),
        'Hypothesis': ('假设', '待检验的可能解释'),
        'Prediction': ('预测', '假设预期出现的结果'),
        'Method': ('方法', '采用什么分析或处理方式'),
        'Test': ('检验', '如何检查假设或方法'),
        'Observation': ('观察', '实际观察或材料记载的内容'),
        'Measurement': ('测量', '有测量方法与条件的结果'),
        'Dataset': ('数据集', '分析使用的数据及范围'),
        'Interpretation': ('解释', '如何理解已有现象或结果'),
        'Synthesis': ('综合', '如何整合多项依据'),
        'Conclusion': ('结论', '在限定范围内形成的判断'),
        'Decision': ('决定', '选择了什么方案'),
        'Blocker': ('阻塞', '什么条件妨碍哪个目标'),
        'NextAction': ('下一行动', '接下来具体要做什么'),
        'PaperClaim': ('论文主张', '论文中提出或准备表达的主张'),
    }
    out.append('<h3>节点分类 · 英文类型与 ID 字母前缀</h3><ul>')
    kinds = sorted({n['kind'] for n in graph['nodes']})
    for kind in kinds:
        chinese, meaning = meanings.get(kind, (kind, '此类型暂无中文说明，详见原始记录'))
        prefixes = sorted({n['id'].split('_', 1)[0]+'_' for n in graph['nodes']
                           if n['kind'] == kind and '_' in n['id']})
        out.append('<li><strong>'+text(kind+' · '+chinese)+'</strong> — '+text(meaning)
                   +('；ID 前缀：'+text(' / '.join(prefixes)) if prefixes else '')+'</li>')
    out.append('</ul><p>类型描述记录的作用，不表示可信度、重要性或是否通过验收。</p>')
    numbered = [n for n in graph['nodes'] if re.match(r'^R[0-9]+(?:\s|$)', n['label'])]
    if numbered:
        example = min(n['label'].split()[0] for n in numbered)
        out.append('<p>'+text(example)+' 等 R 加数字是本图的阅读编号，不是节点类型；'
                   '编号大小也不代表时间、因果或重要性。实际分类以节点类型字段为准。</p>')
    if graph.get('aliasLegend'):
        out.append('<p>字母是本案例别名，不是节点 kind 类型代码；编号稳定不变。</p><ul>')
        out.extend('<li>'+text(line)+'</li>' for line in graph['aliasLegend'])
        out.append('</ul>')
    out.append('<p>箭头保持原始 from → to。推导自/依据的终点是依据；依赖的终点是前提。颜色与线型只区分关系类型，不表示可信度或已验证。</p><ul class="relation-legend">')
    seen = set()
    for e in graph['edges']+graph['revisionEdges']:
        if (e['category'], e['type']) in seen:
            continue
        seen.add((e['category'], e['type']))
        _, (_, color, dash) = edge_style(e)
        pattern = f' stroke-dasharray="{dash}"' if dash else ''
        out.append(f'<li><svg width="58" height="14" aria-hidden="true"><path d="M 1 7 L 54 7" stroke="{color}" stroke-width="2"{pattern}/></svg> '+text(e['label']+' · '+e['type'])+'</li>')
    out.append('</ul><p>修订：旧 → 新，非科研推导。灰色点线来源卡片 / SOURCE：仅为明确来源引用，非科研边；编写依据不等于行动目标。</p><p>已撤销关系仅在历史层显示“已撤销（历史）”并淡化；线型仍表示原关系类型。角色看文字；未评价与时效未判定独立显示。</p>')
    out.append('<p>节点位置固定；可见集合变化时从有界附近候选中选最短安全路线，不保证全局最优。未找到安全路线的关系暂不画线，仍保留原始详情与搜索；无脚本时亦可在下方原文中查阅。</p>')
    hidden_labels = sum(e.get('routeStatus') == 'routed' and e.get('labelStatus') == 'hidden' for e in visible(graph)['edges'])
    out.append(f'<p>初始视图 {hidden_labels} 条可画线关系的标签暂隐（不是隐藏关系）。标签按精确 ID 从附近有界候选中避让，短引线连接其原始路径；没有安全文字位置时仍画原线、箭头并保留点击目标。可悬停、聚焦、搜索或在详情查阅完整类型与方向；选择不会强行叠加文字。文字范围为近似，不是浏览器字体视觉验证。</p>')
    return ''.join(out)


def wire_data(data):
    """Transport only: canonical records live once in native fallback text.

    Keep projection APIs unchanged. Non-title labels are presentation overrides.
    """
    graph = data['graph']
    nodes = []
    for n in graph['nodes']:
        item = {k: v for k, v in n.items() if k not in ('raw', 'title', 'kind')}
        if n['label'] == n['raw']['title']:
            del item['label']
        nodes.append(item)
    edges = [{k: v for k, v in e.items() if k != 'raw'} for e in graph['edges']]
    wire = {k: v for k, v in data.items() if k not in ('records', 'graph', 'project', 'snapshot', 'observed', 'rp_sha256', 'thread')}
    wire.update(wireVersion=1, graph=graph | dict(nodes=nodes, edges=edges))
    if 'project' in data:
        wire['projectId'] = data['project']['id']
    return wire


def render(data, evidence):
    graph = data['graph']
    css = (HERE/'reader.css').read_text()
    # One renderer script, routing definition before its consumer. The exact
    # concatenated bytes receive their own CSP hash; both sources are inventoried.
    js = '\n'.join((HERE/name).read_text() for name in ('routing.js', 'graph.js', 'svg.js'))
    bootstrap = (HERE/'bootstrap.js').read_text()
    embedded = script_json(wire_data(data))
    hashes = [base64.b64encode(hashlib.sha256(s.encode()).digest()).decode() for s in (embedded, js, bootstrap)]
    csp = "default-src 'none'; script-src " + ' '.join("'sha256-"+h+"'" for h in hashes)
    csp += "; style-src 'unsafe-inline'; img-src 'none'; connect-src 'none'; base-uri 'none'; form-action 'none'; object-src 'none'"
    fallback = '<pre id="canonical-records">' + escape(compact(data['records']), quote=False) + '</pre>'
    source_cards = []
    for aid, source in data['sources'].items():
        source_cards.append('<article class="source-card"><h3>'+text(source['title'])+'</h3><p>'+text(aid)+'</p><p>'+text(source['original_path'])+'</p><pre>'+text(source['excerpt'])+'</pre></article>')
    current = visible(graph)
    single = evidence.get('mode') == 'single-snapshot'
    generic = evidence.get('mode') == 'generic-snapshot'
    corrected = evidence.get('case_id') == 'csp-writing-r2'
    if generic:
        case_meta_text = f"{evidence.get('project_title', '研究项目')} · 只读探索快照"
        bg_text = data.get('project', {}).get('statement', data.get('project', {}).get('description', '本地受信只读探索快照；展示已记录关系，不外推推论。'))
        val_mode_text = f"通用模式：经 rp 完整校验通过，共 {evidence.get('canonical_count', len(data.get('records', {})))} 个规范对象，零 Findings。"
    elif corrected:
        case_meta_text = 'CSP 启动调查 · 撰写修订候选 · 独立单快照，保留旧修订'
        bg_text = '此图重建一条历史故障、诊断与用户反馈线；不是新浏览器实验。候选内容待研究者审阅，记录冻结不表示人的批准。'
        val_mode_text = '单快照：按已准入 case manifest 的对象数和类型清单验证，零 Findings；不存在 before 更新。'
    elif single:
        case_meta_text = 'CSP 启动调查 · Agent 候选 · 单快照，无伪造更新'
        bg_text = '此图重建一条历史故障、诊断与用户反馈线；不是新浏览器实验。候选内容待研究者审阅，记录冻结不表示人的批准。'
        val_mode_text = '单快照：按已准入 case manifest 的对象数和类型清单验证，零 Findings；不存在 before 更新。'
    else:
        case_meta_text = '历史工程试点 · before / after 快照'
        bg_text = '旧试点保留 32 → 35 对象及行动修订；历史阅读安排不覆盖当前图探索目标。'
        val_mode_text = '固定旧试点：验证 before / after 的 32 / 35 个对象、零 Findings。'

    values = dict(CSP=text(csp), CSS=css, JS=js, BOOTSTRAP=bootstrap,
                  DATA_TAG='<script id="graph-data" type="application/json">'+embedded+'</script>',
                  CASE_META=text(case_meta_text),
                  COUNTS=text(f'{len(current["nodes"])} / {len(graph["nodes"])} 节点 · {len(current["edges"])} / {len(graph["edges"])} 科研关系 · 历史修订线未展开'),
                  BACKGROUND=text(bg_text),
                  VALIDATION_MODE=text(val_mode_text),
                  SVG='<svg id="graph" role="group" aria-label="研究节点图；脚本启动后生成"></svg>',
                  GUIDE=reading_guide(graph), FALLBACK=fallback, SOURCES=''.join(source_cards),
                  AS_OF=text(evidence['as_of']), GENERATED_AT=text(evidence['generated_at']),
                  EVIDENCE=text(json.dumps(evidence, ensure_ascii=False, indent=2)))
    template = (HERE/'template.html').read_text()
    # Single substitution pass so untrusted text containing a token isn't reprocessed.
    html = re.sub(r'@@([A-Z_]+)@@', lambda m: values[m[1]], template)
    require(len(html.encode()) < 500_000, 'HTML exceeds 500KB')
    return html


def rebuild(project_root, before_root, rp, output, as_of, generated_at, runner=subprocess.run,
            case_manifest=None):
    if case_manifest is not None:
        require(before_root is None, 'Single snapshot does not accept before')
        return rebuild_case(project_root, rp, output, as_of, generated_at, case_manifest, runner)
    project_root, before_root, rp, output = map(Path, (project_root, before_root, rp, output))
    source_names = ('build.py', 'graph_projection.py', 'routing.py', 'routing.js', 'svg.js', 'pilot-pins.json',
                    'template.html', 'reader.css', 'graph.js', 'bootstrap.js')

    def reader_sources():
        result = {}
        for name in source_names:
            path = HERE/name
            no_symlink_chain(path)
            with path.open('rb') as f:
                result[name] = f.read(MAX_FILE+1)
            require(len(result[name]) <= MAX_FILE, 'Reader source exceeds bound')
        return result

    observed_sources = reader_sources()
    pins_bytes = observed_sources['pilot-pins.json']
    pins = json.loads(pins_bytes)
    for stamp in (as_of, generated_at):
        require(datetime.fromisoformat(stamp.replace('Z', '+00:00')).tzinfo is not None,
                'Explicit timezone timestamp required')
    require(as_of == pins['as_of'], 'Pinned snapshot as_of mismatch')
    require(str(rp) == pins['rp_path'] and sha(rp.read_bytes()) == pins['rp_sha256'],
            'Pinned rp executable path/digest mismatch')
    no_symlink_chain(project_root)
    no_symlink_chain(before_root)
    require(not project_root.is_relative_to(before_root) and not before_root.is_relative_to(project_root),
            'Input roots must be disjoint')
    check_output(output, [project_root, before_root, HERE], legacy=True)
    observed_before, observed_after = observe(before_root), observe(project_root)
    validations = dict(before=validate(rp, before_root, 32, runner),
                       after=validate(rp, project_root, 35, runner))
    unchanged(before_root, observed_before)
    unchanged(project_root, observed_after)
    require(reader_sources() == observed_sources, 'Reader source inventory changed after validation')
    data = load_data(observed_before, observed_after, pins)
    evidence = dict(as_of=as_of, generated_at=generated_at,
                    binary_path=str(rp), binary_sha256=pins['rp_sha256'],
                    pin_file_sha256=sha(pins_bytes), validations=validations,
                    reader_source_inventory=inventory(observed_sources),
                    input_identities=dict(before=identity(observed_before), after=identity(observed_after)),
                    inventories=dict(before=inventory(observed_before), after=inventory(observed_after)),
                    graph_counts=dict(current_nodes=10, current_science_edges=5, historical_nodes=11,
                                      binding_heads=11, artifacts=5),
                    input_files=dict(before=len(observed_before), after=len(observed_after)),
                    visual_acceptance='PENDING: no browser or GUI inspection',
                    snapshot_guarantee='Repeated observed inventories, NOT atomic snapshot or TOCTOU immunity')
    html = render(data, evidence)

    def final_check():
        unchanged(before_root, observed_before)
        unchanged(project_root, observed_after)
        require(reader_sources() == observed_sources, 'Reader source inventory changed')
        require(sha(rp.read_bytes()) == pins['rp_sha256'], 'rp binary changed')
        check_output(output, [project_root, before_root, HERE], legacy=True)

    final_check()
    atomic_write(output, html, final_check)
    return {k:v for k,v in evidence.items() if k != 'inventories'} | dict(html_bytes=len(html.encode()), html_sha256=sha(html.encode()))


def rebuild_case(root, rp, output, as_of, generated_at, manifest_path, runner=subprocess.run):
    import case_io
    root, rp, output, manifest_path = map(Path, (root, rp, output, manifest_path))
    require(output.name == 'csp-reader.html', 'Second case must not replace legacy readers')
    require(datetime.fromisoformat(generated_at.replace('Z', '+00:00')).tzinfo is not None,
            'Explicit generation timezone required')
    admitted, _, _ = case_io.manifest(manifest_path)
    admission = case_io.admission_path(admitted['case_id'])
    names = ('build.py', 'case_io.py', 'graph_projection.py', 'routing.py', 'routing.js', 'svg.js',
             'template.html', 'reader.css', 'graph.js', 'bootstrap.js')
    def reader_sources():
        return {n: case_io.read_bounded(HERE/n) for n in names} | {
            admission.name: case_io.read_bounded(admission)}
    observed_sources = reader_sources()
    data = case_io.load_case(root, manifest_path, rp, runner)
    require(as_of == data['manifest']['as_of'], 'Pinned case as_of mismatch')
    check_output(output, [root, HERE, manifest_path], legacy=True)
    graph = data['graph']; view = visible(graph)
    evidence = dict(mode='single-snapshot', case_id=data['manifest']['case_id'],
                    as_of=as_of, generated_at=generated_at,
                    binary_path=str(rp), binary_sha256=data['manifest']['rp_sha256'],
                    manifest_sha256=sha(data['manifest_bytes']), validation=data['validation'],
                    input_identity=identity(data['observed']), input_inventory=inventory(data['observed']),
                    reader_source_inventory=inventory(observed_sources),
                    graph_counts=dict(current_nodes=len(view['nodes']), current_science_edges=len(view['edges']),
                                      historical_nodes=len(graph['nodes']), binding_heads=len(graph['bindingHeads']),
                                      artifacts=len(data['sources'])),
                    visual_acceptance='PENDING: no browser/GUI; historical recovery does not accept this candidate',
                    snapshot_guarantee='Repeated observed inventories, NOT atomic snapshot or TOCTOU immunity')
    display = {k:data[k] for k in ('graph','sources','notes','records','names')}
    html = render(display, evidence)
    def final_check():
        case_io.recheck(data, root, manifest_path, rp)
        require(reader_sources() == observed_sources, 'Reader source inventory changed')
        check_output(output, [root, HERE, manifest_path], legacy=True)
    final_check()
    atomic_write(output, html, final_check)
    return evidence | dict(html_bytes=len(html.encode()), html_sha256=sha(html.encode()))


def rebuild_generic(project_root, rp, output, thread=None, as_of=None, generated_at=None,
                    force=False, include_local_sources=False, runner=subprocess.run, node_statuses=None,
                    source_locators=None):
    import case_io
    project_root = check_lexical_path(project_root)
    rp = check_lexical_path(rp)
    from datetime import timezone
    as_of = as_of or datetime.now(timezone.utc).isoformat()
    generated_at = generated_at or datetime.now(timezone.utc).isoformat()
    status_path = check_lexical_path(node_statuses) if node_statuses is not None else None
    locator_path = check_lexical_path(source_locators) if source_locators is not None else None
    protected = [project_root, HERE] + [p for p in (status_path, locator_path) if p is not None]
    output = check_output(output, protected, force=force, legacy=False)

    source_names = ('build.py', 'case_io.py', 'source_locators.py', 'graph_projection.py', 'routing.py', 'routing.js', 'svg.js',
                    'template.html', 'reader.css', 'graph.js', 'bootstrap.js')
    sources_before = {}
    for name in source_names:
        p = HERE / name
        no_symlink_chain(p)
        with p.open('rb') as f:
            sources_before[name] = sha(f.read(MAX_FILE + 1))
    sources_before['rp_bin'] = sha(rp.read_bytes())

    data = case_io.load_generic_project(project_root, rp, thread_id=thread, as_of=as_of,
                                        include_local_sources=include_local_sources, runner=runner)
    status_bytes = case_io.load_node_statuses(status_path, data) if status_path is not None else None
    locator_bytes = None
    if locator_path is not None:
        import source_locators as locator_io
        locator_bytes = locator_io.load(locator_path, data, project_root)
    project_obj = data['project']
    evidence = dict(
        mode='generic-snapshot',
        project_id=project_obj['id'],
        project_title=project_obj.get('title', '研究项目'),
        thread=data['thread'],
        as_of=as_of,
        generated_at=generated_at,
        sources=sources_before,
        canonical_count=len(data['records']),
    )
    if status_bytes is not None:
        evidence['node_statuses'] = dict(sha256=sha(status_bytes),
            entries=sum('presentationStatus' in n for n in data['graph']['nodes']),
            digest_format='reader-canonical-json-v1: sha256 of build.compact(record) UTF-8; not Core JCS',
            role='Explicit private presentation evidence, not Assessment or human acceptance')
    display = {k: data[k] for k in ('graph', 'sources', 'notes', 'records', 'names', 'project')}
    if locator_bytes is not None:
        display['sourceLocators'] = data['sourceLocators']
        evidence['source_locators'] = dict(sha256=sha(locator_bytes),
            mapped_nodes=sum(bool(n['sourceLocators']) for n in data['graph']['nodes']),
            unavailable_nodes=sum(not n['sourceLocators'] for n in data['graph']['nodes']),
            locators=sum(len(n['sourceLocators']) for n in data['graph']['nodes']),
            unique_chunks=len(data['sourceLocators']['chunks']),
            excerpts=sum(bool(s['excerpt']) for s in locator_io.unpack(data['sourceLocators'])),
            role='Verified archived capture only; not live original verification or scientific acceptance')
    html = render(display, evidence)

    def final_check():
        for name in source_names:
            p = HERE / name
            no_symlink_chain(p)
            with p.open('rb') as f:
                require(sha(f.read(MAX_FILE + 1)) == sources_before[name], 'Reader source modified during build')
        require(sha(rp.read_bytes()) == sources_before['rp_bin'], 'rp binary modified during build')
        # Recheck all observed files (including any consulted local source files)
        observed_after = case_io.observe_contained(project_root, extra_paths=[
            p for p in data['observed'] if not p.startswith('.research/')
        ])
        require(observed_after == data['observed'], 'Project files modified during build')
        if status_path is not None:
            require(case_io.read_bounded(status_path) == status_bytes, 'Node status sidecar changed during build')
        if locator_path is not None:
            require(locator_io.read_sidecar(locator_path) == locator_bytes, 'Source locator sidecar changed during build')
        check_output(output, protected, force=force, legacy=False)

    final_check()
    atomic_write(output, html, final_check)
    return evidence | dict(html_bytes=len(html.encode()), html_sha256=sha(html.encode()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project', required=True)
    parser.add_argument('--rp', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--as_of', '--as-of', dest='as_of')
    parser.add_argument('--generated_at', '--generated-at', dest='generated_at')
    parser.add_argument('--before')
    parser.add_argument('--case-manifest')
    parser.add_argument('--thread')
    parser.add_argument('--force', action='store_true')
    parser.add_argument('--include-local-sources', action='store_true')
    parser.add_argument('--node-statuses', help='Explicit private presentation JSON, generic mode only')
    parser.add_argument('--source-locators', help='Explicit private archive locator JSON, generic mode only')
    args = parser.parse_args()
    try:
        require(not (args.node_statuses or args.source_locators) or not (args.case_manifest or args.before),
                '--node-statuses / --source-locators are generic mode only')
        if args.case_manifest is not None:
            if not args.as_of or not args.generated_at:
                parser.error('--as-of and --generated-at are required for case manifest mode')
            report = rebuild(args.project, None, args.rp, args.output, args.as_of, args.generated_at,
                             case_manifest=args.case_manifest)
        elif args.before is not None:
            if not args.as_of or not args.generated_at:
                parser.error('--as-of and --generated-at are required for legacy before mode')
            report = rebuild(args.project, args.before, args.rp, args.output, args.as_of, args.generated_at)
        else:
            report = rebuild_generic(args.project, args.rp, args.output, thread=args.thread,
                                     as_of=args.as_of, generated_at=args.generated_at,
                                     force=args.force, include_local_sources=args.include_local_sources,
                                     node_statuses=args.node_statuses, source_locators=args.source_locators)
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError) as err:
        print(f'BUILD REJECTED: {err}')
        return 1
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
