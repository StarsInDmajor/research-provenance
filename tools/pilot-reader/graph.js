'use strict';
function nodeHistory(n) {
  const status=n.presentationStatus,historical=status && ['historical','superseded'].includes(status.status);
  const revision=n.ghost?'历史端点(当前引用)':!n.current?'旧修订':'最新记录修订';
  const content=historical?'描述'+status.label:status?status.label:'内容状态待核实';
  return {dashed:!n.current || n.ghost || Boolean(historical),label:revision+'·'+content};
}
// Display vocabulary only: membership comes exclusively from recorded tags.
const DOMAIN_LABELS={design:'参数与设计',simulation:'模拟',observations:'观测',uvlf:'紫外光度函数',
  xhi:'中性氢比例',forest:'莱曼 α 森林',cmb:'宇宙微波背景',inference:'统计推断',
  lifecycle:'运行生命周期',guard:'守卫与边界',manuscript:'论文写作',governance:'研究治理'};
function tagGroups(nodes) {
  const groups=new Map(),seen=new Set();
  for(const n of nodes) {
    if(seen.has(n.id))continue;seen.add(n.id);
    const tags=new Set((Array.isArray(n.raw?.tags)?n.raw.tags:[]).filter(t=>typeof t==='string'));
    if(!tags.size)tags.add(null);
    for(const tag of tags) {
      if(!groups.has(tag))groups.set(tag,{tag,label:tag===null?'未标记':Object.hasOwn(DOMAIN_LABELS,tag)?DOMAIN_LABELS[tag]:tag,
        category:tag===null?'未标记':Object.hasOwn(DOMAIN_LABELS,tag)?'研究领域':'其他标签',nodes:[],current:0,history:0});
      const group=groups.get(tag);group.nodes.push(n);group[n.current?'current':'history']++;
    }
  }
  return [...groups.values()].sort((a,b)=>String(a.tag)<String(b.tag)?-1:String(a.tag)>String(b.tag)?1:0);
}
/* No DOM in Model: state tested without a browser. The adapter below changes only
 * existing SVG attributes/classes or creates text nodes/elements, never HTML. */
