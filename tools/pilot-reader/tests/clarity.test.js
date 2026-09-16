'use strict';
// Actual generated/final scripts in strict fake DOM, never browser font evidence.
const assert=require('node:assert/strict');
const vm=require('node:vm');
const {execFileSync}=require('node:child_process');
const {fromTree}=require('./dom-double.js');
const {readData}=require('./wire-fixture.js');
const fixture=JSON.parse(execFileSync('python3',[__dirname+'/entry_fixture.py',...process.argv.slice(2)],{maxBuffer:4e6}));
const doc=fromTree(fixture.tree),get=id=>doc.getElementById(id);
const data=readData(doc),ctx=vm.createContext({document:doc});
assert.ok(get('start'),'visible start control');assert.equal(get('start').disabled,true);
assert.ok(get('reading-guide').open,'native disclosure usable without JS');
const guideText=get('reading-guide').textContent;
for(const script of fixture.scripts)vm.runInContext(script,ctx);
assert.equal(doc.body.getAttribute('data-app-state'),'ready');
const api=vm.runInContext('boot(document).api',ctx),model=api.model;
assert.equal(model.selected,null);
const detail=get('detail-content');
// A native details toggle does not replace/reset the selected record or add navigation.
const node=data.graph.nodes.find(n=>n.current);
model.reveal(node.id);api.render();const heading=detail.querySelectorAll('h2')[0],stack=model.stack.length;
get('reading-guide').open=false;get('reading-guide').dispatch('toggle');
assert.equal(detail.querySelectorAll('h2')[0],heading);assert.equal(model.stack.length,stack);
get('reading-guide').open=true;get('reading-guide').dispatch('toggle');
get('start').dispatch('click');
const starts=data.graph.startIds;
assert.ok(Array.isArray(starts));
if(starts.length===1){
 assert.equal(model.selected,starts[0]);assert.match(get('notice').textContent,/建议入口.*不是.*根/);
 const q=data.graph.nodes.find(n=>n.id===starts[0]);assert.equal(q.kind,'Question');
 if(q.label.startsWith('Q00 ')){
  assert.match(guideText,/案例别名.*不是.*kind/);assert.match(guideText,/I03.*Observation.*I04.*Interpretation.*I06.*Method.*I09.*Conclusion/);
  const source=detail.querySelectorAll('button').find(b=>/故障现象 I02.*SOURCE/.test(b.textContent));assert.ok(source,'immediate explicit source navigation');
  const count=data.graph.edges.length;source.dispatch('click');assert.match(detail.querySelectorAll('h2')[0].textContent,/I02/);
  get('back').dispatch('click');assert.equal(model.selected,starts[0]);assert.equal(data.graph.edges.length,count);
 }
 get('back').dispatch('click');assert.equal(model.selected,node.id,'start is one reversible transition');
}else{
 assert.equal(model.selected,node.id,'multiple/absent questions must not silently select');
 const buttons=get('start-choices').querySelectorAll('button');assert.equal(buttons.length,starts.length);
 if(buttons.length){buttons[0].dispatch('click');assert.equal(model.selected,starts[0]);get('back').dispatch('click');assert.equal(model.selected,node.id);}
}
for(const history of [false,true]){
 model.setHistory(history);api.render();
 for(const e of [...data.graph.edges,...(history?data.graph.revisionEdges:[])]){
  const group=get('graph').querySelectorAll('[data-key]').find(el=>el.dataset.key===e.id);
  const line=group.querySelectorAll('path').find(el=>el.getAttribute('class')==='edge-line');
  const before=['stroke','stroke-dasharray','marker-end'].map(k=>line.getAttribute(k));
  model.reveal(e.id);api.render();assert.ok(group.classes.has('selected'));
  assert.deepEqual(['stroke','stroke-dasharray','marker-end'].map(k=>line.getAttribute(k)),before);
 }
}
assert.equal(get('reading-guide').open,false,'selection returns the pane from help to details');assert.equal(get('reading-guide').textContent,guideText);
console.log('clarity generated/final DOM: start / choice / source / Back / native disclosure isolation / type attributes PASS; visual pending');
