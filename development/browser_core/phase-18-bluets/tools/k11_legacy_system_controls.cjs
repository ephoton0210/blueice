// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Fresh bounded map/code controls rerun the complete comparer on new views.
const assert=require('node:assert/strict'),{stream}=require('./ge5_role_stream.cjs'),{mutate,encode}=require('./ge5_role_mutations.cjs');
function run(ts,native,actual,options,source,proof,compare){
  const controls=[],points=new Map(),add=(offset,role)=>{if(offset>=0&&offset<=actual.code.length){if(!points.has(offset))points.set(offset,new Set());points.get(offset).add(role);}},same=(a,b)=>JSON.stringify(a)===JSON.stringify(b);
  const check=changed=>compare(ts,stream(ts,native.code,native.raw),changed,options,source),tokens=actual.tokens(actual.source);
  for(const token of tokens){add(token.start,'token-first');add(token.end-1,'token-last');add(token.start+Math.floor((token.end-token.start)/2),'token-interior');}
  let previous=0;for(const token of tokens){if(previous<token.start){add(previous,'gap-first');add(token.start-1,'gap-last');add(previous+Math.floor((token.start-previous)/2),'gap-interior');}previous=token.end;}if(previous<actual.code.length){add(previous,'trailing-gap-first');add(actual.code.length-1,'trailing-gap-last');}
  for(const comment of actual.comments()){add(comment.start,'comment-start');add(comment.start+1,'comment-interior');add(comment.end-1,'comment-last');add(comment.end,'comment-end');}
  for(const pair of proof.delegated?.pairs??[])add(pair.actual.end,'decorator-call-end');for(const pair of proof.delegated?.statementPairs??[])add(pair.actual.end,'decorator-statement-end');
  for(const cls of proof.classes??[])for(const pair of cls.declarationChain??[])add(pair.actual.end,'class-logical-end:'+pair.label);
  for(const pair of proof.callableEnds??[])add(pair.actual,'callable-logical-end:'+pair.label);for(const pair of proof.sourceCommentEnds??[])add(pair.actual,'comment-logical-end');for(const point of proof.ends??[])add(point.actual,'System-logical-end:'+point.label);add(actual.code.length,'module-EOF');
  function changedPoint(offset,owner){
    if(offset<actual.code.length)return mutate(ts,actual,offset,owner);
    const json=JSON.parse(actual.raw),segments=actual.decoded.segments.map(s=>[...s]),p=actual.source.getLineAndCharacterOfPosition(offset);while(segments.at(-1)?.[0]===p.line&&segments.at(-1)?.[1]===p.character)segments.pop();segments.push([p.line,p.character,...(owner??[])]);return stream(ts,actual.code,JSON.stringify({...json,mappings:encode(segments)}));
  }
  function mapControl(offset,owner,label,roles){const before=actual.owner(offset);assert(!same(before,owner),'effective fresh owner change');const changed=changedPoint(offset,owner);assert(same(changed.owner(offset),owner),'valid isolated changed map owner');assert.equal(changed.code,actual.code);let error;try{check(changed);}catch(value){error=value;}assert(error,'complete comparer must reject '+label+' at '+offset);controls.push({domain:'map',label,offset,roles:[...roles],before,after:owner,effective:true,validMap:true,rejected:true,error:error.message});}
  const absent=[0,0,1],owners=[...new Map(native.decoded.segments.filter(s=>s.length>2).map(s=>[JSON.stringify(s.slice(2)),s.slice(2)])).values()];assert(!owners.some(x=>same(x,absent)),'within-bounds negative owner is absent from every Native role');assert(owners.length>1);
  for(const[offset,roles]of [...points].sort((a,b)=>a[0]-b[0])){
    const before=actual.owner(offset);mapControl(offset,absent,before===null?'grant generated character':'retarget original role to different bounded coordinate',roles);
    const strong=[...roles].some(role=>/^(token|comment|decorator|class-logical|callable-logical|System-logical|module-EOF)/.test(role));
    if(strong){const replacement=owners.find(owner=>!same(owner,before));assert(replacement);mapControl(offset,replacement,before===null?'grant an existing Native source role to generated syntax':'retarget to a different existing Native source role',roles);if(before!==null)mapControl(offset,null,'erase required original source role',roles);}
  }
  for(const[key,value]of[['file','wrong.js'],['sourceRoot','/wrong/'],['names',['fabricated']],['sources',['wrong.ts']]]){const map=JSON.parse(actual.raw);assert(!same(map[key],value));const changed=stream(ts,actual.code,JSON.stringify({...map,[key]:value}));let error;try{check(changed);}catch(value){error=value;}assert(error,'fresh valid metadata control '+key);controls.push({domain:'metadata',label:key,effective:true,validMap:true,rejected:true,error:error.message});}
  function edit(label,start,end,text){assert.equal(text.length,end-start,'same-width semantic edit');const code=actual.code.slice(0,start)+text+actual.code.slice(end);assert.notEqual(code,actual.code);const changed=stream(ts,code,actual.raw);let error;try{check(changed);}catch(value){error=value;}assert(error,'valid JavaScript semantic control '+label);controls.push({domain:'code',label,start,end,effective:true,validJavaScript:true,rejected:true,error:error.message});}
  function nodes(predicate){const result=[];function visit(node){if(predicate(node))result.push(node);ts.forEachChild(node,visit);}visit(actual.source);return result;}
  if(proof.case==='k11-legacy-class-member'){const values=nodes(n=>ts.isReturnStatement(n)&&n.expression&&ts.isNumericLiteral(n.expression)&&n.expression.text==='42');assert.equal(values.length,1);edit('original method result',values[0].expression.getStart(actual.source),values[0].expression.end,'43');}
  if(proof.case==='k11-legacy-parameter-metadata'){const calls=nodes(n=>ts.isCallExpression(n)&&n.expression.getText(actual.source)==='__bluetsMetadata'&&n.arguments.length===2&&ts.isStringLiteral(n.arguments[0])&&n.arguments[0].text==='design:paramtypes');assert.equal(calls.length,1);const value=calls[0].arguments[1].elements[0];assert(ts.isIdentifier(value)&&value.text==='Number');edit('original parameter metadata constructor',value.getStart(actual.source),value.end,'String');}
  if(proof.case==='k11-system-wrapper'){const fn=nodes(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='bump')[0],update=fn.body.statements[0].expression;edit('original live-binding operator',update.operatorToken.getStart(actual.source),update.operatorToken.end,'-=');const slots=nodes(n=>ts.isCallExpression(n)&&n.expression.getText(actual.source).endsWith('_slot')&&n.arguments.length===1&&ts.isStringLiteral(n.arguments[0])&&n.arguments[0].text==='answer');assert.equal(slots.length,1);const key=slots[0].arguments[0];edit('original export slot identity',key.getStart(actual.source)+1,key.end-1,'bump__');}
  const operator=actual.code.indexOf('===');assert(operator>=0);edit('complete generated helper algorithm',operator,operator+3,'!==');
  return controls;
}
module.exports={run};
