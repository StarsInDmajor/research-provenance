'use strict';
// Shared test adapter: current artifacts use the actual production decoder;
// immutable historical artifacts retain their own embedded old contract.
const {decodeWire}=require('../bootstrap.js');
function readData(doc) {
  const wire=JSON.parse(doc.getElementById('graph-data').textContent);
  if(wire.wireVersion===undefined)return wire;
  const decoded=decodeWire(doc);
  // Phase B decodeWire returns {data, records}; tests expect the data object.
  return decoded.data!==undefined && decoded.records!==undefined ? decoded.data : decoded;
}
function writeData(doc,data) {
  if(data.wireVersion===undefined) {
    doc.getElementById('graph-data').textContent=JSON.stringify(data);return;
  }
  const wire={...data,graph:{...data.graph}};delete wire.records;delete wire.project;
  wire.graph.nodes=data.graph.nodes.map(n=>{
    const item={...n};delete item.raw;
    if(item.title===n.raw.title)delete item.title;
    if(item.kind===n.raw.kind)delete item.kind;
    return item;
  });
  wire.graph.edges=data.graph.edges.map(e=>{const item={...e};delete item.raw;return item;});
  doc.getElementById('graph-data').textContent=JSON.stringify(wire);
  doc.getElementById('canonical-records').textContent=JSON.stringify(data.records);
}
module.exports={readData,writeData};
