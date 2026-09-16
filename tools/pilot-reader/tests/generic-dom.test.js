'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
const vm = require('node:vm');
const { execFileSync } = require('node:child_process');
const { fromTree } = require('./dom-double.js');
const {readData,writeData}=require('./wire-fixture.js');

// Build Project Alpha and Beta to temporary directory
const tmp = execFileSync('python3', ['-c', `
import tempfile, sys, os
from pathlib import Path
sys.path.insert(0, 'pkgs/misc/research-provenance/tools/pilot-reader')
sys.path.insert(0, 'pkgs/misc/research-provenance/tools/pilot-reader/tests')
import build, case_io, test_reusable_reader
tmp = Path(tempfile.mkdtemp(prefix='rp-test-dom-'))
p_alpha = tmp / 'alpha'
p_alpha.mkdir()
test_reusable_reader.create_project_alpha(p_alpha)
out_alpha = tmp / 'reader-alpha.html'
build.rebuild_generic(p_alpha, 'pkgs/misc/research-provenance/target/debug/rp', out_alpha)
print(out_alpha)
`], { encoding: 'utf-8' }).trim();

const fixture = JSON.parse(execFileSync('python3', [__dirname + '/entry_fixture.py', tmp], { maxBuffer: 4e6 }));

function setup(change = () => {}) {
  const doc = fromTree(fixture.tree), get = id => doc.getElementById(id);
  const data = readData(doc);
  const ctx = vm.createContext({ document: doc });
  change({ doc, get, data, ctx });
  writeData(doc,data);
  const run = () => { for (const script of fixture.scripts) vm.runInContext(script, ctx); };
  return { doc, get, data, ctx, run };
}

test('assessed node renders honest assessment count and does not say 未评价 in details', () => {
  const s = setup();
  s.run();
  // Select hyp_01J000000000000000000000A2 which has 1 assessment
  const hypId = 'hyp_01J000000000000000000000A2';
  const el = s.get('graph').querySelectorAll('[data-key]').find(e => e.dataset.key === hypId);
  assert.ok(el, 'hyp node exists in graph');
  el.dispatch('click');

  const detailText = s.get('detail-content').textContent;
  assert.ok(detailText.includes('评价记录 (1)'), 'must include assessment count');
  assert.ok(detailText.includes('评价展示暂不支持'), 'must explain assessment display is unsupported');
  assert.ok(!detailText.includes('未评价 ·'), 'must not falsely claim 未评价 when assessment exists');
});

test('unassessed node renders 未记录评价 in details', () => {
  const s = setup();
  s.run();
  // Select qst_01J000000000000000000000A1 which has NO assessments
  const qstId = 'qst_01J000000000000000000000A1';
  const el = s.get('graph').querySelectorAll('[data-key]').find(e => e.dataset.key === qstId);
  assert.ok(el, 'qst node exists in graph');
  el.dispatch('click');

  const detailText = s.get('detail-content').textContent;
  assert.ok(detailText.includes('未记录评价'), 'must render 未记录评价');
  assert.ok(!detailText.includes('评价展示暂不支持'), 'must not claim assessment display unsupported for unassessed node');
});

test('isolated node with assessment preserves assessment status and renders isolation notice', () => {
  const s = setup(({ data }) => {
    // Mark hyp node as isolated and verify it preserves assessment count
    const node = data.graph.nodes.find(n => n.id === 'hyp_01J000000000000000000000A2');
    node.isolated = true;
  });
  s.run();

  const hypId = 'hyp_01J000000000000000000000A2';
  const el = s.get('graph').querySelectorAll('[data-key]').find(e => e.dataset.key === hypId);
  el.dispatch('click');

  const detailText = s.get('detail-content').textContent;
  assert.ok(detailText.includes('评价记录 (1)'), 'isolated node must still show assessment count');
  assert.ok(detailText.includes('本快照未记录连接'), 'isolated node must show isolation notice');
});

test('all freshness statuses render correct labels in details', () => {
  const statuses = [
    { key: 'fresh', label: '时效正常' },
    { key: 'review-due', label: '待复核' },
    { key: 'stale', label: '已过保' },
    { key: 'unknown', label: '时效未判定' },
  ];

  for (const { key, label } of statuses) {
    const s = setup(({ data }) => {
      const node = data.graph.nodes.find(n => n.id === 'qst_01J000000000000000000000A1');
      node.freshness = key;
    });
    s.run();
    const el = s.get('graph').querySelectorAll('[data-key]').find(e => e.dataset.key === 'qst_01J000000000000000000000A1');
    el.dispatch('click');
    const detailText = s.get('detail-content').textContent;
    assert.ok(detailText.includes('时效：' + label), `must display 时效：${label} for freshness ${key}`);
  }
});

test('source cards render truthful descriptions based on source_type, verification, and text status', () => {
  const sourceCases = [
    {
      source: { title: 'Local Verified', source_type: 'local-file', verified: true, is_text: true, excerpt: 'local text content', sections: [] },
      expectedLabel: '本次构建核验的本地内容',
      expectedSection: '原文摘录',
    },
    {
      source: { title: 'Metadata Only', source_type: 'metadata-only', verified: false, is_text: true, excerpt: '[元数据引用；未包含本地全文摘录]', sections: [] },
      expectedLabel: '未读取正文，仅引用元数据',
      expectedSection: '原文摘录',
    },
    {
      source: { title: 'External Paper', source_type: 'external-uri', verified: false, original_path: 'https://example.invalid/paper.pdf', excerpt: '[外部 URI 引用，未执行网络检索]', sections: [] },
      expectedLabel: '未获取外部正文',
      expectedSection: '原文摘录',
    },
    {
      source: { title: 'Binary Dataset', source_type: 'local-file', verified: true, is_text: false, excerpt: '[二进制或非UTF-8文件，省略文本摘录]', sections: [] },
      expectedLabel: '二进制或非UTF-8文件',
      expectedSection: '文件状态',
    },
    {
      source: { title: 'Legacy Pilot Source', excerpt: 'historical text', sections: [] },
      expectedLabel: '已验证复制摘录；历史文档，不是本次重新测量。',
      expectedSection: '原文摘录',
    },
  ];

  for (const { source, expectedLabel, expectedSection } of sourceCases) {
    const s = setup(({ data }) => {
      const artId = 'art_test_source';
      data.sources[artId] = { ...source, artifact_id: artId };
      data.records[artId] = {id:artId,schema:'rp/artifact-manifest/v1',title:source.title};
      const node = data.graph.nodes.find(n => n.id === 'hyp_01J000000000000000000000A2');
      node.raw.source = { artifacts: [artId], revisions: [] };
    });
    s.run();

    const hypId = 'hyp_01J000000000000000000000A2';
    const el = s.get('graph').querySelectorAll('[data-key]').find(e => e.dataset.key === hypId);
    el.dispatch('click');

    const detailText = s.get('detail-content').textContent;
    assert.ok(detailText.includes(expectedLabel), `must render ${expectedLabel} for source ${source.title}`);
    assert.ok(detailText.includes(expectedSection), `must render section ${expectedSection} for source ${source.title}`);
  }
});
