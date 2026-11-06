// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Valid typed delegates never authorize their enclosing original declaration endpoint.
const test=require('node:test'),assert=require('node:assert/strict'),path=require('node:path'),root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc'),{stream,compareTokens}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{compareClass}=require('./ge5_class_roles.cjs');
function fixture(kind,owner) {
  const text=kind==='field'?'class Box { value = (n: number) => n + 1; other() { return 2; } }\n':'class Box { async read(n: number) { return await Promise.resolve(n); } other() { return 2; } }\n',source=ts.createSourceFile('original.ts',text,ts.ScriptTarget.Latest,true),original=source.statements[0],output=ts.transpileModule(text,{fileName:'original.ts',compilerOptions:{target:ts.ScriptTarget.ES2022,sourceMap:true,inlineSources:true}}),native=stream(ts,output.outputText,output.sourceMapText);let actual=stream(ts,output.outputText,output.sourceMapText);
  if(owner!==undefined)actual=mutate(ts,actual,actual.source.statements[0].end,owner);
  const nn=native.source.statements[0],an=actual.source.statements[0],pair=kind==='field'?{source:original.members[0].initializer,native:nn.members[0].initializer,actual:an.members[0].initializer}:{source:original.members[0],native:nn.members[0],actual:an.members[0]};
  compareTokens(native,native.tokens(pair.native),actual,actual.tokens(pair.actual),'complete independently reproved delegate');assert.deepEqual(actual.owner(pair.actual.end),native.owner(pair.native.end),'delegate exact logical endpoint remains correct');
  return {native,actual,nn,an,original,options:{classRoles:kind==='field'?{pairedExpressions:[pair]}:{pairedCallables:[pair]}}};
}
for(const kind of['field','method']) {
  test(kind+' typed proof retains the complete class declaration endpoint',()=>{const f=fixture(kind);assert(compareClass(ts,f.native,f.nn,f.actual,f.an,f.original,f.options));});
  for(const[mode,owner]of[['erase',null],['retarget',[0,0,0]]])test(kind+' valid delegate cannot '+mode+' its enclosing class declaration endpoint',()=>{const f=fixture(kind,owner);assert.notDeepEqual(f.actual.owner(f.an.end),f.native.owner(f.nn.end),'effective valid map attack');assert.throws(()=>compareClass(ts,f.native,f.nn,f.actual,f.an,f.original,f.options),/exact original class construction logical endpoint/);});
}
test('an original class expression retains its separate class-value contract path',()=>{const text='const Value=class Box { value=1; };\n',source=ts.createSourceFile('original.ts',text,ts.ScriptTarget.Latest,true),original=source.statements[0].declarationList.declarations[0].initializer,output=ts.transpileModule(text,{fileName:'original.ts',compilerOptions:{target:ts.ScriptTarget.ES2022,sourceMap:true,inlineSources:true}}),native=stream(ts,output.outputText,output.sourceMapText),actual=stream(ts,output.outputText,output.sourceMapText),node=view=>view.source.statements[0].declarationList.declarations[0].initializer;assert(ts.isClassExpression(original));assert(compareClass(ts,native,node(native),actual,node(actual),original));});

test('a static initializer retains Native U at the class syntax end and rejects source grants',()=>{
  const text='export class Box { static value:number=1; }\n',original=ts.createSourceFile('original.ts',text,ts.ScriptTarget.Latest,true).statements[0];
  for(const target of[ts.ScriptTarget.ES2015,ts.ScriptTarget.ES2022,ts.ScriptTarget.ESNext])for(const module of[ts.ModuleKind.CommonJS,ts.ModuleKind.ESNext]){
    const output=ts.transpileModule(text,{fileName:'original.ts',compilerOptions:{target,module,useDefineForClassFields:true,sourceMap:true,inlineSources:true}}),native=stream(ts,output.outputText,output.sourceMapText),actual=stream(ts,output.outputText,output.sourceMapText),node=view=>view.source.statements.find(ts.isClassDeclaration);assert.equal(native.owner(node(native).end),null,'precise Native static-initialization phase');assert(compareClass(ts,native,node(native),actual,node(actual),original));
    const changed=mutate(ts,actual,node(actual).end,[0,0,0]);assert.notEqual(changed.owner(node(changed).end),null,'effective valid generated-source grant');assert.throws(()=>compareClass(ts,native,node(native),changed,node(changed),original),/exact original class construction logical endpoint/);
  }
});
