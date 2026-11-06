// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test=require('node:test'),assert=require('node:assert/strict'),path=require('node:path'),root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{compareClass}=require('./ge5_class_roles.cjs');
function fixture(target) {
  const text=(target===ts.ScriptTarget.ES5?'':'function dec(...args:any[]):any{}\n@dec ')+'export class Box { method(value:number){return value;} }\n',original=ts.createSourceFile('original.ts',text,ts.ScriptTarget.Latest,true).statements.find(ts.isClassDeclaration),output=ts.transpileModule(text,{fileName:'original.ts',compilerOptions:{target,module:ts.ModuleKind.CommonJS,experimentalDecorators:true,useDefineForClassFields:true,sourceMap:true,inlineSources:true}});
  const view=()=>stream(ts,output.outputText,output.sourceMapText),node=view=>view.source.statements.find(node=>ts.isVariableStatement(node)&&node.declarationList.declarations[0].name.text==='Box');return {original,view,node};
}
for(const[target,label]of[[ts.ScriptTarget.ES2015,'retained'],[ts.ScriptTarget.ES5,'ES5']]) {
  test(label+' declaration exposes its complete construction and member chain',()=>{const f=fixture(target),n=f.view(),a=f.view(),proof=compareClass(ts,n,f.node(n),a,f.node(a),f.original);assert(proof.declarationChain.some(pair=>pair.label==='outer statement'));assert(proof.declarationChain.some(pair=>pair.label==='binding declaration'));assert(proof.declarationChain.some(pair=>pair.label==='initializer'));assert(proof.declarationChain.some(pair=>pair.label==='method body 0'));});
  for(const mode of['erase','retarget'])test(label+' complete chain rejects '+mode+' at every independently paired end',()=>{
    const f=fixture(target),n=f.view(),a=f.view(),proof=compareClass(ts,n,f.node(n),a,f.node(a),f.original),seen=new Set();
    for(const pair of proof.declarationChain) {
      const offset=pair.actual.end;if(seen.has(offset))continue;seen.add(offset);const owner=mode==='erase'?null:[0,0,1];if(JSON.stringify(a.owner(offset))===JSON.stringify(owner))continue;
      const changed=mutate(ts,a,offset,owner),fresh=f.view();assert.throws(()=>compareClass(ts,fresh,f.node(fresh),changed,f.node(changed),f.original),undefined,pair.label+' complete chain');
    }
  });
}
test('the retained outer binding end cannot hide behind a correct inner class end',()=>{const f=fixture(ts.ScriptTarget.ES2015),n=f.view(),a=f.view(),nn=f.node(n),an=f.node(a),inner=an.declarationList.declarations[0].initializer,changed=mutate(ts,a,an.end,null);assert.notEqual(an.end,inner.end);assert.deepEqual(changed.owner(inner.end),n.owner(nn.declarationList.declarations[0].initializer.end));assert.throws(()=>compareClass(ts,n,nn,changed,f.node(changed),f.original),/outer statement/);});

function retainedModule() {
  const text='export class Base { constructor(public value:number=40){} get amount(){return this.value;} set amount(value:number){this.value=value;} }\nexport class Box extends Base {constructor(){super();} run(n:number){return super.amount+=n;}}\nexport const result = new Box().run(2);\n';
  const output=ts.transpileModule(text,{fileName:'original.ts',compilerOptions:{target:ts.ScriptTarget.ES2015,module:ts.ModuleKind.CommonJS,sourceMap:true,inlineSources:true}});
  // A leading formatting newline keeps every token/source role unchanged but
  // selects the divergent-output contract instead of decoded-map equality.
  const native=()=>stream(ts,output.outputText,output.sourceMapText),actual=()=>stream(ts,'\n'+output.outputText,JSON.stringify({...JSON.parse(output.sourceMapText),mappings:';'+JSON.parse(output.sourceMapText).mappings}));
  const compare=require('./ge5_class_retained.cjs').compare,options={target:'ES2015',module:'CommonJS'};
  return {text,native,actual,compare,options};
}
test('the retained complete-module reader exposes every exact class construction chain',()=>{
  const f=retainedModule(),proof=f.compare(ts,f.native(),f.actual(),f.options,f.text);
  for(const item of proof.classes) {
    assert(Array.isArray(item.declarationChain),'retained reader exposes complete construction endpoint proofs');
    assert(item.declarationChain.some(pair=>pair.label==='outer statement'));
    assert(item.declarationChain.some(pair=>pair.label==='constructor body'));
    assert(item.declarationChain.some(pair=>pair.label==='method body 0'));
  }
});
for(const mode of ['erase','retarget'])test('retained complete-module reader rejects '+mode+' at class, constructor and member logical ends',()=>{
  const f=retainedModule(),native=f.native(),actual=f.actual(),chain=require('./ge5_class_declaration_chain.cjs');
  const nn=native.source.statements.filter(ts.isClassDeclaration),an=actual.source.statements.filter(ts.isClassDeclaration),seen=new Set();
  f.compare(ts,f.native(),f.actual(),f.options,f.text);
  for(let index=0;index<nn.length;index++)for(const pair of chain.compare(ts,native,nn[index],{native:true,node:nn[index]},actual,an[index],{native:true,node:an[index]})) {
    const offset=pair.actual.end;if(seen.has(offset))continue;seen.add(offset);
    const owner=mode==='erase'?null:[0,0,1];if(JSON.stringify(actual.owner(offset))===JSON.stringify(owner))continue;
    const changed=mutate(ts,actual,offset,owner);
    assert.throws(()=>f.compare(ts,f.native(),changed,f.options,f.text),mode==='erase'?/class construction logical endpoint/:undefined,pair.label+' '+index);
  }
});
