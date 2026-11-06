// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Receiver-binding transport preserves separate initializer and field-completion roles.
const test=require('node:test'),assert=require('node:assert/strict'),path=require('node:path');
const root=path.resolve(__dirname,'../../../..'),directory=path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps');
const input=require(path.join(directory,'cases.json')),reference=require(path.join(directory,'reference.json'));
const graph=require('./ge5_inputs.cjs'),parents=graph.parents(root,input),authority=require('./ge5_native_authority.cjs');
const ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const {stream}=require('./ge5_role_stream.cjs'),{encode,mutate}=require('./ge5_role_mutations.cjs'),fields=require('./ge5_decorator_fields.cjs');
function fixture(index,wrapped){
  const original=graph.resolve(input,input.cases[index],parents),source=ts.createSourceFile('original.ts',original.files[original.entry],ts.ScriptTarget.Latest,true);
  const field=source.statements.find(ts.isClassDeclaration).members.find(node=>ts.isPropertyDeclaration(node)&&node.name.text==='static x');assert(field);
  const expected=reference.cases[index],[name,map]=Object.entries(expected.maps).find(([,map])=>map.kind==='javascript');
  const view=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name));let store;
  function visit(node){if(ts.isBinaryExpression(node)){const item=fields.readStore(ts,view,node);if(item?.key==='static x'&&ts.isCallExpression(item.value))store=item;}ts.forEachChild(node,visit);}visit(view.source);assert(store);
  const start=store.expression.getStart(view.source),value=store.value,begin=value.getStart(view.source),last=view.tokens(value).at(-1),prefix=view.owner(begin),suffix=view.owner(last.start),completion=view.owner(value.end);
  assert.notDeepEqual(suffix,completion,'the exact Native inner token and field completion are distinct source phases');
  function artifact(transport){
    let code='',owners=[];const append=(text,owner)=>{for(let offset=0;offset<text.length;offset++)owners.push(typeof owner==='function'?owner(offset):owner);code+=text;};
    append(view.code.slice(start,begin),offset=>view.owner(start+offset));if(transport)append('(function () { return ',prefix);
    const innerStart=code.length;append(view.code.slice(begin,value.end),offset=>view.owner(begin+offset));const innerEnd=code.length;
    if(transport)append('; }).call('+store.receiver.getText(view.source)+')',suffix);const outerEnd=code.length;
    append(';',completion);owners.push(view.owner(store.expression.end));
    const generated=ts.createSourceFile('field.js',code,ts.ScriptTarget.Latest,true),segments=owners.map((owner,offset)=>{const point=generated.getLineAndCharacterOfPosition(offset);return[point.line,point.character,...(owner||[])];});
    return {view:stream(ts,code,JSON.stringify({...JSON.parse(view.raw),mappings:encode(segments)})),innerStart,innerEnd,outerEnd};
  }
  const native=artifact(false),actual=artifact(wrapped);
  return {field,native,actual,suffix,completion,compare(observed=actual.view){const n=stream(ts,native.view.code,native.view.raw);return fields.compareStore(ts,n,n.source.statements[0],observed,observed.source.statements[0],field);}};
}
for(const index of[6214,6215])for(const wrapped of[false,true]){
  const label=index+'/'+(wrapped?'wrapped':'unwrapped');
  test(label+': exact Native token interiors and both completion phases remain admissible',()=>{const f=fixture(index,wrapped);assert(f.compare());});
  for(const role of['innerEnd','outerEnd'])for(const erase of[false,true])test(label+': '+role+' rejects '+(erase?'erase':'retarget'),()=>{
    const f=fixture(index,wrapped);assert(f.compare(),'the unchanged complete field is accepted before an isolated mutation');
    const offset=f.actual[role],owner=erase?null:(wrapped&&role==='innerEnd'?f.completion:f.suffix);
    assert.notDeepEqual(f.actual.view.owner(offset),owner,'the bounded endpoint mutation is effective');
    const changed=mutate(ts,f.actual.view,offset,owner);assert.equal(changed.source.parseDiagnostics.length,0,'the isolated mutation retains valid JavaScript');
    assert.throws(()=>f.compare(changed));
  });
}
