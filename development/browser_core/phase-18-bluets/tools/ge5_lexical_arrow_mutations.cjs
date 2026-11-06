// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every mutation keeps inline publication coherent before challenging a role.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{coherent}=require('./ge5_async_arrow_guard_mutations.cjs');
function run(ts,native,actual,options,original,proof,compare){const controls=[],item=proof.items.actual,start=node=>node.getStart(actual.source),check=changed=>compare(ts,native,coherent(ts,changed),options,original);
  function mapping(label,offset,generated=false){if(process.env.BLUEICE_GE5_ROLE_PROGRESS)console.error(label);assert.equal(actual.owner(offset)===null,generated,label+' precondition');assert.throws(()=>check(mutate(ts,actual,offset,generated?[0,0,0]:null)),undefined,label);controls.push({label,rejected:true,publicationCoherent:true});}
  function code(label,node,text){if(process.env.BLUEICE_GE5_ROLE_PROGRESS)console.error(label);const from=start(node);assert(text.length<=node.end-from);const changed=stream(ts,actual.code.slice(0,from)+text.padEnd(node.end-from,' ')+actual.code.slice(node.end),actual.raw);assert.throws(()=>check(changed),undefined,label);controls.push({label,rejected:true,publicationCoherent:true});}
  for(const[index,token]of actual.tokens(item.awaitCall).entries()){mapping('erase awaited-call token '+index,token.start);if(token.end-token.start>1)mapping('erase awaited-call UTF-16 interior '+index,token.start+1);}
  for(const[index,token]of actual.tokens(item.property).entries()){mapping('erase receiver/property token '+index,token.start);if(token.end-token.start>1)mapping('erase receiver/property UTF-16 interior '+index,token.start+1);}
  const parameter=item.retained?item.fn.parameters[0]:item.callback.parameters[0];mapping('erase relocated formal interior',start(parameter.name)+1);
  for(const[label,node]of[['complete field initializer',item.expression],...item.retained?[]:[['promise driver',item.driver],['callback',item.callback]]])if(actual.owner(node.end)!==null)mapping('erase '+label+' logical end',node.end);
  mapping('grant map publication comment source',actual.code.indexOf('//# sourceMappingURL=')+4,true);
  code('replace awaited operation',item.awaitCall.expression.name,'reject');code('replace awaited argument',item.awaitCall.arguments[0],'0');code('replace source property',item.property.name,'other');code('replace source formal',parameter.name,'other');
  if(!item.retained){
    mapping('grant receiver factory parameter source',start(item.capture.receiver)+1,true);mapping('grant callable transport parameter source',start(item.entryFunction.parameters[0].name)+1,true);
    code('replace lexical receiver with call arguments',item.property.expression,'arguments');code('replace receiver factory input',item.capture.invocation.arguments[0],'null');code('replace lexical arguments factory input',item.capture.invocation.arguments[1],'null');code('replace driver call receiver',item.driver.arguments[0],'null');code('replace forwarded call arguments',item.driver.arguments[1],'null');
    for(const[label,offset]of[['driver entry',start(item.driver)],['callback body entry',start(item.callback.body)]])mapping('erase '+label,offset);
    const helper=actual.source.statements.find(node=>ts.isFunctionDeclaration(node)&&node.name?.text===item.driver.expression.getText(actual.source));assert(helper);mapping('grant promise helper binding source',start(helper.name)+1,true);
    const operations=[];function visit(node){if(ts.isStringLiteral(node)&&['next','throw'].includes(node.text))operations.push(node);ts.forEachChild(node,visit);}visit(helper);assert(operations.length);code('replace helper resume operation',operations[0],'"x"');
    if(options.target==='ES5'){
      const nm=proof.expression.core.native.machine,am=proof.expression.core.actual.machine,group=am.lexicalReceiver.conciseResultGroup;
      mapping('erase concise-result grouping entry',start(group));mapping('erase concise-result grouping completion',group.end-1);
      code('skip receiver evaluation state',am.call.arguments[1],'0');
      const originalState=[...am.states.entries()].find(([,statements])=>statements.some(statement=>statement.getText(actual.source).includes(item.property.getText(actual.source))));assert(originalState);const clause=am.dispatcher.caseBlock.clauses.find(clause=>clause.expression?.text===String(originalState[0]));code('erase receiver evaluation state',clause,'');
      controls.push(...require('./ge5_relocated_signature_mutations.cjs').run(ts,native,nm,actual,am,proof.expression.signature));
    }
  }
  controls.push(...require('./ge5_async_arrow_publication_mutations.cjs').run(ts,native,actual,options,original,check));
  return controls;
}
module.exports={run};
