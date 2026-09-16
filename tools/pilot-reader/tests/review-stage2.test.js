'use strict';
const assert=require('node:assert/strict');
const {test}=require('node:test');
const vm=require('node:vm');
const {execFileSync}=require('node:child_process');
const {fromTree}=require('./dom-double.js');
const {readData,writeData}=require('./wire-fixture.js');
const fixture=JSON.parse(execFileSync('python3',[__dirname+'/entry_fixture.py',...process.argv.slice(2)],{maxBuffer:8e6}));
const hash='sha256:'+'a'.repeat(64);
function inject(d) {
  const n=d.graph.nodes[0],aid=Object.keys(d.sources)[0] || 'art_synthetic';
  if(!d.records[aid]) {
    d.records[aid]={id:aid,schema:'rp/artifact-manifest/v1',title:'Synthetic archive'};
    d.sources[aid]={title:'Synthetic archive',sections:[],excerpt:'metadata only'};
  }
  n.raw.source={...n.raw.source,artifacts:[aid]};
  d.sourceLocators={strings:['src/config.py','# Config',hash,'2026-09-12T09:19:56Z','中文 </pre></script>\n'],
    chunks:[[0,10,42,1,2,2,3,10,2,4]],artifacts:[aid]};
  for(const node of d.graph.nodes)node.sourceLocators=node===n?[[0,0]]:[];
}
function setup(edit=()=>{},synthetic=true,ready=true) {
  const doc=fromTree(fixture.tree),get=id=>doc.getElementById(id),data=readData(doc);
  if(synthetic)inject(data);edit(data);writeData(doc,data);
  const ctx=vm.createContext({document:doc});for(const script of fixture.scripts)vm.runInContext(script,ctx);
  assert.equal(doc.body.getAttribute('data-app-state'),ready?'ready':'failed');
  return {doc,get,data,...(ready?vm.runInContext('boot(document).api',ctx):{})};
}
function select(s,n){s.model.select(n.id);s.render();return s.get('detail-content');}
test('per-node readable source locator, exact cited/display ranges, capture disclaimers, safe text',()=>{
  const s=setup(),n=s.data.graph.nodes[0],c=select(s,n);
  assert.match(c.textContent,/依据出处/);assert.match(c.textContent,/src\/config.py/);
  assert.match(c.textContent,/第 10–42 行/);assert.match(c.textContent,/显示第 10–10 行/);
  assert.match(c.textContent,/节选，完整原文未嵌入/);assert.match(c.textContent,/捕获于 2026-09-12T09:19:56Z 的来源节选/);
  assert.match(c.textContent,/中文 <\/pre><\/script>/);assert.equal(c.querySelectorAll('script,a').length,0);
  assert.match(c.textContent,/捕获原文件摘要/);assert.match(c.textContent,/捕获完整节选摘要/);assert.match(c.textContent,/显示节选摘要/);
  const absent=select(s,s.data.graph.nodes[1]);assert.match(absent.textContent,/精确出处不可用/);
  assert.ok(!absent.textContent.includes('src/config.py'));
});
test('source summary identity restores exact node+source focus/open/scroll through Back',()=>{
  const s=setup(),n=s.data.graph.nodes[0],c=select(s,n);
  const source=c.querySelectorAll('details').find(e=>e.children[0].textContent.includes('src/config.py'));
  assert.ok(source,'source disclosure missing');source.open=true;source.children[0].focus();s.get('details').scrollTop=37;
  // Existing graph event captures current source focus before navigating.
  s.get('graph').querySelectorAll('[data-key]').find(e=>e.dataset.key===s.data.graph.nodes[1].id).dispatch('click');
  s.get('back').dispatch('click');assert.equal(s.model.selected,n.id);
  assert.equal(s.doc.activeElement.dataset.focus,source.children[0].dataset.focus);
  assert.equal(s.get('details').scrollTop,37);
  assert.equal(s.get('detail-content').querySelectorAll('details').find(e=>e.children[0].textContent.includes('src/config.py')).open,true);
});
for(const [name,mutate] of [
  ['array envelope',d=>d.sourceLocators=[]],['unknown envelope key',d=>d.sourceLocators.extra=1],
  ['unknown node chunk',d=>d.graph.nodes[0].sourceLocators=[[9,0]]],
  ['duplicate link',d=>d.graph.nodes[0].sourceLocators=[[0,0],[0,0]]],
  ['unlinked artifact',d=>d.graph.nodes[0].raw.source.artifacts=[]],
  ['bad path',d=>d.sourceLocators.strings[0]='../escape'],
  ['bad digest',d=>d.sourceLocators.strings[2]='wrong'],
  ['bad original line',d=>d.sourceLocators.chunks[0][1]=0],
  ['bad display line',d=>d.sourceLocators.chunks[0][7]=43],
  ['bad tuple',d=>d.sourceLocators.chunks[0].push(1)],
  ['UTF8 byte overflow',d=>d.sourceLocators.strings[4]='中'.repeat(80)],
  ['missing node mapping',d=>delete d.graph.nodes[0].sourceLocators],
])test('BOOT fails closed: '+name,()=>setup(mutate,true,false));
if(process.argv[2])test('actual whole graph BOOT per-node R02 and historical R86 archive mappings',()=>{
  const s=setup(()=>{},false);
  assert.equal(s.data.graph.nodes.length,86);assert.equal(Object.keys(s.data.records).length,299);
  for(const [logical,path,range] of [['reion3-parameter-order','src/simulation/config.py','10–42'],
    ['reion3-historical-pt-stack','docs/reports/2026-07-26-current-pt-posterior-structure-study.md','51–81']]) {
    const n=s.data.graph.nodes.find(n=>n.raw.logical_id===logical),c=select(s,n);
    assert.ok(c.textContent.includes(path));assert.ok(c.textContent.includes(range));assert.match(c.textContent,/节选，完整原文未嵌入/);
  }
});
