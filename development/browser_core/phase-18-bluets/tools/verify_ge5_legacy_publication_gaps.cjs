// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The complete legacy contract rejects a source grant in generated System publication trivia.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const graph=require('./ge5_inputs.cjs'),authority=require('./ge5_native_authority.cjs');
const {stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs');
const {compare}=require('./ge5_decorator_legacy_module.cjs'),{coherent}=require('./ge5_decorator_preserved_mutations.cjs');
const root=path.resolve(__dirname,'../../../..'),directory=path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps');
const input=JSON.parse(fs.readFileSync(path.join(directory,'cases.json'))),reference=JSON.parse(fs.readFileSync(path.join(directory,'reference.json'))),parents=graph.parents(root,input);
const ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const rows=JSON.parse(fs.readFileSync(0,'utf8'));assert.equal(rows.length,8);let controls=0;
for(const row of rows){
 const spec=input.cases[row.index],original=graph.resolve(input,spec,parents),expected=reference.cases[row.index];assert.equal(row.id,expected.id);assert.equal(spec.form,'ge2_module_helpers/legacy-parameter-decorator');assert.equal(original.options.target,'ES5');assert.equal(original.options.module,'System');
 for(const [name,map]of Object.entries(expected.maps)){
  if(map.kind!=='javascript')continue;
  const n=stream(ts,reference.blobs[expected.outputs[map.output]],authority.raw(reference,expected,name)),a=stream(ts,row.outputs[map.output],row.outputs[name]);
  const proof=compare(ts,n,a,original.options,original.files[original.entry]);
  assert.equal(proof.classes.length,1);const logicalEnd=proof.classes[0].actual.node.end,nativeEndOwner=n.owner(proof.classes[0].native.node.end);
  const source=ts.createSourceFile('original.ts',original.files[original.entry],ts.ScriptTarget.Latest,true),classes=source.statements.filter(ts.isClassDeclaration);assert.equal(classes.length,1);const c=classes[0];
  const pubs=[];function visit(node){if(ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)&&ts.isPropertyAccessExpression(node.expression.left)&&node.expression.left.expression.getText(a.source)==='exports'&&node.expression.left.name.text===c.name.text)pubs.push(node);ts.forEachChild(node,visit);}visit(a.source);assert.equal(pubs.length,1);
  const end=pubs[0].getStart(a.source);let start=end;while(start>0&&/\s/.test(a.code[start-1]))start--;assert(start<end,'existing generated publication formatting');const wrong=c.getSourceFile().getLineAndCharacterOfPosition(c.end),owner=[0,wrong.line,wrong.character];
  let tail=pubs[0].end;while(tail<a.code.length&&/\s/.test(a.code[tail]))tail++;
  const offsets=[...Array.from({length:end-start},(_,index)=>start+index),...Array.from({length:tail-pubs[0].end},(_,index)=>pubs[0].end+index)];
  assert.deepEqual(a.owner(logicalEnd),nativeEndOwner);if(nativeEndOwner){const changed=coherent(ts,mutate(ts,a,logicalEnd,null));assert.throws(()=>compare(ts,stream(ts,n.code,n.raw),changed,original.options,original.files[original.entry]));controls++;}
  for(const offset of offsets){assert.deepEqual(a.owner(offset),offset===logicalEnd?nativeEndOwner:null,'exact source construction endpoint and generated publication trivia stay distinct');const changed=coherent(ts,mutate(ts,a,offset,owner));assert.throws(()=>compare(ts,stream(ts,n.code,n.raw),changed,original.options,original.files[original.entry]));controls++;}
 }
}
console.log(JSON.stringify({configurations:rows.length,controls,passed:true}));
