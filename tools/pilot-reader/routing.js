'use strict';
/* Pure bounded geometry: stable greedy corridor reservations over a finite local
 * candidate family, NOT a universal/optimal router. No graph mutation or IO. */
const Routing = (()=>{
  const rect=n=>({id:n.id,l:n.x-12,r:n.x+282,t:n.y-12,b:n.y+126});
  const length=ps=>ps.slice(1).reduce((sum,p,i)=>sum+Math.hypot(p[0]-ps[i][0],p[1]-ps[i][1]),0);
  function intersects(a,b,r) {
    if(Math.max(a[0],b[0])<=r.l || Math.min(a[0],b[0])>=r.r || Math.max(a[1],b[1])<=r.t || Math.min(a[1],b[1])>=r.b)return false;
    let lo=0,hi=1;
    for(const [p,q,min,max] of [[a[0],b[0],r.l,r.r],[a[1],b[1],r.t,r.b]]) {
      if(p===q){if(p<=min||p>=max)return false;continue;}
      const x=(min-p)/(q-p),y=(max-p)/(q-p);lo=Math.max(lo,Math.min(x,y));hi=Math.min(hi,Math.max(x,y));
    }
    return lo<hi-1e-9 && hi>1e-9 && lo<1-1e-9;
  }
  const path=ps=>ps.map((p,i)=>(i?'L ':'M ')+p.map(v=>v.toFixed(2)).join(' ')).join(' ');
  const points=d=>{const ns=d.match(/-?\d+(?:\.\d+)?/g).map(Number);return ns.reduce((ps,n,i)=>i%2?ps:[...ps,[n,ns[i+1]]],[]);};
  const overlap=(a,b)=>a.l<b.r&&a.r>b.l&&a.t<b.b&&a.b>b.t;
  const text=e=>(e.label||'')+(e.category==='science'&&!e.current?(e.raw?.relation_state==='invalidated'?' · 已撤销（历史）':' · 历史'):'');
  // Conservative 12px text estimate, including wide Latin, Unicode and suffix.
  const halfWidth=e=>Math.max(24,Array.from(text(e)).reduce((n,c)=>n+(c.codePointAt(0)<128?(/[MWmw@%&]/.test(c)?12:8):14),0)/2+6);
  function allocate(result,edges,rects) {
    const occupied=[],leaders=[];
    const lines=Object.values(result).flatMap(r=>{const ps=points(r.path);return ps.slice(1).map((b,i)=>[ps[i],b]);});
    for(const e of edges.slice().sort((a,b)=>a.id<b.id?-1:a.id>b.id?1:0)) {
      const r=result[e.id],half=halfWidth(e);
      Object.assign(r,{labelX:0,labelY:0,labelHalfWidth:half,labelStatus:'hidden',labelLeaderPath:''});
      if(r.routeStatus!=='routed'||half>180)continue;
      const ps=points(r.path),segments=ps.slice(1).map((b,i)=>({a:ps[i],b,i,len:+length([ps[i],b]).toFixed(6)})).sort((a,b)=>b.len-a.len||a.i-b.i);
      // <=5 segments * 5 anchors * 10 nearby positions =250 candidates/edge.
      // Leader ends are at most36 units from their own segment. No global float.
      candidates: for(const {a,b} of segments)for(const t of [.5,.25,.75,.125,.875]) {
        const ax=Math.floor((a[0]+(b[0]-a[0])*t)*100+.5)/100,ay=Math.floor((a[1]+(b[1]-a[1])*t)*100+.5)/100;
        for(const [x,y,lx,ly] of [[ax,ay-12,ax,ay-4],[ax,ay+32,ax,ay+12],[ax-half-6,ay+6,ax-6,ay],[ax+half+6,ay+6,ax+6,ay],[ax,ay-44,ax,ay-36],[ax,ay+56,ax,ay+36],[ax-half,ay-12,ax,ay-4],[ax+half,ay-12,ax,ay-4],[ax-half,ay+32,ax,ay+12],[ax+half,ay+32,ax,ay+12]]) {
          const box={l:x-half,r:x+half,t:y-20,b:y+8},leader=[[ax,ay],[lx,ly]];
          if(rects.some(q=>overlap(box,q)||intersects(...leader,q))||occupied.some(q=>overlap(box,q)||intersects(...leader,q)))continue;
          // Background may not conceal another relation (or a prior leader).
          const padded={l:box.l-2,r:box.r+2,t:box.t-2,b:box.b+2};
          if(lines.some(s=>intersects(...s,padded))||leaders.some(s=>intersects(...s,padded)))continue;
          Object.assign(r,{labelX:+x.toFixed(2),labelY:+y.toFixed(2),labelStatus:'placed',labelLeaderPath:path(leader)});
          occupied.push(box);leaders.push(leader);break candidates;
        }
      }
    }
  }
  // Same bounded greedy soft cost as Python; never trades node clearance.
  function congestion(ps,reserved) {
    let cost=0;
    for(let i=1;i<ps.length;i++) {
      const a=ps[i-1],b=ps[i],ux=b[0]-a[0],uy=b[1]-a[1],size=Math.hypot(ux,uy);
      if(size<1e-6)continue;
      for(const [c,d] of reserved) {
        const vx=d[0]-c[0],vy=d[1]-c[1],wx=c[0]-a[0],wy=c[1]-a[1],den=ux*vy-uy*vx;
        if(Math.abs(den)<1e-6) {
          if(Math.abs(wx*uy-wy*ux)>1e-6)continue;
          const [low,high]=(Math.abs(ux)>=Math.abs(uy)?[(c[0]-a[0])/ux,(d[0]-a[0])/ux]:[(c[1]-a[1])/uy,(d[1]-a[1])/uy]).sort((a,b)=>a-b);
          cost+=2*Math.max(0,Math.min(1,high)-Math.max(0,low))*size;
        } else {
          const t=(wx*vy-wy*vx)/den,s=(wx*uy-wy*ux)/den;
          if(t>1e-6&&t<1-1e-6&&s>1e-6&&s<1-1e-6)cost+=70;
        }
      }
    }
    return cost;
  }
  function one(a,b,e,index,count,rects,reserved=[]) {
    const ax=a.x+135,ay=a.y+57,bx=b.x+135,by=b.y+57,dx=bx-ax,dy=by-ay,d=Math.hypot(dx,dy);
    let start,end,sa,sb;
    if(!d){start=[ax-65,a.y];end=[ax+65,a.y];sa=[start[0],a.y-16];sb=[end[0],a.y-16];}
    else {
      const ux=dx/d,uy=dy/d,r=Math.min(ux?135/Math.abs(ux):1e9,uy?57/Math.abs(uy):1e9);
      start=[ax+ux*r,ay+uy*r];end=[bx-ux*(r+5),by-uy*(r+5)];
      const port=(index-(count-1)/2)*Math.min(8,48/Math.max(1,count-1));
      if(count>1&&Math.abs(ux)*57>=Math.abs(uy)*135) {
        start[1]=Math.max(a.y+12,Math.min(a.y+102,start[1]+port));end[1]=Math.max(b.y+12,Math.min(b.y+102,end[1]+port));
      } else if(count>1) {
        start[0]=Math.max(a.x+12,Math.min(a.x+258,start[0]+port));end[0]=Math.max(b.x+12,Math.min(b.x+258,end[0]+port));
      }
      sa=[start[0]+ux*16,start[1]+uy*16];sb=[end[0]-ux*16,end[1]-uy*16];
    }
    const lane=(index-(count-1)/2)*12;
    const sign=e.from<e.to?1:-1,offset=d?[-dy/d*lane*sign,dx/d*lane*sign]:[0,0];
    const candidates=[];
    const add=ps=>{const clean=ps.filter((p,i)=>!i||p[0]!==ps[i-1][0]||p[1]!==ps[i-1][1]);candidates.push(clean);};
    if(d) {
      if(count===1)add([start,end]);
      else add([start,sa,[(sa[0]+sb[0])/2+offset[0],(sa[1]+sb[1])/2+offset[1]],sb,end]);
    }
    // Only twelve nearest corridor obstacles propose lanes; all visible cards
    // still validate candidates. Endpoint cards also propose local outer lanes.
    const l=Math.min(sa[0],sb[0]),r=Math.max(sa[0],sb[0]),t=Math.min(sa[1],sb[1]),bot=Math.max(sa[1],sb[1]);
    const distance=q=>Math.max(l-q.r,q.l-r,0)+Math.max(t-q.b,q.t-bot,0);
    const nearby=rects.slice().sort((a,b)=>distance(a)-distance(b)||(a.id<b.id?-1:a.id>b.id?1:0)).slice(0,12);
    const xs=new Set(),ys=new Set();
    const gap=2+index*6;
    for(const q of nearby){xs.add(q.l-gap);xs.add(q.r+gap);ys.add(q.t-gap);ys.add(q.b+gap);}
    if(d){xs.add((sa[0]+sb[0])/2+lane);ys.add((sa[1]+sb[1])/2+lane);}
    // <=151 local candidates, no giant exterior padding or unbounded search.
    const spread=set=>[...new Set([...set].flatMap(v=>[-10,0,10].map(d=>v+d)))].sort((a,b)=>a-b);
    for(const y of spread(ys))add([start,sa,[sa[0],y],[sb[0],y],sb,end]);
    for(const x of spread(xs))add([start,sa,[x,sa[1]],[x,sb[1]],sb,end]);
    // Non-endpoints use full 12px clearance. Endpoints allow the unavoidable
    // port approach inside that margin, but never passage through their card.
    const collisionRects=rects.map(q=>q.id===a.id||q.id===b.id?{...q,l:q.l+12,r:q.r-12,t:q.t+12,b:q.b-12}:q);
    // Quantized costs make mathematically tied candidates agree with Python.
    const ordered=candidates.map((ps,i)=>({ps,i,len:+length(ps).toFixed(6)})).sort((a,b)=>a.len-b.len||a.i-b.i);
    const valid=[];
    for(const {ps} of ordered) {
      // Extra collinear gutter vertices are not a distinct parallel lane.
      if(d && count>1 && lane!==0 && ps.every(p=>Math.abs((p[0]-start[0])*dy-(p[1]-start[1])*dx)<1e-7))continue;
      if(ps.length<2 || ps.some((p,i)=>i && collisionRects.some(q=>intersects(ps[i-1],p,q))))continue;
      const size=length(ps);
      if(valid.length&&size>valid[0].size*1.5+180)break;
      valid.push({ps,size});if(valid.length===24)break;
    }
    if(valid.length) {
      const best=valid.map((v,i)=>({...v,i,cost:+(v.size+congestion(v.ps,reserved)).toFixed(6)})).sort((a,b)=>a.cost-b.cost||a.i-b.i)[0];
      return {path:path(best.ps),routeStatus:'routed'};
    }
    // Do not reuse unvalidated Beziers or knowingly draw through a card. A
    // move-only path is explicit missing geometry, with canonical edge retained
    // for details/search and a visible adapter warning. No unbounded search.
    return {path:path([start]),routeStatus:'unroutable'};
  }
  function route(nodes,edges,allEdges=edges) {
    if(nodes.length>100||edges.length>600||allEdges.length>600)throw Error('Routing bound exceeded');
    const by=new Map(nodes.map(n=>[n.id,n])),rects=nodes.map(rect),groups=new Map();
    for(const e of allEdges){const key=JSON.stringify([e.from,e.to].sort());if(!groups.has(key))groups.set(key,[]);groups.get(key).push(e.id);}
    for(const ids of groups.values())ids.sort();
    const result={},reserved=[];
    for(const e of edges.slice().sort((a,b)=>a.id<b.id?-1:a.id>b.id?1:0)) {
      const ids=groups.get(JSON.stringify([e.from,e.to].sort()));
      result[e.id]=one(by.get(e.from),by.get(e.to),e,ids.indexOf(e.id),ids.length,rects,reserved);
      const ps=points(result[e.id].path);reserved.push(...ps.slice(1).map((b,i)=>[ps[i],b]));
    }
    allocate(result,edges,rects);
    return result;
  }
  return {route,text};
})();
if(typeof module!=='undefined'&&module.exports)module.exports=Routing;
