'use strict';
// Small DOM/event double exercises mount's real callbacks, NOT layout, SVG hit
// testing, native pointer retargeting, browser CSP enforcement or visual proof.
const assert=require('node:assert/strict');
const {mount,viewport}=require('../graph.js');
const {Doc,descendants}=require('./dom-double.js');
const doc=new Doc();
// Explicit fixture elements: missing IDs return null, never auto-created.
const tags={domains:'button','domain-panel':'section','domain-history':'input','domain-groups':'div','domain-counts':'p',help:'button','reading-guide':'details','reading-guide-summary':'summary',workspace:'section',fullscreen:'button','toggle-details':'button','canvas-status':'p',graph:'svg',toolbar:'div',details:'aside','detail-content':'div',
  search:'input',filter:'select',history:'input','zoom-in':'button','zoom-out':'button',
  fit:'button',reset:'button','show-all':'button',back:'button','restore-all':'button',start:'button','start-choices':'div',
  'search-results':'div',counts:'p','zoom-level':'span',notice:'p'};
for(const [id,tag] of Object.entries(tags)){
  const el=doc.createElement(tag);el.setAttribute('id',id);doc.appendChild(el);
}
assert.equal(doc.getElementById('missing'),null);
const raw=(id)=>({id,title:'Original '+id,statement:'exact <script> text',logical_id:id==='old'?'a':id});
const node=(id,x,y,current,roles)=>({id,x,y,current,roles,kind:'Question',typeLabel:'问题',label:id==='old'?'旧行动':id,raw:raw(id),bindingIds:[]});
const data={graph:{nodes:[node('a',0,0,true,['primary']),node('b',320,0,true,['alternative']),node('old',0,190,false,['historical'])],
 edges:[{id:'e',from:'a',to:'b',current:true,category:'science',type:'depends-on',label:'依赖',raw:{rationale:'exact reason',scope:{statement:'exact scope'},source:{revisions:['b']}}}],
 revisionEdges:[{id:'r',from:'old',to:'a',category:'revision',label:'修订',raw:{summary:'exact revision'}}],
 roles:{primary:'主线',alternative:'备选',historical:'历史'},width:660,height:400},sources:{},notes:{},records:{}};
