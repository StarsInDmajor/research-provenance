'use strict';
const assert=require('node:assert/strict');
const {test}=require('node:test');
const vm=require('node:vm');
const {execFileSync}=require('node:child_process');
const {fromTree}=require('./dom-double.js');
const f=JSON.parse(execFileSync('python3',[__dirname+'/entry_fixture.py',...process.argv.slice(2)],{maxBuffer:8e6}));
function setup(edit=()=>{}) {
  const doc=fromTree(f.tree),get=id=>doc.getElementById(id);
  const wire=JSON.parse(get('graph-data').textContent),records=JSON.parse(get('canonical-records').textContent);
  edit({doc,get,wire,records});
  get('graph-data').textContent=JSON.stringify(wire);get('canonical-records').textContent=JSON.stringify(records);
  const ctx=vm.createContext({document:doc});
  const run=()=>{for(const script of f.scripts)vm.runInContext(script,ctx);};
  return {doc,get,wire,records,ctx,run};
}
test('final artifact CSP hashes match exact bytes across all three script channels',()=>{
  const doc=fromTree(f.tree),crypto=require('node:crypto');
  const policy=doc.querySelectorAll('meta').find(e=>e.getAttribute('http-equiv')==='Content-Security-Policy').getAttribute('content');
  const hashes=policy.split(';').find(s=>s.trim().startsWith('script-src ')).trim().split(/\s+/).slice(1);
  assert.equal(hashes.length,3);assert.doesNotMatch(policy,/unsafe-eval|script-src[^;]*unsafe-inline/);
  for(const script of doc.querySelectorAll('script'))assert.ok(hashes.includes("'sha256-"+crypto.createHash('sha256').update(script.textContent).digest('base64')+"'"));
});
function failed(s) {
  const raw=s.get('canonical-records').textContent;
  s.run();assert.equal(s.doc.body.getAttribute('data-app-state'),'failed');
  assert.equal(s.get('canonical-records').textContent,raw);
  assert.notEqual(s.get('canonical-records').inert,true);
  assert.notEqual(s.get('canonical-fallback').inert,true);
  assert.ok(s.get('toolbar').querySelectorAll('button,input,select').every(e=>e.disabled));
}
test('actual final decode/hydrate shares the one full canonical map and runtime builds all SVG keys',()=>{
  const s=setup();assert.equal(s.get('graph').children.length,0);s.run();
  assert.equal(s.doc.body.getAttribute('data-app-state'),'ready');
  const data=vm.runInContext('decodeWire(document).data',s.ctx),g=data.graph;
  for(const n of [...g.nodes,...g.edges])assert.equal(n.raw,data.records[n.id]);
  assert.deepEqual(JSON.parse(JSON.stringify(data.records)),s.records);
  const keys=s.get('graph').querySelectorAll('[data-key]');
  assert.deepEqual(keys.map(e=>e.dataset.key).sort(),[...g.nodes,...g.edges,...g.revisionEdges].map(e=>e.id).sort());
  for(const e of s.get('graph').querySelectorAll('g,path,text,rect,clipPath,marker,title,defs'))assert.equal(e.namespaceURI,'http://www.w3.org/2000/svg');
  for(const n of g.nodes){const el=keys.find(e=>e.dataset.key===n.id);assert.equal(el.getAttribute('transform'),`translate(${n.x} ${n.y})`);assert.ok(el.getAttribute('aria-label').includes(n.title));}
});
for(const [name,edit] of [
  ['unknown wire version',s=>s.wire.wireVersion=99],
  ['missing binding reference',s=>s.wire.graph.nodes[0].bindingIds=['missing']],
  ['missing assessment reference',s=>s.wire.graph.nodes[0].assessments=['missing']],
  ['malformed optional label geometry',s=>s.wire.graph.edges[0].labelHalfWidth={}],
  ['malformed leader geometry',s=>s.wire.graph.edges[0].labelLeaderPath='javascript:bad'],
  ['raw UTF8 byte bound',s=>{for(let i=0;i<35;i++)s.records['extra'+i]={id:'extra'+i,schema:'rp/assessment/v1',title:'😀'.repeat(16000)};}],
  ['duplicate raw channel',s=>s.wire.records=s.records],
  ['missing canonical record',s=>delete s.records[s.wire.graph.nodes[0].id]],
  ['wrong canonical raw id',s=>s.records[s.wire.graph.nodes[0].id].id='wrong'],
  ['wrong canonical node schema',s=>s.records[s.wire.graph.nodes[0].id].schema='rp/assessment/v1'],
  ['unknown schema',s=>s.records[s.wire.graph.nodes[0].id].schema='rp/unknown/v1'],
  ['raw null',s=>s.records[s.wire.graph.nodes[0].id]=null],
  ['wrong raw endpoint',s=>s.records[s.wire.graph.edges[0].id].to_revision=s.wire.graph.edges[0].id],
  ['wrong relation type',s=>s.records[s.wire.graph.edges[0].id].type='supports-not'],
  ['missing source record',s=>s.records[s.wire.graph.nodes[0].id].source={revisions:['missing']}],
  ['missing external source reference',s=>s.records[s.wire.graph.nodes[0].id].source={external_references:['missing']}],
  ['duplicate metadata node',s=>s.wire.graph.nodes.push(s.wire.graph.nodes[0])],
  ['omitted metadata node',s=>s.wire.graph.nodes.pop()],
  ['omitted metadata edge',s=>s.wire.graph.edges.pop()],
  ['duplicate metadata raw',s=>s.wire.graph.nodes[0].raw=s.records[s.wire.graph.nodes[0].id]],
  ['__proto__ lookup',s=>s.wire.graph.nodes[0].id='__proto__'],
  ['__proto__ own record',s=>Object.defineProperty(s.records,'__proto__',{value:{id:'__proto__',schema:'rp/assessment/v1'},enumerable:true})],
  ['cross-script duplicate canonical ID',s=>{const el=s.doc.createElement('pre');el.setAttribute('id','canonical-records');s.doc.body.appendChild(el);}],
  ['fallback inside failed region',s=>{const raw=s.get('canonical-records');s.get('workspace').appendChild(raw);}],
])test(name+' fails closed with complete native fallback untouched',()=>failed(setup(edit)));

