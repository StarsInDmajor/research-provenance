'use strict';
// Actual generated/final entry + strict event double; no browser/layout proof.
const assert=require('node:assert/strict');
const {test}=require('node:test');
const vm=require('node:vm');
const {execFileSync}=require('node:child_process');
const {fromTree}=require('./dom-double.js');
const {wheelFactor}=require('../graph.js');
const fixture=JSON.parse(execFileSync('python3',[__dirname+'/entry_fixture.py',...process.argv.slice(2)],{maxBuffer:4e6}));
const near=(a,b)=>assert.ok(Math.abs(a-b)<1e-8,`${a} != ${b}`);
const settle=async()=>{for(let i=0;i<8;i++)await Promise.resolve();};
function setup(native='unavailable') {
  const doc=fromTree(fixture.tree),get=id=>doc.getElementById(id),workspace=get('workspace');
  const win={scrollX:12,scrollY:340,scrollTo(x,y){this.scrollX=x;this.scrollY=y;}};
  doc.defaultView=win;doc.fullscreenElement=null;
  let requests=0,exits=0,resolve,reject;
  if(native!=='unavailable') workspace.requestFullscreen=()=>{
    requests++;
    if(native==='denied')return Promise.reject(Error('SECRET_DIAGNOSTIC'));
    if(native==='throws')throw Error('SECRET_DIAGNOSTIC');
    return new Promise((yes,no)=>{resolve=()=>{doc.fullscreenElement=workspace;doc.dispatch('fullscreenchange');yes();};reject=no;});
  };
  doc.exitFullscreen=()=>{exits++;doc.fullscreenElement=null;doc.dispatch('fullscreenchange');return Promise.resolve();};
  const ctx=vm.createContext({document:doc});
  for(const script of fixture.scripts)vm.runInContext(script,ctx);
  assert.equal(doc.body.getAttribute('data-app-state'),'ready');
  const life=vm.runInContext('boot(document)',ctx),api=life.api;
  return {doc,get,win,workspace,api,life,resolve:()=>resolve(),reject:()=>reject(Error('SECRET_DIAGNOSTIC')),requests:()=>requests,exits:()=>exits};
}
const click=(s,id)=>s.get(id).dispatch('click');
const mode=s=>s.workspace.getAttribute('data-canvas-mode');
function selected(s){const m=s.api.model; m.select(m.graph.nodes.find(n=>n.current).id);s.api.render();return m;}

