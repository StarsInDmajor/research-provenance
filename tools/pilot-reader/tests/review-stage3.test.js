'use strict';
// Generated scripts + strict DOM/event double, not a browser or visual acceptance.
const assert=require('node:assert/strict');
const {test}=require('node:test');
const vm=require('node:vm');
const fs=require('node:fs');
const {execFileSync}=require('node:child_process');
const {tagGroups}=require('../graph.js');
const {fromTree}=require('./dom-double.js');
const {readData,writeData}=require('./wire-fixture.js');
const fixture=JSON.parse(execFileSync('python3',[__dirname+'/entry_fixture.py',...process.argv.slice(2)],{maxBuffer:8e6}));
function setup(edit=()=>{}) {
  const doc=fromTree(fixture.tree),get=id=>doc.getElementById(id),data=readData(doc);
  edit(data);writeData(doc,data);
  const ctx=vm.createContext({document:doc});for(const script of fixture.scripts)vm.runInContext(script,ctx);
  assert.equal(doc.body.getAttribute('data-app-state'),'ready');
  return {doc,get,data,...vm.runInContext('boot(document).api',ctx)};
}
const click=(s,id)=>s.get(id).dispatch('click');
const groups=s=>s.get('domain-groups').querySelectorAll('details');
const rows=s=>s.get('domain-groups').querySelectorAll('button');
const plain=x=>JSON.parse(JSON.stringify(x));
// Target -> document propagation including capture, absent from the shared double.
function escape(s,target) {
  let stopped=false;
  const event={key:'Escape',target,defaultPrevented:false,preventDefault(){this.defaultPrevented=true;},stopPropagation(){stopped=true;}};
  // Production registers the fullscreen then panel capture handlers before bubble.
  const handlers=s.doc.handlers.keydown;
  for(const fn of handlers.slice(0,-1)){fn(event);if(stopped)return;}
  for(const fn of target.handlers.keydown || []){fn(event);if(stopped)return;}
  if(!stopped)handlers.at(-1)(event);
}
function tagged(d) {
  for(const n of d.graph.nodes)n.raw.tags=['custom'];
  d.graph.nodes[0].raw.tags=['design','design','custom'];
  d.graph.nodes[1].raw.tags=[];
  d.graph.nodes[2].current=false;d.graph.nodes[2].ghost=false;
}
test('pure tag inventory: exact membership, unique IDs, multi tags, unknown safe fallback and untagged discovery',()=>{
  assert.equal(typeof tagGroups,'function','missing tag membership helper');
  const a={id:'a',current:true,raw:{tags:['design','design','</script>','__proto__']}},b={id:'b',current:false,raw:{tags:['design']}},c={id:'c',current:true,raw:{}};
  const input=[a,b,c,a],before=JSON.stringify(input),all=tagGroups(input);
  assert.equal(JSON.stringify(input),before);
  const design=all.find(g=>g.tag==='design');assert.deepEqual(design.nodes.map(n=>n.id),['a','b']);
  assert.equal(design.current,1);assert.equal(design.history,1);assert.equal(design.label,'参数与设计');
  assert.equal(all.find(g=>g.tag==='</script>').category,'其他标签');assert.equal(all.find(g=>g.tag==='__proto__').label,'__proto__');
  assert.deepEqual(all.find(g=>g.tag===null).nodes.map(n=>n.id),['c']);
  assert.deepEqual(new Set(all.flatMap(g=>g.nodes.map(n=>n.id))),new Set(['a','b','c']));
  assert.deepEqual(tagGroups([{id:'empty',current:true,raw:{tags:[]}}])[0].nodes.map(n=>n.id),['empty']);
});
test('optional toolbar panel and group browsing do not change scientific state, selection, cache, stack or details',()=>{
  const s=setup(tagged),toggle=s.get('domains');assert.ok(toggle,'domain toolbar control missing');
  assert.equal(toggle.disabled,false);assert.equal(toggle.getAttribute('aria-controls'),'domain-panel');assert.equal(toggle.getAttribute('aria-expanded'),'false');
  assert.equal(s.get('domain-panel').hidden,true);assert.ok(s.get('workspace').contains(s.get('domain-panel')));
  s.model.select(s.data.graph.nodes[1].id);s.render();
  const state=JSON.stringify(s.model.snapshot()),stack=JSON.stringify(s.model.stack),stats=JSON.stringify(s.model.routeStats),data=JSON.stringify(s.model.graph),children=[...s.get('detail-content').children];
  click(s,'domains');groups(s)[0].open=true;groups(s)[0].children[0].focus();
  assert.match(s.get('domain-panel').textContent,/领域\/标签/);assert.match(s.get('domain-panel').textContent,/多标签.*不相加/);assert.match(s.get('domain-panel').textContent,/不是科研关系/);
  assert.match(s.get('domain-panel').textContent,/未标记/);assert.match(s.get('domain-panel').textContent,/其他标签/);
  s.get('domain-history').checked=true;s.get('domain-history').dispatch('change');
  click(s,'domains');
  assert.equal(JSON.stringify(s.model.snapshot()),state);assert.equal(JSON.stringify(s.model.stack),stack);assert.equal(JSON.stringify(s.model.routeStats),stats);assert.equal(JSON.stringify(s.model.graph),data);assert.deepEqual(s.get('detail-content').children,children);
});
test('list defaults to current, explicitly counts history, history toggle is list-only; unknown text never creates HTML',()=>{
  const s=setup(d=>{tagged(d);d.graph.nodes[0].raw.tags=['</script><img src=x>'];});click(s,'domains');
  assert.equal(s.get('domain-history').checked,false);
  const old=s.data.graph.nodes[2].id;assert.ok(!rows(s).some(b=>b.dataset.target===old));
  assert.match(s.get('domain-panel').textContent,/历史 1/);assert.ok(s.get('domain-panel').textContent.includes('</script><img src=x>'));
  assert.equal(s.get('domain-panel').querySelectorAll('script,img,a').length,0);
  s.get('domain-history').checked=true;s.get('domain-history').dispatch('change');
  assert.ok(rows(s).some(b=>b.dataset.target===old));assert.equal(s.model.history,false);
  const b=rows(s).find(b=>b.dataset.target===old);assert.match(b.textContent,/旧修订/);b.dispatch('click');assert.equal(s.model.selected,old);assert.equal(s.model.history,true);
});
test('domain exact navigation does not force one-hop; Back restores named origin, open groups and scroll even from empty selection',()=>{
  const s=setup(tagged);click(s,'domains');const group=groups(s).find(g=>g.dataset.tag===JSON.stringify('design'));group.open=true;s.get('domain-panel').scrollTop=46;
  const b=group.querySelectorAll('button')[0],id=b.dataset.target,key=b.dataset.focus,canon=JSON.stringify(s.model.graph),local=s.model.local,box=plain(s.model.box);
  assert.deepEqual(JSON.parse(key),['domain','design',id]);assert.ok(b.textContent.includes(s.model.items.get(id).title));
  const frames=s.model.stack.length;b.dispatch('click');assert.equal(s.model.selected,id);assert.equal(s.model.local,local);assert.equal(s.model.stack.length,frames+1);
  assert.equal(s.get('domain-panel').hidden,true);assert.equal(s.get('reading-guide').open,false);assert.ok(s.get('detail-content').contains(s.doc.activeElement));
  click(s,'back');assert.equal(s.model.selected,null);assert.deepEqual(plain(s.model.box),box);
  assert.equal(s.get('domain-panel').hidden,false);assert.equal(s.get('domains').getAttribute('aria-expanded'),'true');assert.equal(s.doc.activeElement.dataset.focus,key);assert.equal(s.get('domain-panel').scrollTop,46);
  assert.equal(groups(s).find(g=>g.dataset.tag===JSON.stringify('design')).open,true);assert.equal(JSON.stringify(s.model.graph),canon);
});
test('help, detail toggle and Escape cannot trap details or clear graph selection; fullscreen Escape wins first',()=>{
  const s=setup(tagged);s.model.select(s.data.graph.nodes[0].id);s.render();const id=s.model.selected;
  const details=s.get('detail-content'),source=details.querySelectorAll('details').at(-1);source.open=true;source.children[0].focus();s.get('details').scrollTop=35;
  click(s,'help');click(s,'domains');assert.equal(s.get('reading-guide').open,false);assert.equal(s.get('details').scrollTop,35);
  click(s,'help');assert.equal(s.get('domain-panel').hidden,true);assert.equal(s.get('reading-guide').open,true);
  click(s,'domains');click(s,'toggle-details');click(s,'toggle-details');assert.equal(s.model.selected,id);assert.equal(source.open,true);
  click(s,'fullscreen');escape(s,s.get('search'));assert.equal(s.get('workspace').getAttribute('data-canvas-mode'),'normal');assert.equal(s.get('domain-panel').hidden,false);
  escape(s,s.get('search'));assert.equal(s.get('domain-panel').hidden,true);assert.equal(s.model.selected,id);assert.equal(s.doc.activeElement,s.get('domains'));
  // Direct canvas selection also closes auxiliary panels, without content replacement on browsing.
  click(s,'domains');s.get('graph').querySelectorAll('[data-key]').find(e=>e.dataset.key===s.data.graph.nodes[1].id).dispatch('click');assert.equal(s.get('domain-panel').hidden,true);assert.equal(s.get('reading-guide').open,false);
});
test('same-node domain selection returns focus to visible details; local/source state survives domain jump and Back',()=>{
  const s=setup(tagged),n=s.data.graph.nodes[0];s.model.select(n.id);s.model.focus();s.render();
  const source=s.get('detail-content').querySelectorAll('details').at(-1);source.open=true;source.children[0].focus();s.get('details').scrollTop=73;
  const local=plain(s.model.local);click(s,'domains');rows(s).find(b=>b.dataset.target===n.id).dispatch('click');
  assert.equal(s.get('domain-panel').hidden,true);assert.ok(s.get('detail-content').contains(s.doc.activeElement),'same-node jump left focus in hidden domain panel');
  assert.deepEqual(plain(s.model.local),local);assert.equal(source.open,true);assert.equal(s.get('details').scrollTop,73);
  click(s,'domains');const other=rows(s).find(b=>b.dataset.target!==n.id);assert.ok(other);other.dispatch('click');
  assert.deepEqual(plain(s.model.local),local);click(s,'back');assert.equal(s.model.selected,n.id);assert.equal(s.get('details').scrollTop,73);
  assert.equal(s.get('detail-content').querySelectorAll('details').at(-1).open,true);
});
test('domain Back works with native-like iterable NodeList, not Array-only methods',()=>{
  const s=setup(tagged),root=s.get('domain-groups'),query=root.querySelectorAll.bind(root);
  root.querySelectorAll=selector=>{const array=query(selector);return Object.assign({length:array.length,[Symbol.iterator]:function*(){yield* array;}},array);};
  click(s,'domains');const b=[...root.querySelectorAll('button')][0];b.dispatch('click');click(s,'back');
  assert.equal(s.doc.body.getAttribute('data-app-state'),'ready');assert.equal(s.doc.activeElement.dataset.focus,b.dataset.focus);
});
test('BOOT rejects missing/mis-scoped domain controls and locks dynamic controls after failure',()=>{
  for(const broken of ['missing','scope']) {
    const doc=fromTree(fixture.tree),panel=doc.getElementById('domain-panel');assert.ok(panel);
    if(broken==='missing')panel.parentNode.children=panel.parentNode.children.filter(c=>c!==panel);
    else doc.getElementById('details').appendChild(panel);
    const ctx=vm.createContext({document:doc});for(const script of fixture.scripts)vm.runInContext(script,ctx);
    assert.equal(doc.body.getAttribute('data-app-state'),'failed');assert.equal(doc.getElementById('domains').disabled,true);
  }
  const s=setup(tagged);click(s,'domains');s.model.view=()=>{throw Error('synthetic render failure');};click(s,'fit');
  assert.equal(s.doc.body.getAttribute('data-app-state'),'failed');assert.equal(s.get('domain-panel').inert,true);
  assert.equal(s.get('domain-history').disabled,true);assert.ok(rows(s).every(b=>b.disabled));
});
if(process.argv[2])test('actual recorded 12 areas cover all 86 nodes and 23 weak components; isolated R48/R51/R68 discoverable',()=>{
  const s=setup();assert.equal(s.data.graph.nodes.length,86);assert.equal(s.data.graph.edges.length,77);
  const mapPath=process.env.RP_NODE_MAP||'node-map.json';
  const map=JSON.parse(fs.readFileSync(mapPath,'utf8'));
  const areas=['design','simulation','observations','uvlf','xhi','forest','cmb','inference','lifecycle','guard','manuscript','governance'];
  const all=tagGroups(s.data.graph.nodes);assert.deepEqual(all.filter(g=>g.category==='研究领域').map(g=>g.tag).sort(),areas.sort());
  assert.deepEqual(new Set(all.flatMap(g=>g.nodes.map(n=>n.id))),new Set(s.data.graph.nodes.map(n=>n.id)));
  for(const [logical,row] of Object.entries(map)) {
    const n=s.data.graph.nodes.find(n=>n.id===row.revision_id);assert.equal(n.raw.logical_id,logical);assert.ok(n.raw.tags.includes(row.area));
    assert.ok(all.find(g=>g.tag===row.area).nodes.some(x=>x.id===n.id));
  }
  const unseen=new Set(s.data.graph.nodes.map(n=>n.id));let components=0;
  while(unseen.size){components++;const queue=[unseen.values().next().value];unseen.delete(queue[0]);while(queue.length){const id=queue.pop();for(const e of s.data.graph.edges){const other=e.from===id?e.to:e.to===id?e.from:null;if(unseen.delete(other))queue.push(other);}}}
  assert.equal(components,23);const canonical=JSON.stringify(s.model.graph);
  click(s,'domains');assert.equal(new Set(rows(s).map(b=>b.dataset.target)).size,86);assert.match(s.get('domain-panel').textContent,/当前 86.*历史 0/);
  for(const number of [48,51,68]) {
    const mapped=Object.values(map).find(n=>n.number===number),g=groups(s).find(g=>g.dataset.tag===JSON.stringify(mapped.area));g.open=true;
    const b=g.querySelectorAll('button').find(b=>b.dataset.target===mapped.revision_id);assert.ok(b);const key=b.dataset.focus;b.dispatch('click');
    assert.equal(s.model.selected,mapped.revision_id);assert.equal(s.model.local,null);assert.equal(s.get('domain-panel').hidden,true);
    if(number===68)assert.match(s.get('detail-content').textContent,/历史/);
    click(s,'back');assert.equal(s.doc.activeElement.dataset.focus,key);assert.equal(s.get('domain-panel').hidden,false);
  }
  assert.equal(JSON.stringify(s.model.graph),canonical);
});
