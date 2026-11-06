// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Prove the entire default guard before composing the ordinary continuation.
const assert=require('node:assert/strict'),{compareGap}=require('./ge5_role_stream.cjs');
function read(ts,native,n,actual,a,nm,am,exact) {
  const start=(view,node)=>node.getStart(view.source),word=(view,node)=>node.getText(view.source).replace(/\s/g,''),name=n.parameter.name.getText(n.source),guard=n.defaultGuard;
  assert(ts.isIfStatement(guard)&&!guard.elseStatement&&ts.isBlock(guard.thenStatement)&&guard.thenStatement.statements.length===1);
  assert(ts.isExpressionStatement(guard.thenStatement.statements[0]));
  const assignment=guard.thenStatement.statements[0].expression;assert(ts.isBinaryExpression(assignment)&&assignment.operatorToken.kind===ts.SyntaxKind.EqualsToken);
  assert.equal(word(native,guard.expression),name+'===void0');assert.equal(word(native,assignment.left),name);assert.equal(word(native,assignment.right),n.parameter.initializer.getText(n.source));
  const entry=am.entry,predicate=am.states.get(entry);assert(predicate&&predicate.length===3);
  const expression=predicate[0].expression;assert(ts.isExpressionStatement(predicate[0])&&ts.isBinaryExpression(expression)&&expression.operatorToken.kind===ts.SyntaxKind.EqualsToken&&ts.isIdentifier(expression.left));
  const slot=expression.left.text;assert(am.slots.has(slot)&&!am.userSlots.has(slot)&&!am.identifiers.has(slot));
  exact(predicate[0],`${slot}=${name}===void 0;`);const condition=expression.right;
  function transfer(statement){assert(ts.isExpressionStatement(statement)&&ts.isBinaryExpression(statement.expression)&&statement.expression.operatorToken.kind===ts.SyntaxKind.EqualsToken&&word(actual,statement.expression.left)===am.context+'.label'&&ts.isNumericLiteral(statement.expression.right));return Number(statement.expression.right.text);}
  const branch=transfer(predicate[1]);exact(predicate[2],'continue;');const dispatch=am.states.get(branch);assert(dispatch&&dispatch.length===2);exact(dispatch[1],'continue;');
  const routing=dispatch[0].expression;assert(ts.isExpressionStatement(dispatch[0])&&ts.isBinaryExpression(routing)&&routing.operatorToken.kind===ts.SyntaxKind.EqualsToken&&word(actual,routing.left)===am.context+'.label'&&ts.isConditionalExpression(routing.right));
  const selected=routing.right;assert(ts.isIdentifier(selected.condition)&&selected.condition.text===slot&&ts.isNumericLiteral(selected.whenTrue)&&ts.isNumericLiteral(selected.whenFalse));
  const defaultState=Number(selected.whenTrue.text),core=Number(selected.whenFalse.text),defaults=am.states.get(defaultState);assert(defaults&&defaults.length===3&&am.states.has(core));
  const removed=new Set([entry,branch,defaultState]);assert.equal(removed.size,3);assert(!removed.has(core));
  exact(defaults[0],`${name}=(${n.parameter.initializer.getText(n.source)});`);assert.equal(transfer(defaults[1]),core);exact(defaults[2],'continue;');
  const references=[];function visit(node){if(ts.isIdentifier(node)&&node.text===slot)references.push(node);ts.forEachChild(node,visit);}visit(a.callback);
  const declaration=references.find(node=>ts.isVariableDeclaration(node.parent)&&node.parent.name===node);assert(declaration&&!declaration.parent.initializer);
  actual.noSource(start(actual,declaration),declaration.end,'complete generated default predicate capture declaration');
  assert.deepEqual(references.map(node=>start(actual,node)).sort((x,y)=>x-y),[declaration,expression.left,selected.condition].map(node=>start(actual,node)).sort((x,y)=>x-y),'complete predicate capture declaration/write/read inventory');
  for(const [label,statements]of am.states)if(!removed.has(label))for(const statement of statements){function audit(node){if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.EqualsToken&&word(actual,node.left)===am.context+'.label'){assert(ts.isNumericLiteral(node.right),'ordinary core has no hidden guard branch');assert(!removed.has(Number(node.right.text)),'ordinary continuation cannot re-enter a stripped guard state');}ts.forEachChild(node,audit);}audit(statement);}
  const begin=native.owner(start(native,guard)),end=native.owner(assignment.right.end);assert(begin&&end);
  function uniform(view,from,to,owner,label){for(let offset=from;offset<to;offset++)assert.deepEqual(view.owner(offset),owner,label+' at '+offset);view.cover(from,to);}
  uniform(native,start(native,guard),assignment.right.end,begin,'Native complete default predicate/assignment prefix');uniform(native,assignment.right.end,guard.end,end,'Native complete default guard ending');
  uniform(actual,start(actual,predicate[0]),predicate[0].end,begin,'complete captured default predicate');
  const defaultValue=defaults[0].expression.right.expression;assert(ts.isParenthesizedExpression(defaults[0].expression.right));
  uniform(actual,start(actual,defaults[0]),defaultValue.end,begin,'complete selected default assignment');uniform(actual,defaultValue.end,defaults[0].end,end,'complete selected default ending');
  for(const label of removed){const statements=am.states.get(label),clause=am.dispatcher.caseBlock.clauses.find(clause=>ts.isCaseClause(clause)&&Number(clause.expression.text)===label);assert(clause);let cursor=start(actual,clause);
    for(const statement of statements){const from=start(actual,statement);if(statement===predicate[1])compareGap(native,guard.expression.end,start(native,guard.thenStatement.statements[0]),actual,cursor,from,'captured default predicate/transfer boundary');else if(statement===defaults[1])compareGap(native,guard.end,nm.returned.getStart(native.source),actual,cursor,from,'selected default/transfer boundary');else actual.noSource(cursor,from,'generated default state boundary');
      if(statement!==predicate[0]&&statement!==defaults[0])actual.noSource(from,statement.end,'complete generated default routing');cursor=statement.end;}
    actual.noSource(cursor,clause.end,'generated default clause ending');for(let offset=start(actual,clause);offset<clause.end;offset++)assert(actual.covered(offset),'complete physical default guard clause');
  }
  n.defaultCondition=guard.expression;a.defaultCondition=condition;a.defaultValue=defaultValue;
  return {nativeGuard:guard,predicate:predicate[0],condition,defaultAssignment:defaults[0],defaultValue,slot,removed,entry,branch,defaultState,core,
    nativeMachine:{...nm,reviewedPrologue:Object.freeze({kind:'verified-original-default-parameter-v1',statement:guard,parameter:n.parameter,range:Object.freeze([start(native,guard),guard.end])})},actualMachine:{...am,states:new Map([...am.states].filter(([label])=>!removed.has(label))),entry:core,slots:new Set([...am.slots].filter(name=>name!==slot))}};
}
module.exports={read};
