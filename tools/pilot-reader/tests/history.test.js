'use strict';
const assert=require('node:assert/strict');
const {test}=require('node:test');
const vm=require('node:vm');
const {execFileSync}=require('node:child_process');
const {fromTree}=require('./dom-double.js');
const f=JSON.parse(execFileSync('python3',[__dirname+'/entry_fixture.py',...process.argv.slice(2)],{maxBuffer:8e6}));
function setup(edit=()=>{}) {
  const doc=fromTree(f.tree),get=id=>doc.getElementById(id),wire=JSON.parse(get('graph-data').textContent),records=JSON.parse(get('canonical-records').textContent);
  const n=wire.graph.nodes.find(n=>n.current),ref={id:'ref_synthetic',schema:'rp/external-reference/v1',title:'Synthetic evidence'};
  records[ref.id]=ref;
  n.presentationStatus={revision_id:n.id,canonical_digest:'sha256:'+'1'.repeat(64),status:'historical',label:'历史记录',reason:'<script>not executed</script> documented historical scope',assessed_at:'2026-09-12T12:00:00Z',source_refs:[{id:ref.id,canonical_digest:'sha256:'+'2'.repeat(64),locator:'javascript:alert(1) plain reference only'}]};
  edit({wire,records,n});
  get('graph-data').textContent=JSON.stringify(wire);get('canonical-records').textContent=JSON.stringify(records);
  const ctx=vm.createContext({document:doc});for(const s of f.scripts)vm.runInContext(s,ctx);
  return {doc,get,ctx,n,wire};
}
test('documented content survives compact boot, selection, history, back, reset without hiding',()=>{
  const s=setup();assert.equal(s.doc.body.getAttribute('data-app-state'),'ready');
  const el=s.get('graph').querySelectorAll('[data-key]').find(e=>e.dataset.key===s.n.id);
  assert.ok(el.classes.has('history-mark'));assert.ok(el.getAttribute('aria-label').includes('最新记录修订·描述历史记录'));
  vm.runInContext(`boot(document).api.model.select(${JSON.stringify(s.n.id)});boot(document).api.render();`,s.ctx);
  assert.ok(el.classes.has('history-mark'));assert.ok(el.classes.has('selected'));
  assert.ok(s.get('detail-content').textContent.includes('documented historical scope'));
  assert.ok(s.get('detail-content').textContent.includes('javascript:alert(1) plain reference only'));
  assert.equal(s.get('detail-content').querySelectorAll('a,script').length,0);
  for(const id of ['help','fullscreen','back','show-all','reset']) {
    s.get(id).dispatch('click');assert.ok(el.classes.has('history-mark'));
  }
  assert.ok(!el.classes.has('is-hidden'));
  const css=require('node:fs').readFileSync(__dirname+'/../reader.css','utf8');
  assert.match(css,/\.node\.history-mark \.node-card\{stroke-dasharray:4 4\}/);
  assert.equal((css.match(/stroke-dasharray:/g)||[]).length,1,'selection must not reset the border dash');
});
test('ghost and non-head revisions dashed independently of content',()=>{
  const s=setup();
  for(const n of s.wire.graph.nodes.filter(n=>!n.current)) {
    const el=s.get('graph').querySelectorAll('[data-key]').find(e=>e.dataset.key===n.id);
    assert.ok(el.classes.has('history-mark'));
    assert.ok(el.getAttribute('aria-label').includes(n.ghost?'历史端点(当前引用)':'旧修订'));
  }
});
test('unknown/freshness/frozen never imply historical content',()=>{
  const s=setup(s=>delete s.n.presentationStatus);
  assert.equal(s.doc.body.getAttribute('data-app-state'),'ready');
  const el=s.get('graph').querySelectorAll('[data-key]').find(e=>e.dataset.key===s.n.id);
  assert.ok(!el.classes.has('history-mark'));assert.ok(el.getAttribute('aria-label').includes('内容状态待核实'));
});
for(const [label,mutate] of [
  ['extra field',s=>s.n.presentationStatus.extra=true],
  ['invalid enum',s=>s.n.presentationStatus.status='unknown'],
  ['wrong revision',s=>s.n.presentationStatus.revision_id='missing'],
  ['missing source',s=>s.n.presentationStatus.source_refs[0].id='missing'],
  ['bad digest',s=>s.n.presentationStatus.canonical_digest='not-digest'],
  ['empty reason',s=>s.n.presentationStatus.reason=''],
  ['bad label',s=>s.n.presentationStatus.label='invalid'],
  ['source field',s=>s.n.presentationStatus.source_refs[0].url='javascript:x'],
  ['bad time',s=>s.n.presentationStatus.assessed_at='yesterday'],
  ['invented successor',s=>s.n.presentationStatus.superseded_by=['missing']],
])test('compact rejects '+label,()=>assert.equal(setup(mutate).doc.body.getAttribute('data-app-state'),'failed'));
