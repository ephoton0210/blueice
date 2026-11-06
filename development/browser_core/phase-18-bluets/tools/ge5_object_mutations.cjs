// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Isolated source interiors, statement gaps, helper ABIs and module identities.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{compare}=require('./ge5_object_roles.cjs');
function run(ts,native,actual,options,original,proof) {
  const controls=[];
  function mapping(label,offset,owner){const changed=mutate(ts,actual,offset,owner);assert.throws(()=>compare(ts,native,changed,options,original),undefined,label);controls.push({label,rejected:true});}
  function code(label,start,end,text){assert.equal(text.length,end-start);const changed=stream(ts,actual.code.slice(0,start)+text+actual.code.slice(end),actual.raw);assert.throws(()=>compare(ts,native,changed,options,original),undefined,label);controls.push({label,rejected:true});}
  for(const node of proof.actual.user) {
    const tokens=actual.tokens(node),binding=tokens.find(token=>['source','a','copy','result'].includes(token.text)&&token.owners.some(Boolean));assert(binding);
    const offset=binding.start+Math.floor((binding.end-binding.start)/2),owner=actual.owner(offset);mapping('erase binding interior '+binding.text,offset,null);mapping('retarget binding interior '+binding.text,offset,JSON.stringify(owner)==='[0,0,0]'?[0,0,1]:[0,0,0]);
    for(const token of tokens)if(/^\d+$/.test(token.text)&&token.owners.some(Boolean)){mapping('erase original numeric operand',token.start,null);code('change source value',token.start,token.end,(token.text[0]==='9'?'8':'9')+token.text.slice(1));}
    const terminator=tokens.findLast(token=>token.text===';'&&token.owners.some(Boolean));assert(terminator);mapping('erase statement terminator',terminator.start,null);
    const spread=tokens.find(token=>token.text==='...'&&token.owners.some(Boolean));if(spread)mapping('erase spread/rest token interior',spread.start+1,null);
    const gap=tokens.slice(1).map((token,i)=>({from:tokens[i].end,to:token.start})).find(gap=>gap.from<gap.to&&actual.owner(gap.from));if(gap)mapping('retarget ordered statement gap',gap.from,JSON.stringify(actual.owner(gap.from))==='[0,0,0]'?[0,0,1]:[0,0,0]);
  }
  for(const node of proof.actual.generated)mapping('grant generated helper/header/import source',node.getStart(actual.source)+1,[0,0,0]);
  for(const token of proof.actual.wrapperTokens.slice(0,1))mapping('grant module envelope source',token.start,[0,0,0]);
  mapping('grant trailing map comment source',actual.code.lastIndexOf('//# sourceMappingURL=')+4,[0,0,0]);
  const astral=actual.code.indexOf('/* G-E5');if(astral>=0)mapping('erase original astral comment interior',astral+9,null);
  for(const [label,key,value]of [['wrong metadata file','file','other.js'],['unexpected name metadata','names',['wrong']]]){assert.throws(()=>compare(ts,native,stream(ts,actual.code,JSON.stringify({...JSON.parse(actual.raw),[key]:value})),options,original),undefined,label);controls.push({label,rejected:true});}
  const first=actual.tokens(proof.actual.user[0]).find(token=>token.text==='source');assert(first);code('change source object identity',first.start,first.end,'forget');
  for(const node of proof.actual.user){function visit(node){if(ts.isCallExpression(node)){const leaf=ts.isPropertyAccessExpression(node.expression)?node.expression.name:node.expression;if(ts.isIdentifier(leaf)){mapping('erase helper reference interior',leaf.getStart(actual.source)+1,null);for(const operand of node.arguments)if(ts.isIdentifier(operand)&&operand.text==='source')mapping('erase copied source operand interior',operand.getStart(actual.source)+1,null);const text=leaf.text;code('change invocation identity',leaf.getStart(actual.source),leaf.end,(text[0]==='X'?'Y':'X')+text.slice(1));}}if(ts.isCallExpression(node)&&proof.actual.helpers.has(node.expression.getText())){const rest=proof.actual.helpers.get(node.expression.getText())==='rest',flag=node.arguments[rest?2:2];assert(flag);mapping('grant generated copy/rest flag source',flag.getStart(actual.source),[0,0,0]);if(rest)mapping('grant generated rest target source',node.arguments[0].getStart(actual.source),[0,0,0]);code('change copy/rest value policy',flag.getStart(actual.source),flag.end,flag.kind===ts.SyntaxKind.TrueKeyword?'null':'true ');}ts.forEachChild(node,visit);}visit(node);}
  const helper=proof.actual.generated.find(node=>ts.isFunctionDeclaration(node));if(helper){const token=actual.tokens(helper).find(token=>token.text==='getOwnPropertyNames');assert(token);code('change helper intrinsic',token.start,token.end,'getOwnPropertyNamet');}
  if(proof.actual.dependencies.length){const token=actual.tokens(actual.source).find(token=>/^['"]tslib['"]$/.test(token.text));assert(token);code('change dependency identity',token.start+1,token.end-1,'wrong');}
  return controls;
}
module.exports={run};
