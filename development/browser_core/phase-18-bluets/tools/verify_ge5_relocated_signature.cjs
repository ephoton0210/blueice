// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// API units test capability validation; callers separately prove physical roles.
const assert=require('node:assert/strict'),path=require('node:path'),signature=require('./ge5_relocated_callable_signature.cjs');
const root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
function parse(text,name='unit.js'){return ts.createSourceFile(name,text,ts.ScriptTarget.Latest,true,name.endsWith('.ts')?ts.ScriptKind.TS:ts.ScriptKind.JS);}
function find(source,predicate){const nodes=[];function visit(node){if(predicate(node))nodes.push(node);ts.forEachChild(node,visit);}visit(source);return nodes;}
const original=parse('const object={async read(this: {value:number},extra:number){return this.value+await Promise.resolve(extra);}};','original.ts'),source=find(original,ts.isMethodDeclaration)[0],nativeSource=parse('const object={read(extra){return awaiter(this,void 0,void 0,function(){return generator(this,function(state){return [2,extra];});});}};'),actualSource=parse('const object={read(slot){return (function(){return driver(this,arguments,function(extra){return generator(this,1,function(state){return {kind:"return",value:extra};});});}).apply(this,arguments);}};'),nativeOuter=find(nativeSource,ts.isMethodDeclaration)[0],actualOuter=find(actualSource,ts.isMethodDeclaration)[0],nativeCallback=find(nativeSource,ts.isFunctionExpression)[0],actualCallback=find(actualSource,ts.isFunctionExpression)[1];
function view(source){const covered=new Set();return {source,owner:()=>[0,0,0],covered:offset=>covered.has(offset),cover:(from,to)=>{for(let offset=from;offset<to;offset++)covered.add(offset);}};}
const native=view(nativeSource),actual=view(actualSource),nm={view:native,node:nativeCallback,originals:[original.text]},am={view:actual,node:actualCallback,originals:[original.text]},ranges=[[nativeCallback.getStart(nativeSource),nativeCallback.body.getStart(nativeSource)+1],[actualCallback.getStart(actualSource),actualCallback.body.getStart(actualSource)+1]],cap={kind:'verified-relocated-callable-signature-v1',sourceKind:'async-object-method',source,nativeOuter,actualOuter,nativeCallback,actualCallback,formalPairs:[{source:source.parameters[1],native:nativeOuter.parameters[0],actual:actualCallback.parameters[0]}],ranges};
for(const[v,range]of[[native,ranges[0]],[actual,ranges[1]]])v.cover(...range);signature.verify(ts,native,nm,actual,am,cap);const rejected=[];
function reject(label,value,left=native,nativeMachine=nm,right=actual,actualMachine=am){assert.throws(()=>signature.verify(ts,left,nativeMachine,right,actualMachine,value),undefined,label);rejected.push(label);}
reject('method cannot claim field-arrow kind',{...cap,sourceKind:'class-field-async-arrow'});
for(const[label,text]of[['class method ancestry','class Other{async read(extra:number){return extra;}}'],['non-async object method','const object={read(extra:number){return extra;}};'],['default runtime formal','const object={async read(extra:number=1){return extra;}};'],['rest runtime formal','const object={async read(...extra:number[]){return extra;}};']]){
  const replacement=parse(text,'other.ts'),method=find(replacement,ts.isMethodDeclaration)[0],parameters=method.parameters.filter(p=>p.name.getText(replacement)!=='this'),changed={...cap,source:method,formalPairs:[{...cap.formalPairs[0],source:parameters[0]}]};reject(label,changed,native,{...nm,originals:[replacement.text]},actual,{...am,originals:[replacement.text]});
}
reject('erased this is not a runtime formal',{...cap,formalPairs:[{...cap.formalPairs[0],source:source.parameters[0]}]});
reject('outer callable cannot be its callback',{...cap,actualOuter:actualCallback});
reject('Native callback cannot be an arbitrary sibling',{...cap,nativeCallback:find(nativeSource,ts.isFunctionExpression)[1]});
reject('partial method callback header',{...cap,ranges:[ranges[0],[ranges[1][0]+1,ranges[1][1]]]});
reject('source must belong to both independent machines',cap,native,nm,actual,{...am,originals:[]});
const unproved={...actual,covered:offset=>offset===ranges[1][0]+1?false:actual.covered(offset)};reject('method header interior must already be proved',cap,native,nm,unproved,{...am,view:unproved});
console.log(JSON.stringify({scope:'relocated signature capability API unit',positive:1,rejected:rejected.length,controls:rejected},null,2));
