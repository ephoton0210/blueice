// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Every source token, fresh capture, gap, helper and generated comment is challenged.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{compare}=require('./ge5_binding_roles.cjs');
function run(ts,native,actual,options,original,proof) {
  const controls=[];
  function mapping(label,offset,owner) {const changed=mutate(ts,actual,offset,owner);assert.throws(()=>compare(ts,native,changed,options,original),undefined,label);controls.push({label,rejected:true});}
  const tokens=proof.actual.body.flatMap(node=>actual.tokens(node));
  for(const token of tokens)if(token.owners.some(Boolean)) {
    const offset=token.start+Math.floor((token.end-token.start)/2);mapping('erase token '+token.text+' at '+offset,offset,null);
    const owner=actual.owner(offset);mapping('retarget token '+token.text+' at '+offset,offset,JSON.stringify(owner)==='[0,0,0]'?[0,0,1]:[0,0,0]);
  }
  for(const node of proof.actual.generated)mapping('grant helper source at '+node.getStart(actual.source),node.getStart(actual.source)+1,[0,0,0]);
  mapping('grant generated trailing map comment',actual.code.lastIndexOf('//# sourceMappingURL=')+4,[0,0,0]);
  const gaps=tokens.flatMap((token,index)=>index&&tokens[index-1].end<token.start?[{start:tokens[index-1].end,end:token.start}]:[]);
  for(const gap of gaps)if(actual.owner(gap.start)!==null)mapping('retarget gap at '+gap.start,gap.start,JSON.stringify(actual.owner(gap.start))==='[0,0,0]'?[0,0,1]:[0,0,0]);
  for(const [label,key,value]of [['wrong map file','file','other.js'],['unexpected name','names',['wrong']]]) {const changed=stream(ts,actual.code,JSON.stringify({...JSON.parse(actual.raw),[key]:value}));assert.throws(()=>compare(ts,native,changed,options,original),undefined,label);controls.push({label,rejected:true});}
  const comment=actual.code.indexOf('/* G-E5');if(comment>=0)mapping('erase original astral comment interior',comment+8,null);
  const numeric=tokens.filter(token=>/^\d+$/.test(token.text)&&token.owners.some(Boolean));
  for(const token of numeric) {const changed=stream(ts,actual.code.slice(0,token.start)+(token.text[0]==='9'?'8':'9')+token.text.slice(1)+actual.code.slice(token.end),actual.raw);assert.throws(()=>compare(ts,native,changed,options,original),undefined,'wrong binding value/index/default');controls.push({label:'change binding value/index/default at '+token.start,rejected:true});}
  if(proof.actual.generated.length) {
    const helper=proof.actual.generated.find(node=>node.name?.text.includes('binding_read'));assert(helper);const token=actual.tokens(helper).find(token=>token.text==='count');assert(token);
    const changed=stream(ts,actual.code.slice(0,token.start)+'other'+actual.code.slice(token.end),actual.raw);assert.throws(()=>compare(ts,native,changed,options,original),undefined,'changed helper control');controls.push({label:'change iterator helper control',rejected:true});
  }
  if(options.target==='ES5') {
    const fn=proof.actual.body.find(node=>ts.isFunctionDeclaration(node)&&node.name?.text==='values');
    if(fn) {const targets=[];function visit(node){if(ts.isBinaryExpression(node)&&ts.isPropertyAccessExpression(node.left)&&node.left.name.text==='label'&&ts.isNumericLiteral(node.right))targets.push(node.right);ts.forEachChild(node,visit);}visit(fn);assert(targets.length);for(const target of targets){const from=target.getStart(actual.source),changed=stream(ts,actual.code.slice(0,from)+'9'.repeat(target.end-from)+actual.code.slice(target.end),actual.raw);assert.throws(()=>compare(ts,native,changed,options,original),undefined,'wrong continuation state');controls.push({label:'wrong continuation state at '+from,rejected:true});}}
  }
  return controls;
}
module.exports={run};
