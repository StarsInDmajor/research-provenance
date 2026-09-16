'use strict';
const assert=require('node:assert/strict');
const {test}=require('node:test');
const {Model}=require('../graph.js');
const node=(id,x,y,roles=['primary'])=>({id,x,y,roles,current:true,kind:'Question',bindingIds:[],raw:{},label:id});
const edge=(id,from,to)=>({id,from,to,current:true,category:'science',type:'depends-on',label:'依赖',raw:{},path:'M 270 257 L 290 -300 L 620 -300 L 640 257',labelX:455,labelY:-310});
const graph=()=>({nodes:[node('A',0,200),node('B',320,200,['diagnostic']),node('C',640,200)],edges:[edge('ac','A','C')],revisionEdges:[],width:960,height:700,roles:{}});
const coords=g=>g.nodes.map(n=>[n.id,n.x,n.y]);
function points(e){return e.path.match(/-?\d+(?:\.\d+)?/g).map(Number).reduce((a,n,i,all)=>i%2?a:[...a,[n,all[i+1]]],[]);}
function crosses(a,b,n,margin=12){
  // Independent exact open-rectangle segment slab clipping, not route sampling.
  let lo=0,hi=1;
  for(const [p,q,min,max] of [[a[0],b[0],n.x-margin,n.x+270+margin],[a[1],b[1],n.y-margin,n.y+114+margin]]){
    if(p===q){if(p<=min||p>=max)return false;continue;}
    const t=[(min-p)/(q-p),(max-p)/(q-p)].sort((a,b)=>a-b);lo=Math.max(lo,t[0]);hi=Math.min(hi,t[1]);
  }
  return lo<hi && hi>0 && lo<1;
}
function clear(e,nodes){assert.equal(e.routeStatus,'routed',e.id);const ps=points(e);for(let i=1;i<ps.length;i++)for(const n of nodes.filter(n=>n.id!==e.from&&n.id!==e.to))assert.ok(!crosses(ps[i-1],ps[i],n),JSON.stringify({e,n}));
 if(e.labelStatus!=='placed')return;
 for(const n of nodes){const half=e.labelHalfWidth;assert.ok(e.labelX+half<=n.x-12||e.labelX-half>=n.x+282||e.labelY+8<=n.y-12||e.labelY-20>=n.y+126,'label intersects '+n.id);}}
