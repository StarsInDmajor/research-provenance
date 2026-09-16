'use strict';
// Pure Model + real generated/final entry in a strict DOM double, not a browser.
const assert=require('node:assert/strict');
const {test}=require('node:test');
const vm=require('node:vm');
const {execFileSync}=require('node:child_process');
const {Model,viewport}=require('../graph.js');
const {fromTree}=require('./dom-double.js');
const {readData,writeData}=require('./wire-fixture.js');
const fixture=JSON.parse(execFileSync('python3',[__dirname+'/entry_fixture.py',...process.argv.slice(2)],{maxBuffer:8e6}));
const near=(a,b)=>assert.ok(Math.abs(a-b)<1e-7,`${a} != ${b}`);
function setup(edit=()=>{}) {
  const doc=fromTree(fixture.tree),get=id=>doc.getElementById(id),data=readData(doc);
  edit(data);writeData(doc,data);
  const ctx=vm.createContext({document:doc});for(const script of fixture.scripts)vm.runInContext(script,ctx);
  assert.equal(doc.body.getAttribute('data-app-state'),'ready');
  return {doc,get,data,...vm.runInContext('boot(document).api',ctx)};
}
function select(s,n) {s.model.select(n.id);s.render();return s.get('detail-content');}
function section(content,heading) {
  const i=content.children.findIndex(e=>e.localName==='h3'&&e.textContent===heading);
  assert.ok(i>=0,`visible heading missing: ${heading}`);return content.children[i+1];
}
const node=(id,raw={})=>({id,label:'label '+id,title:'title '+id,current:true,roles:[],x:0,y:0,raw});
const graph=(nodes,width=5500,height=2968,edges=[])=>({nodes,width,height,edges,revisionEdges:[]});

