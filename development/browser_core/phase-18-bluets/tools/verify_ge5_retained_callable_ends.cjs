// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Query exact Native callable ends without changing unrelated System EOF defects.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const maps=require('./ge5_maps.cjs'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs');
function nativeView(code,raw,original) {
  const source=ts.createSourceFile('emitted.js',code,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS),decoded=maps.decode(raw,code,{recordInvalidCoordinates:true}),widths=original.split('\n').map(line=>line.length);
  return {code,raw,source,decoded,owner(offset){const p=source.getLineAndCharacterOfPosition(offset);let found;for(const point of decoded.segments)if(point[0]===p.line&&point[1]<=p.character)found=point;const owner=found?.length>2?found.slice(2):null;if(owner)assert(owner[0]===0&&owner[1]<widths.length&&owner[2]<=widths[owner[1]],'queried Native tuple remains in original UTF-16 bounds');return owner;}};
}
function points(view,kind) {
  const found=[];function visit(node){if((ts.isClassDeclaration(node)||ts.isClassExpression(node))&&node.name?.text==='C')found.push(node);ts.forEachChild(node,visit);}visit(view.source);assert.equal(found.length,1);
  const cls=found[0],callables=cls.members.filter(node=>node.kind===ts.SyntaxKind[kind]);assert.equal(callables.length,1);const fn=callables[0];assert(fn.body);
  return [fn.end,fn.body.end,cls.end-1];
}
function compare(n,a,kind) {const np=points(n,kind),ap=points(a,kind);for(let i=0;i<np.length;i++)assert.deepEqual(a.owner(ap[i]),n.owner(np[i]),'Native callable/body/class-close exact boundary '+i);return ap;}
const rows=JSON.parse(fs.readFileSync(0,'utf8'));assert.equal(rows.length,960);let controls=0,upstreamEofIssues=0;
for(const row of rows) {
  const output=ts.transpileModule(row.source,{fileName:'main.ts',compilerOptions:{target:ts.ScriptTarget[row.target],module:ts.ModuleKind[row.module],useDefineForClassFields:row.define,sourceMap:true,inlineSources:true}}),native=()=>nativeView(output.outputText,output.sourceMapText,row.source),n=native(),a=stream(ts,row.code,row.raw);upstreamEofIssues+=n.decoded.coordinateIssues.length;
  compare(native(),stream(ts,row.code,row.raw),row.kind);const offsets=compare(n,a,row.kind);
  for(const offset of new Set(offsets))for(const owner of[null,[0,0,1]])if(JSON.stringify(owner)!==JSON.stringify(a.owner(offset))){assert.throws(()=>compare(native(),mutate(ts,a,offset,owner),row.kind),undefined,'effective exact retained callable boundary');controls++;}
}
console.log(JSON.stringify({configurations:rows.length,queriedEndpoints:rows.length*3,controls,recordedUnmodifiedNativeEofIssues:upstreamEofIssues,passed:true}));