const edgeRouting = typeof module !== 'undefined' && module.exports ? require('./routing.js') : Routing;
class Model {
  constructor(graph) {
    if (graph.nodes.length > 100 || graph.edges.length > 300) throw new Error('Graph bound: 100 nodes / 300 science relations');
    this.graph = graph;
    this.routeCache = new Map();
    this.routeStats = {computations:0,hits:0,key:'',entries:0};
    const items=[...graph.nodes, ...graph.edges, ...graph.revisionEdges];
    if(new Set(items.map(x=>x.id)).size!==items.length)throw new Error('Duplicate projection key');
    this.items = new Map(items.map(x => [x.id, x]));
    this.stack = []; this.capture = () => {};
    this.defaults();
  }
  defaults() {
    this.mode = 'all'; this.history = false; this.selected = null; this.notice = '';
    this.local = null; this.revealed = []; this.detail = {scroll:0,open:[],focus:null};
    this.searchState = {query:'',index:-1,closed:true};
    this.box = {x:0,y:0,w:this.graph.width,h:this.graph.height};
    this.fit(); // actual default-visible hull, including negative placed labels
  }
  snapshot() {
    return JSON.parse(JSON.stringify(Object.fromEntries(['mode','history','selected','notice','local','revealed','box','detail','searchState'].map(k=>[k,this[k]]))));
  }
  transition(change) {
    this.capture();
    const before = this.snapshot(); change();
    const after = this.snapshot();
    // Notices alone are not navigation; saturation and repeated selection add no frame.
    if (JSON.stringify({...before,notice:''}) === JSON.stringify({...after,notice:''})) return false;
    this.stack.push(before); if (this.stack.length > 50) this.stack.shift();
    return true;
  }
  back() {
    if (!this.stack.length) return false;
    Object.assign(this,this.stack.pop()); return true;
  }
  reset() { return this.transition(()=>this.defaults()); }
  eligibleEdges() {
    return [...this.graph.edges.filter(e=>this.history || e.current),...(this.history ? this.graph.revisionEdges : [])];
  }
  view() {
    const role = {mainline:'primary',alternative:'alternative'}[this.mode];
    const eligible = this.graph.nodes.filter(n=>this.history || n.current || n.ghost);
    const seeds = new Set(eligible.filter(n=>!role || n.roles.includes(role)).map(n=>n.id));
    const ids = new Set(this.local || seeds);
    if (!this.local && role) for (const e of this.eligibleEdges()) {
      if (seeds.has(e.from) || seeds.has(e.to)) {ids.add(e.from);ids.add(e.to);}
    }
    for (const id of this.revealed) ids.add(id);
    const nodes = eligible.filter(n=>ids.has(n.id));
    const shown = new Set(nodes.map(n=>n.id));
    const endpoints = e=>shown.has(e.from) && shown.has(e.to);
    let edges = this.graph.edges.filter(e=>(this.history || e.current) && endpoints(e));
    let revisionEdges = this.history ? this.graph.revisionEdges.filter(endpoints) : [];
    const key=JSON.stringify([nodes.map(n=>n.id).sort(),[...edges,...revisionEdges].map(e=>e.id).sort()]);
    let routes=this.routeCache.get(key);
    if(!routes) {
      routes=edgeRouting.route(nodes,[...edges,...revisionEdges],[...this.graph.edges,...this.graph.revisionEdges]);
      this.routeCache.set(key,routes);this.routeStats.computations++;
      if(this.routeCache.size>16)this.routeCache.delete(this.routeCache.keys().next().value);
    } else this.routeStats.hits++;
    Object.assign(this.routeStats,{key,entries:this.routeCache.size});
    edges=edges.map(e=>({...e,...routes[e.id]}));revisionEdges=revisionEdges.map(e=>({...e,...routes[e.id]}));
    const focused = new Set(nodes.filter(n=>role ? seeds.has(n.id) : this.local ? n.id===this.selected : true).map(n=>n.id));
    const focusedEdgeIds = new Set([...edges,...revisionEdges].filter(e=>focused.has(e.from)&&focused.has(e.to)).map(e=>e.id));
    return {nodes,edges,revisionEdges,focused,focusedEdgeIds,focusedNodes:focused.size,contextNodes:nodes.length-focused.size,
      focusedEdges:edges.filter(e=>focusedEdgeIds.has(e.id)).length,
      contextEdges:edges.filter(e=>!focusedEdgeIds.has(e.id)).length,
      hiddenNodes:this.graph.nodes.length-nodes.length,hiddenEdges:this.graph.edges.length-edges.length,
      hiddenRevisions:this.graph.revisionEdges.length-revisionEdges.length};
  }
  retainSelection() {
    const v=this.view();
    if (![...v.nodes,...v.edges,...v.revisionEdges].some(n=>n.id===this.selected)) this.selected=null;
  }
  setFilter(mode) {
    if (!['all','mainline','alternative'].includes(mode) || this.mode===mode && !this.local) return;
    this.transition(()=>{
      this.mode=mode;this.local=null;this.revealed=[];this.retainSelection();
      this.notice=this.view().focusedNodes ? '角色聚焦：保留一跳连接上下文（淡化），不推断额外角色。' : '没有符合此角色的节点；可恢复全图或搜索定位。';
      this.fit();
    });
  }
  setHistory(on) {
    if (this.history===Boolean(on)) return;
    this.transition(()=>{
      this.history=Boolean(on);this.revealed=[];this.retainSelection();
      this.notice=on ? '已展开历史层；点划线是修订，不是科研关系。' : '当前层保留当前关系精确引用的历史端点。';this.fit();
    });
  }
  select(id) {
    if (this.items.has(id) && this.selected!==id) this.transition(()=>{this.selected=id;this.detail={scroll:0,open:[],focus:null};});
  }
  clear() { if(this.selected) this.transition(()=>{this.selected=null;this.notice='已清除选择。';}); }
  focus() {
    const item=this.items.get(this.selected);if (!item || item.from) return;
    this.transition(()=>{
      const ids=new Set([item.id]);
      for(const e of this.eligibleEdges()) if(e.from===item.id || e.to===item.id){ids.add(e.from);ids.add(e.to);}
      this.local=[...ids];this.revealed=[];this.notice='只看相关：所选节点的一跳上下文；可逐次沿入向或出向关系展开。';this.fit();
    });
  }
  expand(direction) {
    const item=this.items.get(this.selected);if(!item || item.from || !['in','out'].includes(direction))return;
    const ids=new Set([...(this.local || []),...this.revealed,item.id]);
    const frontier=new Set(ids);
    for(const e of this.eligibleEdges()) {
      const from=direction==='in'?e.to:e.from, to=direction==='in'?e.from:e.to;
      if(frontier.has(from)) ids.add(to);
    }
    if(this.local && ids.size===this.local.length) {this.notice='此方向没有更多节点（已访问，或当前层未记录）。';return;}
    this.transition(()=>{this.local=[...ids];this.revealed=[];this.notice='沿'+(direction==='in'?'入':'出')+'向关系展开一跳；方向不代表证据或因果推断。';this.fit();});
  }
  restoreAll() {
    this.transition(()=>{this.mode='all';this.local=null;this.revealed=[];this.retainSelection();this.notice='已恢复全图；保留当前历史开关与可见选择。';this.fit();});
  }
  showAll() {
    this.transition(()=>{this.mode='all';this.history=true;this.local=null;this.revealed=[];this.notice='全部与历史；修订线和科研关系分开显示。';this.fit();});
  }
  search(query) {
    const q = query.trim().toLocaleLowerCase();
    if (!q) return [];
    const lower=s=>typeof s==='string'?s.toLocaleLowerCase():'';
    const rank=n=>lower(n.id)===q?0:lower(n.raw?.logical_id)===q?1:
      [n.title,n.raw?.title].some(s=>lower(s)===q)?2:3;
    return [...this.items.values()].filter(n => [n.id,n.raw?.logical_id,n.label,n.title,n.raw?.title,n.from,n.to]
      .some(s => lower(s).includes(q)))
      .sort((a,b)=>rank(a)-rank(b) || (a.id<b.id?-1:a.id>b.id?1:0));
  }
  reveal(id) {
    const item = this.items.get(id);
    if (!item) return;
    this.transition(()=>{
      if(item.category==='revision' || !item.current || item.from && [item.from,item.to].some(k=>!this.items.get(k)?.current)) this.history=true;
      const targets=item.from ? [this.items.get(item.from),this.items.get(item.to)] : [item];
      this.revealed=[...new Set([...this.revealed,...targets.map(n=>n.id)])];
      if(this.selected!==id) this.detail={scroll:0,open:[],focus:null};
      this.selected=id;
      this.notice='已定位精确对象'+(this.history?'（历史层已开启）':'')+'；只补所需端点，返回可恢复原视图。';
      this.fitTargets(targets,item.from?[item]:[]);
    });
  }
  start(id) {
    if (!(this.graph.startIds || []).includes(id)) return;
    const item=this.items.get(id);if (!item || item.kind!=='Question' || !item.current) return;
    this.transition(()=>{
      this.mode='all';this.history=false;this.selected=id;
      this.detail={scroll:0,open:[],focus:null};this.revealed=[];
      const ids=new Set([id]);
      for(const e of this.eligibleEdges())if(e.from===id || e.to===id){ids.add(e.from);ids.add(e.to);}
      this.local=[...ids];this.fit();
      this.notice='建议入口：从问题和已记录的一跳邻域开始；不是完整图的时间根或层级根。来源引用单列，不补造科研边。';
    });
  }
  neighborhood() {
    const nodes = new Set(), edges = new Set();
    if (!this.selected) return {nodes,edges};
    const item = this.items.get(this.selected);
    if (item?.from) {
      nodes.add(item.from); nodes.add(item.to); edges.add(item.id);
    } else {
      nodes.add(this.selected);
      for (const e of [...this.graph.edges,...(this.history ? this.graph.revisionEdges : [])]) {
        if (!this.history && !e.current) continue;
        if (e.from === this.selected || e.to === this.selected) {
          nodes.add(e.from); nodes.add(e.to); edges.add(e.id);
        }
      }
    }
    return {nodes,edges};
  }
  fit() {
    const v=this.view();this.fitTargets(v.nodes,[...v.edges,...v.revisionEdges]);
  }
  fitTargets(nodes,edges) {
    if (!nodes.length) {this.box = {x:0,y:0,w:this.graph.width,h:this.graph.height};return;}
    const points=nodes.flatMap(n=>[[n.x,n.y],[n.x+270,n.y+114]]);
    const view=this.view(), geometry=new Map([...view.edges,...view.revisionEdges].map(e=>[e.id,e]));
    for(const original of edges) {
      const e=geometry.get(original.id);
      if(!e || e.routeStatus==='unroutable')continue;
      // Projection emits only absolute M/L/Q/C. The control hull conservatively
      // includes off-node loops, gutters, arrowheads and long label hit targets.
      const numbers=(e.path || '').match(/-?\d+(?:\.\d+)?/g)?.map(Number) || [];
      for(let i=0;i+1<numbers.length;i+=2)points.push([numbers[i],numbers[i+1]]);
      if(e.labelStatus==='placed'&&Number.isFinite(e.labelX)&&Number.isFinite(e.labelY)) {
        const half=e.labelHalfWidth;
        points.push([e.labelX-half,e.labelY-20],[e.labelX+half,e.labelY+20]);
      }
    }
    const x=Math.min(...points.map(p=>p[0]))-30,y=Math.min(...points.map(p=>p[1]))-40;
    const w=Math.max(...points.map(p=>p[0]))-x+30,h=Math.max(...points.map(p=>p[1]))-y+40;
    // Keep viewBox aspect stable so wheel/pointer mapping has one uniform scale.
    const ratio = this.graph.width/this.graph.height;
    const finalW = Math.max(w,h*ratio), finalH = finalW/ratio;
    this.box = {x:x-(finalW-w)/2,y:y-(finalH-h)/2,w:finalW,h:finalH};
    this.bound();
  }
  bound() {
    this.box.x = Math.max(-this.graph.width*2,Math.min(this.graph.width*2,this.box.x));
    this.box.y = Math.max(-this.graph.height*2,Math.min(this.graph.height*2,this.box.y));
  }
  zoom(factor, point={x:.5,y:.5}) {
    if (!Number.isFinite(factor) || factor <= 0 || factor === 1) return;
    const px = Math.max(0,Math.min(1,point.x)), py = Math.max(0,Math.min(1,point.y));
    // Cap the reference at four 270-unit cards, not the whole large graph.
    // Fit may legitimately lie outside either zoom limit: saturate only in the
    // requested direction, never snap a fitted box backwards into the range.
    const min=Math.min(this.graph.width,270*4)/8, max=this.graph.width*3;
    const w = factor>1 ? Math.min(this.box.w,Math.max(min,this.box.w/factor)) :
      Math.max(this.box.w,Math.min(max,this.box.w/factor));
    if(w===this.box.w)return;
    const h = this.box.h*(w/this.box.w);
    this.box = {x:this.box.x+(this.box.w-w)*px,y:this.box.y+(this.box.h-h)*py,w,h};
    this.bound();
  }
  pan(dx,dy) {
    if (!Number.isFinite(dx) || !Number.isFinite(dy)) return;
    this.box.x += dx; this.box.y += dy; this.bound();
  }
}

const FIELD_LABELS = {title:'原始标题',statement:'原始陈述',scope:'适用范围',assumptions:'前提',limitations:'限制',
  next_action:'行动',completion_criteria:'完成标准',done_when:'完成条件',status:'状态',record_state:'记录状态',
  hypothesis:'思路',question:'问题',observation:'观察',decision:'决定',blocker:'阻塞',description:'说明',summary:'摘要',
  conclusion:'结论',interpretation:'解释',method:'方法',residual_uncertainty:'残余不确定性',use_limitations:'使用限制',
  unresolved_alternatives:'未决替代解释',controls:'对照',protocol:'方案',
  rationale:'理由',success_criteria:'成功标准',acceptance_criteria:'验收标准',action:'行动内容',intended_outcome:'预期结果',dependencies:'依赖对象',owner:'负责人'};
