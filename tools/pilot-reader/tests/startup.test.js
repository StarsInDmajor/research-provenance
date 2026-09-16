'use strict';
const assert=require('node:assert/strict');
const {test}=require('node:test');
const vm=require('node:vm');
const {createHash}=require('node:crypto');
const {execFileSync}=require('node:child_process');
const {fromTree}=require('./dom-double.js');
const {readData,writeData}=require('./wire-fixture.js');
const fixture=JSON.parse(execFileSync('python3',[__dirname+'/entry_fixture.py',...process.argv.slice(2)],{maxBuffer:4e6}));
// Immutable older artifacts execute their own embedded bootstrap contract.
// New-template controls/metadata regressions apply only to new deliveries.
const clarityTemplate=JSON.stringify(fixture.tree).includes('reading-clarity-user4');
const fullscreenTemplate=fixture.scripts[0].includes('function wheelFactor');
const followTemplate=fixture.scripts[0].includes('function reverseSources');
function setup(change=()=>{}) {
  const doc=fromTree(fixture.tree),get=id=>doc.getElementById(id);
  const data=readData(doc);
  const ctx=vm.createContext({document:doc});
  change({doc,get,data,ctx});
  const run=()=>{for(const script of fixture.scripts){
    if(ctx.afterSVG && script.includes('function boot(') && data.wireVersion===1)vm.runInContext('const originalSVG=buildSVG; buildSVG=(...args)=>{originalSVG(...args);afterSVG();};',ctx);
    vm.runInContext(script,ctx);
  }};
  return {doc,get,data,ctx,run};
}
const controls=s=>s.get('toolbar').querySelectorAll('button,input,select');
function failed(s){
  assert.equal(s.doc.body.getAttribute('data-app-state'),'failed');
  assert.ok(controls(s).every(c=>c.disabled));
  assert.match(s.get('app-status').textContent,/启动失败|交互已停止/);
  assert.match(s.get('app-status').textContent,/重新加载/);
  assert.doesNotMatch(s.get('app-status').textContent+s.get('detail-content').textContent,/SECRET_DIAGNOSTIC/);
  assert.equal(s.get('graph').getAttribute('aria-disabled'),'true');
  assert.equal(s.get('details').inert,true);
}
function changeData(fn){return s=>{fn(s.data);writeData(s.doc,s.data);};}
// Tamper after the real builder, before validateDOM (static archives: immediately).
function changeSVG(fn){return s=>{if(s.data.wireVersion===1)s.ctx.afterSVG=()=>fn(s);else fn(s);};}
function rename(s,id){s.get(id).setAttribute('id','removed-'+id);}

