'use strict';
// Strict structural/event double, NOT a browser: no CSP/layout/hit testing.
function descendants(el,selector) {
  const matches=c=>selector.split(',').some(s=>{
    s=s.trim();
    if(s==='[id]')return c.getAttribute('id')!==null;
    if(s==='[data-key]')return c.getAttribute('data-key')!==null;
    return c.localName===s;
  });
  return el.children.flatMap(c=>[...(matches(c)?[c]:[]),...descendants(c,selector)]);
}
class El {
  constructor(tag='div',doc=null){this.tag=tag;this.ownerDocument=doc;this.children=[];this.handlers={};this.attrs={};this.dataset={};this.value='';this._text='';this.disabled=false;this.classes=new Set();this.classList={toggle:(k,on)=>on?this.classes.add(k):this.classes.delete(k)};}
  get localName(){return this.tag;}
  get namespaceURI(){return this.ns||'http://www.w3.org/1999/xhtml';}
  get tagName(){return this.namespaceURI==='http://www.w3.org/2000/svg'?this.tag:this.tag.toUpperCase();}
  set textContent(x){this.replaceChildren();this._text=String(x);}
  get textContent(){return this._text+this.children.map(x=>x.textContent).join('');}
  appendChild(x){if(x.parentNode)x.parentNode.children=x.parentNode.children.filter(c=>c!==x);x.parentNode=this;x.ownerDocument=this.ownerDocument;this.children.push(x);return x;}
  replaceChildren(){for(const c of this.children)c.parentNode=null;this.children=[];this._text='';}
  addEventListener(k,fn){(this.handlers[k]??=[]).push(fn);}
  dispatch(k,extra={}){const event={type:k,target:this,defaultPrevented:false,preventDefault(){this.defaultPrevented=true;},stopPropagation(){},button:0,isPrimary:true,pointerId:1,...extra};for(const fn of this.handlers[k]||[])fn(event);}
  setAttribute(k,v){v=String(v);this.attrs[k]=v;if(k.startsWith('data-'))this.dataset[k.slice(5).replace(/-([a-z])/g,(_,c)=>c.toUpperCase())]=v;if(k==='class')this.classes=new Set(v.split(/\s+/));if(k==='disabled')this.disabled=true;if(k==='open')this.open=true;}
  getAttribute(k){return this.attrs[k]??null;}
  removeAttribute(k){delete this.attrs[k];}
  querySelectorAll(selector){return descendants(this,selector);}
  focus(){this.ownerDocument.activeElement=this;}
  contains(el){return el===this||this.children.some(c=>c.contains(el));}
  getBoundingClientRect(){return {left:0,top:0,width:800,height:500};}
  setPointerCapture(){this.captured=true;}
  hasPointerCapture(){return this.captured;}
  releasePointerCapture(){this.captured=false;}
}
class Doc extends El {
  constructor(){super('#document');this.ownerDocument=this;this.activeElement=null;}
  getElementById(id){return this.querySelectorAll('[id]').find(el=>el.getAttribute('id')===id)||null;}
  createElement(tag){return new El(tag.toLowerCase(),this);}
  createElementNS(ns,tag){const el=new El(tag,this);el.ns=ns;return el;}
}
function fromTree(tree){
  const doc=new Doc();
  function add(parent,item){const ns=item.tag==='svg'||parent.namespaceURI==='http://www.w3.org/2000/svg';const tag=({clippath:'clipPath'})[item.tag]||item.tag;const el=ns?doc.createElementNS('http://www.w3.org/2000/svg',tag):doc.createElement(tag);for(const [k,v] of Object.entries(item.attrs))el.setAttribute(k,v??'');el._text=item.text;parent.appendChild(el);for(const child of item.children)add(el,child);return el;}
  for(const child of tree.children)add(doc,child);
  doc.body=doc.querySelectorAll('body')[0];return doc;
}
module.exports={El,Doc,descendants,fromTree};
