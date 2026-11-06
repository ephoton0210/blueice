// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Single bounded mutations challenge original leaves, ABI and helper transport.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{compare}=require('./ge5_spread_roles.cjs');
function run(ts,native,actual,options,originals,proof) {
  const item=proof.actual,binding=proof.binding,copied=proof.copiedBindings.actual,controls=[];
  const tokens=actual.tokens(item.user[0]),fn=item.user[0],returned=tokens.find(t=>t.text==='return'),bodyPlus=tokens.find(t=>t.text==='+');
  const values=proof.values.actual.expression,addition=binding.expression;assert(ts.isBinaryExpression(addition));
  const lowered=options.target==='ES5';
  const copyOperand=lowered?proof.copied.actual.leaf:copied.expression.elements[0].expression;
  const callee=lowered?proof.call.callee:addition.left.expression;
  const argument=lowered?proof.call.argument:addition.left.arguments[0].expression;
  function mapping(label,offset,owner) {const changed=mutate(ts,actual,offset,owner);assert.throws(()=>compare(ts,native,changed,options,originals),undefined,label);controls.push({label,rejected:true});}
  function code(label,from,to,replacement) {assert.equal(replacement.length,to-from);const changed=stream(ts,actual.code.slice(0,from)+replacement+actual.code.slice(to),actual.raw);assert.throws(()=>compare(ts,native,changed,options,originals),undefined,label);controls.push({label,rejected:true});}
  const start=node=>node.getStart(actual.source);
  mapping('erase original function name interior',start(fn.name)+1,null);
  mapping('erase original return interior',returned.start+1,null);
  mapping('retarget original function addition',bodyPlus.start,[0,0,0]);
  mapping('erase function endpoint',fn.end,null);
  mapping('retarget original array item',start(values.elements[0]),[0,0,0]);
  mapping('erase copied binding interior',start(copied.name)+1,null);
  mapping('erase copied array operand interior',start(copyOperand)+1,null);
  mapping('erase spread callee interior',start(callee)+1,null);
  mapping('retarget spread argument interior',start(argument)+1,[0,0,0]);
  mapping('erase result binding interior',start(binding.name)+1,null);
  mapping('retarget final addition',start(addition.operatorToken),[0,0,0]);
  mapping('erase copied length interior',start(addition.right.name)+1,null);
  mapping('retarget copied statement terminator',copied.statement.end-1,[0,0,0]);
  mapping('erase copied statement endpoint',copied.statement.end,null);
  mapping('retarget result statement terminator',binding.statement.end-1,[0,0,0]);
  mapping('erase result statement endpoint',binding.statement.end,null);
  mapping('grant sourceMappingURL user source',actual.code.lastIndexOf('//# sourceMappingURL=')+4,[0,0,0]);
  code('replace function operation',bodyPlus.start,bodyPlus.end,'-');
  code('replace array item',start(values.elements[0]),values.elements[0].end,'21');
  code('replace copied source',start(copyOperand),copyOperand.end,'copied');
  code('replace called function',start(callee),callee.end,'other');
  code('replace spread argument',start(argument),argument.end,'copied');
  code('replace published binding',start(binding.name),binding.name.end,'forget');
  const strictPlain=item.format==='plain'&&item.generated.some(node=>ts.isExpressionStatement(node)&&ts.isStringLiteral(node.expression));
  if(item.format!=='plain'||strictPlain) {
    mapping('grant generated module header user source',actual.source.statements[0].getStart(actual.source)+1,[0,0,0]);
    if(item.format==='wrapped') {
      const dep=actual.tokens(actual.source).find(token=>token.text==='"require"');assert(dep);code('replace wrapper dependency',dep.start+1,dep.end-1,'missing');
    } else if(item.format==='system') {
      const registration=actual.source.statements[0].expression.expression.name;code('replace System operation',start(registration),registration.end,'registex');
      const slot=item.generated.find(node=>ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)&&node.expression.expression.getText()===item.system.stem+'_slot').expression.arguments[0];code('replace System slot',start(slot)+1,slot.end-1,'forget');
    } else {const marker=actual.tokens(actual.source).find(token=>token.text==='"__esModule"');assert(marker);code('replace CommonJS marker',marker.start+1,marker.end-1,'__esModulf');}
  }
  if(lowered) {
    mapping('retarget array helper prefix',start(proof.copied.actual.root),[0,0,0]);
    mapping('erase array helper suffix',proof.copied.actual.root.end-1,null);
    mapping('retarget receiver argument transport',proof.call.callee.end+1,[0,0,0]);
    const flags=proof.copied.actual.root.arguments,pack=flags[2];code('disable sparse packing',start(pack),pack.end,'null');
    if(proof.call.functionTransport) {
      mapping('grant inline helper user source',start(proof.call.functionTransport)+2,[0,0,0]);
      const helperTokens=actual.tokens(proof.call.functionTransport),apply=helperTokens.find(token=>token.text==='apply');assert(apply);code('replace invocation intrinsic',apply.start,apply.end,'other');
    } else {
      const helper=item.generated.find(node=>ts.isFunctionDeclaration(node)&&item.helpers.get(node.name?.text)==='array');assert(helper);mapping('grant helper definition user source',start(helper.name)+1,[0,0,0]);
      const flag=flags[3];code('enable unrequested iteration',start(flag),flag.end,'true ');
    }
  }
  if(item.dependency.length) {
    const source=actual.tokens(actual.source).find(token=>/^['"]tslib['"]$/.test(token.text));assert(source);code('replace helper dependency',source.start+1,source.end-1,'wrong');
    mapping('grant helper import source',source.start+1,[0,0,0]);
  }
  assert.equal(controls.length,23+(item.format==='system'?3:item.format==='wrapped'||strictPlain?2:0)+(lowered?6:0)+(item.dependency.length?2:0));
  return controls;
}
module.exports={run};
