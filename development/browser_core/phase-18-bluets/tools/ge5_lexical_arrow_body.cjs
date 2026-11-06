// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Concise async bodies retain their complete expression and generated grouping roles.
const assert=require('node:assert/strict'),{compareTokens}=require('./ge5_role_stream.cjs'),{unwrap}=require('./ge5_lexical_arrow_read.cjs');
function compare(ts,native,n,actual,a){const ns=n.callback.body.statements,as=a.callback.body.statements;assert.equal(ns.length,1);assert.equal(as.length,1);assert(ts.isReturnStatement(ns[0])&&ts.isReturnStatement(as[0]));const nr=ns[0].expression,outer=as[0].expression;assert(ts.isBinaryExpression(nr)&&ts.isParenthesizedExpression(outer)&&ts.isBinaryExpression(outer.expression));const ar=outer.expression;
  assert(nr.operatorToken.kind===ts.SyntaxKind.PlusToken&&ar.operatorToken.kind===ts.SyntaxKind.PlusToken);assert.strictEqual(nr.left,n.property);assert.strictEqual(ar.left,a.property);
  const ny=unwrap(ts,nr.right),ay=unwrap(ts,ar.right);assert(ts.isYieldExpression(ny)&&ts.isYieldExpression(ay)&&!ny.asteriskToken&&!ay.asteriskToken);assert.strictEqual(ny.expression,n.awaitCall);assert(ts.isParenthesizedExpression(ay.expression)&&ay.expression.expression===a.awaitCall);assert(ts.isParenthesizedExpression(nr.right)&&ts.isParenthesizedExpression(ar.right));
  const start=(view,node)=>node.getStart(view.source),uniform=(view,from,to,owner,label)=>{for(let offset=from;offset<to;offset++)assert.deepEqual(view.owner(offset),owner,label+' at '+offset);view.cover(from,to);};
  const nativeGroups=[nr.right],actualGroups=[outer,ar.right,ay.expression],nHoles=nativeGroups.flatMap(node=>[[start(native,node),start(native,node)+1],[node.end-1,node.end]]),aHoles=actualGroups.flatMap(node=>[[start(actual,node),start(actual,node)+1],[node.end-1,node.end]]);
  uniform(native,nHoles[0][0],nHoles[0][1],native.owner(nr.operatorToken.end),'Native generated await-group opening');uniform(native,nHoles[1][0],nHoles[1][1],native.owner(n.awaitCall.end),'Native generated await-group closing');
  uniform(actual,start(actual,outer),start(actual,outer)+1,native.owner(start(native,ns[0])),'concise return grouping entry');uniform(actual,outer.end-1,outer.end,native.owner(n.awaitCall.end),'concise return grouping completion');
  for(const node of [ar.right,ay.expression]){actual.noSource(start(actual,node),start(actual,node)+1,'generated await precedence opening');actual.noSource(node.end-1,node.end,'generated await precedence closing');}
  const owner=native.owner(start(native,n.property.expression));uniform(native,start(native,n.property.expression),n.property.expression.end,owner,'Native lexical receiver');uniform(actual,start(actual,a.property.expression),a.property.expression.end,owner,'captured lexical receiver');assert.deepEqual(actual.owner(a.property.expression.end),native.owner(n.property.expression.end));
  function tokens(view,item,holes){const receiver=item.property.expression;return view.tokens(item.callback.body).filter(token=>!holes.some(([from,to])=>from<=token.start&&token.end<=to)).map(token=>token.start===start(view,receiver)?{...token,text:'$receiver',owners:[owner]}:token);}
  compareTokens(native,tokens(native,n,nHoles),actual,tokens(actual,a,aHoles),'complete concise lexical-arrow callback body',aHoles);
  assert.deepEqual(actual.owner(a.callback.end),native.owner(n.callback.end),'complete callback logical ending');
}
module.exports={compare};
