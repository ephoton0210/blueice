// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Independent source interiors, iterator protocols, transport and module identities.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate}=require('./ge5_role_mutations.cjs'),{compare}=require('./ge5_iteration_roles.cjs');
function run(ts,native,actual,options,original,proof) {
  const controls=[],start=node=>node.getStart(actual.source),other=owner=>JSON.stringify(owner)==='[0,0,0]'?[0,0,1]:[0,0,0];
  function mapping(label,offset,owner){const changed=mutate(ts,actual,offset,owner);assert.throws(()=>compare(ts,native,changed,options,original),undefined,label);controls.push({label,rejected:true});}
  function code(label,from,to,text){assert.equal(text.length,to-from);const changed=stream(ts,actual.code.slice(0,from)+text+actual.code.slice(to),actual.raw);assert.throws(()=>compare(ts,native,changed,options,original),undefined,label);controls.push({label,rejected:true});}
  const chosen=new Map(),all=[];
  for(const [role,nodes]of proof.roles){const tokens=actual.tokens(nodes.actual);all.push(...tokens);const mapped=tokens.filter(token=>token.owners.some(Boolean));assert(mapped.length);for(const token of [mapped[0],mapped.at(-1)])chosen.set(token.start,{role,token});
    for(const text of ['events','first','second','total','result','values','value','of','+=','break']){const token=mapped.find(token=>token.text===text);if(token)chosen.set(token.start,{role,token});}
    const gap=tokens.slice(1).map((token,i)=>({from:tokens[i].end,to:token.start})).find(gap=>gap.from<gap.to&&actual.owner(gap.from));if(gap)mapping('retarget '+role+' ordered gap',gap.from,other(actual.owner(gap.from)));
  }
  for(const {role,token}of chosen.values()){const offset=token.start+Math.floor((token.end-token.start)/2),owner=actual.owner(offset);mapping('erase '+role+' '+token.text+' UTF-16 interior',offset,null);mapping('retarget '+role+' '+token.text+' UTF-16 interior',offset,other(owner));}
  for(const token of all)if(/^\d+$/.test(token.text)&&token.owners.some(Boolean)){mapping('erase mapped numeric operand',token.start,null);code('change source value or binding index',token.start,token.end,(token.text[0]==='9'?'8':'9')+token.text.slice(1));}
  for(const node of proof.actual.generated)mapping('grant generated helper/header/hoist source',start(node)+1,[0,0,0]);
  for(const token of proof.actual.wrapperTokens.slice(0,1))mapping('grant module envelope source',token.start,[0,0,0]);
  mapping('grant trailing map comment source',actual.code.lastIndexOf('//# sourceMappingURL=')+4,[0,0,0]);
  const comment=actual.comments().find(comment=>comment.text.startsWith('/* G-E5'));if(comment){mapping('erase astral comment UTF-16 interior',comment.start+9,null);mapping('retarget mapped comment endpoint',comment.end,other(actual.owner(comment.end)));}
  for(const [label,key,value]of [['wrong file metadata','file','other.js'],['unexpected name metadata','names',['wrong']]]){assert.throws(()=>compare(ts,native,stream(ts,actual.code,JSON.stringify({...JSON.parse(actual.raw),[key]:value})),options,original),undefined,label);controls.push({label,rejected:true});}
  const total=all.find(token=>token.text==='total'&&token.owners.some(Boolean));assert(total);code('change original binding identity',total.start,total.end,'other');
  if(options.target==='ES5'){
    const loop=proof.loop.actual,tokens=actual.tokens(loop.node);
    for(const text of ['next','return','call','done','value']){const token=tokens.find(token=>token.text===text);assert(token);code('change iterator '+text+' protocol',token.start,token.end,(text[0]==='X'?'Y':'X')+text.slice(1));}
    for(const text of ['true','false']){const token=tokens.find(token=>token.text===text);assert(token);mapping('grant generated iterator completion flag',token.start,[0,0,0]);code('change iterator completion flag',token.start,token.end,text==='true'?'null':'true ');}
    const closing=tokens.find(token=>token.text===loop.closing);assert(closing);mapping('grant generated iterator closing source',closing.start+1,[0,0,0]);
    if(!proof.functions.continuation.machines.actual.adapter){const machine=proof.functions.continuation.machines.actual,entry=machine.call.arguments[1];assert(ts.isNumericLiteral(entry));code('wrong private generator entry',start(entry),entry.end,'9'.repeat(entry.end-start(entry)));const state=machine.dispatcher.caseBlock.clauses.find(ts.isCaseClause).expression;code('wrong private generator state',start(state),state.end,'9'.repeat(state.end-start(state)));mapping('grant private generator dispatcher source',start(machine.dispatcher)+1,[0,0,0]);}
  }
  const helper=proof.actual.generated.find(node=>ts.isFunctionDeclaration(node)&&/^__blueice_target_iterator_/.test(node.name?.text));if(helper){const token=actual.tokens(helper).find(token=>token.text==='Symbol');assert(token);code('change iterator helper intrinsic',token.start,token.end,'Sumbol');}
  if(proof.actual.dependencies.length){const token=actual.tokens(actual.source).find(token=>/^['"]tslib['"]$/.test(token.text));assert(token);code('change helper dependency identity',token.start+1,token.end-1,'wrong');}
  controls.push(...require('./ge5_tslib_generator_mutations.cjs').run(ts,native,actual,options,original,proof.functions));
  return controls;
}
module.exports={run};