function readable(value, depth=0) {
  if(value===undefined) return '未记录';
  if(value===null) return '空值';
  if(depth>6) return '（深层字段见完整原文）';
  if(Array.isArray(value)) return value.slice(0,40).map(v=>'• '+readable(v,depth+1)).join('\n')+(value.length>40?'\n（其余见完整原文）':'');
  if(typeof value==='object') return Object.entries(value).slice(0,40).map(([k,v])=>(FIELD_LABELS[k]||k)+'：'+readable(v,depth+1)).join('\n')+(Object.keys(value).length>40?'\n（其余见完整原文）':'');
  return String(value).slice(0,4000)+(String(value).length>4000?'…（其余见完整原文）':'');
}
function fieldDiff(before,after) {
  const rows=[];let truncated=false;
  const fields=['title','statement','scope','assumptions','limitations','record_state','question','hypothesis','observation','decision','blocker','next_action','conclusion','interpretation','method'];
  function capped(value,depth=0) {
    if(depth>6)return true;
    if(typeof value==='string')return value.length>4000;
    if(value && typeof value==='object')return Object.keys(value).length>40 || Object.values(value).some(v=>capped(v,depth+1));
    return false;
  }
  function walk(a,b,path,depth) {
    if(JSON.stringify(a)===JSON.stringify(b))return;
    if(rows.length>=40){truncated=true;return;}
    if(depth<5 && (a && typeof a==='object' && !Array.isArray(a) || b && typeof b==='object' && !Array.isArray(b))) {
      for(const key of new Set([...Object.keys(a||{}),...Object.keys(b||{})]))walk(a?.[key],b?.[key],[...path,key],depth+1);
    } else {
      const old=readable(a),next=readable(b);
      if(old.length>4000||next.length>4000||capped(a)||capped(b))truncated=true;
      rows.push({label:path.map(k=>FIELD_LABELS[k]||k).join(' · '),before:old.slice(0,4000),after:next.slice(0,4000)});
    }
  }
  for(const key of fields)walk(before[key],after[key],[key],0);
  return {rows,truncated};
}

// Direction is relative to the selected node; unknown scientific types stay literal.
function relationMeaning(type,outgoing) {
  const meanings={supports:['支持对方','被支持，支持来自对方'],weakens:['削弱对方','被削弱，质疑来自对方'],
    contradicts:['反驳对方','被反驳，反驳来自对方'],'derived-from':['依据对方推导','被用作依据，对方推导自此'],
    'depends-on':['依赖对方','被依赖，对方使用此项'],'blocked-by':['被阻塞，阻塞来自对方','阻塞对方'],
    motivates:['促成对方','被促成，动机来自对方'],requires:['需要对方','被需要，对方需要此项'],
    'has-part':['包含对方','组成对方的一部分'],'input-to':['作为对方的输入','接收对方的输入'],
    generated:['生成对方','生成自对方'],cites:['引用对方','被引用，引用来自对方'],implements:['实现对方','由对方实现'],
    'consistent-with':['与对方一致','与对方一致'],'provides-prior':['向对方提供先验','先验来自对方'],
    predicts:['预测对方','被预测，预测来自对方'],'tested-by':['由对方检验','检验对方'],
    'result-of':['结果来自对方','产生结果，对方是此项结果'],'observed-in':['观测于对方','被观测的环境，观测为对方'],
    reproduces:['复现对方','被复现，复现来自对方'],'fails-to-reproduce':['未能复现对方','未被对方复现'],
    'validated-by':['由对方验证','验证对方'],enables:['使对方可行','由对方使其可行'],'next-step':['下一步是对方','前一步是对方']};
  return (outgoing?'出向 · ':'入向 · ')+(Object.hasOwn(meanings,type)?meanings[type][outgoing?0:1]:type);
}
function recordLabel(item,items) {
  if(item?.category==='science')return (items.get(item.from)?.label || '起点')+' → '+item.label+' → '+(items.get(item.to)?.label || '终点');
  return item?.label || item?.title;
}
function recordState(item) {
  if(!item)return '未判定当前/历史（不在画布）';
  if(item.category==='science')return (item.current?'当前 active head':'历史关系')+(item.raw?.relation_state==='invalidated'?' · 已撤销':'');
  return item.current?'当前修订':'旧修订（历史）';
}
// Read-only mount-time index over the admitted corpus, never new graph elements.
// One referrer per exact record ID/target; JSON-parsed duplicate projections are
// copies, not distinct uses. Keep at most 100 rows per target, count every use.
function reverseSources(data) {
  const projected=new Map([...data.graph.nodes,...data.graph.edges].map(n=>[n.id,n]));
  const records=new Map(Object.entries(data.records));
  for(const [id,item] of projected)records.set(id,item.raw);
  if(records.size>512)throw new Error('Source index bound: 512 records');
  const index=new Map();
  for(const [id,raw] of [...records].sort(([a],[b])=>a<b?-1:a>b?1:0)) {
    const item=projected.get(id);
    for(const target of new Set(raw.source?.revisions || [])) {
      if(!index.has(target))index.set(target,{refs:[],total:0});
      const entry=index.get(target);entry.total++;
      if(entry.refs.length<100)entry.refs.push({id,label:recordLabel(item,projected) || raw.title || raw.kind || raw.schema || '记录',
        type:raw.kind || raw.schema || '记录',current:item?item.current:null,locatable:Boolean(item),state:recordState(item),raw});
    }
  }
  return index;
}

function wireSelection(element, model, render, isDragging, on=(el,...args)=>el.addEventListener(...args)) {
  element.dataset.focus=JSON.stringify(['graph',element.dataset.key]);
  on(element,'click', () => {
    if (isDragging()) return;
    model.select(element.dataset.key); render();
  });
  on(element,'keydown', event => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault(); model.select(element.dataset.key); render();
    }
  });
}

// Account for preserveAspectRatio="xMidYMid meet" letterboxing. Points outside
// the rendered viewBox are clamped to its boundary; no guessed CSS pixel scale.
function viewport(rect, box, clientX, clientY) {
  const scale = Math.min(rect.width/box.w,rect.height/box.h);
  const width = box.w*scale, height = box.h*scale;
  const left = rect.left+(rect.width-width)/2, top = rect.top+(rect.height-height)/2;
  return {scale,x:Math.max(0,Math.min(1,(clientX-left)/width)),
    y:Math.max(0,Math.min(1,(clientY-top)/height))};
}

// 16px/line; page height bounded to 100–1000px (800px without geometry).
// Same fine mapping for Ctrl+wheel pinch. Log cap gives reciprocal ±10% steps.
function wheelFactor(delta, mode=0, pageHeight=800) {
  if(!Number.isFinite(delta) || ![0,1,2].includes(mode))return 1;
  const page=Number.isFinite(pageHeight)?Math.max(100,Math.min(1000,pageHeight)):800;
  const unit=mode===1?16:mode===2?page:1, cap=Math.log(1.1);
  // Saturate converted deltas before exponentiation, including overflow to ±∞.
  const pixels=Math.max(-cap/.0012,Math.min(cap/.0012,delta*unit));
  return Math.exp(-pixels*.0012);
}

