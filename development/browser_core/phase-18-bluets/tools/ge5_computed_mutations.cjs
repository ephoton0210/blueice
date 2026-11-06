// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Challenge the saved object/key/value, their phases and all original roles.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{compare}=require('./ge5_linear_roles.cjs');
function run(ts,native,actual,originals,proof) {
  const trace=proof.actual,store=trace.events.find(e=>e.kind==='store'),write=trace.events.find(e=>e.kind==='write'),returned=trace.events.find(e=>e.kind==='return'),yields=trace.events.filter(e=>e.kind==='yield'),controls=[];
  assert(store&&write&&returned&&yields.length===2);assert.equal(trace.reads.length,2);
  const start=node=>node.getStart(actual.source);
  function mapping(label,offset,owner){const changed=mutate(ts,actual,offset,owner);assert.throws(()=>compare(ts,native,changed,originals,{computed:true}),undefined,label);controls.push({label,rejected:true});}
  function code(label,from,to,replacement){assert.equal(replacement.length,to-from);const changed=stream(ts,actual.code.slice(0,from)+replacement+actual.code.slice(to),actual.raw);assert.throws(()=>compare(ts,native,changed,originals,{computed:true}),undefined,label);controls.push({label,rejected:true});}
  mapping('erase array initializer binding interior',start(store.left)+1,null);
  mapping('retarget first array literal',start(store.value.elements[0].node),[0,0,0]);
  mapping('erase array initializer statement end',store.statement.end,null);
  for(const[index,event]of yields.entries()){mapping('erase yield operand '+index,start(event.value.node),null);mapping('retarget yield opcode '+index,start(event.statement),[0,0,0]);}
  const resumes=trace.values.filter(v=>v.kind==='sent');assert.equal(resumes.length,2);
  for(const[index,value]of resumes.entries())mapping('erase resume token interior '+index,start(value.node)+1,null);
  const groups=trace.values.filter(v=>v.kind==='group');assert.equal(groups.length,2);
  for(const[index,value]of groups.entries()){mapping('erase original grouping open '+index,start(value.node),null);mapping('erase original grouping close '+index,value.node.end-1,null);}
  mapping('erase captured object reference interior',start(write.left.expression)+1,null);
  mapping('retarget captured key reference interior',start(write.left.argumentExpression)+1,[0,0,0]);
  mapping('erase computed reference open',write.left.expression.end,null);
  mapping('retarget computed reference close',write.left.end-1,[0,0,0]);
  mapping('erase computed write operation',start(write.assignment.operatorToken),null);
  mapping('erase carried right operand interior',start(write.right)+1,null);
  mapping('retarget computed write terminator',write.statement.end-1,[0,0,0]);
  mapping('erase computed write statement end',write.statement.end,null);
  mapping('erase return array binding interior',start(returned.value.object.node)+1,null);
  mapping('retarget return index',start(returned.value.key.node),[0,0,0]);
  mapping('grant generated trailing comment source',actual.code.lastIndexOf('//# sourceMappingURL=')+4,[0,0,0]);
  const helper=actual.source.statements.find(n=>ts.isFunctionDeclaration(n)&&/^__blueice_target_generator_\d+$/.test(n.name?.text));assert(helper);mapping('grant generator helper source',start(helper.name)+1,[0,0,0]);
  const machine=trace.machine;mapping('grant continuation dispatch source',start(machine.dispatcher)+1,[0,0,0]);
  code('replace first array value',start(store.value.elements[0].node),store.value.elements[0].node.end,'1');
  code('replace first yielded value',start(yields[0].value.node),yields[0].value.node.end,'2');
  code('replace final array index',start(returned.value.key.node),returned.value.key.node.end,'0');
  code('replace captured key with right operand',start(write.left.argumentExpression),write.left.argumentExpression.end,write.right.getText());
  code('replace right operand with captured key',start(write.right),write.right.end,write.left.argumentExpression.getText());
  code('reevaluate the user array after suspensions',start(write.left.expression),write.left.expression.end,'values'.padEnd(write.left.expression.end-start(write.left.expression),' '));
  code('remove computed write semantics',start(write.assignment.operatorToken),write.assignment.operatorToken.end,'+');
  const resume=resumes[0].node;code('skip the first resume',start(resume),resume.end,'0'.padEnd(resume.end-start(resume),' '));
  assert.equal(controls.length,34);return controls;
}
module.exports={run};
