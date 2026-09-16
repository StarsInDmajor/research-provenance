'use strict';
// HTML-derived SVG, actual boot and handlers; no browser or visual acceptance.
const assert=require('node:assert/strict');
const vm=require('node:vm');
const {execFileSync}=require('node:child_process');
const {fromTree}=require('./dom-double.js');
const {readData}=require('./wire-fixture.js');
const fixture=JSON.parse(execFileSync('python3',[__dirname+'/entry_fixture.py',...process.argv.slice(2)],{maxBuffer:4e6}));
// Missing routing definition cannot publish ready; normal concatenation below
// proves producer ordering without pretending fake DOM enforces browser CSP.
const brokenDoc=fromTree(fixture.tree),brokenCtx=vm.createContext({document:brokenDoc});
assert.match(fixture.scripts[0],/const Routing/);
for(const script of [fixture.scripts[0].slice(fixture.scripts[0].indexOf('const edgeRouting')),fixture.scripts.at(-1)]){
 try{vm.runInContext(script,brokenCtx);}catch(_){}
}
assert.equal(brokenDoc.body.getAttribute('data-app-state'),'failed');
assert.ok(brokenDoc.getElementById('toolbar').querySelectorAll('button,input,select').every(e=>e.disabled));
const doc=fromTree(fixture.tree),get=id=>doc.getElementById(id),data=readData(doc),ctx=vm.createContext({document:doc});
for(const script of fixture.scripts)vm.runInContext(script,ctx);
assert.equal(doc.body.getAttribute('data-app-state'),'ready');
const {model,render}=vm.runInContext('boot(document).api',ctx);
assert.ok(model.routeStats,'startup must route visible graph');
const before=JSON.stringify(data.graph),transforms=get('graph').querySelectorAll('[data-key]').filter(e=>!e.dataset.from).map(e=>e.getAttribute('transform'));
const group=id=>get('graph').querySelectorAll('[data-key]').find(e=>e.dataset.key===id);
function check(){
 for(const e of [...model.view().edges,...model.view().revisionEdges]){
  const g=group(e.id);for(const p of g.querySelectorAll('path').filter(p=>!(p.getAttribute('class')||'').includes('edge-label-leader')))assert.equal(p.getAttribute('d'),e.path,'line and hit path use current geometry');
  const label=g.querySelectorAll('text')[0];assert.equal(+label.getAttribute('x'),e.labelX);assert.equal(+label.getAttribute('y'),e.labelY);
  assert.equal(g.getAttribute('data-label-status'),e.labelStatus);
  const shown=e.labelStatus==='placed'?'visible':'hidden';assert.equal(label.getAttribute('visibility'),shown);
  const bg=g.querySelectorAll('rect').find(x=>(x.getAttribute('class')||'').includes('edge-label-bg')),leader=g.querySelectorAll('path').find(x=>(x.getAttribute('class')||'').includes('edge-label-leader'));
  assert.ok(bg&&leader,'background and explicit short association');assert.equal(bg.getAttribute('visibility'),shown);assert.equal(leader.getAttribute('visibility'),shown);
  assert.equal(leader.getAttribute('d'),e.labelLeaderPath);assert.equal(+bg.getAttribute('x'),e.labelX-e.labelHalfWidth);assert.equal(+bg.getAttribute('width'),2*e.labelHalfWidth);
  const line=g.querySelectorAll('path').find(x=>(x.getAttribute('class')||'').includes('edge-line'));assert.equal(bg.getAttribute('stroke'),line.getAttribute('stroke'));assert.equal(leader.getAttribute('stroke'),line.getAttribute('stroke'));
  assert.ok(g.getAttribute('aria-label').includes(e.from+' → '+e.to));
  if(e.labelStatus==='hidden')assert.ok(g.getAttribute('aria-label').includes('标签暂隐'));
 }
 assert.equal(JSON.stringify(data.graph),before);assert.deepEqual(get('graph').querySelectorAll('[data-key]').filter(e=>!e.dataset.from).map(e=>e.getAttribute('transform')),transforms);
}
check();
const deferred=model.view().edges.find(e=>e.labelStatus==='hidden'&&e.routeStatus==='routed');
if(deferred){
 assert.match(get('notice').textContent,/标签暂隐（不是隐藏关系）/);
 group(deferred.id).dispatch('keydown',{key:'Enter'});check();assert.equal(model.selected,deferred.id);
 assert.match(get('detail-content').textContent,/标签暂隐/);assert.ok(get('detail-content').textContent.includes(deferred.type));
 const paths=group(deferred.id).querySelectorAll('path').filter(p=>(p.getAttribute('class')||'').includes('edge-line')||(p.getAttribute('class')||'').includes('edge-hit'));
 assert.equal(paths.length,2);for(const p of paths)assert.ok(p.getAttribute('d').includes(' L '));
}
const runs=model.routeStats.computations;
get('zoom-in').dispatch('click');get('graph').dispatch('wheel',{deltaY:-1,clientX:200,clientY:100});
group(data.graph.nodes.find(n=>n.current).id).dispatch('click');check();assert.equal(model.routeStats.computations,runs);
const b=data.graph.nodes.find(n=>n.label.startsWith('B01 ')),g=data.graph.nodes.find(n=>n.label.startsWith('G05 '));
if(b&&g){
 const e=data.graph.edges.find(e=>e.from===b.id&&e.to===g.id);assert.ok(e);
 group(b.id).dispatch('click');const focus=get('detail-content').querySelectorAll('button').find(e=>e.textContent==='只看相关');focus.dispatch('click');check();
 const routed=model.view().edges.find(x=>x.id===e.id);assert.ok(routed);assert.equal(routed.routeStatus,'routed');
 const ys=routed.path.match(/-?\d+(?:\.\d+)?/g).map(Number).filter((_,i)=>i%2);assert.ok(Math.min(...ys)>=Math.min(b.y,g.y)-14,'local route stays near its endpoint hull, not obsolete global top');
 const paths=group(e.id).querySelectorAll('path'),style=paths.map(p=>[p.getAttribute('stroke'),p.getAttribute('stroke-dasharray'),p.getAttribute('marker-end')]);
 group(e.id).dispatch('click');assert.equal(model.selected,e.id);check();assert.deepEqual(paths.map(p=>[p.getAttribute('stroke'),p.getAttribute('stroke-dasharray'),p.getAttribute('marker-end')]),style);
}
const box={...model.box},routeKey=model.routeStats.key;
get('history').checked=true;get('history').dispatch('change');check();get('back').dispatch('click');check();assert.deepEqual({...model.box},box);assert.equal(model.routeStats.key,routeKey);
get('filter').value='alternative';get('filter').dispatch('change');check();get('restore-all').dispatch('click');check();
get('search').value=data.graph.edges[0].id;get('search').dispatch('input');model.searchState.index=model.search(get('search').value).findIndex(e=>e.id===data.graph.edges[0].id);get('search').dispatch('keydown',{key:'Enter'});check();assert.equal(model.selected,data.graph.edges[0].id);
console.log('routing real HTML/handlers PASS: startup, paths+labels, canonical identity, stable nodes, local B01/G05 when present, layers/filter/reveal/Back, viewport cache');