const mounts=new WeakMap();
function mount(doc, data, life={active:true,fail:()=>{throw new Error('Reader update failed');}}) {
  if(mounts.has(doc))return mounts.get(doc);
  // Reserve before registering anything: interrupted mounts cannot attach twice.
  const api={model:null,render:null};mounts.set(doc,api);
  const on=(el,event,handler,options)=>el.addEventListener(event,(...args)=>{
    if(!life.active)return;
    try {handler(...args);} catch (_) {life.active=false;life.fail();}
  },options);
  const model = new Model(data.graph), sourceIndex=reverseSources(data);
  const get = id => doc.getElementById(id);
  const svg = get('graph'), content = get('detail-content');
  const elements = [...svg.querySelectorAll('[data-key]')];
  let drag = null, suppressClick = false, renderedSelection, renderedDetailView;
  let detailButtons=[];
  const panel=get('details'),workspace=get('workspace'),win=doc.defaultView;
  let collapsedScroll=0,expanded=false,nativeSeen=false,pending=null,exiting=false,savedPage=null;
  const fullscreen=get('fullscreen'),detailToggle=get('toggle-details');
  function canvasStatus() {
    workspace.classList.toggle('canvas-expanded',expanded);
    doc.body?.classList.toggle('canvas-expanded',expanded);
    const mode=expanded?(doc.fullscreenElement===workspace?'native':'window'):'normal';
    workspace.setAttribute('data-canvas-mode',mode);
    fullscreen.setAttribute('aria-pressed',String(expanded));
    fullscreen.textContent=expanded?'退出全屏画布':'全屏画布';
    fullscreen.disabled=!life.active || (!expanded && (pending!==null || exiting));
    get('canvas-status').textContent=mode==='native'?'浏览器原生全屏 · Esc 退出；详情可独立收起。':
      mode==='window'?'窗口填满模式（非浏览器原生全屏）· Esc 退出；详情可独立收起。':
      pending?'正在取消全屏请求；画布已恢复。':'普通画布 · 全屏画布可填满窗口；原生全屏不可用时使用窗口模式。';
  }
  function restorePage() {
    savedPage?.focus?.focus({preventScroll:true});
    if(win && savedPage)win.scrollTo(savedPage.x,savedPage.y);
    // Keep the original until native teardown (or cancelled entry) settles.
    if(doc.fullscreenElement!==workspace && !pending && !exiting)savedPage=null;
  }
  function leaveCanvas() {
    if(!expanded)return;
    const scroll=panel.hidden?collapsedScroll:panel.scrollTop;
    expanded=false;nativeSeen=false;canvasStatus();
    if(!panel.hidden)panel.scrollTop=scroll;
    restorePage();
  }
  function exitNative() {
    if(doc.fullscreenElement!==workspace || exiting)return;
    exiting=true;canvasStatus();
    function finished() {
      exiting=false;
      // Native exit may itself be denied. Keep a truthful, usable native UI.
      if(doc.fullscreenElement===workspace){enterCanvas();nativeSeen=true;}
      else if(!expanded)restorePage();
      canvasStatus();
    }
    try {Promise.resolve(doc.exitFullscreen()).then(finished,finished);}
    catch (_) {finished();}
  }
  function enterCanvas() {
    if(expanded)return;
    savedPage ??= {x:win?.scrollX || 0,y:win?.scrollY || 0,focus:doc.activeElement};
    const scroll=panel.hidden?collapsedScroll:panel.scrollTop;
    expanded=true;canvasStatus();
    if(!panel.hidden)panel.scrollTop=scroll;
  }
  function exitCanvas() {leaveCanvas();exitNative();}
  function escapeCanvas(event) {
    if(event.key!=='Escape' || !expanded)return false;
    event.preventDefault();event.stopPropagation?.();exitCanvas();return true;
  }
  // Cancellation must run even after readiness fails. Capture precedes search's
  // existing Esc handling; no graph render, fit, navigation or detail DOM writes.
  doc.addEventListener('keydown',escapeCanvas,true);
  doc.addEventListener('fullscreenchange',()=>{
    if(doc.fullscreenElement===workspace) {
      if(!expanded || !life.active){leaveCanvas();exitNative();return;}
      nativeSeen=true;canvasStatus();
    } else if(nativeSeen)leaveCanvas();
    else if(!expanded)restorePage();
  });
  on(fullscreen,'click',()=>{
    if(expanded){exitCanvas();return;}
    if(pending || exiting)return; // settle native operations before another request
    enterCanvas();
    if(typeof workspace.requestFullscreen!=='function' || doc.fullscreenEnabled===false)return;
    const token={};pending=token;
    function settled() {
      if(pending!==token)return;
      pending=null;
      if(!life.active)leaveCanvas();
      if(!expanded){exitNative();restorePage();}
      canvasStatus();
    }
    try {Promise.resolve(workspace.requestFullscreen()).then(settled,settled);}
    catch (_) {settled();} // denial/unavailability is not application failure
  });
  on(detailToggle,'click',()=>{
    if(!panel.hidden) {
      collapsedScroll=panel.scrollTop || 0;
      if(panel.contains(doc.activeElement))detailToggle.focus({preventScroll:true});
    }
    panel.hidden=!panel.hidden;
    workspace.classList.toggle('details-collapsed',panel.hidden);
    detailToggle.setAttribute('aria-expanded',String(!panel.hidden));
    detailToggle.textContent=panel.hidden?'展开详情':'收起详情';
    if(!panel.hidden)panel.scrollTop=collapsedScroll;
  });
  const domainPanel=get('domain-panel'),domainToggle=get('domains'),domainHistory=get('domain-history');
  const inventory=tagGroups(data.graph.nodes);
  function closeDomains() {domainPanel.hidden=true;domainToggle.setAttribute('aria-expanded','false');}
  function openDomains() {leaveHelp();domainPanel.hidden=false;domainToggle.setAttribute('aria-expanded','true');}
  // Fullscreen capture was registered first. Esc from search/list must not
  // propagate into search closing or graph deselection while browsing tags.
  on(doc,'keydown',event=>{
    if(event.defaultPrevented || event.key!=='Escape' || domainPanel.hidden)return;
    event.preventDefault();event.stopPropagation?.();closeDomains();domainToggle.focus({preventScroll:true});
  },true);
  let helpScroll=null;
  function leaveHelp() {
    const guide=get('reading-guide');
    const wasOpen=guide.open;
    guide.open=false;
    if(helpScroll!==null){collapsedScroll=helpScroll;if(!panel.hidden)panel.scrollTop=helpScroll;helpScroll=null;}
    return wasOpen;
  }
  on(get('help'),'click',()=>{
    closeDomains();
    if(helpScroll===null)helpScroll=panel.hidden?collapsedScroll:(panel.scrollTop || 0);
    panel.hidden=false;
    workspace.classList.toggle('details-collapsed',false);
    detailToggle.setAttribute('aria-expanded','true');detailToggle.textContent='收起详情';
    get('reading-guide').open=true;
    get('reading-guide-summary').focus({preventScroll:true});
    panel.scrollTop=0; // explicit help action reveals the guide, not stale detail scroll
  });
  // Native details remains open without JS. Collapse help once on successful
  // mount, independently of selection/Back; never replace a selected detail.
  if(get('reading-guide'))get('reading-guide').open=false;
  model.capture=()=>{
    // Help scrolling must not replace the node's saved navigation scroll.
    leaveHelp();
    if(renderedSelection!==model.selected) return;
    model.detail={...model.detail,scroll:(panel.hidden?collapsedScroll:panel.scrollTop) || 0,
      open:[...content.querySelectorAll('details')].filter(d=>d.open).map(d=>d.dataset.section),
      focus:doc.activeElement?.dataset?.focus || null};
    if(!domainPanel.hidden && domainPanel.contains(doc.activeElement))model.detail.domain={
      history:domainHistory.checked,scroll:domainPanel.scrollTop || 0,
      open:[...get('domain-groups').querySelectorAll('details')].filter(d=>d.open).map(d=>d.dataset.tag)};
    else delete model.detail.domain;
    model.searchState.query=get('search').value;
  };
  function add(parent, tag, value, className) {
    const el = doc.createElement(tag);
    if (value !== undefined) el.textContent = String(value);
    if (className) el.className = className;
    parent.appendChild(el);
    return el;
  }
  const focusKey=(...parts)=>JSON.stringify([model.selected,...parts]);
  function button(parent, label, action, key) {
    const b = add(parent,'button',label); b.type='button';b.dataset.focus=key;
    // A pointer/assistive click need not focus a button in every browser. Capture
    // its actual origin before transition(), rather than stale heading focus.
    on(b,'click',()=>{b.focus({preventScroll:true});action();});return b;
  }
  function locate(parent, id, prefix='定位', context='locate', relation=null) {
    const item = model.items.get(id);
    if (!item) {add(parent,'p',id+'（本图无对应语义节点）','exact-id');return;}
    const b=button(parent, prefix+' · '+(recordLabel(item,model.items) || id),()=>{model.reveal(id);render();},focusKey(context,relation,id));
    b.dataset.action=context;b.dataset.target=id;if(relation)b.dataset.relation=relation;
    b.setAttribute('aria-label',b.textContent+' · 精确目标 '+id+(relation?' · 关系 '+relation:''));
    return b;
  }
  function disclosure(parent,key,label) {
    const d=add(parent,'details');d.dataset.section=key;
    const s=add(d,'summary',label);s.dataset.focus=focusKey('summary',key);return d;
  }
  function raw(parent, label, value, context=label) {
    const d = disclosure(parent,focusKey('raw',context),label);
    add(d,'pre',typeof value === 'string' ? value : JSON.stringify(value,null,2));
    return d;
  }
  function renderDomains() {
    const root=get('domain-groups');root.replaceChildren();
    const current=data.graph.nodes.filter(n=>n.current).length;
    get('domain-counts').textContent='当前 '+current+' · 历史 '+(data.graph.nodes.length-current)+' 个精确节点；默认仅列当前修订。';
    for(const category of ['研究领域','其他标签','未标记']) {
      const list=inventory.filter(g=>g.category===category);if(!list.length)continue;
      add(root,'h3',category);
      for(const g of list) {
        const d=add(root,'details');d.dataset.tag=JSON.stringify(g.tag);
        // Raw tag remains available even when a display translation is known.
        const summary=add(d,'summary',g.label+(g.category==='研究领域'?' / '+g.tag:'')+' · 当前 '+g.current+' · 历史 '+g.history);
        summary.dataset.focus=JSON.stringify(['domain-group',g.tag]);
        const ul=add(d,'ul');
        for(const n of g.nodes.filter(n=>domainHistory.checked || n.current)) {
          const b=button(add(ul,'li'),(n.title || n.label || n.id)+' · '+nodeHistory(n).label,
            ()=>{model.reveal(n.id);render();
              (panel.hidden?detailToggle:content.querySelectorAll('h2')[0])?.focus({preventScroll:true});
            },JSON.stringify(['domain',g.tag,n.id]));
          b.dataset.target=n.id;b.setAttribute('aria-label',b.textContent+' · '+n.id);
        }
        if(!ul.children.length)add(d,'p','当前修订 0；可勾选显示历史修订。','meta');
      }
    }
  }
  domainPanel.hidden=true;domainHistory.checked=false;
  on(domainToggle,'click',()=>{if(domainPanel.hidden){renderDomains();openDomains();}else closeDomains();});
  on(domainHistory,'change',()=>{
    const open=new Set([...get('domain-groups').querySelectorAll('details')].filter(d=>d.open).map(d=>d.dataset.tag));
    renderDomains();for(const d of get('domain-groups').querySelectorAll('details'))d.open=open.has(d.dataset.tag);
  });
  function restoreDomains() {
    const saved=model.detail.domain;if(!saved)return;
    domainHistory.checked=saved.history;renderDomains();openDomains();
    for(const d of get('domain-groups').querySelectorAll('details'))d.open=saved.open.includes(d.dataset.tag);
    [...get('domain-groups').querySelectorAll('button,summary')].find(b=>b.dataset.focus===model.detail.focus)?.focus({preventScroll:true});
    domainPanel.scrollTop=saved.scroll;
  }
  function sourceLocators(parent, item) {
    if(!data.sourceLocators)return;
    add(parent,'h3','依据出处');
    const loc=data.sourceLocators,links=item.sourceLocators || [],str=i=>i<0?'':loc.strings[i];
    if(!links.length){add(parent,'p','精确出处不可用；未提供此修订的捕获映射，不从附件推测。');return;}
    for(const [key,artifact] of links) {
      const [p,start,end,section,full,capture,stamp,last,display,body]=loc.chunks[key];
      const identity=focusKey('source-locator',key);
      const card=disclosure(parent,identity,str(p)+' · 第 '+start+'–'+end+' 行');card.className='source-card';
      add(card,'p',str(section));
      add(card,'p','捕获于 '+str(stamp)+' 的来源节选；仅核验归档复制件，未重读当前原文件，不表示科学认可。');
      add(card,'p','节选，完整原文未嵌入');
      if(str(body)) {
        add(card,'p','显示第 '+start+'–'+last+' 行（完整引用范围：第 '+start+'–'+end+' 行）。');
        add(card,'pre',str(body),'excerpt');
      } else add(card,'p','此项仅定位；为控制快照体积，未嵌入显示节选。');
      const info=disclosure(card,focusKey('source-locator-hashes',key),'捕获摘要与附件标识');
      add(info,'p','附件：'+loc.artifacts[artifact]);
      add(info,'p','捕获原文件摘要：'+str(full));
      add(info,'p','捕获完整节选摘要：'+str(capture));
      if(str(body))add(info,'p','显示节选摘要：'+str(display));
    }
  }
  function sourceCards(parent, ids) {
    add(parent,'h3','来源摘录 · 非科研关系');
    if (!ids.length) {add(parent,'p','未记录直接附件引用。','meta');return;}
    for (const id of ids) {
      const source = data.sources[id];
      if (!source) {add(parent,'p','此附件没有已核对摘录，不打开 URI。');raw(parent,'附件标识',id,id);continue;}
      const card=disclosure(parent,'source:'+id,'阅读来源 · '+source.title);card.className='source-card';
      let sourceLabel = '已验证复制摘录；历史文档，不是本次重新测量。';
      if (source.source_type === 'local-file' || (source.verified && source.is_text !== false)) {
        sourceLabel = '本次构建核验的本地内容';
      } else if (source.is_text === false) {
        sourceLabel = '二进制或非UTF-8文件';
      } else if (source.source_type === 'external-uri' || (source.original_path && /^(?:https?:\/\/)/.test(source.original_path))) {
        sourceLabel = '未获取外部正文';
      } else if (source.source_type === 'metadata-only' || (!source.verified && (source.excerpt || '').includes('元数据引用'))) {
        sourceLabel = '未读取正文，仅引用元数据';
      }
      add(card,'p',sourceLabel);
      add(card,'h3',source.is_text === false ? '文件状态' : '原文摘录');add(card,'pre',source.excerpt,'excerpt');
      if (source.sections && source.sections.length) {
        add(card,'h3','原文定位');
        add(card,'p',(source.sections || []).map(s=>s.heading+' · 第 '+s.start_line+'–'+s.end_line+' 行').join('\n'));
      }
      raw(card,'来源技术信息 · 标识 / 路径 / 摘要',{id,path:source.original_path,sha256:source.sha256,size_bytes:source.size_bytes},['source-technical',id]);
    }
  }
  function historyCards(item) {
    const lineage=data.graph.nodes.filter(n=>n.raw?.logical_id && n.raw.logical_id===item.raw?.logical_id);
    if(lineage.length<2)return;
    const d=disclosure(content,'lineage','修订脉络与新旧对比');d.open=true;
    add(d,'p','按父修订连接阅读；时间是记录字段，不是审阅或执行事件。并存 head 不按时间选赢家。','meta');
    // Parent-first order, with a visited set for defensive bounded traversal.
    const ordered=[],seen=new Set(),by=new Map(lineage.map(n=>[n.id,n]));
    function visit(n){if(seen.has(n.id))return;seen.add(n.id);for(const p of n.raw.revision?.parents || [])if(by.has(p.id))visit(by.get(p.id));ordered.push(n);}
    for(const n of lineage)visit(n);
    const timeline=add(d,'ol');
    for(const n of ordered) {
      const row=add(timeline,'li');locate(row,n.id,n.current?'当前修订':'旧修订','lineage');
      add(row,'p',n.raw.revision?.summary || '未记录修订说明');
      add(row,'p','记录时间：'+(n.raw.created_at || '未记录')+'；'+((n.raw.revision?.parents || []).length?'父修订：'+n.raw.revision.parents.map(p=>by.get(p.id)?.label || '谱系外引用').join(' / '):'未记录父修订'),'meta');
    }
    const previous=ordered.find(n=>(item.raw.revision?.parents || []).some(p=>p.id===n.id)) || ordered[0];
    const next=item.id!==previous.id?item:ordered.at(-1);
    const pick=(label,id)=>{
      const wrapper=add(d,'label',label), select=add(wrapper,'select');select.setAttribute('aria-label',label);select.dataset.focus=focusKey('compare',label==='旧版'?'before':'after');
      for(const n of ordered){const o=add(select,'option',(n.current?'当前 · ':'旧版 · ')+n.label);o.value=n.id;}
      select.value=id;return select;
    };
    const old=pick('旧版',model.detail.compare?.[0] || previous.id), newer=pick('新版',model.detail.compare?.[1] || next.id);
    const comparison=add(d,'div',undefined,'comparison');
    function compare(){
      model.detail.compare=[old.value,newer.value];comparison.replaceChildren();
      const a=by.get(old.value),b=by.get(newer.value);if(!a||!b)return;
      const diff=fieldDiff(a.raw,b.raw);
      add(comparison,'p','字段对比（仅原始内容，不推断变更事件）','meta');
      if(!diff.rows.length)add(comparison,'p','这些内容字段没有变化；完整原始字段仍保留。');
      for(const r of diff.rows){
        const row=add(comparison,'section');add(row,'h3',r.label);
        const columns=add(row,'div',undefined,'diff-columns');
        const left=add(columns,'div');add(left,'h4','旧版');add(left,'p',r.before,'diff-text');
        const right=add(columns,'div');add(right,'h4','新版');add(right,'p',r.after,'diff-text');
      }
      if(diff.truncated) {
        add(comparison,'p','对比已限长（最多 40 字段 / 每侧 4000 字符）；其余见对比双方完整原始字段。');
        raw(comparison,'对比双方完整原始字段',{before:a.raw,after:b.raw},['compare-raw',a.id,b.id]);
      }
    }
    on(old,'change',compare);on(newer,'change',compare);compare();
  }
  function details(force=false) {
    const view=model.view();
    const signature=JSON.stringify([model.history,view.nodes.map(n=>n.id),[...view.edges,...view.revisionEdges].map(e=>[e.id,e.labelStatus])]);
    if(!force && renderedSelection===model.selected && renderedDetailView===signature)return;
    const sameSelection=renderedSelection===model.selected;
    if(!force && sameSelection)model.capture();
    const restoring=force || sameSelection, initialRender=renderedSelection===undefined;
    renderedSelection=model.selected;renderedDetailView=signature;detailButtons=[];
    content.replaceChildren();
    add(content,'p','DETAILS','eyebrow');
    const item = model.items.get(model.selected);
    if (!item) {
      const emptyHeading=add(content,'h2','从一个细节开始');
      emptyHeading.setAttribute('tabindex','-1');
      if(!initialRender){emptyHeading.focus({preventScroll:true});panel.scrollTop=0;}
      add(content,'p','搜索或选择一个细节，查看其内容、来源、去向与历史');
      add(content,'p','未记录 Assessment，不编造可信度；时效未判定不等于可靠或不可靠。','meta');
      if(panel.hidden)collapsedScroll=0;
      return;
    }
    const heading=add(content,'h2',item.label);heading.setAttribute('tabindex','-1');heading.dataset.focus=focusKey('heading');
    const r = item.raw || {};
    add(content,'p',item.from ? (item.category==='revision'?'这条线连接旧修订与新修订，不表示科研推导。':'这条关系记录“'+(model.items.get(item.from)?.label || '起点')+'” → “'+(model.items.get(item.to)?.label || '终点')+'”的'+item.label+'关系。') : item.label+'：'+item.typeLabel+'，'+nodeHistory(item).label+'。','lead-summary');
    const actions=add(content,'div',undefined,'detail-actions');
    const backButton=button(actions,'返回',goBack,focusKey('back'));detailButtons.push(backButton);backButton.disabled=!model.stack.length;
    if(!item.from){button(actions,'只看相关',()=>{model.focus();render();},focusKey('local'));button(actions,'展开入向',()=>{model.expand('in');render();},focusKey('expand','in'));button(actions,'展开出向',()=>{model.expand('out');render();},focusKey('expand','out'));}
    for(const id of new Set(r.source?.revisions || [])) {
      const sourceLabel=data.graph.readingSources?.[item.id]?.[id];
      if(sourceLabel)locate(content,id,sourceLabel,'reading-source');
    }
    add(content,'p','展开按箭头原方向逐跳进行；沿入向关系 / 沿出向关系不是证据或因果判断，依赖箭头指向所依赖对象。','meta');
    if (item.from) {
      const geometry=[...view.edges,...view.revisionEdges].find(e=>e.id===item.id);
      if(geometry?.labelStatus==='hidden')add(content,'p','标签暂隐：当前视图没有安全文字位置；关系类型、方向与原文仍在此处，选择不会叠加遮挡其他标签。','meta');
      add(content,'p',item.category === 'revision' ? '修订层 · 旧 → 新，不是科研推导' : '科研关系 · '+item.type+(item.current ? ' · 当前 active head' : ' · 历史关系'));
      add(content,'h3','关系方向');
      raw(content,'精确标识与端点',item.from+' → '+item.to);
      locate(content,item.from,'起点','endpoint-from',item.id); locate(content,item.to,'终点','endpoint-to',item.id);
      add(content,'h3',item.category === 'revision' ? '修订说明' : '关系解释 · rationale');
      add(content,'p',r.rationale || r.summary || '未记录解释');
      if (r.scope) {add(content,'h3','适用范围');add(content,'p',readable(r.scope),'diff-text');}
      for (const id of new Set(r.source?.revisions || [])) locate(content,id,'来源修订','source');
      sourceCards(content,r.source?.artifacts || []);
      if (item.category !== 'revision' && !(r.source?.artifacts || []).length)
        add(content,'p','本关系未直接引用附件；可从来源修订进入节点详情，再展开其附件。','meta');
    } else {
      add(content,'p',item.typeLabel+' · '+nodeHistory(item).label,'status-line');
      if(item.presentationStatus) {
        const s=item.presentationStatus;
        add(content,'h3','展示判断 · '+s.label);
        add(content,'p',s.reason,'presentation-reason');
        add(content,'p','材料核对时间 assessed_at：'+s.assessed_at+'；不是科学事件或 freshness 时效。','meta');
        for(const ref of s.source_refs)add(content,'p','来源：'+ref.id+' · '+ref.locator+' · '+ref.canonical_digest,'presentation-source');
        add(content,'p','私有展示映射；不改变原记录，不是 Assessment、科学结果无效判定或人的验收。','meta');
      }
      const roles = item.roles.map(role=>data.graph.roles[role] || role).join(' / ') || '未绑定角色';
      add(content,'p','线程角色：'+roles+(item.roleConflict ? ' · 并存角色冲突，未选赢家' : '')+(item.fork ? ' · 多 head 分叉全部保留' : ''));
      function freshnessLabel(f) {
        return {fresh: '时效正常', 'review-due': '待复核', stale: '已过保', unknown: '时效未判定'}[f] || '时效未判定';
      }
      const asmCount = (item.assessments && item.assessments.length) || 0;
      if (asmCount > 0) {
        add(content, 'p', '评价记录 (' + asmCount + ') · 评价展示暂不支持 · 时效：' + freshnessLabel(item.freshness), 'meta');
      } else {
        add(content, 'p', '未记录评价 · 时效：' + freshnessLabel(item.freshness), 'meta');
      }
      if(item.label!==r.title)add(content,'p','展示标题 / 稳定别名仅用于阅读定位，不是冻结记录的新修订；原始标题如下。','meta');
      add(content,'h3','原始标题'); add(content,'p',r.title);
      add(content,'h3','原始陈述'); add(content,'p',r.statement || '未记录陈述');
      for(const key of ['scope','assumptions']) {
        const value=r[key];
        if(value==null || typeof value==='string' && !value.trim() ||
          typeof value==='object' && !Object.keys(value).length)continue;
        const text=readable(value);
        add(content,'h3',FIELD_LABELS[key]);
        add(content,'p',text.slice(0,4000)+(text.length>4000?'…（其余见完整原始字段）':''),'diff-text');
      }
      if (item.isolated) add(content,'p','本快照未记录连接；没有为了布局补造连线。','status-line');
      const v = model.view();
      const incident = data.graph.edges.filter(e=>(model.history || e.current) && (e.from===item.id || e.to===item.id));
      const visibleEdges = new Set(v.edges.map(e=>e.id));
      const hidden = incident.filter(e=>!visibleEdges.has(e.id)).length;
      add(content,'h3','已记录连接（'+incident.length+'）');
      const excluded=data.graph.edges.filter(e=>!e.current && (e.from===item.id || e.to===item.id)).length;
      add(content,'p',model.history?'当前与历史科研关系均列出。':'仅当前科研关系；另有 '+excluded+' 条历史关系，可打开历史层阅读。','meta');
      if (hidden) add(content,'p',hidden+' 条相邻关系被当前筛选隐藏；定位关系将自动展开所需层。','status-line');
      for (const e of incident) {
        const outgoing=e.from===item.id;
        const other=outgoing?e.to:e.from,row=add(content,'section');
        add(row,'p',relationMeaning(e.type,outgoing)+' · '+(model.items.get(other)?.label || other)+' · '+recordState(e));
        locate(row,other,'前往节点','node',e.id);
        locate(row,e.id,'查看关系','edge',e.id);
      }
      for (const id of new Set(r.source?.revisions || [])) {
        if(!data.graph.readingSources?.[item.id]?.[id])locate(content,id,
          r.next_action ? '编写依据，不等于行动目标 · 非科研关系' : '来源引用 · SOURCE · 非科研关系','source');
      }
      sourceLocators(content,item);
      sourceCards(content,r.source?.artifacts || []);
      if(r.limitations?.length){add(content,'h3','限制与未决项');add(content,'p',readable(r.limitations),'diff-text');}
      for(const key of ['interpretation','method','conclusion','observation','blocker','question']) {
        if(r[key]){add(content,'h3','类型内容 · '+key);add(content,'p',readable(r[key]),'diff-text');}
      }
      if (data.notes[item.id]) raw(content,'阅读已验证叙述附件与来源定位',data.notes[item.id]);
      if(r.next_action){add(content,'h3','行动内容与完成标准');add(content,'p',readable(r.next_action),'diff-text');}
      historyCards(item);
      if (item.bindingIds.length) raw(content,'当前绑定 head 原文（角色不由布局推断）',item.bindingIds.map(id=>data.records[id]));
    }
    add(content,'h3','被这些记录引用 · SOURCE · 非科研关系');
    const uses=sourceIndex.get(item.id) || {refs:[],total:0};
    add(content,'p',uses.total+' 条来源引用（含历史；不受历史开关或角色筛选影响）。只统计本快照 source.revisions，不是科研连接或执行证据。','meta');
    if(!uses.total)add(content,'p','本快照未记录反向 SOURCE 引用。','meta');
    for(const ref of uses.refs) {
      const row=add(content,'section');add(row,'p',ref.label+' · '+ref.type+' · '+ref.state);
      if(ref.locatable)locate(row,ref.id,'前往引用记录','reverse-source');
      else raw(row,'类型化引用原文 · 不在画布，不能定位',ref.raw,['reverse-source',ref.id]);
    }
    if(uses.total>uses.refs.length) {
      add(content,'p','列表已截断：显示 '+uses.refs.length+' / '+uses.total+' 条；其余 '+(uses.total-uses.refs.length)+' 条见全部反向 SOURCE 原文。','meta');
      const all=new Map(Object.entries(data.records));
      for(const n of [...data.graph.nodes,...data.graph.edges])all.set(n.id,n.raw);
      raw(content,'全部反向 SOURCE 原文 · 纯文本', [...all.values()].filter(r=>r.source?.revisions?.includes(item.id)),'reverse-source-overflow');
    }
    raw(content,'完整原始字段 · 纯文本（含精确 ID）',r);
    if(restoring){
      const opened=new Set(model.detail.open || []);
      for(const d of content.querySelectorAll('details'))d.open=opened.has(d.dataset.section);
      const target=[heading,...content.querySelectorAll('button'),...content.querySelectorAll('summary'),...content.querySelectorAll('select'),...elements].find(el=>el.dataset.focus===model.detail.focus) || heading;
      target.focus({preventScroll:true});panel.scrollTop=model.detail.scroll || 0;
    } else {heading.focus({preventScroll:true});panel.scrollTop=0;}
    if(panel.hidden)collapsedScroll=restoring?(model.detail.scroll || 0):0;
  }
  function drawViewport() {
    const b = model.box;
    svg.setAttribute('viewBox',[b.x,b.y,b.w,b.h].join(' '));
    get('zoom-level').textContent = Math.round(data.graph.width/b.w*100)+'%';
  }
  function goBack(){if(model.back()){render(true);renderSearch();restoreDomains();}}
  function render(restoring=false) {
    closeDomains();
    const returnedFromHelp=leaveHelp();
    if(returnedFromHelp && renderedSelection===model.selected){
      content.querySelectorAll('h2')[0]?.focus({preventScroll:true});
    }
    const v = model.view(), neighborhood = model.neighborhood();
    const visibleIds = new Set([...v.nodes,...v.edges,...v.revisionEdges].map(n=>n.id));
    const routes=new Map([...v.edges,...v.revisionEdges].map(e=>[e.id,e]));
    for (const el of elements) {
      const id = el.dataset.key, show = visibleIds.has(id);
      const near = neighborhood.nodes.has(id) || neighborhood.edges.has(id);
      const route=routes.get(id);
      if(route) {
        el.setAttribute('data-route-status',route.routeStatus);
        el.setAttribute('data-label-status',route.labelStatus);
        const visibility=route.labelStatus==='placed'?'visible':'hidden';
        const note=route.labelStatus==='hidden'?' · 标签暂隐，请查看详情':'';
        const labelText=edgeRouting.text(route);
        el.setAttribute('aria-label',labelText+': '+route.from+' → '+route.to+note);
        for(const title of el.querySelectorAll('title'))title.textContent=labelText+' · '+route.id+note;
        for(const path of el.querySelectorAll('path')) {
          const leader=(path.getAttribute('class')||'').includes('edge-label-leader');
          path.setAttribute('d',leader?route.labelLeaderPath:route.path);
          if(leader)path.setAttribute('visibility',visibility);
        }
        for(const bg of el.querySelectorAll('rect')) {
          bg.setAttribute('x',route.labelX-route.labelHalfWidth);bg.setAttribute('y',route.labelY-20);
          bg.setAttribute('width',2*route.labelHalfWidth);bg.setAttribute('visibility',visibility);
        }
        for(const label of el.querySelectorAll('text')) {
          label.textContent=labelText;label.setAttribute('x',route.labelX);label.setAttribute('y',route.labelY);
          label.setAttribute('visibility',visibility);
        }
      }
      el.classList.toggle('is-hidden',!show);
      el.classList.toggle('selected',id===model.selected);
      el.classList.toggle('neighbor',near && id!==model.selected);
      el.classList.toggle('contextual',show && !(model.items.get(id)?.from?v.focusedEdgeIds.has(id):v.focused.has(id)));
      el.classList.toggle('dim',Boolean(model.selected) && !near && id!==model.selected);
      el.setAttribute('tabindex',show ? '0' : '-1');
      el.setAttribute('aria-hidden',show ? 'false' : 'true');
      el.setAttribute('aria-pressed',id===model.selected ? 'true' : 'false');
    }
    get('filter').value = model.mode; get('history').checked = model.history;
    get('counts').textContent = `节点：聚焦 ${v.focusedNodes} · 上下文 ${v.contextNodes} · 隐藏 ${v.hiddenNodes} / 共 ${data.graph.nodes.length}；科研关系：聚焦 ${v.focusedEdges} · 上下文 ${v.contextEdges} · 隐藏 ${v.hiddenEdges}；修订线 ${v.revisionEdges.length}（隐藏 ${v.hiddenRevisions}）`;
    get('back').disabled=!life.active || !model.stack.length;
    for(const b of detailButtons)b.disabled=!life.active || !model.stack.length;
    if(restoring)get('search').value=model.searchState.query;
    const missing=[...routes.values()].filter(e=>e.routeStatus==='unroutable').length;
    const hiddenLabels=[...routes.values()].filter(e=>e.routeStatus==='routed'&&e.labelStatus==='hidden').length;
    get('notice').textContent = (model.notice || '选择节点或箭头查看依据；拖动画布移动视野，不编辑节点。')+
      (missing ? ` ${missing} 条关系在有界候选中未找到安全路线，暂不画线；仍可搜索或从节点详情定位原始关系。` : '')+
      (hiddenLabels ? ` ${hiddenLabels} 条可画线关系的标签暂隐（不是隐藏关系）；线与箭头保留，可悬停、聚焦、搜索或在详情查阅类型与方向。` : '');
    drawViewport(); details(restoring);
  }
  for (const el of elements) wireSelection(el,model,render,()=>{const skip=suppressClick;suppressClick=false;return skip;},on);
  on(get('back'),'click',goBack);
  on(get('start'),'click',()=>{
    const choices=get('start-choices'),ids=data.graph.startIds || [];
    choices.replaceChildren();
    if(ids.length===1){model.start(ids[0]);render();return;}
    add(choices,'p',ids.length ? '此线程有多个当前问题；请选择建议入口，不自动指定根。' : '此线程未记录当前 Question 入口；请搜索或选择节点。','meta');
    for(const id of ids)button(choices,model.items.get(id).label,()=>{
      model.start(id);choices.replaceChildren();render();
    },JSON.stringify(['start',id]));
  });
  on(get('restore-all'),'click',()=>{model.restoreAll();render();});
  on(get('filter'),'change',()=>{model.setFilter(get('filter').value);render();});
  on(get('history'),'change',()=>{model.setHistory(get('history').checked);render();});
  on(get('zoom-in'),'click',()=>{model.zoom(1.1);drawViewport();});
  on(get('zoom-out'),'click',()=>{model.zoom(1/1.1);drawViewport();});
  on(get('fit'),'click',()=>{model.transition(()=>model.fit());render();});
  on(get('reset'),'click',()=>{
    model.reset(); get('search').value='';get('search-results').replaceChildren();render();
  });
  on(get('show-all'),'click',()=>{
    model.showAll();render();
  });
  let resultButtons=[];
  function renderSearch(){
    const search=get('search'),results=get('search-results');results.replaceChildren();resultButtons=[];
    search.setAttribute('aria-expanded',model.searchState.closed?'false':'true');
    search.setAttribute('aria-activedescendant','');
    if(model.searchState.closed)return;
    const matches=model.search(search.value),v=model.view(),ids=new Set([...v.nodes,...v.edges,...v.revisionEdges].map(n=>n.id));
    if(search.value.trim())add(results,'p',`${matches.length} 个匹配 · ↑ ↓ 选择，Enter 定位，Esc 收起`,'meta');
    matches.forEach((item,i)=>{
      const b=button(results,(item.label || item.id)+' · '+recordState(item)+
        (item.raw?.logical_id?' · '+item.raw.logical_id:'')+' · '+item.id+
        (ids.has(item.id)?' · 定位':' · 隐藏，展开并定位'),()=>{
        model.reveal(item.id);model.searchState.closed=true;render();renderSearch();
      },JSON.stringify(['search',item.id]));
      b.id='search-result-'+i;b.setAttribute('id',b.id);b.setAttribute('aria-pressed',String(i===model.searchState.index));
      resultButtons.push(b);
    });
    if(resultButtons[model.searchState.index])search.setAttribute('aria-activedescendant',resultButtons[model.searchState.index].id);
  }
  const search=get('search');
  on(search,'input',()=>{model.searchState={query:search.value,index:-1,closed:false};renderSearch();});
  on(search,'keydown',event=>{
    if(event.defaultPrevented || escapeCanvas(event))return;
    if(event.key==='Escape'){event.preventDefault();event.stopPropagation?.();model.searchState.closed=true;renderSearch();return;}
    if(['ArrowDown','ArrowUp'].includes(event.key)){
      event.preventDefault();model.searchState.closed=false;
      const count=model.search(search.value).length;
      if(count)model.searchState.index=model.searchState.index<0 ? (event.key==='ArrowDown'?0:count-1) : (model.searchState.index+(event.key==='ArrowDown'?1:-1)+count)%count;
      renderSearch();
    } else if(event.key==='Enter'){
      event.preventDefault();const matches=model.search(search.value);
      const exact=matches[0]?.id.toLocaleLowerCase()===search.value.trim().toLocaleLowerCase();
      const item=matches[model.searchState.index>=0?model.searchState.index:matches.length===1 || exact?0:-1];
      if(item){model.reveal(item.id);model.searchState.closed=true;render();renderSearch();}
    }
  });
  on(doc,'keydown',event=>{
    if(event.defaultPrevented || escapeCanvas(event) || event.key!=='Escape')return;
    if(event.target===search || get('search-results').contains(event.target)){
      model.searchState.closed=true;renderSearch();search.focus();return;
    }
    model.clear();render();
  });
  on(svg,'wheel',event=>{
    event.preventDefault();
    const rect=svg.getBoundingClientRect();
    const factor=wheelFactor(event.deltaY,event.deltaMode,rect.height);
    if(factor===1 || !Number.isFinite(event.clientX) || !Number.isFinite(event.clientY) || rect.width<=0 || rect.height<=0)return;
    const p=viewport(rect,model.box,event.clientX,event.clientY);
    model.zoom(factor,p);drawViewport();
  },{passive:false});
  on(svg,'pointerdown',event=>{
    if (event.button !== 0 || !event.isPrimary) return;
    suppressClick=false;
    drag={id:event.pointerId,x:event.clientX,y:event.clientY,lastX:event.clientX,lastY:event.clientY,moved:false};
    // Delay capture until actual drag; immediate capture would retarget a normal
    // node click to the SVG and prevent selection in real browsers.
  });
  on(svg,'pointermove',event=>{
    if (!drag || event.pointerId !== drag.id) return;
    if (!drag.moved && Math.hypot(event.clientX-drag.x,event.clientY-drag.y) < 5) return;
    drag.moved=true; suppressClick=true;
    svg.setPointerCapture(event.pointerId);
    const p=viewport(svg.getBoundingClientRect(),model.box,event.clientX,event.clientY);
    model.pan((drag.lastX-event.clientX)/p.scale,(drag.lastY-event.clientY)/p.scale);
    drag.lastX=event.clientX;drag.lastY=event.clientY;drawViewport();
  });
  function endDrag(event) {
    if (!drag || event.pointerId !== drag.id) return;
    suppressClick=event.type==='pointercancel'?false:drag.moved;
    if (svg.hasPointerCapture(event.pointerId)) svg.releasePointerCapture(event.pointerId);
    drag=null;
  }
  on(svg,'pointerup',endDrag);on(svg,'pointercancel',event=>{endDrag(event);suppressClick=false;});
  // A captured drag's click can be retargeted to the SVG rather than a node.
  // Consume suppression there too, so the next assistive click remains usable.
  on(svg,'click',()=>{suppressClick=false;});
  // A keyboard selection must not inherit a preceding mouse drag suppression.
  on(svg,'keydown',()=>{suppressClick=false;});
  render();
  Object.assign(api,{model,render});
  return api;
}

if (typeof module !== 'undefined' && module.exports) module.exports={Model,tagGroups,fieldDiff,relationMeaning,reverseSources,wireSelection,viewport,wheelFactor,mount};