test('hide obstruction shortens route; show restores it without moving nodes/canonical mutation',()=>{
 const g=graph(),before=JSON.stringify(g),m=new Model(g),full=m.view().edges[0];clear(full,g.nodes);
 m.local=['A','C'];const near=m.view().edges[0];clear(near,m.view().nodes);
 assert.ok(Math.min(...points(near).map(p=>p[1]))>=240,'no empty global-top arch');assert.notEqual(full.path,near.path);
 m.local=null;assert.equal(m.view().edges[0].path,full.path);assert.equal(JSON.stringify(g),before);assert.deepEqual(coords(g),coords(graph()));
});
test('cache and Back derive routes by visible sets, retain actual viewport, fit/reveal uses current geometry',()=>{
 const m=new Model(graph());m.view();assert.ok(m.routeStats,'routing cache metadata');const runs=m.routeStats.computations;
 m.pan(17,23);m.zoom(1.2);m.select('A');m.view();assert.equal(m.routeStats.computations,runs);
 m.transition(()=>{m.local=['C','A'];m.fit();});const path=m.view().edges[0].path;const fit={...m.box};assert.ok(fit.y>-200,'fit excludes obsolete label arch');
 m.pan(8,13);const box={...m.box};m.restoreAll();assert.notEqual(m.view().edges[0].path,path);m.back();assert.equal(m.view().edges[0].path,path);assert.deepEqual(m.box,box);
 m.reveal('ac');assert.ok(m.box.y>-200);assert.equal(m.items.get('ac').path,graph().edges[0].path);
 m.local=['A','C'];m.view();assert.equal(m.routeStats.computations,2,'order-independent visible set cache');
});
test('unobstructed horizontal/reverse/vertical/diagonal use short directed paths; nearby multi-obstacle routes clear',()=>{
 for(const [x,y] of [[640,200],[0,700],[640,700]])for(const reverse of [false,true]){
  const g=graph();g.nodes=[node('z',0,200),node('a',x,y)];g.edges=[edge('e',reverse?'a':'z',reverse?'z':'a')];const e=new Model(g).view().edges[0];clear(e,g.nodes);
  assert.ok(points(e).every(p=>p[1]>=190));assert.equal(e.from,g.edges[0].from);
 }
 const g=graph();g.nodes.push(node('D',320,30),node('E',320,390));clear(new Model(g).view().edges[0],g.nodes);
});
test('parallel/reverse pairs and self loops stay distinct with stable lanes when filtering history',()=>{
 const g=graph();g.edges.push(edge('reverse','C','A'),edge('ac2','A','C'),edge('loop1','A','A'),edge('loop2','A','A'));
 g.edges[1].current=false;g.revisionEdges=[{...edge('revision:A:C','A','C'),category:'revision'}];
 const m=new Model(g);m.history=true;const v=m.view(),all=[...v.edges,...v.revisionEdges];for(const e of all)clear(e,v.nodes);
 assert.equal(new Set(all.map(e=>e.path)).size,all.length);const before=new Map(all.map(e=>[e.id,e.path]));m.history=false;
 for(const e of m.view().edges)assert.equal(e.path,before.get(e.id),'filter must not renumber lanes');
});
test('role context, local expansion, historical obstacles and restored views route only visible objects',()=>{
 const g=graph();g.nodes[1].current=false;g.revisionEdges=[{...edge('r','B','C'),category:'revision'}];const m=new Model(g);
 const direct=m.view().edges[0].path;m.setFilter('mainline');assert.equal(m.view().edges[0].path,direct);
 m.setHistory(true);assert.notEqual(m.view().edges[0].path,direct);m.select('A');m.focus();assert.equal(m.view().edges[0].path,direct);
 m.expand('in');assert.ok(m.view().nodes.some(n=>n.id==='B'));assert.notEqual(m.view().edges[0].path,direct);m.back();assert.equal(m.view().edges[0].path,direct);
});
test('bounded cache eviction and 100-node/300-edge work complete without canonical mutation',()=>{
 const g=graph();g.nodes=Array.from({length:100},(_,i)=>node('n'+i,(i%10)*320,Math.floor(i/10)*190));
 g.edges=Array.from({length:300},(_,i)=>edge('e'+i,'n'+(i%100),'n'+((i*7+13)%100)));
 const before=JSON.stringify(g),m=new Model(g);const v=m.view();assert.equal(v.edges.length,300);
 for(const e of v.edges){assert.ok(['routed','unroutable'].includes(e.routeStatus));if(e.routeStatus==='routed')clear(e,v.nodes);}
 for(let i=0;i<20;i++){m.local=['n'+i];m.view();}assert.equal(m.routeStats.entries,16);assert.equal(JSON.stringify(g),before);
});
test('fit includes actual historical label approximate width',()=>{
 const g=graph();g.nodes=g.nodes.filter(n=>n.id!=='B');g.edges[0].label='a'.repeat(16);g.edges[0].current=false;g.edges[0].raw.relation_state='invalidated';const m=new Model(g);m.history=true;m.fit();const e=m.view().edges[0];
 assert.equal(e.labelStatus,'placed');assert.ok(e.labelHalfWidth>=75);assert.ok(m.box.x<=e.labelX-e.labelHalfWidth&&m.box.x+m.box.w>=e.labelX+e.labelHalfWidth);
});
test('adjacent unobstructed edge stays direct even when label needs nearby offset',()=>{
 const g=graph();g.nodes=[node('A',0,200),node('C',320,200)];const e=new Model(g).view().edges[0];clear(e,g.nodes);assert.equal(points(e).length,2,'label must not force path detour');
});
test('enclosed endpoint reports bounded infeasible route instead of crossing a node',()=>{
 const g=graph();g.nodes.push(node('cover',260,140));const e=new Model(g).view().edges[0];assert.equal(e.routeStatus,'unroutable');assert.match(e.path,/^M [-\d.]+ [-\d.]+$/);
});
test('unobstructed parallel and reverse lines cannot collapse to the same direct segment',()=>{
 const g=graph();g.nodes=g.nodes.filter(n=>n.id!=='B');g.edges=[edge('1','A','C'),edge('2','A','C'),edge('3','C','A')];
 const all=new Model(g).view().edges;assert.equal(new Set(all.map(e=>JSON.stringify(points(e).sort((a,b)=>a[0]-b[0]||a[1]-b[1])))).size,3);
 assert.equal(new Set(all.map(e=>[e.labelX,e.labelY].join(','))).size,3,'separate label targets');
 const midY=e=>{const ps=points(e);for(let i=1;i<ps.length;i++){const [a,b]=[ps[i-1],ps[i]];if(Math.min(a[0],b[0])<455&&Math.max(a[0],b[0])>=455)return a[1]+(b[1]-a[1])*(455-a[0])/(b[0]-a[0]);}};
 assert.equal(new Set(all.map(midY)).size,3,'actual line interiors, not extra collinear vertices, must be distinct');
});
test('clipped actual r2 B01/G05 coordinate fixture shortens in both directions and keeps exact ports',()=>{
 const g=graph();g.nodes=[node('blk_00000000000000000000009113',371,306),node('obs_00000000000000000000009102',1011,306)];
 for(const reverse of [false,true]){g.edges=[edge('rel_00000000000000000000009012',g.nodes[+reverse].id,g.nodes[+!reverse].id)];
 const e=new Model(g).view().edges[0];clear(e,g.nodes);assert.ok(points(e).every(p=>p[1]===363));
 assert.deepEqual(points(e)[0],reverse?[1011,363]:[641,363]);assert.deepEqual(points(e).at(-1),reverse?[646,363]:[1006,363]);}
});
module.exports={graph,node,edge,clear,points};
