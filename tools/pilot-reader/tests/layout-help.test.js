'use strict';
const assert=require('node:assert/strict');
const {test}=require('node:test');
const vm=require('node:vm');
const {execFileSync}=require('node:child_process');
const {fromTree}=require('./dom-double.js');
const {Model}=require('../graph.js');
const fixture=JSON.parse(execFileSync('python3',[__dirname+'/entry_fixture.py',...process.argv.slice(2)],{maxBuffer:4e6}));
function setup(){const doc=fromTree(fixture.tree),get=id=>doc.getElementById(id),ctx=vm.createContext({document:doc});for(const s of fixture.scripts)vm.runInContext(s,ctx);assert.equal(doc.body.getAttribute('data-app-state'),'ready');return {doc,get,api:vm.runInContext('boot(document).api',ctx)};}
function contains(m){for(const e of [...m.view().edges,...m.view().revisionEdges])if(e.labelStatus==='placed'){assert.ok(m.box.x<=e.labelX-e.labelHalfWidth&&m.box.x+m.box.w>=e.labelX+e.labelHalfWidth);assert.ok(m.box.y<=e.labelY-20&&m.box.y+m.box.h>=e.labelY+8);}}
test('initial and reset fit visible negative label bounds, without selection or changing coordinates',()=>{
 const g={nodes:[{id:'a',x:0,y:0,current:true,roles:[],raw:{},label:'a'}],edges:[{id:'loop',from:'a',to:'a',label:'依赖',current:true,category:'science',raw:{}}],revisionEdges:[],width:350,height:220,roles:{}};
 const before=JSON.stringify(g),m=new Model(g);contains(m);m.pan(17,29);m.reset();contains(m);assert.equal(m.selected,null);assert.equal(JSON.stringify(g),before);
});
test('toolbar help reveals collapsed pane and native guide, focuses summary; navigation/search/DOM/cache intact in fullscreen',()=>{
 const s=setup(),m=s.api.model,help=s.get('help');assert.ok(help,'discoverable toolbar 说明 action');assert.equal(help.textContent,'说明');assert.ok(s.get('toolbar').contains(help));
 m.select(m.graph.nodes.find(n=>n.current).id);s.api.render();s.get('search').value='keep';s.get('search').dispatch('input');
 const state=JSON.stringify(m.snapshot()),stack=JSON.stringify(m.stack),content=[...s.get('detail-content').children],computations=m.routeStats.computations;
 s.get('details').scrollTop=123;s.get('toggle-details').dispatch('click');s.get('fullscreen').dispatch('click');
 // Native button Enter/Space activation synthesizes click in browsers; do not
 // add a duplicate key handler. Strict double exercises the activation handler.
 help.focus();help.dispatch('click');
 assert.equal(s.get('details').hidden,false);assert.equal(s.get('reading-guide').open,true);assert.equal(s.doc.activeElement,s.get('reading-guide-summary'));assert.equal(s.get('details').scrollTop,0);
 assert.equal(s.get('toggle-details').getAttribute('aria-expanded'),'true');assert.equal(s.get('workspace').getAttribute('data-canvas-mode'),'window');
 assert.equal(JSON.stringify(m.snapshot()),state);assert.equal(JSON.stringify(m.stack),stack);assert.deepEqual(s.get('detail-content').children,content);assert.equal(m.routeStats.computations,computations);
 assert.equal(s.get('search').value,'keep');assert.equal(help.getAttribute('type'),'button');assert.equal(s.get('reading-guide-summary').tagName.toLowerCase(),'summary');
 const guide=s.get('reading-guide').textContent;assert.match(guide,/位置.*不.*时间|不是时间/);assert.match(guide,/原.*箭头/);
 if(m.graph.aliasLegend){for(const letter of ['Q','G','P','I','B','N'])assert.ok(guide.includes(letter+' ='));assert.match(guide,/不是.*kind/);}
});
test('help returns to details on same-node and different-node selection without losing Back scroll',()=>{
 const s=setup(),m=s.api.model,panel=s.get('details'),guide=s.get('reading-guide');
 const nodes=m.graph.nodes.filter(n=>n.current),a=nodes[0],b=nodes[1];
 const el=id=>s.get('graph').querySelectorAll('[data-key]').find(e=>e.dataset.key===id);
 el(a.id).dispatch('click');panel.scrollTop=123;
 const original=[...s.get('detail-content').children];
 s.get('help').dispatch('click');s.get('help').dispatch('click');
 el(a.id).dispatch('click');
 assert.equal(guide.open,false,'same-node click must leave help');
 assert.equal(panel.scrollTop,123);assert.deepEqual(s.get('detail-content').children,original);
 assert.equal(s.doc.activeElement,s.get('detail-content').querySelectorAll('h2')[0]);
 s.get('help').dispatch('click');el(b.id).dispatch('click');
 assert.equal(guide.open,false);assert.equal(m.selected,b.id);assert.equal(panel.scrollTop,0);
 s.get('back').dispatch('click');assert.equal(m.selected,a.id);assert.equal(panel.scrollTop,123);
});
test('filter/history/local/back/fullscreen preserve full-case node coordinates; reset fits; real counts retained',()=>{
 const s=setup(),m=s.api.model,before=JSON.stringify(m.graph.nodes),coords=()=>JSON.stringify(m.graph.nodes);
 m.select(m.graph.nodes.find(n=>n.current).id);m.focus();m.expand('out');m.back();m.setHistory(true);m.setFilter('mainline');m.restoreAll();m.back();m.reset();s.api.render();s.get('fullscreen').dispatch('click');s.get('help')?.dispatch('click');
 assert.equal(coords(),before);contains(m);
 if(m.graph.readingSources){assert.equal(m.view().nodes.length,15);assert.equal(m.view().edges.length,13);m.showAll();assert.equal(m.view().nodes.length,19);assert.equal(m.view().edges.length,23);assert.equal(m.view().revisionEdges.length,4);}
});
