'use strict';
const assert=require('node:assert/strict');
const {test}=require('node:test');
const R=require('../routing.js');
const {Model}=require('../graph.js');
const node=(id,x,y)=>({id,x,y,current:true,roles:['primary'],label:id,raw:{}});
const edge=(id,from,to,label='支持')=>({id,from,to,label,type:'supports',category:'science',current:true,raw:{}});
const box=e=>({l:e.labelX-e.labelHalfWidth,r:e.labelX+e.labelHalfWidth,t:e.labelY-20,b:e.labelY+8});
const overlaps=(a,b)=>a.l<b.r&&a.r>b.l&&a.t<b.b&&a.b>b.t;
function segments(path){const n=(path.match(/-?\d+(?:\.\d+)?/g)||[]).map(Number),ps=[];for(let i=0;i<n.length;i+=2)ps.push([n[i],n[i+1]]);return ps.slice(1).map((p,i)=>[ps[i],p]);}
function cuts([a,b],q){let lo=0,hi=1;for(const [p,r,min,max] of [[a[0],b[0],q.l,q.r],[a[1],b[1],q.t,q.b]]){if(p===r){if(p<=min||p>=max)return false;continue;}const ts=[(min-p)/(r-p),(max-p)/(r-p)].sort((a,b)=>a-b);lo=Math.max(lo,ts[0]);hi=Math.min(hi,ts[1]);}return lo<hi-1e-9&&hi>1e-9&&lo<1-1e-9;}
function audit(nodes,edges){
 const placed=edges.filter(e=>e.labelStatus==='placed');
 for(let i=0;i<placed.length;i++){
  const a=placed[i];for(const b of placed.slice(i+1))assert.ok(!overlaps(box(a),box(b)),`labels ${a.id}/${b.id}`);
  for(const n of nodes)assert.ok(!overlaps(box(a),{l:n.x-12,r:n.x+282,t:n.y-12,b:n.y+126}),`card ${a.id}/${n.id}`);
  for(const e of edges)for(const s of segments(e.path))assert.ok(!cuts(s,box(a)),`background conceals path ${a.id}/${e.id}`);
  const leader=segments(a.labelLeaderPath);assert.equal(leader.length,1);assert.ok(Math.hypot(leader[0][1][0]-leader[0][0][0],leader[0][1][1]-leader[0][0][1])<=36.01);
  assert.ok(segments(a.path).some(([p,q])=>Math.abs(Math.hypot(p[0]-leader[0][0][0],p[1]-leader[0][0][1])+Math.hypot(q[0]-leader[0][0][0],q[1]-leader[0][0][1])-Math.hypot(q[0]-p[0],q[1]-p[1]))<.03),'leader starts on own exact path');
  for(const b of placed.filter(b=>b.id!==a.id))assert.ok(!cuts(leader[0],box(b)),`leader covers ${b.id}`);
 }
 return {edges:edges.length,routed:edges.filter(e=>e.routeStatus==='routed').length,placed:placed.length,hidden:edges.filter(e=>e.labelStatus==='hidden').length,labelOverlaps:0,nodeOverlaps:0,coveredPaths:0};
}
function crossing(){return {nodes:[node('a',0,0),node('b',800,600),node('c',800,0),node('d',0,600),node('e',0,300),node('f',800,300)],edges:[edge('1','a','b'),edge('2','c','d','推导自/依据'),edge('3','e','f','依赖')]};}
test('different endpoints with same midpoint allocate disjoint labels, independent of order',()=>{
 const g=crossing(),before=JSON.stringify(g),r=R.route(g.nodes,g.edges),all=g.edges.map(e=>({...e,...r[e.id]}));
 // Explicit old-behavior reproduction even before labelStatus exists.
 for(let i=0;i<all.length;i++)for(const b of all.slice(i+1))assert.ok(!overlaps(box(all[i]),box(b)),'independently placed midpoint labels collide');
 assert.equal(all.filter(e=>e.labelStatus==='placed').length,3);audit(g.nodes,all);
 assert.deepEqual(R.route([...g.nodes].reverse(),[...g.edges].reverse()),r);assert.equal(JSON.stringify(g),before);
});
test('empty label room retains direct scientific path, never fake unroutable; huge hidden geometry excluded from fit',()=>{
 const g={nodes:[node('a',0,200),node('b',320,200)],edges:[edge('e','a','b','W😀'.repeat(500))],revisionEdges:[],width:960,height:700};
 const m=new Model(g),e=m.view().edges[0];assert.equal(e.routeStatus,'routed','label availability must not decide routability');
 assert.equal(e.labelStatus,'hidden');assert.match(e.path,/^M [\d.]+ [\d.]+ L [\d.]+ [\d.]+$/);
 assert.equal(e.labelLeaderPath,'');m.fit();assert.ok(m.box.w<2000,'hidden width must not expand fit');
 assert.equal(m.search('W😀')[0].id,'e');m.select('e');assert.equal(m.view().edges[0].labelStatus,'hidden');
});
test('dense bound keeps every edge and label/card safety, suffix width and visible-set reallocation',()=>{
 const g=crossing();g.edges=Array.from({length:60},(_,i)=>({...g.edges[i%3],id:'e'+String(i).padStart(3,'0'),current:false,raw:{relation_state:'invalidated'}}));
 const r=R.route(g.nodes,g.edges),all=g.edges.map(e=>({...e,...r[e.id]}));audit(g.nodes,all);
 assert.equal(all.length,60);assert.ok(all.some(e=>e.labelStatus==='hidden'));
 for(const e of all){assert.ok(['placed','hidden'].includes(e.labelStatus));if(e.labelStatus==='hidden')assert.equal(e.labelLeaderPath,'');}
 const single=R.route(g.nodes,[g.edges[59]],g.edges)[g.edges[59].id];assert.equal(single.labelStatus,'placed');
 const wide={...g.edges[0],label:'WW😀中',current:false};const w=R.route(g.nodes,[wide])[wide.id];assert.ok(w.labelHalfWidth>=('WW😀中 · 已撤销（历史）'.length*6));
 const narrow=R.route(g.nodes,[{...wide,label:'ii😀中'}])[wide.id];assert.equal(w.labelHalfWidth-narrow.labelHalfWidth,4,'wide Latin conservative estimate');
 assert.deepEqual(R.route(g.nodes,[...g.edges].reverse()),r);
});
test('600 combined edges remain bounded and none are deleted when label candidates exhaust',()=>{
 const nodes=Array.from({length:100},(_,i)=>node('n'+i,(i%10)*320,Math.floor(i/10)*190));
 const edges=Array.from({length:600},(_,i)=>edge('e'+i,'n'+(i%100),'n'+((i*7+13)%100)));
 const result=R.route(nodes,edges);assert.equal(Object.keys(result).length,600);audit(nodes,edges.map(e=>({...e,...result[e.id]})));
 assert.throws(()=>R.route(nodes,[...edges,{...edges[0],id:'overflow'}]),/bound/);
});
module.exports={audit,box,overlaps,crossing};
