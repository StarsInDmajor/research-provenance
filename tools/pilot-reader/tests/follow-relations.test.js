'use strict';
// Synthetic payloads only; optional final HTML reads the private I09 at runtime.
const assert=require('node:assert/strict');
const vm=require('node:vm');
const {execFileSync}=require('node:child_process');
const {fromTree}=require('./dom-double.js');
const {readData,writeData}=require('./wire-fixture.js');
const helpers=require('../graph.js');
const fixture=()=>JSON.parse(execFileSync('python3',[__dirname+'/entry_fixture.py',...process.argv.slice(2)],{maxBuffer:4e6}));
function bootFixture(edit){
 const f=fixture(),doc=fromTree(f.tree),get=id=>doc.getElementById(id),data=readData(doc);
 if(edit){edit(data);writeData(doc,data);}
 const ctx=vm.createContext({document:doc});for(const s of f.scripts)vm.runInContext(s,ctx);
 assert.equal(doc.body.getAttribute('data-app-state'),'ready');
 return {doc,get,data,...vm.runInContext('boot(document).api',ctx)};
}
const tests=[];function test(name,fn){tests.push([name,fn]);}
test('supported payload diff renders nested arrays, labels and explicit caps',()=>{
 for(const [key,label] of [['conclusion','结论'],['interpretation','解释'],['method','方法']]){
  const a={[key]:{residual_uncertainty:['old uncertainty'],use_limitations:['old limit'],controls:[{nested:['old control']}]}},b={[key]:{residual_uncertainty:['new uncertainty'],use_limitations:['new limit'],controls:[{nested:['new control']}]}};
  const diff=helpers.fieldDiff(a,b);assert.equal(diff.rows.length,3,`${key} must not disappear`);
  assert.ok(diff.rows.every(r=>r.label.startsWith(label)));assert.match(JSON.stringify(diff.rows),/old control.*new control/);
 }
 const d=helpers.fieldDiff({method:{controls:Array(41).fill('old')}},{method:{controls:Array(41).fill('new')}});
 assert.equal(d.truncated,true,'array cap must be disclosed');
 const many=Object.fromEntries(Array.from({length:41},(_,i)=>['k'+i,'x']));assert.equal(helpers.fieldDiff({}, {method:many}).rows.length,40);assert.equal(helpers.fieldDiff({}, {method:many}).truncated,true);
 const long=helpers.fieldDiff({conclusion:{use_limitations:'a'.repeat(5000)}},{conclusion:{use_limitations:'b'.repeat(5000)}});
 assert.equal(long.truncated,true);assert.ok(long.rows.every(r=>r.before.length<=4000&&r.after.length<=4000));
});
test('all typed directions and neutral literal fallback',()=>{
 assert.equal(typeof helpers.relationMeaning,'function');
 const expected={'has-part':['包含','组成'],'input-to':['输入','接收'],'generated':['生成','生成自'],'cites':['引用','被引用'],'implements':['实现','由对方实现'],'consistent-with':['一致','一致'],'provides-prior':['提供先验','先验来自'],'predicts':['预测','被预测'],'tested-by':['由对方检验','检验对方'],'result-of':['结果来自','产生结果'],'observed-in':['观测于','被观测'],'reproduces':['复现','被复现'],'fails-to-reproduce':['未能复现','未被对方复现'],'validated-by':['由对方验证','验证对方'],'enables':['使对方可行','由对方使其可行'],'next-step':['下一步','前一步'],'supports':['支持','被支持'],'weakens':['削弱','被削弱'],'contradicts':['反驳','被反驳'],'derived-from':['依据','被用作依据'],'depends-on':['依赖','被依赖'],'blocked-by':['被阻塞','阻塞'],'motivates':['促成','被促成'],'requires':['需要','被需要']};
 for(const [type,[out,into]] of Object.entries(expected)){assert.ok(helpers.relationMeaning(type,true).includes(out));assert.ok(helpers.relationMeaning(type,false).includes(into));}
 assert.match(helpers.relationMeaning('unknown </script>',false),/入向.*unknown <\/script>/);
});
test('reverse source index deduplicates parsed records/projection, preserves states and bounded overflow',()=>{
 assert.equal(typeof helpers.reverseSources,'function');
 const node={id:'n',label:'same hostile </script>',current:false,raw:{id:'n',schema:'rp/node-revision/v1',source:{revisions:['a','a','b']}}};
 const edge={id:'e',from:'n',to:'n',label:'支持',current:false,category:'science',raw:{id:'e',relation_state:'active',source:{revisions:['a']}}};
 const records={n:JSON.parse(JSON.stringify(node.raw)),e:JSON.parse(JSON.stringify(edge.raw)),extra:{id:'extra',schema:'rp/other/v1',title:'same hostile </script>',source:{revisions:['a']}}};
 const index=helpers.reverseSources({records,graph:{nodes:[node],edges:[edge]}});
 assert.match(index.get('a').refs.find(r=>r.id==='e').label,/same hostile <\/script>.*支持.*same hostile <\/script>/);
 assert.equal(index.get('a').total,3);assert.equal(index.get('b').total,1);assert.equal(index.has('none'),false);
 assert.deepEqual(index.get('a').refs.map(r=>[r.id,r.current]),[['e',false],['extra',null],['n',false]]);
 assert.equal(index.get('a').refs.find(r=>r.id==='extra').locatable,false);
 for(let i=0;i<110;i++)records['x'+i]={id:'x'+i,source:{revisions:['a','a']}};
 const capped=helpers.reverseSources({records,graph:{nodes:[node],edges:[edge]}}).get('a');assert.equal(capped.total,113);assert.equal(capped.refs.length,100);
});
test('exact second same-type action direct A→B / edge / Back focus, disclosure, filter refresh and hidden reveal',()=>{
 const {doc,get,data,model,render}=bootFixture(d=>{
  const [a,b,c]=d.graph.nodes;
  a.label=b.label=c.label='same hostile </script>';a.raw.source={revisions:[]};
 });
 // Actual endpoint mappings are startup checked, so synthetic extra duplicates use the mounted model below instead.
 const a=data.graph.nodes[0],b=data.graph.nodes[1],base=data.graph.edges[0];
 const edge1={...base,id:'synthetic-edge-1',from:b.id,to:a.id,type:'derived-from',current:true};
 const edge2={...base,id:'synthetic-edge-2',from:b.id,to:a.id,type:'derived-from',current:true};
 model.graph.edges.push(edge1,edge2);model.items.set(edge1.id,edge1);model.items.set(edge2.id,edge2);
 model.reveal(a.id);render();const detail=get('detail-content'),panel=get('details');
 const action=(id,kind)=>detail.querySelectorAll('button').find(el=>el.dataset.relation===id&&el.dataset.action===kind);
 const second=action(edge2.id,'node');assert.ok(second,'separate direct-node action required');assert.match(second.textContent,/前往节点.*same hostile/);assert.match(detail.textContent,/被用作依据/);
 assert.notEqual(second.dataset.focus,action(edge1.id,'node').dataset.focus);const focus=second.dataset.focus;
 assert.match(second.getAttribute('aria-label'),/synthetic-edge-2/);
 const disclosure=detail.querySelectorAll('details').at(-1);disclosure.open=true;panel.scrollTop=123;
 const box=JSON.stringify(model.box);second.dispatch('click');assert.equal(model.selected,b.id,'direct jump skips edge');
 get('back').dispatch('click');assert.equal(model.selected,a.id);assert.equal(JSON.stringify(model.box),box);assert.equal(panel.scrollTop,123);assert.equal(doc.activeElement.dataset.focus,focus);assert.ok(detail.querySelectorAll('details').at(-1).open);
 const edgeAction=action(edge2.id,'edge');edgeAction.dispatch('click');assert.equal(model.selected,edge2.id);get('back').dispatch('click');assert.equal(doc.activeElement.dataset.focus,edgeAction.dataset.focus);
 action(edge2.id,'node').focus();panel.scrollTop=81;model.local=[a.id];render();assert.equal(doc.activeElement.dataset.focus,focus);assert.equal(panel.scrollTop,81);
 const saved=JSON.stringify(model.snapshot());action(edge2.id,'node').dispatch('click');assert.equal(model.selected,b.id);get('back').dispatch('click');assert.equal(JSON.stringify(model.snapshot()),saved);
 const controls=detail.querySelectorAll('button,summary,select');assert.equal(new Set(controls.map(el=>el.dataset.focus)).size,controls.length,'all actionable focus identities unique');assert.ok(controls.every(el=>el.dataset.focus));assert.equal(detail.querySelectorAll('script').length,0);
});
test('reverse SOURCE detail lists current/historical independently of history toggle and navigates exact targets',()=>{
 // Mount-time index is exercised in a fresh, JSON-parsed fixture.
 const target=readData(fromTree(fixture().tree)).graph.nodes[0].id;
 const app=bootFixture(d=>{
  d.graph.readingSources={}; // synthetic SOURCE edges replace the case-specific quick source mapping too
  for(const n of d.graph.nodes){n.raw.source={revisions:n.id===target?[]:[target,target]};d.records[n.id]=JSON.parse(JSON.stringify(n.raw));}
  for(const e of d.graph.edges){e.raw.source={revisions:[target]};d.records[e.id]=JSON.parse(JSON.stringify(e.raw));}
  // A supported non-graph record exercises identical reverse-SOURCE semantics.
  d.records.extra={id:'extra',schema:'rp/assessment/v1',title:'hostile </script>',source:{revisions:[target]}};
 });
 app.model.reveal(target);app.render();const detail=app.get('detail-content');
 assert.match(detail.textContent,/被这些记录引用 · SOURCE · 非科研关系/);assert.match(detail.textContent,/含历史.*不受历史开关/);assert.match(detail.textContent,/旧修订|历史关系/);assert.match(detail.textContent,/hostile <\/script>.*不在画布/);
 const nav=detail.querySelectorAll('button').filter(b=>b.dataset.action==='reverse-source');assert.equal(nav.length,app.data.graph.nodes.length-1+app.data.graph.edges.length);
 const oldNav=nav.find(b=>!app.model.items.get(b.dataset.target).current);assert.ok(oldNav);const focus=oldNav.dataset.focus;oldNav.dispatch('click');assert.equal(app.model.selected,oldNav.dataset.target);assert.equal(app.model.history,true);app.get('back').dispatch('click');assert.equal(app.doc.activeElement.dataset.focus,focus);
 assert.equal(detail.querySelectorAll('script').length,0);assert.equal(app.model.graph.edges.length,app.data.graph.edges.length);
 const sourceKeys=()=>detail.querySelectorAll('button').filter(b=>b.dataset.action==='reverse-source').map(b=>b.dataset.focus).sort();
 const keys=sourceKeys();app.model.setHistory(true);app.render();assert.deepEqual(sourceKeys(),keys);assert.match(detail.textContent,/当前修订/);assert.match(detail.textContent,/旧修订/);
 app.model.setHistory(false);app.render();assert.deepEqual(sourceKeys(),keys);assert.match(detail.textContent,/旧修订/);
});
test('actual I09 old/new diff through final HTML controls (synthetic default otherwise)',()=>{
 const {get,data,model,render}=bootFixture();
 const candidate=model.graph.nodes.find(n=>n.current&&n.label.startsWith('I09 '));
 const current=candidate||model.graph.nodes.find(n=>n.raw.revision?.parents?.length);
 const old=model.graph.nodes.find(n=>current.raw.revision.parents.some(p=>p.id===n.id));
 if(!candidate){old.raw.conclusion={residual_uncertainty:['old uncertainty'],use_limitations:['old limitation']};current.raw.conclusion={residual_uncertainty:['new uncertainty'],use_limitations:['new limitation']};}
 model.reveal(current.id);render();const detail=get('detail-content'),selects=detail.querySelectorAll('select');assert.equal(selects.length,2);
 selects[0].value=old.id;selects[1].value=current.id;selects[1].dispatch('change');
 const pure=helpers.fieldDiff(old.raw,current.raw);
 const rows=detail.querySelectorAll('section');for(const key of ['residual_uncertainty','use_limitations']){
  const label={residual_uncertainty:'残余不确定性',use_limitations:'使用限制'}[key];const row=rows.find(r=>r.querySelectorAll('h3').some(h=>h.textContent.includes(label)));assert.ok(row,`${key} must appear`);
  const pureRow=pure.rows.find(r=>r.label.includes(label));assert.ok(pureRow);
  for(const [n,side] of [[old,'before'],[current,'after']]){const value=n.raw.conclusion[key];for(const text of Array.isArray(value)?value:[value]){assert.ok(row.textContent.includes(text),'both original/new raw text retained');assert.ok(pureRow[side].includes(text));}}
 }
 assert.ok(detail.querySelectorAll('details').some(d=>d.textContent.includes('完整原始字段')));
});
test('capped comparison has both full raw revisions, not only selected revision',()=>{
 const app=bootFixture();const current=app.model.graph.nodes.find(n=>n.raw.revision?.parents?.length),old=app.model.items.get(current.raw.revision.parents[0].id);
 old.raw.method={controls:Array.from({length:45},(_,i)=>'old control '+i)};current.raw.method={controls:Array.from({length:45},(_,i)=>'new control '+i)};
 app.model.reveal(current.id);app.render();const detail=app.get('detail-content');assert.match(detail.textContent,/对比已限长/);
 const fallback=detail.querySelectorAll('details').find(d=>d.children[0]?.textContent==='对比双方完整原始字段');assert.ok(fallback);assert.match(fallback.textContent,/old control 44/);assert.match(fallback.textContent,/new control 44/);
});
test('actual recorded I02 derived-from opposites and N01/Q00 reverse uses remain precise',()=>{
 const app=bootFixture(),{data,model,render,get,doc}=app;
 const i02=data.graph.nodes.find(n=>n.current&&n.label.startsWith('I02 '));if(!i02)return;
 model.reveal(i02.id);render();const detail=get('detail-content');
 const edges=data.graph.edges.filter(e=>e.current&&e.type==='derived-from'&&e.to===i02.id);
 assert.ok(edges.length>=2);
 for(const alias of ['I04 ','I09 ']){
  const other=data.graph.nodes.find(n=>n.current&&n.label.startsWith(alias)),edge=edges.find(e=>e.from===other.id);assert.ok(edge);
  const b=detail.querySelectorAll('button').find(b=>b.dataset.action==='node'&&b.dataset.relation===edge.id);assert.ok(b.textContent.includes(other.label));const focus=b.dataset.focus;
  b.dispatch('click');assert.equal(model.selected,other.id);get('back').dispatch('click');assert.equal(doc.activeElement.dataset.focus,focus);
 }
 const q00=data.graph.nodes.find(n=>n.current&&n.label.startsWith('Q00 '));assert.ok(q00.raw.source.revisions.includes(i02.id));
 assert.ok(detail.querySelectorAll('button').some(b=>b.dataset.action==='reverse-source'&&b.dataset.target===q00.id));
 assert.equal(data.graph.edges.some(e=>e.from===q00.id||e.to===q00.id),false,'Q00 remains scientifically isolated');
 const i04=data.graph.nodes.find(n=>n.current&&n.label.startsWith('I04 '));model.reveal(i04.id);render();
 const n01=data.graph.nodes.filter(n=>n.raw.logical_id==='csp-n01'||n.label.startsWith('N01 '));assert.ok(n01.some(n=>n.current));assert.ok(n01.some(n=>!n.current));
 const referrers=n01.filter(n=>n.raw.source?.revisions?.includes(i04.id));assert.ok(referrers.some(n=>n.current));
 // Old N01 actually cites old I09/B02/G05, NOT I04; never manufacture that use.
 const oldN=n01.find(n=>!n.current);assert.ok(!detail.querySelectorAll('button').some(b=>b.dataset.action==='reverse-source'&&b.dataset.target===oldN.id));
 for(const n of referrers){
  const b=detail.querySelectorAll('button').find(b=>b.dataset.action==='reverse-source'&&b.dataset.target===n.id);assert.ok(b);assert.ok(b.textContent.includes(n.label));
  b.dispatch('click');assert.equal(model.selected,n.id);get('back').dispatch('click');assert.equal(doc.activeElement.dataset.focus,b.dataset.focus);
 }
 model.reveal(oldN.raw.source.revisions[0]);render();
 const oldUse=detail.querySelectorAll('button').find(b=>b.dataset.action==='reverse-source'&&b.dataset.target===oldN.id);assert.ok(oldUse);assert.ok(oldUse.textContent.includes(oldN.label));oldUse.dispatch('click');assert.equal(model.selected,oldN.id);get('back').dispatch('click');assert.equal(doc.activeElement.dataset.focus,oldUse.dataset.focus);
});
test('graph keyboard focus identity survives returning from a direct detail action',()=>{
 const {get,model,render,doc}=bootFixture();const node=model.graph.nodes.find(n=>n.current),el=get('graph').querySelectorAll('[data-key]').find(e=>e.dataset.key===node.id);
 el.focus();el.dispatch('keydown',{key:'Enter'});assert.equal(model.selected,node.id);assert.ok(el.dataset.focus.includes(node.id));
 const detail=get('detail-content'),b=detail.querySelectorAll('button').find(b=>b.dataset.action==='node');assert.ok(b);b.focus();const key=b.dataset.focus;b.dispatch('click');get('back').dispatch('click');assert.equal(doc.activeElement.dataset.focus,key);
 // Existing graph-keyboard origin can be restored after a second graph selection too.
 el.focus();const next=get('graph').querySelectorAll('[data-key]').find(e=>e.dataset.key!==node.id);next.dispatch('keydown',{key:'Enter'});get('back').dispatch('click');assert.equal(doc.activeElement.dataset.focus,el.dataset.focus);
});
test('reverse list cap disclosed and omitted raw references available without fake locate',()=>{
 const app=bootFixture(d=>{for(let i=0;i<105;i++)d.records['extra'+i]={id:'extra'+i,schema:'rp/assessment/v1',title:'duplicate',source:{revisions:[d.graph.nodes[0].id]}};});
 app.model.reveal(app.model.graph.nodes[0].id);app.render();const detail=app.get('detail-content');
 assert.match(detail.textContent,/列表已截断/);assert.ok(detail.querySelectorAll('details').some(d=>d.textContent.includes('全部反向 SOURCE 原文')&&d.textContent.includes('extra104')));
});
let failed=0;for(const [name,fn] of tests){try{fn();console.log('PASS',name);}catch(e){failed++;console.error('FAIL',name,e.message);}}if(failed)process.exitCode=1;
