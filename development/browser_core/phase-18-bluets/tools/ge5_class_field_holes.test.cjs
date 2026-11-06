// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const test=require('node:test'),assert=require('node:assert/strict'),path=require('node:path');
const root=path.resolve(__dirname,'../../../..'),ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const {stream,compareTokens}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{compareClass,readClass}=require('./ge5_class_roles.cjs'),{create}=require('./ge5_class_field_holes.cjs');
const source='class Box { value = (n: number) => n + 1; other() { return 2; } }\n';
const original=ts.createSourceFile('original.ts',source,ts.ScriptTarget.Latest,true),box=original.statements[0],field=box.members[0];
const emitted=ts.transpileModule(source,{fileName:'original.ts',compilerOptions:{target:ts.ScriptTarget.ES2022,sourceMap:true,inlineSources:true}});
function fixture() {
  const native=stream(ts,emitted.outputText,emitted.sourceMapText),actual=stream(ts,emitted.outputText,emitted.sourceMapText),nn=native.source.statements[0],an=actual.source.statements[0];
  const pair={source:field.initializer,native:nn.members[0].initializer,actual:an.members[0].initializer};
  return {native,actual,nn,an,pair,n:readClass(ts,native,nn,box),a:readClass(ts,actual,an,box)};
}
function prove(f) {compareTokens(f.native,f.native.tokens(f.pair.native),f.actual,f.actual.tokens(f.pair.actual),'complete delegated initializer');assert.deepEqual(f.actual.owner(f.pair.actual.end),f.native.owner(f.pair.native.end));}
test('typed complete initializer composes with the remaining strict class tokens',()=>{const f=fixture();prove(f);const proof=compareClass(ts,f.native,f.nn,f.actual,f.an,box,{classRoles:{pairedExpressions:[f.pair]}});assert(proof);});
test('unproved and partially covered initializer are refused',()=>{const f=fixture();assert.throws(()=>create(ts,f.native,f.actual,f.n,f.a,box,[f.pair]),/every UTF-16/);f.native.cover(f.pair.native.getStart(f.native.source),f.pair.native.end);f.actual.cover(f.pair.actual.getStart(f.actual.source),f.pair.actual.end-1);assert.throws(()=>create(ts,f.native,f.actual,f.n,f.a,box,[f.pair]),/every UTF-16/);});
test('a duplicate source field cannot delegate two ranges',()=>{const f=fixture();prove(f);assert.throws(()=>create(ts,f.native,f.actual,f.n,f.a,box,[f.pair,f.pair]),/one delegated/);});
test('source and emitted AST ancestry cannot be replaced with offsets or method expressions',()=>{const f=fixture();prove(f);assert.throws(()=>create(ts,f.native,f.actual,f.n,f.a,box,[{...f.pair,source:box.members[1].body.statements[0].expression}]),/this original class field/);assert.throws(()=>create(ts,f.native,f.actual,f.n,f.a,box,[{...f.pair,actual:f.an.members[1].body.statements[0].expression}]),/field descriptor/);assert.throws(()=>create(ts,f.native,f.actual,f.n,f.a,box,[{...f.pair,actual:{start:0,end:1}}]),/exact emitted syntax tree/);});
test('an initializer endpoint source mutation remains outside delegated authority',()=>{const f=fixture();prove(f);const changed=mutate(ts,f.actual,f.pair.actual.end,[0,0,0]),node=changed.source.statements[0],expression=node.members[0].initializer;changed.cover(expression.getStart(changed.source),expression.end);assert.throws(()=>create(ts,f.native,changed,f.n,readClass(ts,changed,node,box),box,[{...f.pair,actual:expression}]),/logical endpoint/);});
test('the surrounding field binding remains strict after valid initializer proof',()=>{const f=fixture();prove(f);const at=f.an.members[0].name.getStart(f.actual.source),code=f.actual.code.slice(0,at)+'vxlue'+f.actual.code.slice(at+5),changed=stream(ts,code,f.actual.raw),node=changed.source.statements[0],expression=node.members[0].initializer;changed.cover(expression.getStart(changed.source),expression.end);assert.throws(()=>compareClass(ts,f.native,f.nn,changed,node,box,{classRoles:{pairedExpressions:[{...f.pair,actual:expression}]}}),/field identity/);});

