'use strict';
const assert = require('node:assert/strict');
const {Model, wireSelection} = require('../graph.js');
const graph = {
  nodes: [
    {id:'a',label:'问题',title:'Original',current:true,roles:['primary'],x:0,y:0},
    {id:'b',label:'备选',title:'Other',current:true,roles:['alternative'],x:340,y:0},
    {id:'old',label:'旧行动',title:'Old',current:false,roles:['historical'],x:0,y:200}
  ],
  edges:[{id:'e',from:'a',to:'b',current:true}],
  revisionEdges:[{id:'revision:old:a',from:'old',to:'a',category:'revision'}],
  width:680,height:400
};
const m = new Model(graph);
assert.equal(m.view().nodes.length, 2);
m.select('a');
assert.deepEqual([...m.neighborhood().nodes].sort(), ['a','b']);
assert.deepEqual([...m.neighborhood().edges], ['e']);
m.setFilter('mainline');
assert.equal(m.view().nodes.length, 2);
assert.equal(m.view().hiddenEdges, 0);
assert.equal(m.view().hiddenNodes, 1);
assert.equal(m.search('旧行动')[0].id, 'old');
m.reveal('old');
assert.equal(m.history, true);
assert.equal(m.mode, 'mainline');
assert.equal(m.selected, 'old');
assert.match(m.notice, /历史/);
m.reveal('revision:old:a');
assert.equal(m.view().revisionEdges.length, 1);
m.select('e');
assert.deepEqual([...m.neighborhood().nodes].sort(), ['a','b']);
m.clear();
assert.equal(m.selected, null);
m.fit();
const oldBox = {...m.box};
m.zoom(2, {x:0.25,y:0.75});
assert.equal(m.box.w, oldBox.w / 2);
assert.equal(m.box.x + m.box.w*.25, oldBox.x+oldBox.w*.25);
m.pan(100000,-100000);
assert.ok(Math.abs(m.box.x) <= graph.width*2);
assert.ok(Math.abs(m.box.y) <= graph.height*2);
for(let i=0;i<100;i++) m.zoom(2);
assert.ok(m.box.w >= graph.width/8);
m.reset();
assert.equal(m.history,false);
assert.equal(m.mode,'all');
assert.deepEqual(m.box, new Model(graph).box); // reset uses default visible geometry, not stale frame
assert.equal(m.search('<script>').length,0);
// Exercise actual selection adapter callbacks with a minimal event target, not a browser.
class FakeElement {
  constructor(id){this.dataset={key:id};this.handlers={};}
  addEventListener(type, fn){this.handlers[type]=fn;}
  dispatch(type, extra={}){this.handlers[type]({preventDefault(){},...extra});}
}
const el = new FakeElement('a');
let renders=0, dragging=false;
wireSelection(el,m,()=>renders++,()=>dragging);
el.dispatch('click');
assert.equal(m.selected,'a');
m.clear(); dragging=true; el.dispatch('click');
assert.equal(m.selected,null);
dragging=false; el.dispatch('keydown',{key:'Enter'});
assert.equal(m.selected,'a');
assert.equal(renders,2);
console.log('state and real selection adapter tests passed (no browser visual proof)');
