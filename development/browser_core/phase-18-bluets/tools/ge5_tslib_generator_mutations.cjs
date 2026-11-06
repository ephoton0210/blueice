// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Challenge adapter slots, unused exits, state synchronization and helper identity.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{compare}=require('./ge5_iteration_generators.cjs');
function run(ts,native,actual,options,original,proof) {
  const machine=proof.continuation?.machines.actual,adapter=machine?.adapter;if(!adapter)return [];
  const controls=[],start=node=>node.getStart(actual.source);
  function code(label,from,to,text){assert.equal(text.length,to-from);const changed=stream(ts,actual.code.slice(0,from)+text+actual.code.slice(to),actual.raw);assert.throws(()=>compare(ts,native,changed,options,original),undefined,label);controls.push({label,rejected:true});}
  function mapping(label,offset,owner){const changed=mutate(ts,actual,offset,owner);assert.throws(()=>compare(ts,native,changed,options,original),undefined,label);controls.push({label,rejected:true});}
  const numeric=(node,text)=>{const found=[];function visit(node){if(ts.isNumericLiteral(node)&&node.text===text)found.push(node);ts.forEachChild(node,visit);}visit(node);return found;};
  const counter=numeric(machine.callback,String(adapter.count))[0];assert(counter);code('wrong adapter state count',start(counter),counter.end,String(adapter.count+1));
  const entry=numeric(machine.callback,String(machine.entry))[0];assert(entry);code('wrong initial source state',start(entry),entry.end,String(machine.entry-1));
  for(const node of adapter.sync){const value=node.expression.right,text=value.text,replacement=(text[0]==='9'?'8':'9')+text.slice(1);code('wrong native label synchronization',start(value),value.end,replacement);mapping('grant native synchronization source',start(node)+1,[0,0,0]);}
  for(const property of adapter.table.properties){const value=property.initializer;code('wrong normalized exit',start(value),value.end,String(Number(value.text)-1));}
  const pending=actual.tokens(proof.actual.fn).find(token=>token.text==='false');assert(pending);code('unexpected pending yield',pending.start,pending.end,'true ');
  const sent=actual.tokens(machine.callback).find(token=>token.text==='sent');assert(sent);code('wrong native sent operation',sent.start,sent.end,'send');
  const receiver=actual.tokens(machine.callback).find((token,index,tokens)=>token.text==='this'&&tokens[index-1]?.text==='(');assert(receiver);code('wrong action receiver',receiver.start,receiver.end,'null');
  const translated=[...machine.callback.body.statements].at(-1);assert(ts.isSwitchStatement(translated));const yieldCase=translated.caseBlock.clauses.find(clause=>ts.isCaseClause(clause)&&clause.expression.text==='yield');assert(yieldCase);const opcode=yieldCase.statements.at(-1).expression.elements[0];code('wrong external yield opcode',start(opcode),opcode.end,'2');
  const push=actual.tokens(translated).find(token=>token.text==='push');assert(push);code('wrong exception registration intrinsic',push.start,push.end,'pash');
  const helper=ts.isPropertyAccessExpression(machine.call.expression)?machine.call.expression.name:machine.call.expression;code('wrong imported generator member',start(helper),helper.end,helper.text.slice(0,-1)+'x');
  const resumed=adapter.sourceStatements.filter(node=>ts.isExpressionStatement(node)&&ts.isCallExpression(node.expression)&&node.expression.expression.getText()===machine.context+'.sent');assert(resumed.length);
  for(const node of resumed){const text=actual.code.slice(start(node),node.end).replace(/[^\r\n]/g,' ');code('erase source resume',start(node),node.end,text);const token=actual.tokens(node).find(token=>token.text==='sent');assert(token);mapping('erase source resume interior',token.start+1,null);}
  for(const statement of adapter.sourceStatements){const literals=numeric(statement,'20').concat(numeric(statement,'21')).concat(numeric(statement,'100'));for(const literal of literals)if(actual.owner(start(literal)))mapping('erase yield operand interior',start(literal),null);}
  const comment=actual.code.indexOf('// Original per-function');assert(comment>=0);mapping('grant adapter comment source',comment+4,[0,0,0]);
  const slot=[...machine.slots][0],reads=[];for(const statement of adapter.sourceStatements){function visit(node){if(ts.isIdentifier(node)&&node.text===slot&&!(ts.isBinaryExpression(node.parent)&&node.parent.left===node))reads.push(node);ts.forEachChild(node,visit);}visit(statement);}const read=reads[0];assert(read);code('wrong carried slot read',start(read),read.end,slot.slice(0,-1)+(slot.at(-1)==='9'?'8':'9'));
  mapping('grant generated callback context source',start(machine.callback.parameters[0].name)+1,[0,0,0]);
  assert(controls.every(control=>control.rejected));return controls;
}
module.exports={run};