test('final HTML script/data CSP hashes pass independent grammar and exact-byte validator; colon fixture fails',()=>{
  const s=setup(),meta=s.doc.querySelectorAll('meta').find(e=>e.getAttribute('http-equiv')==='Content-Security-Policy');
  const policy=meta.getAttribute('content');
  const sources=policy.split(';').find(c=>c.trim().startsWith('script-src ')).trim().split(/\s+/).slice(1);
  function validate(tokens){
    assert.equal(tokens.length,3);
    for(const token of tokens){assert.match(token,/^'sha256-[A-Za-z0-9+/]{43}='$/);assert.equal(Buffer.from(token.slice(8,-1),'base64').length,32);}
    for(const el of s.doc.querySelectorAll('script'))assert.ok(tokens.includes("'sha256-"+createHash('sha256').update(el.textContent).digest('base64')+"'"));
  }
  validate(sources);
  assert.throws(()=>validate(["'sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA='",...sources.slice(1)]),assert.AssertionError);
  assert.throws(()=>validate(["'sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA='",...sources.slice(1)]),assert.AssertionError);
  assert.doesNotMatch(policy,/unsafe-eval|script-src[^;]*unsafe-inline/);
});
if(fixture.scripts[0].includes('labelLeaderPath'))test('optional label geometry is recomputed at boot, never trusted from raw projection',()=>{
 const s=setup(changeData(d=>{for(const e of [...d.graph.edges,...d.graph.revisionEdges])Object.assign(e,{labelStatus:'placed',labelHalfWidth:999999,labelLeaderPath:'M 999999 999999 L 1 1'});}));
 s.run();assert.equal(s.doc.body.getAttribute('data-app-state'),'ready');
 const api=vm.runInContext('boot(document).api',s.ctx);for(const e of api.model.view().edges){assert.ok(e.labelHalfWidth<1000);assert.notEqual(e.labelLeaderPath,'M 999999 999999 L 1 1');}
});
test('generated default entry is graph-first and safely static before any script',()=>{
  const s=setup();assert.equal(s.get('missing'),null,'strict DOM must never invent IDs');
  assert.equal(s.doc.body.getAttribute('data-app-state'),'loading');
  assert.ok(controls(s).every(c=>c.disabled));
  assert.match(s.get('app-status').textContent,/安全策略|预览容器/);
  assert.equal(s.get('app-status').getAttribute('aria-live'),'polite');
  assert.equal(s.get('briefing').tag,'details');assert.ok(!s.get('briefing').open);
  assert.match(s.doc.querySelectorAll('h1')[0].textContent,/项目逻辑 · 节点探索/);
  assert.equal(s.doc.querySelectorAll('a')[0].getAttribute('href'),'#workspace');
  assert.match(s.get('detail-content').textContent,/搜索或选择一个细节，查看其内容、来源、去向与历史/);
});
test('actual generated entry only enables after first render, preserves search focus; boot/mount idempotent',()=>{
  const s=setup();s.get('search').focus();
  let renders=0;
  const original=s.get('detail-content').replaceChildren.bind(s.get('detail-content'));
  s.get('detail-content').replaceChildren=()=>{renders++;assert.ok(controls(s).every(c=>c.disabled),'no premature enable');assert.equal(s.doc.body.getAttribute('data-app-state'),'loading');original();};
  s.run();assert.equal(s.doc.body.getAttribute('data-app-state'),'ready');assert.equal(renders,1);
  assert.equal(s.doc.activeElement,s.get('search'));
  assert.equal(s.get('search').disabled,false);assert.equal(s.get('back').disabled,true);
  assert.equal(s.get('graph').querySelectorAll('[data-key]').filter(e=>e.getAttribute('aria-pressed')==='true').length,0);
  const shown=s.get('graph').querySelectorAll('[data-key]').filter(e=>!e.classes.has('is-hidden'));
  assert.equal(shown.length,s.data.graph.nodes.filter(n=>n.current||n.ghost).length+s.data.graph.edges.filter(e=>e.current).length);
  const count=s.get('zoom-in').handlers.click.length;
  vm.runInContext('boot(document); mount(document, JSON.parse(document.getElementById("graph-data").textContent));',s.ctx);
  assert.equal(s.get('zoom-in').handlers.click.length,count);assert.equal(renders,1);
});
test('blocked renderer leaves bootstrap failed, while blocked bootstrap leaves useful loading fallback',()=>{
  const s=setup();vm.runInContext(fixture.scripts.at(-1),s.ctx);failed(s);
  const blocked=setup();vm.runInContext(fixture.scripts[0],blocked.ctx);
  assert.equal(blocked.doc.body.getAttribute('data-app-state'),'loading');assert.ok(controls(blocked).every(c=>c.disabled));
});
test('first render exception after handler registration fails closed; no unsafe diagnostics or retry',()=>{
  const s=setup();s.get('detail-content').replaceChildren=()=>{throw Error('SECRET_DIAGNOSTIC <img onerror=1>');};
  assert.doesNotThrow(s.run);failed(s);
  const box=s.get('graph').getAttribute('viewBox');
  s.get('zoom-in').dispatch('click');s.get('graph').dispatch('wheel',{deltaY:-1,clientX:1,clientY:1});
  s.get('graph').querySelectorAll('[data-key]')[0].dispatch('keydown',{key:'Enter'});
  assert.equal(s.get('graph').getAttribute('viewBox'),box);failed(s);
  const count=s.get('zoom-in').handlers.click.length;vm.runInContext('boot(document)',s.ctx);assert.equal(s.get('zoom-in').handlers.click.length,count);failed(s);
});
for(const id of ['workspace','graph','toolbar','details','detail-content','search','filter','history',
  'zoom-in','zoom-out','fit','back','restore-all','reset','show-all','search-results',
  'notice','counts','zoom-level','graph-data','app-status',
  ...(clarityTemplate ? ['start','start-choices'] : []),
  ...(fullscreenTemplate ? ['fullscreen','toggle-details','canvas-status'] : []),
  ...(fixture.scripts[1].includes("help:'button'") ? ['help','reading-guide','reading-guide-summary'] : [])]) {
  test('missing required DOM '+id+' rejected without auto creation',()=>{
    const s=setup(x=>rename(x,id));assert.doesNotThrow(s.run);
    assert.equal(s.doc.body.getAttribute('data-app-state'),'failed');
    assert.match(s.doc.textContent,/启动失败/);
    if(id!=='toolbar')assert.ok(controls(s).every(c=>c.disabled));
  });
}
for(const [name,change] of [
  ['duplicate ID',s=>{const e=s.doc.createElement('div');e.setAttribute('id','search');s.doc.body.appendChild(e);}],
  ['duplicate data-key',changeSVG(s=>{const g=s.get('graph'),e=s.doc.createElementNS('http://www.w3.org/2000/svg','g');e.setAttribute('data-key',s.data.graph.nodes[0].id);g.appendChild(e);})],
  ['missing data-key',changeSVG(s=>s.get('graph').querySelectorAll('[data-key]')[0].removeAttribute('data-key'))],
  ['endpoint mismatch',changeSVG(s=>s.get('graph').querySelectorAll('[data-key]').find(e=>e.getAttribute('data-from')).setAttribute('data-to','bad'))],
  ['wrong tag',s=>{s.get('search').tag='div';}],
  ['prematurely enabled control',s=>{s.get('search').disabled=false;}],
  ['missing SVG viewBox',changeSVG(s=>s.get('graph').removeAttribute('viewBox'))],
  ['wrong search type',s=>s.get('search').setAttribute('type','text')],
  ['missing focus attribute',changeSVG(s=>s.get('graph').querySelectorAll('[data-key]')[0].removeAttribute('tabindex'))],
  ['missing button role',changeSVG(s=>s.get('graph').querySelectorAll('[data-key]')[0].removeAttribute('role'))],
  ['bad JSON',s=>s.get('graph-data').textContent='SECRET_DIAGNOSTIC'],
  ['duplicate node key',changeData(d=>d.graph.nodes.push(d.graph.nodes[0]))],
  ['duplicate cross-layer key',changeData(d=>d.graph.revisionEdges.push({...d.graph.edges[0],category:'revision',type:'revision'}))],
  ['dangling edge',changeData(d=>d.graph.edges[0].to='missing')],
  ['edge to edge',changeData(d=>d.graph.edges[0].to=d.graph.edges[0].id)],
  ['invalid size',changeData(d=>d.graph.width=0)],
  ['node bound',changeData(d=>d.graph.nodes=Array(101).fill(d.graph.nodes[0]))],
  ['edge bound',changeData(d=>d.graph.edges=Array(301).fill(d.graph.edges[0]))],
  ['revision bound',changeData(d=>d.graph.revisionEdges=Array(301).fill({...d.graph.edges[0],category:'revision',type:'revision'}))],
  ['roles shape',changeData(d=>d.graph.nodes[0].roles='primary')],
  ['current type',changeData(d=>d.graph.nodes[0].current='true')],
  ['non-finite geometry',changeData(d=>d.graph.nodes[0].x=null)],
  ['raw mismatch',changeData(d=>d.graph.nodes[0].raw.id='other')],
  ['invalid path',changeData(d=>d.graph.edges[0].path='javascript:bad')],
  ['missing sources',changeData(d=>delete d.sources)],
  ['oversized JSON',s=>s.get('graph-data').textContent=' '.repeat(2000001)],
  ['raw edge endpoint mismatch',changeData(d=>d.graph.edges[0].raw.from_revision='missing')],
  ['malformed source',changeData(d=>d.sources.bad={sections:'bad'})],
  ...(followTemplate ? [
    ['reverse records bound',changeData(d=>{for(let i=0;i<513;i++)d.records['extra'+i]={id:'extra'+i};})],
    ['reverse source malformed',changeData(d=>d.records.extra={id:'extra',source:{revisions:'not-array'}})],
    ['reverse source member malformed',changeData(d=>d.records.extra={id:'extra',source:{revisions:[{}]}})],
    ['reverse record mismatch',changeData(d=>d.records.extra={id:'different'})],
    ['reverse record null',changeData(d=>d.records.extra=null)],
  ] : []),
  ...(clarityTemplate ? [
    ['invalid starts',changeData(d=>d.graph.startIds=['missing'])],
    ['duplicate starts',changeData(d=>d.graph.startIds=[d.graph.nodes[0].id,d.graph.nodes[0].id])],
    ['non-question start',changeData(d=>{d.graph.nodes[0].kind='Observation';d.graph.startIds=[d.graph.nodes[0].id];})],
    ['unbound question start',changeData(d=>{d.graph.nodes[0].bindingIds=[];d.graph.startIds=[d.graph.nodes[0].id];})],
    ['bad alias legend',changeData(d=>d.graph.aliasLegend={Q:'Question'})],
    ['invented reading source',changeData(d=>d.graph.readingSources={[d.graph.nodes[0].id]:{missing:'SOURCE'}})],
    ['duplicate clip id',changeSVG(s=>{const clip=s.get('graph').querySelectorAll('clipPath')[0];const e=s.doc.createElementNS('http://www.w3.org/2000/svg','clipPath');e.setAttribute('id',clip.getAttribute('id'));s.get('graph').appendChild(e);})],
    ['prematurely enabled start',s=>{s.get('start').disabled=false;}]
  ] : [])
])test(name+' rejected before handlers and readiness',()=>{
  const s=setup(change);assert.doesNotThrow(s.run);failed(s);
  assert.equal(s.get('zoom-in').handlers.click,undefined);
});
test('normal selected path and later render fault are differentiated from disabled controls',()=>{
  const s=setup();s.run();
  const node=s.data.graph.nodes.find(n=>n.current),edge=s.data.graph.edges.find(e=>e.current && e.type==='depends-on') || s.data.graph.edges.find(e=>e.current);
  assert.ok(edge,'actual startup fixture must contain a current relation');
  s.get('search').value=node.id;s.get('search').dispatch('input');s.get('search').dispatch('keydown',{key:'Enter'});
  assert.ok(s.get('detail-content').textContent.includes(node.raw.statement));
  // Check dependency-specific wording only when the actual corpus has that
  // type; generic actual inputs must not invent a depends-on edge for this test.
  if(edge.type==='depends-on') {
  const from=s.get('graph').querySelectorAll('[data-key]').find(e=>e.dataset.key===edge.from);
  from.dispatch('click');assert.match(s.get('detail-content').textContent,followTemplate?/出向 · 依赖对方/:/依赖来源 · 出向 depends-on/);
  if(followTemplate){
    const jump=s.get('detail-content').querySelectorAll('button').find(b=>b.dataset.relation===edge.id&&b.dataset.action==='node');
    assert.equal(jump.dataset.target,edge.to);assert.ok(jump.textContent.includes(s.data.graph.nodes.find(n=>n.id===edge.to).label));
  }
  s.get('graph').querySelectorAll('[data-key]').find(e=>e.dataset.key===edge.to).dispatch('click');
  assert.match(s.get('detail-content').textContent,followTemplate?/入向 · 被依赖，对方使用此项/:/被依赖方使用 · 入向 depends-on/);
  }
  s.get('search').value=node.id;s.get('search').dispatch('input');s.get('search').dispatch('keydown',{key:'Enter'});
  s.get('graph').querySelectorAll('[data-key]').find(e=>e.dataset.key===edge.id).dispatch('click');
  assert.ok(s.get('detail-content').textContent.includes(edge.raw.rationale));
  s.get('back').dispatch('click');assert.ok(s.get('detail-content').textContent.includes(node.raw.statement));
  s.get('detail-content').replaceChildren=()=>{throw Error('SECRET_DIAGNOSTIC');};
  s.get('graph').querySelectorAll('[data-key]').find(e=>e.dataset.key===edge.id).dispatch('click');failed(s);
  const box=s.get('graph').getAttribute('viewBox');s.get('fit').dispatch('click');assert.equal(s.get('graph').getAttribute('viewBox'),box);
});
