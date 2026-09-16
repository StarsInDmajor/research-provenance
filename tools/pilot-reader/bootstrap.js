'use strict';
/* Delivery boundary only: validate the bounded projection + existing DOM, mount
 * once, then publish readiness. No data refresh/retry, global error listener,
 * storage, loading or untrusted exception details. CSP-blocked boot leaves the
 * server-rendered warning and complete canonical-text fallback intact. */
const boots = new WeakMap();
const REQUIRED = {
  workspace:'section', toolbar:'div', graph:'svg', details:'aside',
  'detail-content':'div', 'graph-data':'script', 'app-status':'p',
  search:'input', filter:'select', history:'input', 'zoom-in':'button',
  'zoom-out':'button', fit:'button', back:'button', 'restore-all':'button',
  reset:'button', 'show-all':'button', 'search-results':'div',
  start:'button', 'start-choices':'div', fullscreen:'button',
  'toggle-details':'button', 'canvas-status':'p', help:'button',
  'reading-guide':'details', 'reading-guide-summary':'summary',
  domains:'button', 'domain-panel':'section', 'domain-history':'input', 'domain-groups':'div', 'domain-counts':'p',
  notice:'p', counts:'p', 'zoom-level':'span'
};
function check(ok) { if (!ok) throw new Error('Invalid reader projection or DOM'); }
function record(x) { return x!==null && typeof x==='object' && !Array.isArray(x); }
function string(x) { return typeof x==='string' && x.length<=65536; }
function strings(x) { return Array.isArray(x) && x.length<=512 && x.every(string); }
function number(x) { return Number.isFinite(x) && Math.abs(x)<=1000000; }
function boundedData(data) {
  let entries=0;
  function walk(x,depth=0) {
    check(depth<=24 && ++entries<=50000);
    if(x && typeof x==='object')for(const value of Object.values(x))walk(value,depth+1);
    else check(x===null || typeof x==='boolean' || string(x) || number(x));
  }
  walk(data);
}
function utf8Bytes(text) {
  let size=0;
  for(const char of text){const cp=char.codePointAt(0);size+=cp<128?1:cp<2048?2:cp<65536?3:4;}
  return size;
}
function uniqueJSONKeys(text) {
  // Called after JSON.parse accepted grammar. Tokenize strings atomically so
  // punctuation/escaped quotes inside canonical text cannot impersonate keys.
  const stack=[];
  for(const token of text.matchAll(/"(?:\\.|[^"\\])*"|[{}\[\],:]/g)) {
    const value=token[0],top=stack.at(-1);
    if(value==='{')stack.push({keys:new Set(),key:true});
    else if(value==='[')stack.push(null);
    else if(value==='}' || value===']')stack.pop();
    else if(value===',' && top)top.key=true;
    else if(value[0]==='"' && top?.key) {
      const key=JSON.parse(value);check(!top.keys.has(key));top.keys.add(key);top.key=false;
    }
  }
}
function decodeWire(doc, override) {
  const source=doc.getElementById('graph-data'),fallback=doc.getElementById('canonical-records');
  if(override) {
    check(override.wireText.length<=2000000);
    const data=JSON.parse(override.wireText);boundedData(data);uniqueJSONKeys(override.wireText);
    const records=JSON.parse(override.recordsText);boundedData(records);uniqueJSONKeys(override.recordsText);
    return finishWire(data, records);
  }
  check(source && source.textContent.length<=2000000);
  const data=JSON.parse(source.textContent);boundedData(data);uniqueJSONKeys(source.textContent);
  check(data.wireVersion===1 && !Object.hasOwn(data,'records') && record(data.graph));
  check(fallback && fallback.tagName.toLowerCase()==='pre' && fallback.textContent.length<=2000000);
  check(utf8Bytes(source.textContent)+utf8Bytes(fallback.textContent)<=2000000);
  check(!doc.getElementById('workspace')?.contains(fallback));
  const records=JSON.parse(fallback.textContent);boundedData(records);uniqueJSONKeys(fallback.textContent);
  return finishWire(data, records);
}
function finishWire(data, records) {
  check(record(records) && Object.keys(records).length<=512);
  const schemas=new Set(['rp/node-revision/v1','rp/scientific-relation-revision/v1','rp/thread-binding/v1',
    'rp/assessment/v1','rp/project/v1','rp/research-thread/v1','rp/artifact-manifest/v1',
    'rp/external-reference/v1','rp/research-run/v1','rp/claim-chain-snapshot/v1']);
  const own=id=>typeof id==='string' && Object.hasOwn(records,id);
  for(const [id,r] of Object.entries(records)) {
    check(!['__proto__','constructor','prototype'].includes(id) && record(r) && r.id===id && schemas.has(r.schema));
    if(r.source!==undefined) {
      check(record(r.source));
      for(const key of ['revisions','artifacts','external_references'])if(r.source[key]!==undefined) {
        check(strings(r.source[key]));for(const ref of r.source[key])check(own(ref));
      }
    }
    if(r.revision!==undefined) {
      check(record(r.revision) && Array.isArray(r.revision.parents));
      for(const p of r.revision.parents)check(record(p) && own(p.id) && records[p.id].schema===r.schema);
    }
  }
  const g=data.graph;
  for(const [name,schema,max] of [['nodes','rp/node-revision/v1',100],['edges','rp/scientific-relation-revision/v1',300]]) {
    check(Array.isArray(g[name]) && g[name].length<=max);
    const ids=new Set();
    for(const item of g[name]) {
      check(record(item) && own(item.id) && !Object.hasOwn(item,'raw') && !ids.has(item.id));ids.add(item.id);
      const raw=records[item.id];check(raw.schema===schema);item.raw=raw;
      if(name==='nodes') {
        check(!Object.hasOwn(item,'title') && !Object.hasOwn(item,'kind'));
        item.title=raw.title;item.kind=raw.kind;
        if(!Object.hasOwn(item,'label'))item.label=raw.title;
      } else check(raw.from_revision===item.from && raw.to_revision===item.to && raw.type===item.type);
    }
    const exact=Object.values(records).filter(r=>r.schema===schema);
    check(exact.length===ids.size && exact.every(r=>ids.has(r.id)));
  }
  for(const n of g.nodes) {
    check(strings(n.bindingIds) && strings(n.assessments));
    for(const id of n.bindingIds)check(own(id) && records[id].schema==='rp/thread-binding/v1' && records[id].target?.id===n.id && records[id].thread_id===g.thread);
    for(const id of n.assessments)check(own(id) && records[id].schema==='rp/assessment/v1' && records[id].target?.id===n.id);
  }
  check(Array.isArray(g.revisionEdges) && g.revisionEdges.length<=300);
  const revisions=new Map();
  for(const n of g.nodes)for(const parent of n.raw.revision?.parents||[]) {
    revisions.set('revision:'+parent.id+':'+n.id,{from:parent.id,to:n.id,parent,summary:n.raw.revision.summary||''});
  }
  check(revisions.size===g.revisionEdges.length);
  for(const e of g.revisionEdges) {
    const expected=revisions.get(e.id);check(expected && record(e.raw));
    check(e.from===expected.from && e.to===expected.to && JSON.stringify(e.raw.parent)===JSON.stringify(expected.parent) && e.raw.summary===expected.summary);
    revisions.delete(e.id);
  }
  if(data.projectId!==undefined) {check(own(data.projectId) && records[data.projectId].schema==='rp/project/v1');data.project=records[data.projectId];}
  data.records=records;
  return {data, records};
}
function validateSourceLocators(data) {
  const loc=data.sourceLocators,nodes=data.graph.nodes;
  if(loc===undefined){check(nodes.every(n=>n.sourceLocators===undefined));return;}
  check(record(loc) && Object.keys(loc).sort().join(',')==='artifacts,chunks,strings');
  check(strings(loc.artifacts) && loc.artifacts.length<=100 && new Set(loc.artifacts).size===loc.artifacts.length);
  check(Array.isArray(loc.strings) && loc.strings.length<=3000 && loc.strings.every(string));
  check(loc.strings.reduce((n,s)=>n+utf8Bytes(s),0)<=100000);
  check(Array.isArray(loc.chunks) && loc.chunks.length<=400);
  const str=i=>{check(Number.isInteger(i) && i>=-1 && i<loc.strings.length);return i<0?'':loc.strings[i];};
  const digest=s=>/^sha256:[0-9a-f]{64}$/.test(s);
  const path=s=>s.length>0 && utf8Bytes(s)<=512 && !/[\\\\:\u0000\n\r]/.test(s) && s.split('/').every(p=>p && p!=='.' && p!=='..');
  for(const id of loc.artifacts)check(records[id]?.schema==='rp/artifact-manifest/v1');
  for(const row of loc.chunks) {
    check(Array.isArray(row) && row.length===10);
    const [p,start,end,section,full,capture,stamp,last,display,body]=row;
    for(const i of [p,section,full,capture,stamp,display,body])str(i);
    check(path(str(p)) && utf8Bytes(str(section))<=1024 && digest(str(full)) && digest(str(capture)));
    check(Number.isInteger(start) && start>=1 && Number.isInteger(end) && end>=start && end<=1000000 && end-start<10000);
    check(str(stamp).length<=40 && /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,9})?(?:Z|[+-]\d\d:\d\d)$/.test(str(stamp)) && Number.isFinite(Date.parse(str(stamp))));
    const excerpt=str(body),lines=excerpt?excerpt.split('\n').length-(excerpt.endsWith('\n')?1:0):0;
    check(utf8Bytes(excerpt)<=160 && lines<=3 && Number.isInteger(last) && last===start+lines-1 && last<=end);
    check(excerpt?digest(str(display)):str(display)==='');
  }
  let count=0;const used=new Set(),usedArtifacts=new Set();
  for(const n of nodes) {
    const links=n.sourceLocators;check(Array.isArray(links) && links.length<=8);const seen=new Set();
    for(const link of links) {
      check(Array.isArray(link) && link.length===2);const [c,a]=link;
      check(Number.isInteger(c) && c>=0 && c<loc.chunks.length && Number.isInteger(a) && a>=0 && a<loc.artifacts.length);
      check(!seen.has(c) && n.raw.source?.artifacts?.includes(loc.artifacts[a]));seen.add(c);used.add(c);usedArtifacts.add(a);
      check(++count<=400);
    }
  }
  check(used.size===loc.chunks.length && usedArtifacts.size===loc.artifacts.length);
  // Producer verifies digests against bounded archived copies. This synchronous
  // boot gate checks shape/identity; no filesystem/URI/async fetch or live claims.
}
function validateProjection(data, records) {
  // Hydrated raw objects intentionally share canonical map entries.
  boundedData(data);boundedData(records);
  check(record(data) && record(data.graph) && record(data.sources) && record(data.notes) && record(records));
  // Reverse SOURCE reads records beyond the rendered graph; validate only the
  // consumed shapes, within the existing admitted corpus bound, not a schema.
  check(Object.keys(records).length<=512);
  data.records=records;
  for(const [id,r] of Object.entries(records)) {
    check(record(r) && r.id===id);
    for(const k of ['title','kind','schema'])if(r[k]!==undefined)check(string(r[k]));
    if(r.source!==undefined) {
      check(record(r.source));
      if(r.source.revisions!==undefined)check(strings(r.source.revisions));
    }
  }
  const g=data.graph;
  check(number(g.width) && g.width>0 && number(g.height) && g.height>0 && record(g.roles));
  check(Object.values(g.roles).every(string));
  for(const [name,max] of [['nodes',100],['edges',300],['revisionEdges',300]])check(Array.isArray(g[name]) && g[name].length<=max);
  const all=[...g.nodes,...g.edges,...g.revisionEdges],ids=new Set();
  for(const item of all) {
    check(record(item) && string(item.id) && item.id.length>0 && !ids.has(item.id));ids.add(item.id);
    check(string(item.label) && record(item.raw));
    if(item.raw.revision!==undefined) {
      const r=item.raw.revision;
      check(record(r) && Array.isArray(r.parents) && r.parents.every(p=>record(p)&&string(p.id)));
    }
    if(item.raw.source!==undefined) {
      const s=item.raw.source;check(record(s));
      for(const k of ['artifacts','revisions'])if(s[k]!==undefined)check(strings(s[k]));
    }
  }
  const nodes=new Set(g.nodes.map(n=>n.id));
  for(const n of g.nodes) {
    check(n.raw.id===n.id && string(n.title) && string(n.typeLabel) && string(n.kind));
    check(typeof n.current==='boolean' && typeof n.ghost==='boolean' && typeof n.fork==='boolean' && typeof n.roleConflict==='boolean' && typeof n.isolated==='boolean');
    check(number(n.x) && number(n.y) && strings(n.roles) && strings(n.bindingIds));
    check(n.from===undefined && n.to===undefined);
    if(n.presentationStatus!==undefined) {
      const s=n.presentationStatus;
      const exact=(o,fields)=>record(o) && Object.keys(o).sort().join(',')===fields.sort().join(',');
      const digest=d=>typeof d==='string' && /^sha256:[0-9a-f]{64}$/.test(d);
      const text=(t,max)=>typeof t==='string' && t.trim().length>0 && t.length<=max;
      check(exact(s,['revision_id','canonical_digest','status','label','reason','assessed_at','source_refs']));
      check(s.revision_id===n.id && digest(s.canonical_digest));
      const labels={historical:['历史方案','历史记录'],superseded:['已替代'],current:['材料声明现用']};
      check(Object.hasOwn(labels,s.status) && labels[s.status].includes(s.label) && text(s.reason,2000));
      check(text(s.assessed_at,40) && /(?:Z|[+-]\d{2}:\d{2})$/.test(s.assessed_at) && Number.isFinite(Date.parse(s.assessed_at)));
      check(Array.isArray(s.source_refs) && s.source_refs.length>0 && s.source_refs.length<=8);
      const seen=new Set();
      for(const ref of s.source_refs) {
        check(exact(ref,['id','canonical_digest','locator']) && string(ref.id) && Object.hasOwn(records,ref.id));
        check(['rp/artifact-manifest/v1','rp/external-reference/v1'].includes(records[ref.id].schema));
        check(digest(ref.canonical_digest) && text(ref.locator,1000) && !seen.has(ref.id));seen.add(ref.id);
      }
      // Digest equality is checked against canonical copies by the producer;
      // browser checks bounded shape/identity, not a second JSON canonicalizer.
    }
  }
  // Optional for older projections; every new producer emits startIds. New
  // metadata may only point to real Questions / explicit source.revisions.
  if(g.startIds!==undefined) {
    check(strings(g.startIds) && new Set(g.startIds).size===g.startIds.length);
    for(const id of g.startIds)check(g.nodes.some(n=>n.id===id && n.current && n.kind==='Question' && n.bindingIds.length));
  }
  if(g.aliasLegend!==undefined)check(strings(g.aliasLegend) && g.aliasLegend.length<=7);
  if(g.readingSources!==undefined) {
    check(record(g.readingSources));
    for(const [id, refs] of Object.entries(g.readingSources)) {
      const n=g.nodes.find(n=>n.id===id);check(n && record(refs));
      for(const [target,label] of Object.entries(refs))check(nodes.has(target) && n.raw.source?.revisions?.includes(target) && string(label));
    }
  }
  for(const e of [...g.edges,...g.revisionEdges]) {
    check(nodes.has(e.from) && nodes.has(e.to) && string(e.type));
    check(string(e.path) && /^M(?:[MLQC\s\d.,-]+)$/.test(e.path));
    check(number(e.labelX) && number(e.labelY));
    if(e.labelHalfWidth!==undefined)check(number(e.labelHalfWidth) && e.labelHalfWidth>=0);
    if(e.labelLeaderPath!==undefined)check(string(e.labelLeaderPath) && (e.labelLeaderPath==='' || /^M(?:[MLQC\s\d.,-]+)$/.test(e.labelLeaderPath)));
    if(e.labelStatus!==undefined)check(['placed','hidden'].includes(e.labelStatus));
    if(e.routeStatus!==undefined)check(['routed','unroutable'].includes(e.routeStatus));
  }
  for(const e of g.edges)check(e.category==='science' && typeof e.current==='boolean' && e.raw.id===e.id && e.raw.from_revision===e.from && e.raw.to_revision===e.to);
  for(const e of g.revisionEdges)check(e.category==='revision' && e.type==='revision');
  for(const s of Object.values(data.sources))check(record(s) && string(s.title) && string(s.excerpt) && Array.isArray(s.sections) && s.sections.every(p=>record(p)&&string(p.heading)&&Number.isInteger(p.start_line)&&Number.isInteger(p.end_line)));
  validateSourceLocators(data);
  return all;
}
function validateDOM(doc, items) {
  const ids=new Set();
  for(const el of doc.querySelectorAll('[id]')) {const id=el.getAttribute('id');check(!ids.has(id));ids.add(id);}
  for(const [id,tag] of Object.entries(REQUIRED)) {
    const el=doc.getElementById(id);check(el && el.tagName.toLowerCase()===tag);
  }
  const get=id=>doc.getElementById(id),svg=get('graph');
  const box=svg.getAttribute('viewBox');
  check(box!==null && box.trim().split(/\s+/).length===4 && box.trim().split(/\s+/).map(Number).every(number));
  check(svg.getAttribute('preserveAspectRatio')==='xMidYMid meet');
  check(get('graph-data').getAttribute('type')==='application/json');
  check(get('search').getAttribute('type')==='search' && get('history').getAttribute('type')==='checkbox');
  check(get('workspace').contains(svg) && get('workspace').contains(get('toolbar')) && get('workspace').contains(get('details')) && get('details').contains(get('detail-content')));
  for(const id of ['search','filter','history','zoom-in','zoom-out','fit','back','restore-all','reset','show-all','start','fullscreen','toggle-details','help','domains'])check(get('toolbar').contains(get(id)) && get(id).disabled);
  check(get('workspace').contains(get('domain-panel')) && !get('details').contains(get('domain-panel')));
  check(get('domain-panel').contains(get('domain-history')) && get('domain-panel').contains(get('domain-groups')) && get('domain-panel').contains(get('domain-counts')));
  check(get('domain-history').getAttribute('type')==='checkbox' && get('domain-history').disabled);
  check(get('domains').getAttribute('aria-controls')==='domain-panel' && get('domains').getAttribute('aria-expanded')==='false');
  check(get('help').getAttribute('type')==='button' && get('help').getAttribute('aria-controls')==='reading-guide');
  check(get('details').contains(get('reading-guide')) && get('reading-guide').contains(get('reading-guide-summary')));
  check(get('fullscreen').getAttribute('type')==='button' && get('fullscreen').getAttribute('aria-pressed')==='false');
  check(get('toggle-details').getAttribute('type')==='button' && get('toggle-details').getAttribute('aria-controls')==='details' && get('toggle-details').getAttribute('aria-expanded')==='true');
  const by=new Map(items.map(x=>[x.id,x])),seen=new Set();
  for(const el of svg.querySelectorAll('[data-key]')) {
    const key=el.getAttribute('data-key'),item=by.get(key);
    check(item && !seen.has(key));seen.add(key);
    check(el.tagName.toLowerCase()==='g' && el.getAttribute('role')==='button' && el.getAttribute('tabindex')!==null);
    if(item.from)check(el.getAttribute('data-from')===item.from && el.getAttribute('data-to')===item.to);
    else check(el.getAttribute('data-from')===null && el.getAttribute('data-to')===null);
  }
  check(seen.size===by.size);
}
function boot(doc) {
  if(boots.has(doc))return boots.get(doc);
  const life={active:false,api:null};boots.set(doc,life);
  const get=id=>doc.getElementById(id);
  const controls=()=>[...(get('toolbar')?.querySelectorAll('button,input,select') || []),...(get('domain-panel')?.querySelectorAll('button,input') || [])];
  function lock() {
    for(const c of controls())c.disabled=true;
    for(const id of ['graph','details','search-results','start-choices','domain-panel']) {
      const el=get(id);if(el){el.inert=true;el.setAttribute('aria-disabled','true');}
    }
    for(const el of get('graph')?.querySelectorAll('[data-key]') || [])el.setAttribute('tabindex','-1');
  }
  life.fail=()=>{
    life.active=false;doc.body.setAttribute('data-app-state','failed');lock();
    // If the status ID itself is missing, don't rely on it for the diagnostic.
    let status=get('app-status');
    if(!status){status=doc.createElement('p');doc.body.appendChild(status);}
    status.setAttribute('role','status');status.setAttribute('aria-live','polite');
    status.textContent='交互启动失败或交互已停止；下方完整规范记录原文仍可阅读，图需脚本成功启动。重试请使用浏览器重新加载原文件（不会更新快照数据）。';
  };
  async function loadLive() {
    const [wireRes, recordsRes] = await Promise.all([
      fetch('/api/wire'), fetch('/api/records')
    ]);
    if(!wireRes.ok || !recordsRes.ok) throw new Error('fetch failed');
    const [wireText, recordsText] = await Promise.all([wireRes.text(), recordsRes.text()]);
    return {wireText, recordsText};
  }
  function buildGraph(wireText, recordsText) {
    const override = wireText ? {wireText, recordsText} : undefined;
    const decoded = decodeWire(doc, override);
    const items = validateProjection(decoded.data, decoded.records);
    buildSVG(doc, decoded.data.graph);
    validateDOM(doc, items);
    life.api = mount(doc, decoded.data, life);
    for(const c of controls())c.disabled = false;
    get('back').disabled = !life.api.model.stack.length;
    for(const id of ['graph','details','search-results','start-choices','domain-panel']) {
      get(id).inert = false;
      get(id).setAttribute('aria-disabled','false');
    }
    life.active = true;
    doc.body.setAttribute('data-app-state','ready');
    get('app-status').textContent = '私有快照 · 交互已启动；当前记录并非项目完整逻辑。';
  }
  try {
    doc.body.setAttribute('data-app-state','loading');
    if(typeof fetch==='function' && typeof EventSource==='function') {
      loadLive().then(o=>{
        buildGraph(o.wireText, o.recordsText);
        let generation = 0;
        try {
          const es = new EventSource('/api/events');
          es.onmessage = (ev) => {
            try {
              const msg = JSON.parse(ev.data);
              if(msg.generation && msg.generation !== generation) {
                // Re-mount is unsupported by design (SVG shell and mount are
                // single-boot); a full reload is the honest live update.
                generation = msg.generation;
                location.reload();
              }
            } catch(_) {}
          };
        } catch(_) {}
      }).catch(()=>{ buildGraph(); });
    } else { buildGraph(); }
  } catch (_) { life.fail(); }
  return life;
}
if(typeof module!=='undefined' && module.exports)module.exports={boot,decodeWire,validateProjection,validateDOM,finishWire};
if(typeof document!=='undefined')boot(document);
