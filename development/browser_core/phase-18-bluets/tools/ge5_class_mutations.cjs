// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Isolated bounded attacks must fail against the same complete Native contract.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs');
const {coherent}=require('./ge5_decorator_preserved_mutations.cjs');
function run(ts,native,actual,options,original,proof,compareContract) {
  assert.equal(typeof compareContract,'function','complete Native contract supplied to each mutation');
  const compare=compareContract,controls=[];
  compare(ts,stream(ts,native.code,native.raw),stream(ts,actual.code,actual.raw),options,original);
  const lines=original.split('\n'),wrong=[0,lines.length-1,lines.at(-1).length];
  function mapping(label,position,owner) {
    assert.notDeepEqual(actual.owner(position),owner,'effective isolated map mutation');
    assert.throws(()=>compare(ts,stream(ts,native.code,native.raw),coherent(ts,mutate(ts,actual,position,owner)),options,original),undefined,label);controls.push({label,rejected:true});
  }
  function code(label,start,end,replacement) {
    assert.equal(replacement.length,end-start,'bounded fixed-width code mutation');
    const source=actual.code.slice(0,start)+replacement+actual.code.slice(end),changed=stream(ts,source,actual.raw);
    assert.deepEqual(changed.source.parseDiagnostics.map(diagnostic=>diagnostic.code),[],'isolated code control remains valid JavaScript');
    assert.throws(()=>compare(ts,stream(ts,native.code,native.raw),changed,options,original),undefined,label);controls.push({label,rejected:true});
  }
  for(const [index,item]of proof.classes.entries()) {
    const endpoint=item.actual.node.end;
    mapping('retarget exact original class construction endpoint '+index,endpoint,wrong);
    if(actual.owner(endpoint)!==null)mapping('erase exact original class construction endpoint '+index,endpoint,null);
    const node=item.actual.native?item.actual.node:item.actual.constructor;
    const binding=node.name;assert(binding);
    const first=binding.getStart(actual.source),token=actual.token(binding);
    if(actual.owner(first)!==null)mapping('erase class '+index+' binding interior',first+Math.min(1,token.end-token.start-1),null);
    const name=binding.getText(actual.source);code('replace class '+index+' binding',first,first+1,name[0]==='Z'?'Y':'Z');
    const body=item.actual.native?item.actual.node:node.body;
    const originalToken=actual.tokens(node).find(token=>token.text.length>2&&token.owners[1]!==null&&token.text!==name);
    assert(originalToken,'class has a retained mapped body token');
    mapping('retarget class '+index+' complete token interior',originalToken.start+1,wrong);
    const challenged=new Set([endpoint]);
    for(const pair of item.declarationChain??[]) {
      const offset=pair.actual.end;if(challenged.has(offset))continue;challenged.add(offset);
      const target=JSON.stringify(actual.owner(offset))===JSON.stringify(wrong)?[0,0,0]:wrong;
      mapping('retarget complete declaration '+index+' '+pair.label,offset,target);
      if(actual.owner(offset)!==null)mapping('erase complete declaration '+index+' '+pair.label,offset,null);
    }
  }
  for(const [index,item]of proof.classes.entries())if(item.rest) {
    const {array,loop,copy}=item.rest.actual;
    mapping('erase class rest array '+index,array.getStart(actual.source)+1,null);
    code('invert complete constructor rest bound '+index,loop.condition.operatorToken.getStart(actual.source),loop.condition.operatorToken.end,'>');
    code('reverse constructor rest advance '+index,loop.incrementor.end-2,loop.incrementor.end,'--');
    const source=copy.expression.right.expression;code('replace constructor rest source '+index,source.getStart(actual.source),source.end,'argumentX');
  }
  for(const [index,item]of (proof.constructions??[]).entries()) {
    const callee=actual.tokens(item.actualCallee).find(token=>token.text.length>1&&token.owners[1]!==null);assert(callee);
    mapping('erase complete new/spread callee '+index,callee.start+1,null);
    code('replace complete new/spread callee '+index,callee.start,callee.start+1,callee.text[0]==='Z'?'Y':'Z');
    const operand=actual.tokens(item.actualOperand).find(token=>token.text.length>1&&token.owners[1]!==null);assert(operand);
    mapping('retarget complete new/spread operand '+index,operand.start+1,wrong);
    const argument=item.actual.arguments[1],copy=ts.isParenthesizedExpression(argument)?argument.expression:argument;
    if(ts.isCallExpression(copy)&&copy.arguments.length===4)code('alter construction iterable mode '+index,copy.arguments[3].getStart(actual.source),copy.arguments[3].end,copy.arguments[3].kind===ts.SyntaxKind.TrueKeyword?'null':'true ');
  }
  const tokens=actual.tokens(actual.source);
  const gap=tokens.find((token,index)=>index+1<tokens.length&&token.end<tokens[index+1].start&&actual.owner(token.end)!==null&&JSON.stringify(actual.owner(token.end))!==JSON.stringify(wrong));
  assert(gap);mapping('retarget ordered class/module gap',gap.end,wrong);
  const mappedGap=tokens.find((token,index)=>index+1<tokens.length&&token.end<tokens[index+1].start&&!/[\r\n]/.test(actual.code.slice(token.end,tokens[index+1].start))&&actual.owner(token.end)!==null&&actual.owner(tokens[index+1].start)!==null);
  assert(mappedGap,'complete class role has a mapped single-line physical gap');mapping('erase mapped single-line class/module gap',mappedGap.end,null);
  const footer=actual.code.lastIndexOf('//# sourceMappingURL=');assert(footer>=0);mapping('grant generated map footer source',footer+4,[0,0,0]);
  const superSites=tokens.filter(token=>token.text==='super'&&token.owners[0]!==null);
  for(const [index,token]of superSites.entries())mapping('retarget retained super reference '+index,token.start+1,wrong);
  if(superSites.length)code('replace retained super receiver',superSites[0].start,superSites[0].end,'this ');
  const operation=tokens.find(token=>token.text==='+='&&token.owners[0]!==null)||tokens.find(token=>['+','==='].includes(token.text)&&token.owners[0]!==null);
  if(operation)code('replace retained class/source operation',operation.start,operation.end,operation.text==='+='?'-=':operation.text==='+'?'-':'!==');
  const helper=proof.actual.sourceNodes.find(node=>ts.isFunctionDeclaration(node)&&proof.helpers.has(node.name?.text));
  if(helper) {
    const token=actual.tokens(helper).find(token=>token.text==='apply'||token.text==='Object'||token.text==='return');assert(token);
    code('alter complete class helper ABI',token.start,token.end,token.text==='apply'?'applx':token.text==='Object'?'Xbject':'void  ');
    mapping('grant class helper definition source',helper.getStart(actual.source)+1,[0,0,0]);
  }
  return controls;
}
module.exports={run};
