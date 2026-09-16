'use strict';
// Actual admitted HTML data, pure geometry only; no browser/font measurement.
const assert=require('node:assert/strict');
const {execFileSync}=require('node:child_process');
const {Model}=require('../graph.js');
const {audit}=require('./labels.test.js');
const {fromTree}=require('./dom-double.js');
const {readData}=require('./wire-fixture.js');
const fixture=JSON.parse(execFileSync('python3',[__dirname+'/entry_fixture.py',...process.argv.slice(2)],{maxBuffer:4e6}));
const doc=fromTree(fixture.tree),g=readData(doc).graph,before=JSON.stringify(g),m=new Model(g),metrics=[],cases=[];
function check(name){const v=m.view(),all=[...v.edges,...v.revisionEdges];const row={view:name,nodes:v.nodes.length,science:v.edges.length,revisions:v.revisionEdges.length,...audit(v.nodes,all),hiddenIds:all.filter(e=>e.labelStatus==='hidden').map(e=>e.id)};metrics.push(row);cases.push({nodes:v.nodes,edges:all,all:[...g.edges,...g.revisionEdges]});if(g.nodes.length===19)assert.equal(row.routed,all.length,'actual r2 view routability');return all;}
const initial=check('default');
if(fixture.scripts[0].includes('labelLeaderPath'))for(const e of initial)for(const key of ['path','labelX','labelY','labelHalfWidth','labelStatus','labelLeaderPath'])assert.equal(e[key],g.edges.find(x=>x.id===e.id)[key],'static default matches runtime');
m.setHistory(true);check('all-history');m.setHistory(false);
for(const alias of ['B01','N01']){const n=g.nodes.find(n=>n.current&&n.label.startsWith(alias+' '));if(!n)continue;m.restoreAll();m.select(n.id);m.focus();const local=check(alias+'-local');const localGeometry=JSON.stringify(local);m.expand('in');check(alias+'-expand-in');m.back();assert.equal(JSON.stringify(m.view().edges),localGeometry);m.expand('out');check(alias+'-expand-out');m.back();check(alias+'-back');}
m.restoreAll();m.setFilter('mainline');check('mainline');m.setFilter('alternative');check('alternative');m.back();check('filter-back');m.showAll();check('show-all');
assert.equal(JSON.stringify(g),before);
// Full Python/JS geometry parity on every real visible set, not a data rewrite.
const js="import sys,json;sys.path.insert(0,sys.argv[1]);from routing import route_edges;cases=json.load(sys.stdin);out=[]\nfor c in cases:\n route_edges(c['nodes'],c['edges'],c['all']);out.append(c['edges'])\nprint(json.dumps(out))";
const py=JSON.parse(execFileSync('python3',['-c',js,__dirname+'/..'],{input:JSON.stringify(cases),maxBuffer:4e6}));
for(let i=0;i<cases.length;i++)for(let j=0;j<cases[i].edges.length;j++)assert.deepEqual(py[i][j],cases[i].edges[j]);
console.log(JSON.stringify(metrics,null,2));
