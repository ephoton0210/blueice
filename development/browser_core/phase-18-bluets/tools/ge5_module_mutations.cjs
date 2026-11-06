// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Keep every module configuration while challenging its complete role contract.
const assert=require('node:assert/strict');
const {stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs');
const {compare}=require('./ge5_optional_roles.cjs'),expression=require('./ge5_optional_expression.cjs');
function run(ts,native,actual,options,originals,proof) {
  const body=proof.actual,binding=proof.binding,controls=[];
  const addition=binding.expression;assert(ts.isBinaryExpression(addition)&&addition.operatorToken.getText()==='+');
  const grouped=addition.left;assert(ts.isParenthesizedExpression(grouped));
  const lowered=options.target!=='ES2022',normalized=lowered?expression.read(ts,body,grouped.expression,true):null;
  let receiver,key,fallback;
  if(lowered)({receiver,key}=normalized.optional),fallback=normalized.fallback;
  else {
    const coalesce=grouped.expression;assert(ts.isBinaryExpression(coalesce)&&coalesce.operatorToken.getText()==='??');
    const member=coalesce.left;assert(ts.isPropertyAccessExpression(member)&&member.questionDotToken);
    receiver=member.expression;key=member.name;fallback=coalesce.right;
  }
  const object=body.user[1],getter=actual.tokens(object),returnToken=getter.find(token=>token.text==='return');
  assert(returnToken);const literal=getter.find(token=>token.text==='41'),operation=getter.find(token=>token.text==='+='),getterName=getter.find(token=>token.text==='answer');assert(literal&&operation&&getterName);
  function mapping(label,offset,owner) {
    const changed=mutate(ts,actual,offset,owner);
    assert.throws(()=>compare(ts,native,changed,options,originals),undefined,label);controls.push({label,rejected:true});
  }
  function code(label,start,end,replacement) {
    assert.equal(replacement.length,end-start);const changed=stream(ts,actual.code.slice(0,start)+replacement+actual.code.slice(end),actual.raw);
    assert.throws(()=>compare(ts,native,changed,options,originals),undefined,label);controls.push({label,rejected:true});
  }
  mapping('erase getter return interior',returnToken.start+1,null);
  mapping('retarget original getter key interior',getterName.start+1,[0,0,0]);
  mapping('erase original coalesce grouping open',grouped.getStart(actual.source),null);
  mapping('erase original coalesce grouping close',grouped.end-1,null);
  mapping('retarget original addition',addition.operatorToken.getStart(actual.source),[0,0,0]);
  mapping('erase original reads interior',addition.right.getStart(actual.source)+1,null);
  mapping('erase published binding interior',binding.name.getStart(actual.source)+1,null);
  mapping('retarget nullish fallback',fallback.getStart(actual.source),[0,0,0]);
  mapping('retarget optional receiver interior',receiver.getStart(actual.source)+1,[0,0,0]);
  mapping('retarget published statement terminator',binding.statement.end-1,[0,0,0]);
  mapping('erase published statement endpoint',binding.statement.end,null);
  mapping('grant generated sourceMappingURL source',actual.code.lastIndexOf('//# sourceMappingURL=')+4,[0,0,0]);
  code('replace original getter update',operation.start,operation.end,'-=');
  code('replace original getter result',literal.start,literal.end,'42');
  code('replace original reads binding',addition.right.getStart(actual.source),addition.right.end,'value');
  code('replace nullish fallback value',fallback.getStart(actual.source),fallback.end,'1');
  const keyStart=key.getStart(actual.source)+Number(ts.isStringLiteral(key));
  code('replace optional property name',keyStart,keyStart+6,'foobar');
  code('replace published binding',binding.name.getStart(actual.source),binding.name.end,'forget');
  const strictPlain=body.format==='plain'&&body.generated.some(node=>ts.isExpressionStatement(node)&&ts.isStringLiteral(node.expression));
  if(body.format!=='plain'||strictPlain) {
    mapping('grant generated module header source',actual.source.statements[0].getStart(actual.source)+1,[0,0,0]);
    if(body.format==='wrapped') {
      const dependency=actual.tokens(actual.source).find(token=>token.text==='"require"');assert(dependency);
      code('replace wrapper dependency',dependency.start+1,dependency.end-1,'missing');
    } else if(body.format==='system') {
      const registration=actual.source.statements[0].expression.expression.name;
      code('replace System registration operation',registration.getStart(actual.source),registration.end,'registex');
      const publication=body.generated.find(node=>ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)&&node.expression.expression.getText()===body.system.stem+'_slot');assert(publication);
      const exported=publication.expression.arguments[0];code('replace System publication slot',exported.getStart(actual.source)+1,exported.end-1,'forget');
    } else {
      const marker=actual.tokens(actual.source).find(token=>token.text==='"__esModule"');assert(marker);
      code('replace CommonJS module marker',marker.start+1,marker.end-1,'__esModulf');
    }
  }
  if(lowered) {
    const helper=[...body.helpers.keys()][0];
    const declaration=helper?body.generated.find(node=>ts.isFunctionDeclaration(node)&&node.name?.text===helper):body.generated.find(node=>ts.isVariableStatement(node)&&node.declarationList.declarations.some(d=>d.name.getText()===normalized.slotName));assert(declaration);
    mapping('grant generated helper/slot declaration source',declaration.getStart(actual.source)+1,[0,0,0]);
    mapping('erase captured coalesce binding interior',normalized.capture.left.getStart(actual.source)+1,null);
    mapping('retarget nullish predicate',normalized.conditional.condition.getStart(actual.source),[0,0,0]);
    let predicate;
    if(helper) {
      const nullish=[...body.helpers].find(([,kind])=>kind==='nullish')[0],fn=body.generated.find(node=>ts.isFunctionDeclaration(node)&&node.name?.text===nullish);predicate=fn.body.statements[0].expression;
    } else predicate=normalized.conditional.condition;
    assert(ts.isBinaryExpression(predicate)&&predicate.operatorToken.getText()==='||');
    code('invert the strict nullish predicate',predicate.operatorToken.getStart(actual.source),predicate.operatorToken.end,'&&');
    const slot=normalized.conditional.whenFalse,wrong=slot.getText().slice(0,-1)+'9';assert(!body.slots.has(wrong));
    code('read a missing captured slot',slot.getStart(actual.source),slot.end,wrong);
  }
  const expected=(lowered?23:18)+(body.format==='system'?3:body.format==='wrapped'||strictPlain?2:0);
  assert.equal(controls.length,expected);
  return controls;
}
module.exports={run};