test('wheel factor uses magnitude, finite pixel/line/page normalization, cap and reciprocity',()=>{
  assert.equal(typeof wheelFactor,'function');
  assert.equal(wheelFactor(0),1);
  for(const bad of [NaN,Infinity,-Infinity,undefined,null,'1'])assert.equal(wheelFactor(bad),1);
  assert.equal(wheelFactor(10,99),1);
  near(wheelFactor(1),Math.exp(-.0012));near(wheelFactor(-1),Math.exp(.0012));
  for(const d of [1,50,100,1e300])near(wheelFactor(d)*wheelFactor(-d),1);
  near(wheelFactor(1,1),wheelFactor(16));near(wheelFactor(1,2,500),wheelFactor(500));
  near(wheelFactor(.01,2,1e9),wheelFactor(10)); // page unit capped at 1000px
  near(wheelFactor(.01,2,NaN),wheelFactor(8)); // default 800px
  assert.ok(wheelFactor(-1e300)<=1.101);assert.ok(wheelFactor(1e300)>=1/1.101);
  near(wheelFactor(1)**50,wheelFactor(50));
});
test('real wheel ignores zero/invalid, maintains letterbox pointer anchor and route cache; pane scroll untouched',()=>{
  const s=setup(),m=selected(s),svg=s.get('graph');m.box={x:0,y:0,w:m.graph.width,h:m.graph.height};
  const original=JSON.stringify(m.box),stats=JSON.stringify(m.routeStats),stack=m.stack.length;
  for(const deltaY of [0,NaN,Infinity])svg.dispatch('wheel',{deltaY,clientX:300,clientY:200});
  assert.equal(JSON.stringify(m.box),original);
  const {viewport}=require('../graph.js');
  const p=viewport(svg.getBoundingClientRect(),m.box,300,200),anchor={x:m.box.x+p.x*m.box.w,y:m.box.y+p.y*m.box.h};
  svg.dispatch('wheel',{deltaY:-1,deltaMode:0,clientX:300,clientY:200,ctrlKey:true});
  near(m.box.x+p.x*m.box.w,anchor.x);near(m.box.y+p.y*m.box.h,anchor.y);
  near(m.graph.width/m.box.w,Math.exp(.0012));
  svg.dispatch('wheel',{deltaY:1,deltaMode:0,clientX:300,clientY:200});near(m.box.w,m.graph.width);near(m.box.x,0);
  let prevented=false;s.get('details').dispatch('wheel',{deltaY:100,preventDefault(){prevented=true;}});assert.equal(prevented,false);
  const box=JSON.stringify(m.box);svg.dispatch('wheel',{deltaY:10,clientX:NaN,clientY:200});assert.equal(JSON.stringify(m.box),box);
  click(s,'zoom-in');near(m.graph.width/m.box.w,1.1);click(s,'zoom-out');near(m.box.w,m.graph.width);
  assert.equal(JSON.stringify(m.routeStats),stats);assert.equal(m.stack.length,stack);
});
test('CSS/window fallback preserves exact view, DOM disclosures, scroll, focus and Back; pane toggle independent',()=>{
  const s=setup(),m=selected(s),panel=s.get('details'),content=s.get('detail-content');
  const disclosure=content.querySelectorAll('details').at(-1);disclosure.open=true;panel.scrollTop=123;s.get('reading-guide').open=true;
  s.get('search').value='keep';s.get('search').dispatch('input');s.get('search').focus();
  const state=JSON.stringify(m.snapshot()),stack=JSON.stringify(m.stack),children=[...content.children],stats=JSON.stringify(m.routeStats);
  click(s,'fullscreen');assert.equal(mode(s),'window');assert.equal(s.requests(),0);
  assert.match(s.get('canvas-status').textContent,/窗口.*非浏览器原生全屏/);
  assert.equal(s.get('fullscreen').getAttribute('aria-pressed'),'true');
  click(s,'toggle-details');assert.equal(panel.hidden,true);assert.equal(s.get('toggle-details').getAttribute('aria-expanded'),'false');
  panel.scrollTop=0; // hiding/resizing a scroll container may clamp it in a browser
  click(s,'toggle-details');assert.equal(panel.hidden,false);assert.equal(panel.scrollTop,123);
  s.win.scrollY=0;s.doc.dispatch('keydown',{key:'Escape',target:s.get('search')});
  assert.equal(mode(s),'normal');assert.equal(s.win.scrollY,340);assert.equal(s.win.scrollX,12);assert.equal(s.doc.activeElement,s.get('search'));
  assert.equal(JSON.stringify(m.snapshot()),state);assert.equal(JSON.stringify(m.stack),stack);assert.deepEqual(content.children,children);
  assert.equal(disclosure.open,true);assert.equal(s.get('reading-guide').open,true);assert.equal(panel.scrollTop,123);assert.equal(JSON.stringify(m.routeStats),stats);
  assert.equal(s.get('fullscreen').getAttribute('aria-pressed'),'false');
  const old=m.selected,other=m.graph.nodes.find(n=>n.current&&n.id!==old);
  click(s,'toggle-details');m.select(other.id);s.api.render();click(s,'back');click(s,'toggle-details');
  assert.equal(m.selected,old);assert.equal(panel.scrollTop,123);assert.ok(content.querySelectorAll('details').at(-1).open);
  s.get('search').dispatch('keydown',{key:'Escape'});assert.equal(m.searchState.closed,true);assert.equal(m.selected,old);
  s.doc.dispatch('keydown',{key:'Escape',target:s.get('graph')});assert.equal(m.selected,null);
});
for(const kind of ['denied','throws'])test('native '+kind+' request stays ready in explicitly labeled window fallback',async()=>{
  const s=setup(kind);assert.equal(s.requests(),0);click(s,'fullscreen');await settle();
  assert.equal(s.requests(),1);assert.equal(mode(s),'window');assert.equal(s.life.active,true);
  assert.doesNotMatch(s.doc.textContent,/SECRET_DIAGNOSTIC/);click(s,'fullscreen');assert.equal(mode(s),'normal');
});
test('native root includes toolbar and details; native Esc/fullscreenchange restores without graph navigation',async()=>{
  const s=setup('pending'),m=selected(s),state=JSON.stringify(m.snapshot()),stack=JSON.stringify(m.stack);
  assert.ok(s.workspace.contains(s.get('toolbar'))&&s.workspace.contains(s.get('details')));
  click(s,'fullscreen');assert.equal(mode(s),'window');s.resolve();await settle();assert.equal(mode(s),'native');
  assert.match(s.get('canvas-status').textContent,/浏览器原生全屏/);
  s.doc.fullscreenElement=null;s.doc.dispatch('fullscreenchange');assert.equal(mode(s),'normal');
  assert.equal(JSON.stringify(m.snapshot()),state);assert.equal(JSON.stringify(m.stack),stack);
  click(s,'fullscreen');s.resolve();await settle();click(s,'fullscreen');await settle();assert.equal(mode(s),'normal');assert.equal(s.exits(),1);
});
for(const finish of ['resolve','reject'])test('pending request '+finish+' after Escape cannot resurrect fullscreen',async()=>{
  const s=setup('pending'),m=selected(s),state=JSON.stringify(m.snapshot());
  click(s,'fullscreen');s.doc.dispatch('keydown',{key:'Escape',target:s.get('search')});assert.equal(mode(s),'normal');
  s[finish]();await settle();assert.equal(mode(s),'normal');assert.equal(s.doc.fullscreenElement,null);assert.equal(s.life.active,true);assert.equal(JSON.stringify(m.snapshot()),state);
});
test('pending cancellation blocks reentry, and late native resolution is immediately exited',async()=>{
  const s=setup('pending');click(s,'fullscreen');click(s,'fullscreen');
  assert.equal(s.get('fullscreen').disabled,true);click(s,'fullscreen');assert.equal(s.requests(),1);
  s.resolve();await settle();assert.equal(mode(s),'normal');assert.equal(s.doc.fullscreenElement,null);
  assert.equal(s.get('fullscreen').disabled,false);click(s,'fullscreen');assert.equal(s.requests(),2);s.reject();await settle();
  assert.equal(mode(s),'window');
});
test('native exit restores page scroll/focus after asynchronous browser teardown',async()=>{
  const s=setup('pending');s.get('search').focus();click(s,'fullscreen');s.resolve();await settle();
  let exit;
  s.doc.exitFullscreen=()=>new Promise(resolve=>{exit=()=>{s.win.scrollY=0;s.doc.activeElement=null;s.doc.fullscreenElement=null;s.doc.dispatch('fullscreenchange');resolve();};});
  click(s,'fullscreen');exit();await settle();assert.equal(mode(s),'normal');assert.equal(s.win.scrollY,340);assert.equal(s.doc.activeElement,s.get('search'));
});
test('native exit rejection keeps truthful native state and still allows native Escape',async()=>{
  const s=setup('pending');click(s,'fullscreen');s.resolve();await settle();
  s.doc.exitFullscreen=()=>Promise.reject(Error('SECRET_DIAGNOSTIC'));click(s,'fullscreen');await settle();
  assert.equal(mode(s),'native');assert.equal(s.life.active,true);assert.doesNotMatch(s.doc.textContent,/SECRET_DIAGNOSTIC/);
  s.doc.fullscreenElement=null;s.doc.dispatch('fullscreenchange');assert.equal(mode(s),'normal');
});
test('actual wheel event distribution, line/page bounds and graph-only preventDefault',()=>{
  const a=setup(),b=setup();
  const wheel=(s,deltaY,deltaMode=0)=>{let prevented=false;s.get('graph').dispatch('wheel',{deltaY,deltaMode,clientX:400,clientY:250,preventDefault(){prevented=true;}});assert.equal(prevented,true);};
  for(let i=0;i<50;i++)wheel(a,1);wheel(b,50);
  for(const k of ['x','y','w','h'])near(a.api.model.box[k],b.api.model.box[k]);
  const width=b.api.model.box.w;wheel(b,-1,1);near(b.api.model.box.w,width/Math.exp(.0012*16));
  const before=b.api.model.box.w;wheel(b,-1e6,2);near(b.api.model.box.w,before/1.1);
  // Different SVG rectangle after expanding/resizing does not itself change viewBox.
  const box=JSON.stringify(b.api.model.box);click(b,'fullscreen');
  b.get('graph').getBoundingClientRect=()=>({left:0,top:0,width:400,height:900});
  assert.equal(JSON.stringify(b.api.model.box),box);click(b,'fullscreen');assert.equal(JSON.stringify(b.api.model.box),box);
});
test('failure readiness locks both controls but cannot swallow pending native cancellation',async()=>{
  const s=setup('pending');click(s,'fullscreen');s.life.fail();
  assert.equal(s.get('fullscreen').disabled,true);assert.equal(s.get('toggle-details').disabled,true);
  s.doc.dispatch('keydown',{key:'Escape'});s.resolve();await settle();assert.equal(mode(s),'normal');assert.equal(s.doc.fullscreenElement,null);
  click(s,'fullscreen');assert.equal(mode(s),'normal');
});
test('template/CSS contract: viewport fallback, flex remaining height, right overlay, small screens, normal print',()=>{
  const s=setup(),css=s.doc.querySelectorAll('style')[0].textContent;
  for(const id of ['fullscreen','toggle-details'])assert.equal(s.get(id).getAttribute('type'),'button');
  assert.equal(s.get('toggle-details').getAttribute('aria-controls'),'details');
  assert.match(css,/body\.canvas-expanded\s*\{[^}]*overflow:hidden/);
  assert.match(css,/#workspace\.canvas-expanded\s*\{[^}]*position:fixed[^}]*inset:0[^}]*100dvh/);
  assert.match(css,/\.canvas-expanded \.workspace-grid\s*\{[^}]*position:relative[^}]*flex:1[^}]*min-height:0/);
  assert.match(css,/\.canvas-expanded \.chart-wrap\s*\{[^}]*flex:1[^}]*min-height:0[^}]*height:auto/);
  assert.match(css,/\.canvas-expanded \.detail-panel\s*\{[^}]*position:absolute[^}]*right:[^}]*width:min\(380px,[^}]*overflow:auto/);
  assert.match(css,/@media\s*print[\s\S]*#workspace\.canvas-expanded[^}]*position:static/);
  assert.match(css,/@media\s*print[\s\S]*\.canvas-expanded \.detail-panel[^}]*position:static/);
  assert.match(css,/@media\s*print[\s\S]*#details\[hidden\][^}]*display:block/);
});