function receiverFixture() {
  const source='export class Box { value = 1; read = async (n: number) => this.value + n; }\n',original=ts.createSourceFile('original.ts',source,ts.ScriptTarget.Latest,true),box=original.statements[0],field=box.members[1],emitted=ts.transpileModule(source,{fileName:'original.ts',compilerOptions:{target:ts.ScriptTarget.ES5,module:ts.ModuleKind.ESNext,sourceMap:true,inlineSources:true}});
  const native=stream(ts,emitted.outputText,emitted.sourceMapText),actual=stream(ts,emitted.outputText,emitted.sourceMapText);
  const statement=view=>view.source.statements.find(node=>ts.isVariableStatement(node)&&node.declarationList.declarations.some(declaration=>declaration.name.getText(view.source)==='Box'));
  const nn=statement(native),an=statement(actual),n=readClass(ts,native,nn,box),a=readClass(ts,actual,an,box),capture=n.constructor.body.statements[0];
  const assignment=(view,constructor)=>constructor.body.statements.find(node=>ts.isExpressionStatement(node)&&ts.isBinaryExpression(node.expression)&&ts.isPropertyAccessExpression(node.expression.left)&&node.expression.left.name.text==='read').expression.right;
  const pair={source:field.initializer,native:assignment(native,n.constructor),actual:assignment(actual,a.constructor)};
  let nativeUse;const name=capture.declarationList.declarations[0].name.text;function visit(node){if(ts.isIdentifier(node)&&node.text===name)nativeUse=node;ts.forEachChild(node,visit);}visit(pair.native);
  compareTokens(native,native.tokens(pair.native),actual,actual.tokens(pair.actual),'complete actual receiver-linked initializer');
  return {native,actual,n,a,box,pair,capture,proof:{source:field.initializer,native:capture,nativeUse}};
}
test('a constructor receiver phase is tied to its original arrow and sole async receiver use',()=>{const f=receiverFixture();f.native.cover(f.capture.getStart(f.native.source),f.capture.end);const proof=create(ts,f.native,f.actual,f.n,f.a,f.box,[f.pair],f.proof);assert.equal(proof.capture,f.capture);assert(!proof.project(f.native,f.native.tokens(f.n.constructor.body)).some(token=>token.start===f.capture.getStart(f.native.source)));});
test('receiver capture refuses unproved code, a wrong statement and a use outside the initializer',()=>{const f=receiverFixture();assert.throws(()=>create(ts,f.native,f.actual,f.n,f.a,f.box,[f.pair],f.proof),/every UTF-16/);f.native.cover(f.capture.getStart(f.native.source),f.capture.end);assert.throws(()=>create(ts,f.native,f.actual,f.n,f.a,f.box,[f.pair],{...f.proof,native:f.n.constructor.body.statements[1]}),/constructor entry/);assert.throws(()=>create(ts,f.native,f.actual,f.n,f.a,f.box,[f.pair],{...f.proof,nativeUse:f.capture.declarationList.declarations[0].name}),/delegated initializer/);});

test('only exact typed initializer proofs preserve their independently covered generated comments',()=>{
  const f=receiverFixture(),comment=f.native.comments().find(comment=>f.pair.native.getStart(f.native.source)<=comment.start&&comment.end<=f.pair.native.end);assert(comment,'TypeScript async initializer contains independently mapped suspension comments');
  f.native.cover(f.capture.getStart(f.native.source),f.capture.end);
  for(const [view,node]of [[f.native,f.pair.native],[f.actual,f.pair.actual]])for(let offset=node.getStart(view.source);offset<node.end;offset++)view.cover(offset,offset+1);
  const proof=create(ts,f.native,f.actual,f.n,f.a,f.box,[f.pair],f.proof),{coversComment}=require('./ge5_class_field_holes.cjs');
  assert(coversComment(proof,f.native,comment));assert.throws(()=>coversComment({...proof},f.native,comment),/exact typed/);
  assert.throws(()=>coversComment(proof,f.native,{...comment}),/exact scanned/);
  const other=fixture();assert.throws(()=>coversComment(proof,other.native,comment),/proven initializer view/);
});
