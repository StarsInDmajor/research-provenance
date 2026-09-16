'use strict';
/* Compact delivery: construct the diagram only after projection validation.
 * No markup parser or data-derived tag/attribute names. Python still owns layout.
 * Keep geometry/text equivalent to the historical static render_svg oracle. */
const SVG_NS='http://www.w3.org/2000/svg';
const SVG_STYLES={
  supports:['#287344',null], weakens:['#b23838','3 3'],
  contradicts:['#b23838','10 3 2 3'], 'derived-from':['#246ca6','8 4'],
  'depends-on':['#956400','12 3'], 'blocked-by':['#b23838','7 4'],
  motivates:['#147d79','2 4'], revision:['#80529b','9 4 2 4'],
  neutral:['#66717b',null]
};
function svgWrap(value,size,lines) {
  const chars=Array.from(String(value).replace(/[\n\r]/g,' ')),result=[];
  const advance=c=>size*(c.codePointAt(0)<128?.8:1.1);
  let cursor=0;
  for(let index=0;index<lines;index++) {
    const row=[];let used=0;
    while(cursor<chars.length && used+advance(chars[cursor])<=238) {
      const c=chars[cursor++];row.push(c);used+=advance(c);
    }
    if(cursor<chars.length && index===lines-1) {
      while(row.length && used+advance('…')>238)used-=advance(row.pop());
      row.push('…');
    }
    result.push(row.join(''));if(cursor===chars.length)break;
  }
  return result;
}
function buildSVG(doc,g) {
  const svg=doc.getElementById('graph');
  if(!svg || svg.namespaceURI!==SVG_NS || svg.localName!=='svg' || svg.children.length)throw Error('Invalid SVG shell');
  function add(parent,tag,attrs={},text) {
    const el=doc.createElementNS(SVG_NS,tag);
    for(const [key,value] of Object.entries(attrs))el.setAttribute(key,value);
    if(text!==undefined)el.textContent=text;
    parent.appendChild(el);return el;
  }
  svg.setAttribute('viewBox',`0 0 ${g.width} ${g.height}`);
  svg.setAttribute('preserveAspectRatio','xMidYMid meet');
  svg.setAttribute('aria-label','研究节点图，可用 Tab 与 Enter 选中记录');
  add(svg,'title',{},'精确修订节点与已记录关系');
  const defs=add(svg,'defs');
  for(const [key,[color]] of Object.entries(SVG_STYLES)) {
    const marker=add(defs,'marker',{id:'arrow-'+key,viewBox:'0 0 10 10',refX:9,refY:5,markerWidth:7,markerHeight:7,orient:'auto'});
    add(marker,'path',{d:'M 0 0 L 10 5 L 0 10 z',fill:color});
  }
  g.nodes.forEach((n,i)=>{
    const clip=add(defs,'clipPath',{id:'node-clip-'+i,clipPathUnits:'userSpaceOnUse'});
    add(clip,'rect',{x:12,y:10,width:246,height:98});
  });
  for(const e of [...g.edges,...g.revisionEdges]) {
    const revision=e.category==='revision',hidden=revision || !e.current;
    let key=revision?'revision':e.type;
    if(!Object.hasOwn(SVG_STYLES,key) || !revision && key==='revision')key='neutral';
    const [color,dash]=SVG_STYLES[key],invalidated=!revision && e.raw.relation_state==='invalidated';
    const label=e.label+(invalidated?' · 已撤销（历史）':!revision && !e.current?' · 历史':'');
    const status=e.routeStatus==='unroutable'?'hidden':e.labelStatus||'hidden';
    const visibility=status==='placed'?'visible':'hidden',note=status==='hidden'?' · 标签暂隐，请查看详情':'';
    const group=add(svg,'g',{class:`edge ${revision?'revision':'science'} type-${key}${hidden?' is-hidden':''}${invalidated?' invalidated':''}`,
      'data-key':e.id,'data-from':e.from,'data-to':e.to,tabindex:hidden?-1:0,role:'button',
      'aria-label':`${label}: ${e.from} → ${e.to}${note}`,'data-route-status':e.routeStatus||'routed','data-label-status':status});
    add(group,'title',{},label+' · '+e.id+note);
    add(group,'path',{class:'edge-hit',d:e.path});
    add(group,'path',{class:'edge-line',d:e.path,stroke:color,...(dash?{'stroke-dasharray':dash}:{}),'marker-end':`url(#arrow-${key})`});
    add(group,'path',{class:'edge-label-leader',d:e.labelLeaderPath||'',stroke:color,visibility});
    add(group,'rect',{class:'edge-label-bg',x:e.labelX-(e.labelHalfWidth||0),y:e.labelY-20,width:2*(e.labelHalfWidth||0),height:28,rx:3,fill:'#fffdf5',stroke:color,visibility});
    add(group,'text',{x:e.labelX,y:e.labelY,class:'edge-label',visibility},label);
  }
  g.nodes.forEach((n,i)=>{
    const hidden=!n.current && !n.ghost;
    const role=n.roles.map(r=>Object.hasOwn(g.roles,r)?g.roles[r]:r).join(' / ')||'未绑定角色';
    let flags=(n.fork?' · 分叉':'')+(n.roleConflict?' · 角色冲突':'');
    const history=nodeHistory(n);
    flags+=' · '+history.label;
    const full=n.label+' · 原始标题：'+n.title+' · '+role+flags+' · '+n.id;
    const group=add(svg,'g',{class:`node ${n.kind}${hidden?' is-hidden':''}${n.ghost?' ghost':''}${history.dashed?' history-mark':''}`,'data-key':n.id,
      transform:`translate(${n.x} ${n.y})`,tabindex:hidden?-1:0,role:'button','aria-label':n.typeLabel+'：'+full});
    add(group,'title',{},full);
    add(group,'rect',{class:'node-card',width:270,height:114,rx:10});
    const clip=add(group,'g',{'clip-path':`url(#node-clip-${i})`});
    const freshness={unknown:'时效未判定',fresh:'时效正常','review-due':'待复核',stale:'已过保'};
    const fresh=Object.hasOwn(freshness,n.freshness)?freshness[n.freshness]:freshness.unknown;
    const status=n.assessments?.length?`评价记录 (${n.assessments.length})${n.isolated?' · 未记录连接':''} · ${fresh}`:
      n.isolated?'未记录连接':'未评价 · '+fresh;
    for(const [cls,value,size,y,count] of [['node-type',n.typeLabel+(n.presentationStatus?' · '+n.presentationStatus.label:''),12,26,1],['node-label',n.label,15,50,2],
      ['node-role',role+flags,10,88,1],['node-status',status,9,104,1]]) {
      svgWrap(value,size,count).forEach((line,j)=>add(clip,'text',{class:cls,x:16,y:y+j*20},line));
    }
  });
}
if(typeof module!=='undefined' && module.exports)module.exports={buildSVG,svgWrap};
