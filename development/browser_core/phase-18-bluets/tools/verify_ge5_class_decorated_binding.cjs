// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ordinary regression for the generated System assignment preceding a legacy class factory.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),authority=require('./ge5_native_authority.cjs');
const root=path.resolve(__dirname,'../../../..'),reference=require(path.join(root,'backend/bluets/tests/fixtures/ge5_source_maps/reference.json'));
const ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const rows=JSON.parse(fs.readFileSync(0,'utf8'));assert.deepEqual(rows.map(row=>row.index),[4492,4493,4572,4573,4612,4613,4652,4653]);
function unwrap(node){while(ts.isParenthesizedExpression(node))node=node.expression;return node;}
function binding(view) {
  const candidates=[];
  function visit(node) {
    if(ts.isVariableDeclaration(node)&&ts.isIdentifier(node.name)&&node.name.text==='Box'&&node.initializer) {
      const call=unwrap(node.initializer);if(ts.isCallExpression(call)){const factory=unwrap(call.expression);if(ts.isFunctionExpression(factory)&&factory.body.statements.some(statement=>ts.isFunctionDeclaration(statement)&&statement.name?.text==='Box'))candidates.push({binding:node.name,factory});}
    }
    if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isIdentifier(node.left)&&node.left.text==='Box') {
      const call=unwrap(node.right);
      if(ts.isCallExpression(call)) {const factory=unwrap(call.expression);if(ts.isFunctionExpression(factory)&&factory.body.statements.some(statement=>ts.isFunctionDeclaration(statement)&&statement.name?.text==='Box'))candidates.push({binding:node.left,factory});}
    }
    ts.forEachChild(node,visit);
  }
  visit(view.source);assert.equal(candidates.length,1,'unique original class factory assignment');return candidates[0];
}
for(const row of rows) {
  const expected=reference.cases[row.index],native=stream(ts,reference.blobs[expected.outputs['out/main.js']],authority.raw(reference,expected,'out/main.js.map')),actual=stream(ts,row.outputs['out/main.js'],row.outputs['out/main.js.map']);
  const wanted=binding(native),owner=native.owner(wanted.binding.getStart(native.source)),system=row.index>=4652;
  assert.equal(owner===null,system,'the original transport selects only System generated class bindings');
  for(const view of[native,actual]) {
    const {binding:name,factory}=binding(view),constructor=factory.body.statements.find(ts.isFunctionDeclaration),start=name.getStart(view.source),end=constructor.getStart(view.source);
    if(system)for(let offset=start;offset<end;offset++)assert.equal(view.owner(offset),null,'System legacy binding and factory head are complete generated UTF-16 roles at '+offset);
    else for(const token of view.tokens(view.source).filter(token=>start<=token.start&&token.end<=end))for(let offset=token.start;offset<token.end;offset++)assert.deepEqual(view.owner(offset),owner,'complete retained class binding/factory token role');
  }
}
console.log(JSON.stringify({ordinaryConfigurations:rows.length,completeGeneratedBindingAndFactoryHead:true}));