test('node scope and nonempty assumptions are plain, ordered, safe, complete and immutable',()=>{
  let target;const s=setup(d=>{target=d.graph.nodes.find(n=>n.current);Object.assign(target.raw,{scope:{statement:'Historical only <script>alert(1)</script>',conditions:['not current','approval needed']},assumptions:['<img onerror=x>','conditional only']});});
  const before=JSON.stringify(s.data),c=select(s,target);
  const scope=section(c,'适用范围'),assumptions=section(c,'前提');
  assert.match(scope.textContent,/Historical only <script>alert\(1\)<\/script>/);assert.match(scope.textContent,/not current/);assert.match(scope.textContent,/approval needed/);
  assert.match(assumptions.textContent,/<img onerror=x>/);assert.match(assumptions.textContent,/conditional only/);
  const headings=c.children.filter(e=>e.localName==='h3').map(e=>e.textContent),i=headings.indexOf('原始陈述');
  assert.deepEqual(headings.slice(i,i+3),['原始陈述','适用范围','前提']);
  assert.equal(c.querySelectorAll('script,img,a').length,0);assert.equal(JSON.stringify(s.data),before);
});
test('empty assumptions omitted; relation scope not duplicated',()=>{
  let target;const s=setup(d=>{target=d.graph.nodes.find(n=>n.current);target.raw.assumptions=[];});
  assert.ok(!select(s,target).children.some(e=>e.localName==='h3'&&e.textContent==='前提'));
  const e=s.data.graph.edges.find(e=>e.raw.scope);assert.ok(e);
  const c=select(s,e);assert.equal(c.children.filter(e=>e.localName==='h3'&&e.textContent==='适用范围').length,1);
});
for(const [label,value,tail] of [
  ['string','x'.repeat(4200)+'END','END'],
  ['array',Array.from({length:42},(_,i)=>'assumption '+i),'assumption 41'],
  ['object',Object.fromEntries(Array.from({length:42},(_,i)=>['field'+i,'value'+i])),'value41'],
  ['depth',{a:{b:{c:{d:{e:{f:{g:{h:'DEEP-END'}}}}}}}},'DEEP-END'],
])test('bounded '+label+' qualifiers explicitly disclose truncation and retain full raw fallback',()=>{
  let target;const s=setup(d=>{target=d.graph.nodes.find(n=>n.current);target.raw.assumptions=value;});
  const c=select(s,target),p=section(c,'前提');assert.ok(p.textContent.length<4200);assert.match(p.textContent,/见完整原文|见完整原始字段/);
  const raw=c.querySelectorAll('details').find(e=>e.children[0]?.textContent==='完整原始字段 · 纯文本（含精确 ID）');
  assert.ok(raw.textContent.includes(tail));
});
test('logical search ranks exact ID, logical ID, title then substrings; all duplicates/history survive deterministically',()=>{
  const nodes=[node('z',{logical_id:'needle'}),node('b',{logical_id:'needle'}),node('old',{logical_id:'needle'}),node('title'),node('needle'),node('a-sub')];
  nodes[2].current=false;nodes[3].title='needle';nodes[5].label='needle suffix';
  const m=new Model(graph(nodes)),before=JSON.stringify(m.snapshot());
  assert.deepEqual(m.search(' NEEDLE ').map(n=>n.id),['needle','b','old','z','title','a-sub']);
  assert.deepEqual(new Model(graph([...nodes].reverse())).search('needle').map(n=>n.id),m.search('needle').map(n=>n.id));
  assert.deepEqual(m.search('label').map(n=>n.id),['b','needle','old','title','z'],'ordinary substring ties explicitly use exact ID order');
  assert.equal(JSON.stringify(m.snapshot()),before);assert.deepEqual(m.search(''),[]);
});
test('duplicate raw titles remain candidates below exact logical matches; reading numbers are not invented aliases',()=>{
  const a=node('a',{title:'Duplicated title'}),b=node('b',{title:'Duplicated title'}),c=node('c',{logical_id:'Duplicated title'});
  a.label='R02 actual reading title';b.current=false;
  const m=new Model(graph([b,a,c]));assert.deepEqual(m.search('duplicated title').map(n=>n.id),['c','a','b']);
  assert.deepEqual(m.search('R02').map(n=>n.id),['a'],'existing label substring remains searchable');
  assert.deepEqual(m.search('R99'),[],'no manufactured R-number registry');
});
test('exact revision ID Enter selects the exact record despite endpoint substring candidates',()=>{
  const s=setup(),n=s.model.graph.nodes.find(n=>n.current),q=s.get('search');
  q.value=n.id;q.dispatch('input');q.dispatch('keydown',{key:'Enter'});assert.equal(s.model.selected,n.id);
});
test('same logical ID multiple heads and old revision: labeled candidates, no implicit Enter winner, keyboard and Back retained',()=>{
  const s=setup(d=>{for(const n of d.graph.nodes.slice(0,3)){n.raw.logical_id='shared-lineage';n.label='Same label';}d.graph.nodes[0].current=true;d.graph.nodes[1].current=true;d.graph.nodes[2].current=false;});
  const q=s.get('search');q.value='shared-lineage';q.dispatch('input');
  const rows=s.get('search-results').querySelectorAll('button');assert.equal(rows.length,3);
  assert.equal(new Set(rows.map(b=>b.textContent)).size,3);assert.match(s.get('search-results').textContent,/当前修订/);assert.match(s.get('search-results').textContent,/旧修订/);
  assert.equal(s.model.selected,null);q.dispatch('keydown',{key:'Enter'});assert.equal(s.model.selected,null,'ambiguous Enter must require explicit candidate choice');
  q.dispatch('keydown',{key:'ArrowDown'});q.dispatch('keydown',{key:'ArrowDown'});
  const wanted=s.model.search(q.value)[1].id,snapshot=JSON.stringify(s.model.snapshot());
  q.dispatch('keydown',{key:'Enter'});assert.equal(s.model.selected,wanted);s.get('back').dispatch('click');
  assert.equal(JSON.stringify(s.model.snapshot()),snapshot);assert.equal(q.value,'shared-lineage');assert.equal(s.get('search-results').querySelectorAll('button').length,3);
});
for(const [label,w,h,nodes,edges] of [
  ['large R02-sized fit',5500,2968,[{...node('a'),x:4380,y:916}],[]],
  ['tiny graph',20,10,[node('a')],[]],
  ['negative hull',800,600,[{...node('a'),x:-200,y:-100}],[]],
  ['long edge hull',800,600,[node('a'),{...node('b'),x:4000,y:100}], [{id:'edge',from:'a',to:'b',current:true,category:'science',label:'depends',type:'depends-on'}]],
  ['self loop',800,600,[node('a')],[{id:'loop',from:'a',to:'a',current:true,category:'science',label:'loop',type:'supports'}]],
])test(label+': fit unchanged, +/- monotonic including out-of-range saturation, no routing or navigation',()=>{
  const g=graph(nodes,w,h,edges),before=JSON.stringify(g),m=new Model(g);
  m.fitTargets(nodes,edges);const fit={...m.box};
  for(const factor of [1.1,1/1.1]) {
    m.box={...fit};const stats=JSON.stringify(m.routeStats),stack=m.stack.length;
    m.zoom(factor);assert.ok(factor>1?m.box.w<=fit.w:m.box.w>=fit.w);
    assert.equal(JSON.stringify(m.routeStats),stats);assert.equal(m.stack.length,stack);
  }
  if(label==='large R02-sized fit'){near(fit.w,194*5500/2968);m.box={...fit};m.zoom(1.1);near(m.box.w,fit.w/1.1);}
  for(const width of [1e-6,w*10,fit.w])for(const factor of [1.1,1/1.1]) {
    m.box={x:0,y:0,w:width,h:width*h/w};
    for(let i=0;i<200;i++){const old=m.box.w;m.zoom(factor);assert.ok(factor>1?m.box.w<=old:m.box.w>=old);assert.ok(Number.isFinite(m.box.w)&&m.box.w>0);}
    const box=JSON.stringify(m.box);m.zoom(1);assert.equal(JSON.stringify(m.box),box);
  }
  assert.equal(JSON.stringify(g),before);
});
test('finite positive boxes keep both dimensions monotonic and preserve their own aspect',()=>{
  const m=new Model(graph([node('a')]));
  for(const w of [1e-6,100,359.5,20000])for(const h of [1e-6,200,10000])for(const factor of [1.1,1/1.1,Number.MAX_VALUE,Number.MIN_VALUE]) {
    m.box={x:0,y:0,w,h};m.zoom(factor);
    assert.ok(factor>1?m.box.w<=w&&m.box.h<=h:m.box.w>=w&&m.box.h>=h);
    assert.ok(Number.isFinite(m.box.w)&&Number.isFinite(m.box.h));near(m.box.h/m.box.w,h/w);
  }
});
test('actual entry reveal +/- and fine/zero wheel preserve pointer anchor, reciprocal scale, cache, and Back snapshot',()=>{
  const s=setup(),m=s.model,n=m.graph.nodes.find(n=>n.label.startsWith('R02 '))||m.graph.nodes.find(n=>n.current);
  const before=JSON.stringify(m.snapshot());m.reveal(n.id);s.render();const fit={...m.box},stats=JSON.stringify(m.routeStats),stack=m.stack.length;
  s.get('zoom-in').dispatch('click');near(m.box.w,fit.w/1.1);s.get('zoom-out').dispatch('click');near(m.box.w,fit.w);
  const svg=s.get('graph'),p=viewport(svg.getBoundingClientRect(),m.box,400,250),anchor={x:m.box.x+p.x*m.box.w,y:m.box.y+p.y*m.box.h};
  svg.dispatch('wheel',{deltaY:-1,deltaMode:0,clientX:400,clientY:250});near(m.box.w,fit.w/Math.exp(.0012));near(m.box.x+p.x*m.box.w,anchor.x);near(m.box.y+p.y*m.box.h,anchor.y);
  svg.dispatch('wheel',{deltaY:1,deltaMode:0,clientX:400,clientY:250});near(m.box.w,fit.w);
  const box=JSON.stringify(m.box);svg.dispatch('wheel',{deltaY:0,clientX:400,clientY:250});assert.equal(JSON.stringify(m.box),box);
  assert.equal(JSON.stringify(m.routeStats),stats);assert.equal(m.stack.length,stack);s.get('back').dispatch('click');assert.equal(JSON.stringify(m.snapshot()),before);
});
if(process.argv[2])test('actual reion3 R86/R72 qualifiers and logical lookup, candidate and five history marks retained',()=>{
  const s=setup(),d=s.data;assert.equal(d.graph.nodes.length,86);assert.equal(d.graph.edges.length,77);
  assert.equal(d.graph.nodes.filter(n=>n.presentationStatus).length,5);assert.equal(Object.keys(d.records).length,299);
  assert.equal(Object.keys(d.sources).length,24);assert.ok(Object.values(d.sources).every(s=>s.source_type==='metadata-only'));
  const before=JSON.stringify(d);
  for(const [prefix,query] of [['R86 ','reion3-historical-pt-stack'],['R72 ','forest-resumption']]) {
    const n=d.graph.nodes.find(n=>n.label.startsWith(prefix));assert.ok(n);
    assert.deepEqual(Array.from(s.model.search(query),n=>n.id),[n.id]);
    assert.deepEqual(Array.from(s.model.search(n.id).filter(x=>!x.from),n=>n.id),[n.id]);
    const c=select(s,n);assert.ok(section(c,'适用范围').textContent.includes(n.raw.scope.statement));
    for(const condition of n.raw.scope.conditions)assert.ok(section(c,'适用范围').textContent.includes(condition));
  }
  const candidate=d.graph.nodes.find(n=>n.label.startsWith('R27 '));assert.match(select(s,candidate).textContent,/unpromoted/);assert.equal(candidate.presentationStatus,undefined);
  assert.equal(JSON.stringify(d),before);
});