test('duplicate canonical map key cannot be silently overwritten by JSON.parse',()=>{
  const s=setup(),raw=s.get('canonical-records').textContent,id=Object.keys(s.records)[0];
  s.get('canonical-records').textContent='{'+JSON.stringify(id)+':'+JSON.stringify(s.records[id])+','+raw.slice(1);
  failed(s);
});
for(const raw of ['null','[]','"not a map"','{bad',' '.repeat(2000001)])test('malformed/bounded canonical text rejected without losing fallback: '+raw.slice(0,16),()=>{
  const s=setup();s.get('canonical-records').textContent=raw;failed(s);
});
test('hostile closing tags, Unicode, ampersands and template tokens roundtrip through actual generated boot',()=>{
  const script=`import sys,json\nsys.path.insert(0,sys.argv[1]);sys.path.insert(0,sys.argv[1]+'/tests')\nfrom entry_fixture import Tree\nfrom test_projection import node\nfrom graph_projection import project\nimport build\na=node('a');a['title']=a['statement']='</pre></script><img src=x onerror=1> & 😀 中文 \\u2028\\u2029 @@JS@@ @@DATA@@';d=dict(records={'a':a},graph=project([a],'t'),sources={},notes={});p=Tree();p.feed(build.render(d,dict(as_of='2026-09-12T00:00:00Z',generated_at='2026-09-12T00:00:00Z')));print(json.dumps(dict(tree=p.root,scripts=[s['text'] for s in p.scripts if s['attrs'].get('type')!='application/json'])))`;
  const fixture=JSON.parse(execFileSync('python3',['-c',script,__dirname+'/..']));
  const doc=fromTree(fixture.tree),ctx=vm.createContext({document:doc});
  const expected=JSON.parse(doc.getElementById('canonical-records').textContent);
  for(const s of fixture.scripts)vm.runInContext(s,ctx);
  assert.equal(doc.body.getAttribute('data-app-state'),'ready');
  assert.equal(doc.querySelectorAll('script').length,3);assert.equal(doc.querySelectorAll('img').length,0);
  assert.equal(vm.runInContext('decodeWire(document).data.graph.nodes[0].raw.statement',ctx),expected.a.statement);
  assert.ok(doc.getElementById('graph').querySelectorAll('[data-key]')[0].getAttribute('aria-label').includes(expected.a.title));
});
test('all supported non-graph canonical schemas survive boot without projection reduction',()=>{
  const s=setup(s=>{s.records.extra={id:'extra',schema:'rp/claim-chain-snapshot/v1'};});
  s.run();assert.equal(s.doc.body.getAttribute('data-app-state'),'ready');
});
test('synthetic revision links must match actual canonical parent pairs and retain exact raw parent',()=>{
  const s=setup();if(!s.wire.graph.revisionEdges.length)return;
  s.wire.graph.revisionEdges[0].raw.parent.id='wrong';
  s.get('graph-data').textContent=JSON.stringify(s.wire);failed(s);
});
test('runtime initial SVG equals historical static oracle for every attribute and text',()=>{
  const s=setup();
  // Run producer scripts but not bootstrap; compare pre-mount initial SVG.
  vm.runInContext(f.scripts[0],s.ctx);
  vm.runInContext(f.scripts[1].replace("if(typeof document!=='undefined')boot(document);",''),s.ctx);
  const data=vm.runInContext('decodeWire(document).data',s.ctx);
  const source=`import sys,json\nfrom pathlib import Path\nsys.path.insert(0,sys.argv[1]);sys.path.insert(0,sys.argv[1]+'/tests')\nimport build\nfrom entry_fixture import Tree\ng=json.load(sys.stdin);p=Tree();p.feed(build.render_svg(g));print(json.dumps(p.root))`;
  const tree=JSON.parse(execFileSync('python3',['-c',source,__dirname+'/..'],{input:JSON.stringify(data.graph),maxBuffer:8e6}));
  const oracle=fromTree(tree).getElementById('graph');
  vm.runInContext('const initial=decodeWire(document).data;validateProjection(initial, initial.records);buildSVG(document,initial.graph);validateDOM(document,[...initial.graph.nodes,...initial.graph.edges,...initial.graph.revisionEdges]);',s.ctx);
  function normal(el){
    // HTML foreign-content parser restores mixed-case SVG attribute names.
    const names={refx:'refX',refy:'refY',markerwidth:'markerWidth',markerheight:'markerHeight',clippathunits:'clipPathUnits'};
    const attrs={};for(const [k,v] of Object.entries(el.attrs))attrs[names[k]||k]=v;
    // Python float formatting emits trailing .0; SVG numeric values equivalent.
    for(const key of ['x','y','width','height'])if(attrs[key]!==undefined)attrs[key]=String(Number(attrs[key]));
    return {tag:el.localName,attrs,text:el._text,children:el.children.map(normal)};
  }
  assert.deepEqual(normal(s.get('graph')),normal(oracle));
});