const svg=doc.getElementById('graph');
svg.query=['a','b','old','e','r'].map(key=>{const el=doc.createElement('g');el.setAttribute('data-key',key);svg.appendChild(el);return el;});
const toolbar=doc.getElementById('toolbar');toolbar.query=['search','filter','history','zoom-in','zoom-out','fit','reset','show-all'].map(k=>doc.getElementById(k));
for(const el of toolbar.query){doc.children=doc.children.filter(c=>c!==el);toolbar.appendChild(el);el.disabled=true;}
data.graph.nodes[0].raw.statement='new statement';data.graph.nodes[0].raw.next_action={completion_criteria:['new criterion']};
data.graph.nodes[2].raw.statement='old statement';data.graph.nodes[2].raw.next_action={completion_criteria:['old criterion']};
data.graph.nodes[0].raw.source={artifacts:['source1'],revisions:['b']};
data.sources.source1={title:'性能来源',excerpt:'historical excerpt',sections:[],sha256:'digest',size_bytes:18};
const {model,render}=mount(doc,data);
assert.ok(toolbar.query.every(el=>el.disabled===true),'mount does not publish readiness; bootstrap owns enabling');
const el=(key)=>svg.query.find(el=>el.dataset.key===key);
el('a').dispatch('click');
assert.equal(model.selected,'a');assert.ok(el('b').classes.has('neighbor'));
assert.match(doc.getElementById('detail-content').textContent,/new statement/);
assert.match(doc.getElementById('detail-content').textContent,/编写依据，不等于行动目标 · 非科研关系/);
assert.equal(descendants(doc.getElementById('detail-content'),'script').length,0);
const histButton=descendants(doc.getElementById('detail-content'),'button').find(b=>b.textContent.includes('旧修订'));
assert.ok(histButton);histButton.dispatch('click');assert.equal(model.selected,'old');assert.equal(model.history,true);
el('e').dispatch('keydown',{key:'Enter'});assert.equal(model.selected,'e');
assert.match(doc.getElementById('detail-content').textContent,/exact reason/);
assert.match(doc.getElementById('detail-content').textContent,/exact scope/);
assert.match(doc.getElementById('detail-content').textContent,/a → b/);
doc.getElementById('filter').value='mainline';doc.getElementById('filter').dispatch('change');
assert.equal(model.mode,'mainline');assert.ok(!el('b').classes.has('is-hidden'));
assert.ok(el('b').classes.has('contextual'));
assert.match(doc.getElementById('counts').textContent,/聚焦 1.*上下文/);
doc.getElementById('search').value='旧行动';doc.getElementById('search').dispatch('input');
const results=descendants(doc.getElementById('search-results'),'button');assert.equal(results.length,1);
results[0].dispatch('click');
assert.equal(model.selected,'old');assert.equal(model.mode,'mainline');
doc.getElementById('history').checked=false;doc.getElementById('history').dispatch('change');assert.equal(model.history,false);
doc.getElementById('show-all').dispatch('click');assert.equal(model.history,true);
doc.dispatch('keydown',{key:'Escape'});assert.equal(model.selected,null);
let box={...model.box};doc.getElementById('zoom-in').dispatch('click');assert.ok(model.box.w<box.w);
box={...model.box};svg.dispatch('wheel',{deltaY:-10,clientX:100,clientY:200});assert.ok(model.box.w<box.w);
svg.dispatch('pointerdown',{clientX:100,clientY:100});
assert.ok(!svg.captured,'ordinary click must not be retargeted by immediate capture');
svg.dispatch('pointermove',{clientX:103,clientY:100});assert.ok(!svg.captured);
box={...model.box};svg.dispatch('pointermove',{clientX:130,clientY:120});assert.ok(svg.captured);assert.notEqual(model.box.x,box.x);
svg.dispatch('pointerup',{clientX:130,clientY:120});el('a').dispatch('click');assert.equal(model.selected,null,'drag must not select');
svg.dispatch('pointerdown',{clientX:100,clientY:100});svg.dispatch('pointerup',{clientX:100,clientY:100});el('a').dispatch('click');assert.equal(model.selected,'a');
doc.getElementById('reset').dispatch('click');assert.equal(model.selected,null);assert.equal(model.history,false);assert.deepEqual(model.box,new (require('../graph.js').Model)(data.graph).box);
const p=viewport({left:20,top:10,width:1000,height:1000},{w:1000,h:500},520,510);assert.equal(p.x,.5);assert.equal(p.y,.5);assert.equal(p.scale,1);
// New exploration workflow runs actual controls, not only Model methods.
assert.equal(typeof model.back,'function');
el('a').dispatch('click');
const detail=doc.getElementById('detail-content'), panel=doc.getElementById('details');
const disclosure=descendants(detail,'details').at(-1); disclosure.open=true;panel.scrollTop=123;
const action=label=>descendants(detail,'button').find(b=>b.textContent===label);
assert.ok(action('只看相关'));action('只看相关').dispatch('click');
assert.equal(descendants(detail,'details').at(-1),disclosure,'same selection retains actual detail DOM');
assert.equal(disclosure.open,true);assert.equal(panel.scrollTop,123);
assert.ok(action('展开入向'));assert.ok(action('展开出向'));assert.ok(action('返回'));
let before={...model.box};svg.dispatch('wheel',{deltaY:-10,clientX:200,clientY:200});assert.notDeepEqual(model.box,before);
before={...model.box};
const oldButton=descendants(detail,'button').find(b=>b.textContent.includes('旧修订'));
oldButton.dispatch('click');assert.equal(panel.scrollTop,0);assert.equal(doc.activeElement.tag,'h2');
action('返回').dispatch('click');assert.equal(model.selected,'a');assert.deepEqual(model.box,before);assert.equal(panel.scrollTop,123);assert.equal(descendants(detail,'details').at(-1).open,true);
const search=doc.getElementById('search');search.value='旧行动';search.dispatch('input');
search.dispatch('keydown',{key:'ArrowDown'});
assert.equal(doc.getElementById('search-results').children.filter(c=>c.tag==='button')[0].attrs['aria-pressed'],'true');
search.dispatch('keydown',{key:'Enter'});assert.equal(model.selected,'old');
search.dispatch('keydown',{key:'Escape'});assert.equal(model.selected,'old');assert.equal(doc.getElementById('search-results').children.length,0);
// Native result buttons must also retain visible keyboard focus and Enter activation.
search.value='a';search.dispatch('input');search.dispatch('keydown',{key:'ArrowUp'});assert.ok(search.attrs['aria-activedescendant']);
assert.equal(model.searchState.index,model.search('a').length-1,'first ArrowUp selects last result');
const comparisons=descendants(detail,'select');assert.equal(comparisons.length,2);
assert.match(detail.textContent,/完成标准/);assert.match(detail.textContent,/old criterion/);assert.match(detail.textContent,/new criterion/);
comparisons[0].value='a';comparisons[1].value='a';comparisons[0].dispatch('change');assert.match(detail.textContent,/没有变化/);
// No repeated filter/render should close source disclosures or replace their DOM.
el('a').dispatch('click');
const source=descendants(detail,'details').find(d=>d.dataset.section==='source:source1');assert.ok(source);source.open=true;
const sourceSummary=source.children[0];sourceSummary.focus();panel.scrollTop=77;
el('b').dispatch('click');
action('返回').dispatch('click');assert.equal(model.selected,'a');assert.equal(panel.scrollTop,77);assert.equal(doc.activeElement.dataset.focus,sourceSummary.dataset.focus);
assert.match(detail.textContent,/修订脉络/);assert.match(detail.textContent,/旧版.*新版/);
// pointercancel must not poison a later non-pointer/assistive click.
svg.dispatch('pointerdown',{clientX:100,clientY:100});svg.dispatch('pointermove',{clientX:130,clientY:120});svg.dispatch('pointercancel');el('b').dispatch('click');assert.equal(model.selected,'b');
// Captured drag click is retargeted to SVG; consume there, not at the next node.
svg.dispatch('pointerdown',{clientX:100,clientY:100});svg.dispatch('pointermove',{clientX:140,clientY:100});svg.dispatch('pointerup');svg.dispatch('click');el('a').dispatch('click');assert.equal(model.selected,'a','retargeted click must not leave stale suppression');
const panelBefore=panel.scrollTop=201;doc.dispatch('keydown',{key:'Escape'});assert.equal(model.selected,null);assert.equal(panel.scrollTop,0,'clear selection resets details scroll');assert.equal(doc.activeElement.tag,'h2');
el('a').dispatch('click');
model.local=['a'];render();
assert.match(detail.textContent,/1 条相邻关系被当前筛选隐藏/);
const keptSource=descendants(detail,'details').find(d=>d.dataset.section==='source:source1');keptSource.open=true;panel.scrollTop=89;
doc.getElementById('restore-all').dispatch('click');
assert.doesNotMatch(detail.textContent,/1 条相邻关系被当前筛选隐藏/,'same selected node must refresh view-dependent details');
assert.equal(descendants(detail,'details').find(d=>d.dataset.section==='source:source1').open,true);
assert.equal(panel.scrollTop,89);
while(model.back());assert.equal(mount(doc,data).model,model,'idempotent mount returns same scene');mount(doc,data).render();assert.equal(doc.getElementById('back').disabled,true);
// Case provenance is navigable but never fabricated as a scientific arrow.
el('b').dispatch('click');
data.graph.nodes[0].raw.source.revisions=['b'];
data.graph.nodes[0].raw.limitations=['not a browser measurement'];
delete data.graph.nodes[0].raw.next_action; // This later fixture exercises a non-action source label.
data.graph.nodes[0].raw.interpretation={unresolved_alternatives:['sandbox remains possible']};
el('a').dispatch('click');
const provenance=descendants(detail,'button').find(b=>b.textContent.startsWith('来源引用 · SOURCE · 非科研关系'));
assert.ok(provenance,'node source.revisions needs a navigable distinct provenance control');
assert.ok(descendants(detail,'h3').some(h=>h.textContent==='限制与未决项'));
assert.ok(descendants(detail,'h3').some(h=>h.textContent==='类型内容 · interpretation'));
assert.match(detail.textContent,/not a browser measurement/);
assert.match(detail.textContent,/sandbox remains possible/);
provenance.dispatch('click');assert.equal(model.selected,'b');
assert.equal(data.graph.edges.length,1,'provenance must not create a science edge');
console.log('real mount handlers exercised via minimal fake DOM; browser visual acceptance remains pending');
